//! Buffer 接入（Windows，docs/design/buffer.md §3）：上屏前的文本工作台。
//!
//! - `keys.toggle_buffer`（出厂 Alt+Shift+B）：打开 Buffer 并**显式绑定**当前输入框；已打开但
//!   绑定已暂停（换过框、切过应用）时再按一次重新绑定；已打开且绑定有效时再按一次关闭（内容保留）。
//! - 打开且绑定有效时，按键动作里的上屏正文**改进 Buffer**（每次一块），宿主只收到一次空上屏
//!   用来结束组字——候选窗、组字区照常收尾，文档里什么都没写进去。
//! - `keys.buffer_send`（出厂 Ctrl+Shift+Enter）：短按发送一块，长按发送冻结队列。有组字时只
//!   确认组字，这次按键不顺带发送（契约 7）。
//! - 右键浮窗：发送、删除最后一块、清空、关闭。
//!
//! # 授权与目标
//!
//! 目标身份为 TSF 客户端令牌 + 输入上下文修订号。按键和菜单统一定向推送；编辑会话写入前
//! 同步复核授权，写入后报告实际结果。只有 Accepted 才移除块，未知／超时保留并暂停重发。
//! 旧 DLL 缺少修订号时拒绝绑定；不会降级成没有回执的直接上屏。
//!
//! 正文是临时数据，不落盘、不进日志（契约 12）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use moodflow_bridge::handler::KeyAction;
use moodflow_buffer::{
    BindError, Buffer, DeliverError, DeliveryToken, EditError, FieldKind, Outcome, State, Target,
};
use moodflow_ipc::buffer::{DELIVERY_TIMEOUT_MS, Delivery, Mode, Receipt, ReceiptOutcome};
use moodflow_ui_types::{BufferView, MenuAnchor, MenuCmd, MenuItemSpec as M, MenuKind, UiCommand};

use crate::coordinator::Coordinator;

mod sequence;

/// Buffer 容量：16,384 个 UTF-16 码元（契约 11）。
const CAPACITY_UTF16: usize = 16_384;

pub(crate) struct BufferState {
    inner: Mutex<Inner>,
    next_delivery: AtomicU64,
}

struct Pending {
    id: u64,
    token: DeliveryToken,
    target: Target,
    started: Instant,
    authorized: bool,
}

struct Inner {
    buf: Buffer,
    open: bool,
    /// 当前焦点所在的目标（最近一次 focus_gained）。
    current: Option<Target>,
    pending: Option<Pending>,
    capture_target: Option<Target>,
    last_mode: Mode,
    notice: Option<&'static str>,
    /// 队列、回执与焦点撤销必须在同一把锁下推进；后台节拍不持输入 State 锁。
    sequence: Option<sequence::Sequence>,
    /// 物理按下的锁存独立于队列，完成后仍阻止自动重复启动下一队列。
    send_key_down: Option<u32>,
}

impl Inner {
    fn revoke(&mut self) {
        self.stop_sequence();
        self.send_key_down = None;
        self.capture_target = None;
        // 宿主可能已经写入但回执尚未抵达，不能把在途文字恢复为可自动重发。
        if let Some(pending) = self.pending.take() {
            let _ = self.buf.finish(pending.token, Outcome::Unknown);
        }
        self.buf.suspend();
    }
}

impl Default for BufferState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                buf: Buffer::new(CAPACITY_UTF16),
                open: false,
                current: None,
                pending: None,
                capture_target: None,
                last_mode: Mode {
                    client: 0,
                    revision: 0,
                    generation: 0,
                },
                notice: None,
                sequence: None,
                send_key_down: None,
            }),
            next_delivery: AtomicU64::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            ),
        }
    }
}

/// 截获结果。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Capture {
    /// 不归 Buffer 管，原样交给宿主。
    PassThrough,
    /// 已收进 Buffer；宿主那边改成空上屏。
    Captured,
    /// Buffer 满了，整段拒收（不截断、不绕过 Buffer 写入宿主）。
    Full,
}

impl BufferState {
    pub(crate) fn is_capturing(&self) -> bool {
        Self::captures(&self.lock())
    }

    pub(crate) fn previous_char(&self) -> Option<u16> {
        let g = self.lock();
        Self::captures(&g).then(|| {
            g.buf
                .blocks()
                .last()
                .and_then(|b| b.text().chars().last())
                .filter(|ch| (*ch as u32) <= u16::MAX as u32)
                .map(|ch| ch as u16)
                .unwrap_or(0)
        })
    }
    fn captures(g: &Inner) -> bool {
        g.open && g.capture_target.is_some() && g.capture_target == g.current
    }

    fn mode(&self) -> (Mode, Option<Mode>) {
        let mut g = self.lock();
        let client = g.current.map(|t| t.id).unwrap_or(0);
        let revision = if Self::captures(&g) {
            g.capture_target.unwrap().revision
        } else {
            0
        };
        let old = g.last_mode;
        if old.client != client || old.revision != revision {
            g.last_mode = Mode {
                client,
                revision,
                generation: old.generation.wrapping_add(1),
            };
            (g.last_mode, Some(old))
        } else {
            (old, None)
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn is_open(&self) -> bool {
        self.lock().open
    }

    /// 打开且绑定有效时把一段上屏正文收进 Buffer。
    pub(crate) fn capture(&self, text: &str) -> Capture {
        if text.is_empty() {
            return Capture::PassThrough;
        }
        let mut g = self.lock();
        if !Self::captures(&g) {
            return Capture::PassThrough;
        }
        match g.buf.push_phrase(text) {
            Ok(_) => Capture::Captured,
            Err(EditError::CapacityExceeded) => Capture::Full,
            Err(_) => Capture::PassThrough,
        }
    }

    fn focus_gained(&self, token: u64, revision: u64) {
        let mut g = self.lock();
        let current = (revision != 0).then_some(Target::new(token, revision));
        if g.current != current {
            g.revoke();
        }
        g.current = current;
    }

    fn focus_lost(&self) {
        let mut g = self.lock();
        g.revoke();
        g.current = None;
    }

    /// 冻结当前内容，保留原块直至宿主回执。
    fn begin_all(&self) -> Result<Delivery, DeliverError> {
        let mut g = self.lock();
        if g.sequence.is_some() {
            return Err(DeliverError::Busy);
        }
        self.begin_locked(&mut g, true, Instant::now())
    }

    fn begin_locked(
        &self,
        g: &mut Inner,
        all: bool,
        now: Instant,
    ) -> Result<Delivery, DeliverError> {
        if !g.open {
            return Err(DeliverError::Suspended);
        }
        let Some(cur) = g.current else {
            return Err(DeliverError::NotBound);
        };
        let d = if all {
            g.buf.begin_all(cur)?
        } else {
            g.buf.begin_next(cur)?
        };
        if !g.buf.is_authorized(d.token, cur) {
            return Err(DeliverError::TargetMismatch);
        }
        let id = self.next_delivery.fetch_add(1, Ordering::Relaxed);
        g.pending = Some(Pending {
            id,
            token: d.token,
            target: cur,
            started: now,
            authorized: false,
        });
        Ok(Delivery {
            id,
            client: cur.id,
            revision: cur.revision,
            text: d.text,
        })
    }

    fn authorize(&self, id: u64, client: u64, revision: u64) -> bool {
        let mut g = self.lock();
        let target = Target::new(client, revision);
        let Some(p) = g.pending.as_ref() else {
            return false;
        };
        if !g.open
            || g.current != Some(target)
            || p.id != id
            || p.target != target
            || p.authorized
            || p.started.elapsed() >= Duration::from_millis(DELIVERY_TIMEOUT_MS)
            || !g.buf.is_authorized(p.token, target)
        {
            return false;
        }
        // 单次授权：重复帧不能再次触发写入。
        g.pending.as_mut().unwrap().authorized = true;
        true
    }

    fn finish(&self, receipt: Receipt) -> bool {
        self.finish_at(receipt, Instant::now())
    }

    fn finish_at(&self, receipt: Receipt, now: Instant) -> bool {
        let mut g = self.lock();
        let Some(p) = g.pending.as_ref() else {
            return false;
        };
        if p.id != receipt.id || p.target != Target::new(receipt.client, receipt.revision) {
            return false;
        }
        if receipt.outcome == ReceiptOutcome::Accepted && !p.authorized {
            return false;
        }
        let token = g.pending.take().unwrap().token;
        let outcome = match receipt.outcome {
            ReceiptOutcome::Accepted => Outcome::Accepted,
            ReceiptOutcome::Rejected => Outcome::Rejected,
            ReceiptOutcome::Unknown => Outcome::Unknown,
        };
        let finished = g.buf.finish(token, outcome).is_ok();
        if finished {
            g.sequence_receipt(receipt.outcome, now);
        }
        finished
    }

    fn expire(&self, id: u64) -> bool {
        let mut g = self.lock();
        if !g.pending.as_ref().is_some_and(|p| p.id == id) {
            return false;
        }
        let p = g.pending.take().unwrap();
        let _ = g.buf.finish(p.token, Outcome::Unknown);
        true
    }

    fn view(&self, hint_send: &str, hint_toggle: &str) -> (String, String) {
        let g = self.lock();
        let status = match g.buf.state() {
            State::Ready if g.pending.is_some() => "正在发送 · 等待输入框确认".to_string(),
            State::Ready => format!("已绑定 · {hint_send} 短按一块 / 长按逐块 · 右键更多"),
            State::Suspended | State::Unbound => {
                format!("已暂停（换过输入框）· 回到要发送的框按 {hint_toggle} 重新绑定")
            }
            State::Held => "上次发送结果不明，已暂停 · 右键确认后继续".to_string(),
        };
        (
            g.buf.text(),
            g.notice
                .map(str::to_owned)
                .or_else(|| g.sequence_status())
                .unwrap_or(status),
        )
    }
}

fn deliver_error_tip(e: DeliverError) -> &'static str {
    match e {
        DeliverError::Empty => "Buffer 是空的",
        DeliverError::NotBound | DeliverError::Suspended | DeliverError::TargetMismatch => {
            "Buffer 未绑定当前输入框：先按绑定键重新绑定"
        }
        DeliverError::Held => "上次发送结果不明，已暂停：确认后再发",
        DeliverError::Busy => "正在发送",
    }
}

impl Coordinator {
    pub(crate) fn buffer_mode_action(&self) -> KeyAction {
        KeyAction::BufferMode(self.buffer.mode().0)
    }

    fn push_buffer_mode(&self) {
        let (mode, previous) = self.buffer.mode();
        if let Some(old) = previous {
            let send = |m: Mode| {
                let frame = moodflow_ipc::codec::encode_ext(
                    moodflow_ipc::protocol::ext_kind::BUFFER_MODE,
                    &m.encode(),
                );
                self.push_server.push_to_token(m.client, &frame);
            };
            if old.client != 0 && old.client != mode.client {
                send(Mode {
                    client: old.client,
                    revision: 0,
                    generation: mode.generation,
                });
            }
            if mode.client != 0 {
                send(mode);
            }
        }
    }

    /// Called with the input-state lock held. UI updates happen at the policed exit.
    pub(crate) fn buffer_direct_key(
        &self,
        data: &moodflow_bridge::handler::KeyEventData,
        chinese: bool,
        caps: bool,
        full_width: bool,
    ) -> Option<KeyAction> {
        use moodflow_keys::keymap;
        if !crate::moodflow::input_privacy().input_allowed() {
            return None;
        }
        let mut g = self.buffer.lock();
        if !BufferState::captures(&g) {
            return None;
        }
        let result = if data.key_code == keymap::VK_BACK {
            g.buf.backspace()
        } else if data.key_code == keymap::VK_RETURN {
            g.buf.push_phrase("\n").map(|_| ())
        } else if data.key_code == keymap::VK_SPACE {
            g.buf.push_latin(' ').map(|_| ())
        } else if (!chinese || caps) && !full_width {
            let letter = (keymap::VK_A..=keymap::VK_Z).contains(&data.key_code);
            let shift =
                (data.modifiers & moodflow_ipc::protocol::MOD_SHIFT != 0) ^ (caps && letter);
            let ch = crate::key_convert::full_width_source_char(data.key_code, shift)?;
            g.buf.push_latin(ch).map(|_| ())
        } else {
            return None;
        };
        g.notice = match result {
            Err(EditError::CapacityExceeded) => Some("Buffer 已满，本次输入未收录；请先发送或清理"),
            Err(EditError::Busy) => Some("正在发送的文本暂时不能修改"),
            _ => None,
        };
        Some(KeyAction::Consumed)
    }

    fn buffer_key_labels(&self) -> (String, String) {
        let k = &self.rt().config.keys;
        let label = |s: &str| {
            if s.is_empty() {
                "（未设置）".to_string()
            } else {
                s.to_string()
            }
        };
        (label(&k.buffer_send), label(&k.toggle_buffer))
    }

    /// 刷新或隐藏浮窗。
    pub(crate) fn push_buffer_panel(&self) {
        self.push_buffer_mode();
        if !self.buffer.is_open() {
            let _ = self.ui_tx.send(UiCommand::HideBuffer);
            return;
        }
        let (send, toggle) = self.buffer_key_labels();
        let (text, status) = self.buffer.view(&send, &toggle);
        let (cx, cy, ch) = {
            let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            (s.caret_x, s.caret_y, s.caret_height)
        };
        let (caret_x, caret_y, caret_height, _) = self.resolve_caret_for_ui(cx, cy, ch);
        let _ = self.ui_tx.send(UiCommand::ShowBuffer(BufferView {
            text,
            status,
            caret_x,
            caret_y,
            caret_height,
        }));
    }

    /// `keys.toggle_buffer`。
    pub(crate) fn toggle_buffer(&self) {
        let private = !crate::moodflow::input_privacy().input_allowed();
        let tip = {
            let mut g = self.buffer.lock();
            let cur = g.current;
            match (g.open, g.buf.state()) {
                (true, State::Ready) => {
                    g.open = false;
                    g.revoke();
                    None
                }
                _ => match cur {
                    None => Some("没有可绑定的输入框，请确认 Windows 输入法组件已更新"),
                    Some(t) => {
                        let field = if private {
                            FieldKind::Private
                        } else {
                            FieldKind::Normal
                        };
                        match g.buf.bind(t, field) {
                            Ok(()) => {
                                g.open = true;
                                g.capture_target = Some(t);
                                g.notice = None;
                                None
                            }
                            Err(BindError::PrivateField) => {
                                g.open = false;
                                Some("当前隐私状态不允许启用 Buffer")
                            }
                        }
                    }
                },
            }
        };
        if let Some(t) = tip {
            self.show_tip(t);
        }
        self.push_buffer_panel();
    }

    /// `keys.buffer_send`（按键上下文）。Buffer 没打开时返回 `None`，不吞键。
    pub(crate) fn buffer_send_keyed(&self) -> Option<KeyAction> {
        if !self.buffer.is_open() {
            return None;
        }
        if self.has_pending_composition() {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            return Some(
                self.commit_highlighted(&mut state)
                    .unwrap_or(KeyAction::Consumed),
            );
        }
        self.send_buffer();
        Some(KeyAction::Consumed)
    }

    fn send_buffer(&self) {
        if !crate::moodflow::input_privacy().input_allowed() {
            self.buffer_focus_lost();
            self.show_tip("当前隐私状态不允许发送 Buffer");
            return;
        }
        if self.has_pending_composition() {
            self.show_tip("先确认组字，再发送 Buffer");
            return;
        }
        match self.buffer.begin_all() {
            Ok(delivery) => self.dispatch_buffer_delivery(delivery, true),
            Err(error) => self.show_tip(deliver_error_tip(error)),
        }
        self.push_buffer_panel();
    }

    fn dispatch_buffer_delivery(&self, mut delivery: Delivery, timeout_worker: bool) {
        self.buffer.lock().notice = None;

        if let KeyAction::InsertText { text, .. } =
            self.apply_plugin_transform(KeyAction::InsertText {
                text: delivery.text.clone(),
                new_composition: None,
                mode_changed: false,
                chinese_mode: self.is_chinese_mode(),
                has_new_composition: false,
            })
        {
            delivery.text = text;
        }
        delivery.text = self.convert_commit_newline(delivery.text);
        let queued = delivery.encode().is_some_and(|body| {
            self.push_server.push_to_token(
                delivery.client,
                &moodflow_ipc::codec::encode_ext(
                    moodflow_ipc::protocol::ext_kind::BUFFER_DELIVER,
                    &body,
                ),
            )
        });
        if !queued {
            self.buffer.finish(Receipt {
                id: delivery.id,
                client: delivery.client,
                revision: delivery.revision,
                outcome: ReceiptOutcome::Rejected,
            });
            self.show_tip("输入框未连接，文字已保留");
        } else if timeout_worker && let Some(weak) = self.self_weak.get().cloned() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(DELIVERY_TIMEOUT_MS));
                if let Some(c) = weak.upgrade()
                    && c.buffer.expire(delivery.id)
                {
                    c.push_buffer_panel();
                }
            });
        }
    }

    pub(crate) fn buffer_receipt(&self, receipt: Receipt) {
        if self.buffer.finish(receipt) {
            self.push_buffer_panel();
            if receipt.outcome == ReceiptOutcome::Rejected {
                self.show_tip("输入框未接收，文字已保留");
            }
        }
    }

    pub(crate) fn buffer_authorize(&self, id: u64, client: u64, revision: u64) -> bool {
        crate::moodflow::input_privacy().input_allowed()
            && self.buffer.authorize(id, client, revision)
    }

    /// 按键动作出口：Buffer 打开且绑定有效时，把上屏正文截进 Buffer（见模块文档）。
    pub(crate) fn route_commit_to_buffer(&self, action: KeyAction) -> KeyAction {
        if !crate::moodflow::input_privacy().input_allowed() {
            self.buffer_focus_lost();
            return action;
        }
        if self.buffer.is_capturing() {
            let chinese_mode = self.is_chinese_mode();
            let silent = || KeyAction::InsertText {
                text: String::new(),
                new_composition: None,
                mode_changed: false,
                chinese_mode,
                has_new_composition: false,
            };
            let (result, output) = match action {
                KeyAction::InsertTextWithCursor { ref text, .. } => (
                    self.buffer.lock().buf.push_phrase(text).map(|_| ()),
                    silent(),
                ),
                KeyAction::ReplaceBackward { count, ref text } => (
                    self.buffer.lock().buf.replace_tail(count as usize, text),
                    silent(),
                ),
                KeyAction::CommitThenDeferComposition {
                    ref commit_text,
                    ref deferred_composition,
                    ..
                } => (
                    self.buffer.lock().buf.push_phrase(commit_text).map(|_| ()),
                    KeyAction::InsertText {
                        text: String::new(),
                        new_composition: Some(deferred_composition.clone()),
                        mode_changed: false,
                        chinese_mode,
                        has_new_composition: true,
                    },
                ),
                // Buffer uses its own suffix replacement instead of a host-owned timed preview.
                KeyAction::HoldComposition { ref text, .. }
                | KeyAction::CommitReplacingHeld { ref text, .. } => (
                    self.buffer.lock().buf.push_phrase(text).map(|_| ()),
                    silent(),
                ),
                KeyAction::CommitAndHoldComposition {
                    ref commit_text,
                    ref hold_text,
                    ..
                } => (
                    self.buffer
                        .lock()
                        .buf
                        .push_phrase(&format!("{commit_text}{hold_text}"))
                        .map(|_| ()),
                    silent(),
                ),
                KeyAction::MoveCursorRight { .. } | KeyAction::DeletePair => {
                    (Ok(()), KeyAction::Consumed)
                }
                other => return self.capture_standard_commit(other),
            };
            {
                let mut g = self.buffer.lock();
                g.notice = match result {
                    Err(EditError::CapacityExceeded) => Some("Buffer 已满，本次文本未收录"),
                    Err(EditError::Busy) => Some("正在发送的文本暂时不能修改"),
                    Err(EditError::Empty) => Some("Buffer 中没有可替换的文本"),
                    Ok(()) => None,
                };
            }
            self.push_buffer_panel();
            return output;
        }
        action
    }

    fn capture_standard_commit(&self, action: KeyAction) -> KeyAction {
        let KeyAction::InsertText {
            text,
            new_composition,
            mode_changed,
            chinese_mode,
            has_new_composition,
        } = action
        else {
            return action;
        };
        let text = match self.buffer.capture(&text) {
            Capture::PassThrough => text,
            Capture::Captured => {
                self.push_buffer_panel();
                String::new()
            }
            Capture::Full => {
                self.show_tip("Buffer 已满，这段未收录；请先发送或清理，再重新输入");
                String::new()
            }
        };
        KeyAction::InsertText {
            text,
            new_composition,
            mode_changed,
            chinese_mode,
            has_new_composition,
        }
    }

    pub(crate) fn buffer_focus_gained(&self, token: u64, revision: u64) {
        self.buffer.focus_gained(token, revision);
        if self.buffer.is_open() {
            self.push_buffer_panel();
        }
    }

    pub(crate) fn buffer_focus_lost(&self) {
        self.buffer.focus_lost();
        if self.buffer.is_open() {
            self.push_buffer_panel();
        }
    }

    /// 浮窗右键菜单。
    pub(crate) fn show_buffer_menu(&self, x: i32, y: i32) {
        let cmd = |c: MenuCmd| MenuKind::Command(c);
        let (empty, held) = {
            let g = self.buffer.lock();
            (g.buf.is_empty(), g.buf.state() == State::Held)
        };
        let mut items = vec![
            M::leaf("全部发送", cmd(MenuCmd::BufferSend), !empty, false),
            M::leaf(
                "删除最后一块",
                cmd(MenuCmd::BufferDeleteLast),
                !empty,
                false,
            ),
            M::leaf("清空", cmd(MenuCmd::BufferClear), !empty, false),
        ];
        if self.buffer.lock().sequence.is_some() {
            items.push(M::leaf(
                "停止逐块发送",
                cmd(MenuCmd::BufferCancelSequence),
                true,
                false,
            ));
        }
        items.push(M::separator());
        let config = self.rt().config.input.buffer.clone();
        let mut pace_items = Vec::new();
        for (label, command, selected) in [
            ("连续发送", MenuCmd::BufferPaceOff, config.pace == "off"),
            ("每隔 1 秒", MenuCmd::BufferPace1, config.pace == "1s"),
            ("每隔 2 秒", MenuCmd::BufferPace2, config.pace == "2s"),
            ("每隔 3 秒", MenuCmd::BufferPace3, config.pace == "3s"),
            ("每隔 5 秒", MenuCmd::BufferPace5, config.pace == "5s"),
        ] {
            pace_items.push(M::leaf(label, cmd(command), true, selected));
        }
        items.push(M::submenu("逐块发送间隔", pace_items));
        let mut hold_items = Vec::new();
        for (label, command, value) in [
            ("长按 0.5 秒", MenuCmd::BufferHold500, 500),
            ("长按 1.2 秒", MenuCmd::BufferHold1200, 1200),
            ("长按 3 秒", MenuCmd::BufferHold3000, 3000),
        ] {
            hold_items.push(M::leaf(
                label,
                cmd(command),
                true,
                config.hold_ms() == value,
            ));
        }
        items.push(M::submenu("长按阈值", hold_items));
        items.push(M::leaf(
            "发送完关闭 Buffer",
            cmd(MenuCmd::BufferToggleClose),
            true,
            config.close_after_send,
        ));
        if held {
            items.push(M::separator());
            items.push(M::leaf(
                "我已确认上次没有写入，允许重试",
                cmd(MenuCmd::BufferReleaseHold),
                true,
                false,
            ));
        }
        items.push(M::separator());
        items.push(M::leaf(
            "关闭 Buffer（内容保留）",
            cmd(MenuCmd::BufferClose),
            true,
            false,
        ));
        self.mark_menu_open(0, String::new());
        let _ = self.ui_tx.send(UiCommand::ShowCandidateMenu {
            items,
            anchor: MenuAnchor::at_point(x, y),
        });
    }

    pub(crate) fn run_buffer_menu_cmd(&self, cmd: MenuCmd) {
        match cmd {
            MenuCmd::BufferPaceOff => self.set_buffer_option("pace", "off".into()),
            MenuCmd::BufferPace1 => self.set_buffer_option("pace", "1s".into()),
            MenuCmd::BufferPace2 => self.set_buffer_option("pace", "2s".into()),
            MenuCmd::BufferPace3 => self.set_buffer_option("pace", "3s".into()),
            MenuCmd::BufferPace5 => self.set_buffer_option("pace", "5s".into()),
            MenuCmd::BufferHold500 => self.set_buffer_option("hold_ms", toml::Value::Integer(500)),
            MenuCmd::BufferHold1200 => {
                self.set_buffer_option("hold_ms", toml::Value::Integer(1200))
            }
            MenuCmd::BufferHold3000 => {
                self.set_buffer_option("hold_ms", toml::Value::Integer(3000))
            }
            MenuCmd::BufferToggleClose => {
                let value = !self.rt().config.input.buffer.close_after_send;
                self.set_buffer_option("close_after_send", toml::Value::Boolean(value));
            }
            MenuCmd::BufferCancelSequence => self.cancel_buffer_sequence(),
            MenuCmd::BufferSend => self.send_buffer(),
            MenuCmd::BufferDeleteLast => {
                let _ = self.buffer.lock().buf.delete_last_block();
            }
            MenuCmd::BufferClear => {
                let mut g = self.buffer.lock();
                g.stop_sequence();
                g.pending = None;
                g.buf.clear();
            }
            MenuCmd::BufferReleaseHold => self.buffer.lock().buf.release_hold(),
            MenuCmd::BufferClose => {
                let mut g = self.buffer.lock();
                g.open = false;
                g.revoke();
            }
            _ => {}
        }
        self.push_buffer_panel();
    }

    fn set_buffer_option(&self, key: &str, value: toml::Value) {
        if moodflow_config::Config::set_user_value(&["input", "buffer", key], value).is_ok() {
            self.reload_user_config();
            self.buffer.lock().notice = Some("设置已保存，下次发送生效");
        } else {
            self.show_tip("保存 Buffer 设置失败");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(text: &str) -> KeyAction {
        KeyAction::InsertText {
            text: text.into(),
            new_composition: None,
            mode_changed: false,
            chinese_mode: true,
            has_new_composition: false,
        }
    }

    fn text_of(a: &KeyAction) -> &str {
        match a {
            KeyAction::InsertText { text, .. } => text,
            _ => panic!("不是 InsertText：{a:?}"),
        }
    }

    fn coord() -> std::sync::Arc<Coordinator> {
        crate::moodflow::TEST_PRIVACY.with(|p| p.set(Some(Default::default())));
        Coordinator::new_headless_with_ui(moodflow_config::Config::default(), None).0
    }

    #[test]
    fn closed_buffer_passes_commits_through() {
        let c = coord();
        c.buffer_focus_gained(1, 1);
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("你好"))), "你好");
        assert!(c.buffer_send_keyed().is_none(), "没打开时发送键不吞");
    }

    #[test]
    fn open_buffer_captures_then_sends_once_to_the_bound_field() {
        let c = coord();
        c.buffer_focus_gained(1, 1);
        c.toggle_buffer();
        assert!(c.buffer.is_open());
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("你好"))), "");
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("世界"))), "");
        let sent = c.buffer.begin_all().unwrap();
        assert_eq!(sent.text, "你好世界", "块之间不加空格");
        assert_eq!(c.buffer.lock().buf.text(), "你好世界", "回执前保留原文");
        assert!(c.buffer_authorize(sent.id, sent.client, sent.revision));
        assert!(
            !c.buffer_authorize(sent.id, sent.client, sent.revision),
            "授权仅一次"
        );
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("再来"))), "");
        c.buffer_receipt(receipt(&sent, ReceiptOutcome::Accepted));
        assert_eq!(c.buffer.lock().buf.text(), "再来", "只移除冻结的前缀");
        assert!(!c.buffer.finish(receipt(&sent, ReceiptOutcome::Accepted)));
    }

    #[test]
    fn focus_change_suspends_and_nothing_leaks_into_the_new_field() {
        let c = coord();
        c.buffer_focus_gained(1, 1);
        c.toggle_buffer();
        c.route_commit_to_buffer(insert("给甲的"));
        c.buffer_focus_lost();
        c.buffer_focus_gained(2, 1);
        // 暂停期间上屏直接进宿主，Buffer 内容不动、也发不出去。
        assert_eq!(
            text_of(&c.route_commit_to_buffer(insert("打给乙"))),
            "打给乙"
        );
        assert!(matches!(c.buffer_send_keyed(), Some(KeyAction::Consumed)));
        assert_eq!(c.buffer.lock().buf.text(), "给甲的");
        // 在乙这里重新绑定后才能发。
        c.toggle_buffer();
        assert_eq!(c.buffer.begin_all().unwrap().text, "给甲的");
    }

    #[test]
    fn toggle_again_closes_but_keeps_content() {
        let c = coord();
        c.buffer_focus_gained(1, 1);
        c.toggle_buffer();
        c.route_commit_to_buffer(insert("留着"));
        c.toggle_buffer();
        assert!(!c.buffer.is_open());
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("直上"))), "直上");
        c.toggle_buffer();
        assert_eq!(c.buffer.lock().buf.text(), "留着");
    }

    #[test]
    fn password_field_refuses_to_bind() {
        let c = coord();
        crate::moodflow::TEST_PRIVACY.with(|p| {
            p.set(Some(crate::moodflow::InputPrivacy {
                private_field: true,
                ..Default::default()
            }))
        });
        c.buffer_focus_gained(1, 1);
        c.toggle_buffer();
        assert!(!c.buffer.is_open());
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("mima"))), "mima");
    }

    fn receipt(delivery: &Delivery, outcome: ReceiptOutcome) -> Receipt {
        Receipt {
            id: delivery.id,
            client: delivery.client,
            revision: delivery.revision,
            outcome,
        }
    }

    fn ready() -> std::sync::Arc<Coordinator> {
        let c = coord();
        c.buffer_focus_gained(42, 7);
        c.toggle_buffer();
        c.route_commit_to_buffer(insert("保留内容"));
        c
    }

    #[test]
    fn same_instance_new_field_revokes_pending_authorization() {
        let c = ready();
        let delivery = c.buffer.begin_all().unwrap();
        c.buffer_focus_gained(42, 8);
        assert!(!c.buffer_authorize(delivery.id, 42, 7));
        assert!(
            !c.buffer
                .finish(receipt(&delivery, ReceiptOutcome::Accepted))
        );
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert_eq!(c.buffer.lock().buf.state(), State::Held);
        c.toggle_buffer();
        assert!(matches!(c.buffer.begin_all(), Err(DeliverError::Held)));
        c.run_buffer_menu_cmd(MenuCmd::BufferReleaseHold);
        assert_eq!(c.buffer.begin_all().unwrap().revision, 8);
    }

    #[test]
    fn rejected_is_retryable_but_timeout_requires_explicit_confirmation() {
        let c = ready();
        let first = c.buffer.begin_all().unwrap();
        assert!(c.buffer.finish(receipt(&first, ReceiptOutcome::Rejected)));
        let retry = c.buffer.begin_all().unwrap();
        assert_ne!(first.id, retry.id);
        assert!(c.buffer.expire(retry.id));
        assert!(!c.buffer_authorize(retry.id, retry.client, retry.revision));
        assert!(matches!(c.buffer.begin_all(), Err(DeliverError::Held)));
        assert!(!c.buffer.finish(receipt(&retry, ReceiptOutcome::Accepted)));
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        c.run_buffer_menu_cmd(MenuCmd::BufferReleaseHold);
        assert!(c.buffer.begin_all().is_ok());
    }

    #[test]
    fn wrong_receipts_and_success_without_authorization_cannot_remove_text() {
        let c = ready();
        let delivery = c.buffer.begin_all().unwrap();
        let accepted = receipt(&delivery, ReceiptOutcome::Accepted);
        assert!(!c.buffer.finish(accepted));
        assert!(c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        assert!(!c.buffer.finish(Receipt {
            id: delivery.id + 1,
            ..accepted
        }));
        assert!(!c.buffer.finish(Receipt {
            client: 43,
            ..accepted
        }));
        assert!(!c.buffer.finish(Receipt {
            revision: 8,
            ..accepted
        }));
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert!(c.buffer.finish(accepted));
    }

    #[test]
    fn close_clear_privacy_and_legacy_host_cannot_authorize() {
        for command in [MenuCmd::BufferClose, MenuCmd::BufferClear] {
            let c = ready();
            let delivery = c.buffer.begin_all().unwrap();
            c.run_buffer_menu_cmd(command);
            assert!(!c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        }
        let c = ready();
        let delivery = c.buffer.begin_all().unwrap();
        crate::moodflow::TEST_PRIVACY.with(|p| {
            p.set(Some(crate::moodflow::InputPrivacy {
                incognito: true,
                ..Default::default()
            }))
        });
        assert!(!c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        let c = coord();
        c.buffer_focus_gained(42, 0);
        c.toggle_buffer();
        assert!(!c.buffer.is_open());
    }

    #[test]
    fn disconnected_delivery_does_not_discard_or_hold_text() {
        let c = ready();
        assert!(matches!(c.buffer_send_keyed(), Some(KeyAction::Consumed)));
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert_eq!(c.buffer.lock().buf.state(), State::Ready);
        assert!(c.buffer.lock().pending.is_none());
    }

    #[test]
    fn full_buffer_never_falls_back_to_writing_host() {
        let c = ready();
        c.buffer.lock().buf.clear();
        let full = "字".repeat(CAPACITY_UTF16);
        assert_eq!(text_of(&c.route_commit_to_buffer(insert(&full))), "");
        assert_eq!(text_of(&c.route_commit_to_buffer(insert("越界"))), "");
        assert_eq!(c.buffer.lock().buf.text(), full);
    }

    #[test]
    fn switching_ime_revokes_pending_delivery() {
        use moodflow_bridge::handler::MessageHandler;
        let c = ready();
        let delivery = c.buffer.begin_all().unwrap();
        c.handle_ime_deactivated(0);
        assert!(!c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert!(c.buffer.lock().pending.is_none());
    }

    #[test]
    fn privacy_change_stops_capture_without_erasing_existing_text() {
        let c = ready();
        crate::moodflow::TEST_PRIVACY.with(|p| {
            p.set(Some(crate::moodflow::InputPrivacy {
                incognito: true,
                ..Default::default()
            }))
        });
        assert_eq!(
            text_of(&c.route_commit_to_buffer(insert("私密文字"))),
            "私密文字"
        );
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert_eq!(c.buffer.lock().buf.state(), State::Suspended);
    }

    fn key(vk: u32, modifiers: u32) -> moodflow_bridge::handler::KeyEventData {
        moodflow_bridge::handler::KeyEventData {
            key_code: vk,
            scan_code: 0,
            modifiers,
            event_type: moodflow_ipc::protocol::EVENT_KEY_DOWN,
            toggles: 0,
            event_seq: 0,
            prev_char: 0,
        }
    }

    #[test]
    fn cancel_keeps_physical_press_latched_and_rejects_late_authorization() {
        use moodflow_keys::keymap::VK_RETURN;
        let c = ready();
        let data = key(VK_RETURN, 0);
        assert!(matches!(
            c.buffer_send_pressed(&data),
            Some(KeyAction::Consumed)
        ));
        assert!(c.buffer.lock().sequence.is_some());
        let next_delivery = c.buffer.next_delivery.load(Ordering::Relaxed);
        assert!(matches!(
            c.buffer_send_pressed(&data),
            Some(KeyAction::Consumed)
        ));
        assert_eq!(
            c.buffer.next_delivery.load(Ordering::Relaxed),
            next_delivery
        );
        let delivery = {
            let mut g = c.buffer.lock();
            c.buffer
                .begin_locked(&mut g, false, Instant::now())
                .unwrap()
        };
        assert!(c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        c.cancel_buffer_sequence();
        assert_eq!(c.buffer.lock().buf.state(), State::Held);
        assert_eq!(c.buffer.lock().buf.text(), "保留内容");
        assert!(!c.buffer_authorize(delivery.id, delivery.client, delivery.revision));
        assert!(
            !c.buffer
                .finish(receipt(&delivery, ReceiptOutcome::Accepted))
        );
        c.buffer_send_pressed(&data);
        assert!(
            c.buffer.lock().sequence.is_none(),
            "repeat must not start a new sequence"
        );
        assert!(c.buffer_send_released(&data).is_some());
        assert!(c.buffer.lock().send_key_down.is_none());
    }

    #[test]
    fn latin_capture_handles_case_space_unicode_and_backspace() {
        use moodflow_ipc::protocol::MOD_SHIFT;
        use moodflow_keys::keymap::*;
        let c = ready();
        c.buffer.lock().buf.clear();
        for (vk, mods, caps) in [
            (VK_A, 0, false),
            (VK_A, MOD_SHIFT, false),
            (VK_A, 0, true),
            (VK_A, MOD_SHIFT, true),
            (VK_SPACE, 0, false),
            (VK_RETURN, 0, false),
        ] {
            assert!(matches!(
                c.buffer_direct_key(&key(vk, mods), false, caps, false),
                Some(KeyAction::Consumed)
            ));
        }
        assert_eq!(c.buffer.lock().buf.text(), "aAAa \n");
        c.buffer_direct_key(&key(VK_BACK, 0), true, false, false);
        c.buffer_direct_key(&key(VK_BACK, 0), true, false, false);
        assert_eq!(c.buffer.lock().buf.text(), "aAAa");
        c.route_commit_to_buffer(insert("中文🙂"));
        c.buffer_direct_key(&key(VK_BACK, 0), true, false, false);
        assert_eq!(c.buffer.lock().buf.text(), "aAAa");
        assert!(
            c.buffer_direct_key(&key(VK_A, 0), true, false, false)
                .is_none()
        );
    }

    #[test]
    fn field_revision_controls_capture_and_mode_generation() {
        use moodflow_keys::keymap::*;
        let c = ready();
        let first = c.buffer.mode().0;
        assert_eq!(first.revision, 7);
        c.buffer_focus_gained(first.client, 10);
        let second = c.buffer.mode().0;
        assert_eq!(second.revision, 0);
        assert!(second.generation > first.generation);
        assert!(
            c.buffer_direct_key(&key(VK_A, 0), false, false, false)
                .is_none()
        );
        c.toggle_buffer();
        assert_eq!(c.buffer.mode().0.revision, 10);
    }

    #[test]
    fn english_keys_cross_the_policed_entry_without_host_text() {
        use moodflow_bridge::handler::MessageHandler;
        use moodflow_keys::keymap::*;
        let c = ready();
        c.buffer.lock().buf.clear();
        {
            let mut state = c.state.lock().unwrap();
            state.ime_active = true;
            state.has_edit_context = true;
            state.chinese_mode = false;
        }
        assert!(matches!(
            c.handle_key_event_policed(&key(VK_A, 0)),
            KeyAction::Consumed
        ));
        assert_eq!(c.buffer.lock().buf.text(), "a");
        assert!(matches!(
            c.handle_key_event_policed(&key(VK_BACK, 0)),
            KeyAction::Consumed
        ));
        assert!(c.buffer.lock().buf.is_empty());
    }

    #[test]
    fn buffer_routes_paired_text_deferred_composition_and_replacements_locally() {
        let c = ready();
        c.buffer.lock().buf.clear();
        assert_eq!(
            text_of(&c.route_commit_to_buffer(KeyAction::InsertTextWithCursor {
                text: "（）".into(),
                cursor_offset: 1
            })),
            ""
        );
        assert_eq!(
            text_of(&c.route_commit_to_buffer(KeyAction::ReplaceBackward {
                count: 2,
                text: "()".into()
            })),
            ""
        );
        assert_eq!(c.buffer.lock().buf.text(), "()");
        let action = c.route_commit_to_buffer(KeyAction::CommitThenDeferComposition {
            commit_text: "你好".into(),
            deferred_composition: "余码".into(),
            timeout_ms: 10,
        });
        assert!(
            matches!(action, KeyAction::InsertText { text, new_composition: Some(comp), .. } if text.is_empty() && comp == "余码")
        );
        c.push_commit_text("外部结果");
        assert_eq!(c.buffer.lock().buf.text(), "()你好外部结果");
    }
}
