//! 并击结算：把按下/松开事件归成「一组键」，全部松开时查表出片段。

use std::collections::{BTreeSet, HashMap};

use crate::keymap::{Combo, Hand, Keymap, Mapping, OutputKind};

/// 结算结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordEvent {
    /// 查到映射：把 `output` 送进拼音引擎。
    Emit { output: String, kind: OutputKind },
    /// 这组键没有映射：不出字（界面可提示）。
    NoMatch { keys: Vec<String> },
}

/// 并击结算器。一个输入会话一个；换焦点或切方案时 [`Settler::reset`]。
#[derive(Debug, Clone)]
pub struct Settler {
    hands: HashMap<String, Hand>,
    index: HashMap<Combo, Mapping>,
    interval_ms: u64,
    /// 当前按住的键。
    held: BTreeSet<String>,
    /// 本批按下过的键（松开不移出）。
    batch: BTreeSet<String>,
    /// 先敲完的一只手，等另一只手。
    pending: Option<Pending>,
}

#[derive(Debug, Clone)]
struct Pending {
    keys: BTreeSet<String>,
    hand: Hand,
    released_at: u64,
}

impl Settler {
    /// 用已通过校验的方案构造。
    pub fn new(map: &Keymap) -> Self {
        let mut hands = HashMap::new();
        for k in &map.left_keys {
            hands.insert(k.clone(), Hand::Left);
        }
        for k in &map.right_keys {
            hands.insert(k.clone(), Hand::Right);
        }
        Self {
            hands,
            index: map.index(),
            interval_ms: map.interval_ms,
            held: BTreeSet::new(),
            batch: BTreeSet::new(),
            pending: None,
        }
    }

    /// 该键是否属于并击键区（不属于的键由普通路径处理）。
    pub fn is_chord_key(&self, key: &str) -> bool {
        self.hands.contains_key(key)
    }

    /// 是否有未结算的输入（界面据此显示左右两半的预览）。
    pub fn is_busy(&self) -> bool {
        !self.batch.is_empty() || self.pending.is_some()
    }

    /// 换焦点、切方案、取消：丢弃未结算的一切。
    pub fn reset(&mut self) {
        self.held.clear();
        self.batch.clear();
        self.pending = None;
    }

    /// 按下。返回因此被结算掉的先前输入（等待另一只手时又按了同一只手）。
    pub fn key_down(&mut self, key: &str, now_ms: u64) -> Option<ChordEvent> {
        if !self.is_chord_key(key) {
            return None;
        }
        let mut flushed = self.expire(now_ms);
        if self.batch.is_empty()
            && let Some(p) = &self.pending
            && self.hands.get(key) == Some(&p.hand)
        {
            // 等另一只手时又落下同一只手：先把等待中的那半单独结算。
            let p = self.pending.take().expect("checked");
            flushed = Some(self.lookup(&p.keys));
        }
        self.held.insert(key.to_owned());
        self.batch.insert(key.to_owned());
        flushed
    }

    /// 松开。全部松开时结算本批，或进入「等另一只手」。
    pub fn key_up(&mut self, key: &str, now_ms: u64) -> Option<ChordEvent> {
        if !self.held.remove(key) || !self.held.is_empty() {
            return None;
        }
        let batch = std::mem::take(&mut self.batch);
        let hands: BTreeSet<Hand> = batch
            .iter()
            .filter_map(|k| self.hands.get(k).copied())
            .collect();
        if let Some(p) = self.pending.take() {
            // 另一只手到了：两半合成一组。
            let mut keys = p.keys;
            keys.extend(batch);
            return Some(self.lookup(&keys));
        }
        if hands.len() == 1 && self.prefix_of_longer(&batch) {
            // 单手，且是某个更长组合的一半：可能是「先左后右」的前半，等另一只手；
            // 超时后按单手结算（自己有映射就出，没有就报无匹配）。
            let hand = *hands.iter().next().expect("len 1");
            self.pending = Some(Pending {
                keys: batch,
                hand,
                released_at: now_ms,
            });
            return None;
        }
        Some(self.lookup(&batch))
    }

    /// 时间推进（宿主定时调用或在下一次按键前调用）：等另一只手超时则单独结算。
    pub fn tick(&mut self, now_ms: u64) -> Option<ChordEvent> {
        self.expire(now_ms)
    }

    fn expire(&mut self, now_ms: u64) -> Option<ChordEvent> {
        let p = self.pending.as_ref()?;
        if now_ms.saturating_sub(p.released_at) < self.interval_ms {
            return None;
        }
        let p = self.pending.take().expect("checked");
        Some(self.lookup(&p.keys))
    }

    fn prefix_of_longer(&self, keys: &Combo) -> bool {
        self.index
            .keys()
            .any(|c| c.len() > keys.len() && keys.is_subset(c))
    }

    fn lookup(&self, keys: &Combo) -> ChordEvent {
        match self.index.get(keys) {
            Some(m) => ChordEvent::Emit {
                output: m.output.clone(),
                kind: m.kind,
            },
            None => ChordEvent::NoMatch {
                keys: keys.iter().cloned().collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Mapping;

    fn map() -> Keymap {
        Keymap {
            version: 1,
            name: "t".into(),
            left_keys: vec!["a".into(), "s".into(), "d".into()],
            right_keys: vec!["j".into(), "k".into(), "l".into()],
            interval_ms: 300,
            mappings: vec![
                m(&["s", "j"], "zhang", OutputKind::Syllable),
                m(&["d"], "de", OutputKind::Syllable),
                m(&["l"], "le", OutputKind::Syllable),
                m(&["a", "k"], "ma", OutputKind::Syllable),
                m(&["a"], "a", OutputKind::Syllable),
            ],
        }
    }

    fn m(keys: &[&str], out: &str, kind: OutputKind) -> Mapping {
        Mapping {
            keys: keys.iter().map(|s| s.to_string()).collect(),
            output: out.into(),
            kind,
        }
    }

    fn emit(s: &str) -> Option<ChordEvent> {
        Some(ChordEvent::Emit {
            output: s.into(),
            kind: OutputKind::Syllable,
        })
    }

    #[test]
    fn simultaneous_batch_settles_on_last_release() {
        let mut s = Settler::new(&map());
        assert_eq!(s.key_down("s", 0), None);
        assert_eq!(s.key_down("j", 10), None);
        assert_eq!(s.key_up("s", 50), None, "还有键按着，不结算");
        assert_eq!(s.key_up("j", 60), emit("zhang"));
        assert!(!s.is_busy());
    }

    #[test]
    fn left_then_right_combines_within_interval() {
        let mut s = Settler::new(&map());
        s.key_down("s", 0);
        assert_eq!(
            s.key_up("s", 20),
            None,
            "单手且是更长组合的一半：等另一只手"
        );
        assert!(s.is_busy());
        s.key_down("j", 150);
        assert_eq!(s.key_up("j", 170), emit("zhang"));
    }

    #[test]
    fn single_hand_combo_that_is_complete_settles_immediately() {
        let mut s = Settler::new(&map());
        s.key_down("d", 0);
        assert_eq!(s.key_up("d", 10), emit("de"));
    }

    #[test]
    fn half_of_a_longer_combo_waits_then_falls_back_after_timeout() {
        let mut s = Settler::new(&map());
        s.key_down("a", 0);
        assert_eq!(s.key_up("a", 10), None, "a 自己有映射，但也是 a+k 的一半");
        assert_eq!(s.tick(200), None);
        assert_eq!(s.tick(310), emit("a"));
    }

    #[test]
    fn half_of_a_longer_combo_combines_when_other_hand_comes() {
        let mut s = Settler::new(&map());
        s.key_down("a", 0);
        s.key_up("a", 10);
        s.key_down("k", 100);
        assert_eq!(s.key_up("k", 120), emit("ma"));
    }

    #[test]
    fn same_hand_again_flushes_the_pending_half() {
        let mut s = Settler::new(&map());
        s.key_down("s", 0);
        s.key_up("s", 10);
        assert_eq!(
            s.key_down("d", 50),
            Some(ChordEvent::NoMatch {
                keys: vec!["s".into()]
            })
        );
        assert_eq!(s.key_up("d", 60), emit("de"));
    }

    #[test]
    fn timeout_is_checked_on_next_key_down() {
        let mut s = Settler::new(&map());
        s.key_down("s", 0);
        s.key_up("s", 10);
        assert_eq!(
            s.key_down("j", 1000),
            Some(ChordEvent::NoMatch {
                keys: vec!["s".into()]
            })
        );
        assert_eq!(
            s.key_up("j", 1010),
            None,
            "j 也是 s+j 的一半，同样等另一只手"
        );
        assert_eq!(
            s.tick(1400),
            Some(ChordEvent::NoMatch {
                keys: vec!["j".into()]
            })
        );
    }

    #[test]
    fn unmapped_two_hand_combo_reports_no_match() {
        let mut s = Settler::new(&map());
        s.key_down("d", 0);
        s.key_down("k", 5);
        s.key_up("d", 20);
        assert_eq!(
            s.key_up("k", 25),
            Some(ChordEvent::NoMatch {
                keys: vec!["d".into(), "k".into()]
            })
        );
    }

    #[test]
    fn non_chord_keys_are_ignored_and_reset_clears_state() {
        let mut s = Settler::new(&map());
        assert!(!s.is_chord_key("q"));
        assert_eq!(s.key_down("q", 0), None);
        s.key_down("s", 0);
        s.reset();
        assert!(!s.is_busy());
        assert_eq!(s.key_up("s", 10), None);
    }
}
