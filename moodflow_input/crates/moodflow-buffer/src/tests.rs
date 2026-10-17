use super::*;

const A: Target = Target::new(1, 0);
const A2: Target = Target::new(1, 1); // 同一输入框，上下文重建后
const B: Target = Target::new(2, 0);

fn bound() -> Buffer {
    let mut b = Buffer::default();
    b.bind(A, FieldKind::Normal).unwrap();
    b
}

fn texts(b: &Buffer) -> Vec<&str> {
    b.blocks().iter().map(Block::text).collect()
}

// ── 编辑 ──────────────────────────────────────────────

#[test]
fn each_confirmed_phrase_is_its_own_block() {
    let mut b = Buffer::default();
    b.push_phrase("你好").unwrap();
    b.push_phrase("世界").unwrap();
    assert_eq!(texts(&b), ["你好", "世界"]);
    assert_eq!(b.text(), "你好世界");
}

#[test]
fn latin_merges_until_a_phrase_breaks_it() {
    let mut b = Buffer::default();
    for c in "abc".chars() {
        b.push_latin(c).unwrap();
    }
    b.push_phrase("中").unwrap();
    b.push_latin('d').unwrap();
    assert_eq!(texts(&b), ["abc", "中", "d"]);
    assert_eq!(b.blocks()[0].kind(), BlockKind::Latin);
}

#[test]
fn backspace_removes_latin_char_but_whole_phrase() {
    let mut b = Buffer::default();
    b.push_phrase("你好").unwrap();
    b.push_latin('a').unwrap();
    b.push_latin('b').unwrap();
    b.backspace().unwrap();
    assert_eq!(texts(&b), ["你好", "a"]);
    b.backspace().unwrap();
    assert_eq!(texts(&b), ["你好"]);
    b.backspace().unwrap();
    assert!(b.is_empty());
    assert_eq!(b.backspace(), Err(EditError::Empty));
}

#[test]
fn empty_phrase_is_rejected() {
    let mut b = Buffer::default();
    assert_eq!(b.push_phrase(""), Err(EditError::Empty));
}

#[test]
fn capacity_counts_utf16_and_rejects_whole_phrase() {
    let mut b = Buffer::new(4);
    b.push_phrase("😀").unwrap(); // 2 个 UTF-16 码元
    b.push_phrase("中").unwrap();
    assert_eq!(b.len_utf16(), 3);
    assert_eq!(b.push_phrase("文字"), Err(EditError::CapacityExceeded));
    assert_eq!(texts(&b), ["😀", "中"], "拒收时不截断、不部分写入");
    b.push_latin('x').unwrap();
    assert_eq!(b.push_latin('y'), Err(EditError::CapacityExceeded));
}

// ── 绑定与授权 ────────────────────────────────────────

#[test]
fn private_field_refuses_binding_and_drops_old_one() {
    let mut b = bound();
    assert_eq!(b.bind(B, FieldKind::Private), Err(BindError::PrivateField));
    assert_eq!(b.binding(), None);
    assert_eq!(b.state(), State::Unbound);
}

#[test]
fn unbound_buffer_can_edit_but_not_deliver() {
    let mut b = Buffer::default();
    b.push_phrase("草稿").unwrap();
    assert_eq!(b.begin_next(A), Err(DeliverError::NotBound));
}

#[test]
fn leaving_the_target_suspends_and_returning_does_not_resume() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    b.focus_changed(B);
    assert_eq!(b.state(), State::Suspended);
    assert_eq!(b.begin_next(B), Err(DeliverError::Suspended));
    b.focus_changed(A);
    assert_eq!(
        b.begin_next(A),
        Err(DeliverError::Suspended),
        "回到原框也要显式重绑"
    );
    b.bind(A, FieldKind::Normal).unwrap();
    assert_eq!(b.begin_next(A).unwrap().text, "你好");
}

#[test]
fn same_field_with_new_revision_counts_as_a_different_target() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    b.focus_changed(A2);
    assert_eq!(b.state(), State::Suspended);
}

#[test]
fn delivering_to_another_target_is_refused() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    assert_eq!(b.begin_next(B), Err(DeliverError::TargetMismatch));
}

#[test]
fn suspend_keeps_content() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    b.suspend();
    assert_eq!(texts(&b), ["你好"]);
}

#[test]
fn discard_drops_everything() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    b.discard();
    assert!(b.is_empty());
    assert_eq!(b.state(), State::Unbound);
}

// ── 投递 ──────────────────────────────────────────────

#[test]
fn accepted_delivery_removes_only_the_delivered_block() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    b.push_phrase("世界").unwrap();
    let d = b.begin_next(A).unwrap();
    assert_eq!((d.text.as_str(), d.blocks), ("你好", 1));
    assert!(b.is_authorized(d.token, A));
    b.finish(d.token, Outcome::Accepted).unwrap();
    assert_eq!(texts(&b), ["世界"]);
}

#[test]
fn rejected_delivery_keeps_text_for_retry() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    let d = b.begin_next(A).unwrap();
    b.finish(d.token, Outcome::Rejected).unwrap();
    assert_eq!(texts(&b), ["你好"]);
    assert_eq!(b.begin_next(A).unwrap().text, "你好");
}

#[test]
fn unknown_outcome_holds_instead_of_resending() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    let d = b.begin_next(A).unwrap();
    b.finish(d.token, Outcome::Unknown).unwrap();
    assert_eq!(b.state(), State::Held);
    assert_eq!(b.begin_next(A), Err(DeliverError::Held));
    assert_eq!(b.text(), "你好", "内容保留，可复制");
    b.release_hold();
    assert_eq!(b.begin_next(A).unwrap().text, "你好");
}

#[test]
fn rebinding_does_not_clear_a_hold() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    let d = b.begin_next(A).unwrap();
    b.finish(d.token, Outcome::Unknown).unwrap();
    b.bind(A, FieldKind::Normal).unwrap();
    assert_eq!(b.state(), State::Held);
}

#[test]
fn begin_all_freezes_the_queue_at_press_time() {
    let mut b = bound();
    b.push_phrase("一").unwrap();
    b.push_phrase("二").unwrap();
    let d = b.begin_all(A).unwrap();
    assert_eq!((d.text.as_str(), d.blocks), ("一二", 2));
    b.push_phrase("三").unwrap();
    b.finish(d.token, Outcome::Accepted).unwrap();
    assert_eq!(texts(&b), ["三"]);
}

#[test]
fn block_gaps_never_become_spaces() {
    let mut b = bound();
    b.push_latin('a').unwrap();
    b.push_phrase("中").unwrap();
    b.push_latin('b').unwrap();
    assert_eq!(b.begin_all(A).unwrap().text, "a中b");
}

#[test]
fn only_one_delivery_in_flight() {
    let mut b = bound();
    b.push_phrase("一").unwrap();
    b.push_phrase("二").unwrap();
    b.begin_next(A).unwrap();
    assert_eq!(b.begin_next(A), Err(DeliverError::Busy));
}

#[test]
fn empty_buffer_has_nothing_to_deliver() {
    let mut b = bound();
    assert_eq!(b.begin_next(A), Err(DeliverError::Empty));
}

#[test]
fn focus_change_revokes_in_flight_delivery() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    let d = b.begin_next(A).unwrap();
    b.focus_changed(B);
    assert!(!b.is_authorized(d.token, A));
    assert!(!b.is_authorized(d.token, B));
    assert_eq!(
        b.finish(d.token, Outcome::Accepted),
        Err(FinishError::Stale)
    );
    assert_eq!(texts(&b), ["你好"], "作废的投递不能消费内容");
}

#[test]
fn token_cannot_be_reused() {
    let mut b = bound();
    b.push_phrase("一").unwrap();
    b.push_phrase("二").unwrap();
    let d = b.begin_next(A).unwrap();
    b.finish(d.token, Outcome::Accepted).unwrap();
    assert_eq!(
        b.finish(d.token, Outcome::Accepted),
        Err(FinishError::Stale)
    );
    assert_eq!(texts(&b), ["二"]);
}

#[test]
fn in_flight_blocks_cannot_be_edited_but_later_ones_can() {
    let mut b = bound();
    b.push_latin('a').unwrap();
    let d = b.begin_next(A).unwrap();
    assert!(b.is_in_flight(b.blocks()[0].id()));
    assert_eq!(b.backspace(), Err(EditError::Busy));
    assert_eq!(b.delete_last_block(), Err(EditError::Busy));
    // 末尾英文块在途时，新英文字符另起一块，不改在途内容
    b.push_latin('b').unwrap();
    assert_eq!(texts(&b), ["a", "b"]);
    b.backspace().unwrap();
    assert_eq!(texts(&b), ["a"]);
    b.finish(d.token, Outcome::Accepted).unwrap();
    assert!(b.is_empty());
}

#[test]
fn clear_keeps_binding_and_revokes_in_flight() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    let d = b.begin_next(A).unwrap();
    b.clear();
    assert!(b.is_empty());
    assert_eq!(b.state(), State::Ready);
    assert_eq!(
        b.finish(d.token, Outcome::Accepted),
        Err(FinishError::Stale)
    );
}

// ── 发送键 ────────────────────────────────────────────

#[test]
fn send_key_confirms_composition_without_sending() {
    let mut b = bound();
    b.push_phrase("你好").unwrap();
    assert_eq!(b.send_key_action(true), SendKeyAction::ConfirmComposition);
    assert_eq!(b.send_key_action(false), SendKeyAction::SendNext);
}

#[test]
fn send_key_passes_through_when_nothing_left_to_send() {
    let mut b = bound();
    assert_eq!(b.send_key_action(false), SendKeyAction::PassThrough);
    b.push_phrase("你好").unwrap();
    b.begin_next(A).unwrap();
    assert_eq!(b.send_key_action(false), SendKeyAction::PassThrough);
}
