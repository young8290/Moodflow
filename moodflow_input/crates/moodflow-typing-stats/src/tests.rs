use crate::daily::{ACTIVE_GAP_MS, DayStats, History, Key, KeyClass, Recorder};
use crate::practice::{Practice, bounded_edit_distance};

const D1: &str = "2026-10-17";
const D2: &str = "2026-10-18";

#[test]
fn keys_are_counted_by_class_and_only_letters_digits_by_name() {
    let mut r = Recorder::new();
    r.on_key(D1, 0, Key::Letter('N'));
    r.on_key(D1, 100, Key::Letter('i'));
    r.on_key(D1, 200, Key::Digit('3'));
    r.on_key(D1, 300, Key::Select);
    r.on_key(D1, 400, Key::Punct);
    let day = &r.take()[0];
    assert_eq!(day.classes[&KeyClass::Letter], 2);
    assert_eq!(day.classes[&KeyClass::Select], 1);
    assert_eq!(day.keys[&'n'], 1);
    assert_eq!(day.keys[&'3'], 1);
    assert_eq!(day.keys.len(), 3, "标点与选字不记键名");
    assert_eq!(day.total_keys(), 5);
}

#[test]
fn commit_counts_chars_by_kind_and_keeps_no_text() {
    let mut r = Recorder::new();
    r.on_commit(D1, 0, "你好，Moodflow 2026！😀", false);
    let day = &r.take()[0];
    assert_eq!(day.commits, 1);
    assert_eq!(day.cjk_chars, 2);
    assert_eq!(day.latin_chars, 12);
    assert_eq!(day.other_chars, 3, "，！😀；空格不计");
    let json = serde_json::to_string(day).unwrap();
    assert!(!json.contains("你好") && !json.contains("Moodflow"));
}

#[test]
fn private_commits_and_paused_recorder_record_nothing() {
    let mut r = Recorder::new();
    r.on_commit(D1, 0, "mima123", true);
    r.set_paused(true);
    r.on_key(D1, 10, Key::Letter('a'));
    r.on_commit(D1, 20, "练习", false);
    assert!(r.take().is_empty());
}

#[test]
fn active_time_only_counts_short_gaps() {
    let mut r = Recorder::new();
    r.on_key(D1, 0, Key::Letter('a'));
    r.on_key(D1, 4_000, Key::Letter('b'));
    r.on_key(D1, 4_000 + ACTIVE_GAP_MS, Key::Letter('c')); // 恰好 10 秒：计入
    r.on_key(D1, 4_000 + ACTIVE_GAP_MS + 60_000, Key::Letter('d')); // 走开一分钟：不计
    r.on_key(D1, 4_000 + ACTIVE_GAP_MS + 61_000, Key::Letter('e'));
    assert_eq!(r.take()[0].active_ms, 4_000 + ACTIVE_GAP_MS + 1_000);
}

#[test]
fn pausing_breaks_active_time_continuity() {
    let mut r = Recorder::new();
    r.on_key(D1, 0, Key::Letter('a'));
    r.set_paused(true);
    r.set_paused(false);
    r.on_key(D1, 2_000, Key::Letter('b'));
    assert_eq!(r.take()[0].active_ms, 0);
}

#[test]
fn crossing_midnight_splits_days() {
    let mut r = Recorder::new();
    r.on_key(D1, 0, Key::Letter('a'));
    r.on_key(D2, 1_000, Key::Letter('b'));
    let days = r.take();
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].date, D1);
    assert_eq!(days[1].date, D2);
    assert_eq!(days[1].active_ms, 1_000, "跨零点的间隔记到新的一天");
}

#[test]
fn take_resets_counts_but_empty_days_are_dropped() {
    let mut r = Recorder::new();
    r.on_key(D1, 0, Key::Space);
    assert_eq!(r.take().len(), 1);
    assert!(r.take().is_empty());
    r.on_key(D1, 500, Key::Space);
    let day = &r.take()[0];
    assert_eq!(day.total_keys(), 1);
    assert_eq!(day.active_ms, 500, "取走后活跃时长仍然连续");
}

#[test]
fn history_merges_sums_prunes_and_round_trips() {
    let mut h = History {
        keep_days: 2,
        ..History::default()
    };
    for (date, chars) in [("2026-10-16", 1), ("2026-10-17", 2), ("2026-10-17", 3)] {
        let mut d = DayStats::new(date);
        d.cjk_chars = chars;
        d.active_ms = 60_000;
        h.merge_day(&d);
    }
    assert_eq!(h.day("2026-10-17").unwrap().cjk_chars, 5);
    h.merge_day(&DayStats::new("2026-10-18"));
    assert!(h.day("2026-10-16").is_none(), "超出保留天数丢最旧的");
    let sum = h.sum("2026-10-01", "2026-10-31");
    assert_eq!(sum.cjk_chars, 5);
    assert_eq!(sum.active_ms, 120_000);
    assert_eq!(History::from_json(&h.to_json()), h);
    assert_eq!(History::from_json("坏文件"), History::default());
}

#[test]
fn derived_rates() {
    let mut d = DayStats::new(D1);
    d.cjk_chars = 120;
    d.active_ms = 120_000;
    d.classes.insert(KeyClass::Letter, 300);
    assert_eq!(d.chars_per_minute(), 60.0);
    assert_eq!(d.keys_per_char(), 2.5);
    assert_eq!(DayStats::new(D1).chars_per_minute(), 0.0);
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

#[test]
fn bounded_edit_distance_matches_levenshtein_and_caps() {
    assert_eq!(
        bounded_edit_distance(&chars("kitten"), &chars("sitting"), 10),
        3
    );
    assert_eq!(
        bounded_edit_distance(&chars("今天天气好"), &chars("今天气很好"), 10),
        2
    );
    assert_eq!(bounded_edit_distance(&chars(""), &chars("abc"), 10), 3);
    assert_eq!(
        bounded_edit_distance(&chars("abcdef"), &chars("uvwxyz"), 3),
        3
    );
    assert_eq!(bounded_edit_distance(&chars("a"), &chars("abcdefgh"), 4), 4);
    assert_eq!(bounded_edit_distance(&chars("same"), &chars("same"), 1), 0);
}

#[test]
fn practice_scores_speed_and_accuracies() {
    let mut p = Practice::new("今天天气很好", 0);
    p.on_text(1_000, "今天");
    p.on_text(2_000, "今天田"); // 错字
    p.on_text(3_000, "今天"); // 删掉
    p.on_text(4_000, "今天天气很好");
    let s = p.finish(31_000);
    assert_eq!(s.elapsed_ms, 30_000, "从第一次输入算起");
    assert_eq!(s.distance, 0);
    assert_eq!(s.final_accuracy, 1.0);
    // 新增 2 + 1(错) + 4 = 7 个字，6 个同位一致
    assert!((s.process_accuracy - 6.0 / 7.0).abs() < 1e-9);
    assert_eq!(s.effective_cpm, 12.0);
}

#[test]
fn practice_final_accuracy_uses_edit_distance() {
    let mut p = Practice::new("abcd", 0);
    p.on_text(0, "abxd");
    let s = p.finish(60_000);
    assert_eq!(s.distance, 1);
    assert_eq!(s.final_accuracy, 0.75);
    assert_eq!(s.effective_cpm, 3.0);
    let empty = Practice::new("abc", 0).finish(1_000);
    assert_eq!(empty.final_accuracy, 0.0);
    assert_eq!(empty.process_accuracy, 1.0);
}
