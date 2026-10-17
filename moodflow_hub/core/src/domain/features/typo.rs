//! R1 打错字（FR-STA-04）：字母 X → 600 ms 内退格 → 退格后 800 ms 内字母 Y，
//! Y ≠ X 且 Y 是 X 在 QWERTY 布局上的相邻键（含对角相邻）。1 秒内最多触发 1 次。
//!
//! R1b 拼音手误：组字中出现切不出来的全拼（核心在 `comp{op:"update"}` 里带 `invalid: true`，
//! ADR 0041）后 600 ms 内退格。与 R1 共用 `typo` 事件和 1 秒去重。

use mfp::KeyKind;

pub const BACKSPACE_WITHIN_MS: u64 = 600;
pub const RETYPE_WITHIN_MS: u64 = 800;
pub const DEDUP_MS: u64 = 1_000;

const ROWS: [&[u8]; 3] = [b"QWERTYUIOP", b"ASDFGHJKL", b"ZXCVBNM"];
/// 每行相对上一行的水平错位（以半键为单位）：标准键盘 A 行右移半键，Z 行再右移一键。
const ROW_OFFSET_HALF: [i32; 3] = [0, 1, 3];

fn pos(vk: u8) -> Option<(i32, i32)> {
    let c = vk.to_ascii_uppercase();
    for (r, row) in ROWS.iter().enumerate() {
        if let Some(i) = row.iter().position(|&k| k == c) {
            return Some((r as i32, i as i32 * 2 + ROW_OFFSET_HALF[r]));
        }
    }
    None
}

/// QWERTY 相邻（同行左右，或上下两行中水平距离不超过一个键宽的键）。
pub fn adjacent(a: u8, b: u8) -> bool {
    let (Some((ra, xa)), Some((rb, xb))) = (pos(a), pos(b)) else {
        return false;
    };
    if a.eq_ignore_ascii_case(&b) {
        return false;
    }
    match (ra - rb).abs() {
        0 => (xa - xb).abs() == 2,
        1 => (xa - xb).abs() <= 2,
        _ => false,
    }
}

#[derive(Debug, Default, Clone)]
pub struct TypoDetector {
    last_letter: Option<(u64, u8)>,
    backspace_after: Option<(u64, u8)>,
    last_fire: Option<u64>,
    /// 最近一次组字更新里有切不出来的全拼（R1b）
    invalid_at: Option<u64>,
}

impl TypoDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 组字更新（R1b）：`invalid` 为真时记下时刻，否则清掉。
    pub fn on_comp(&mut self, ts: u64, invalid: bool) {
        self.invalid_at = invalid.then_some(ts);
    }

    fn fire(&mut self, ts: u64) -> bool {
        if self
            .last_fire
            .is_none_or(|f| ts.saturating_sub(f) >= DEDUP_MS)
        {
            self.last_fire = Some(ts);
            return true;
        }
        false
    }

    /// 输入一个按下事件；命中 R1 或 R1b（且不在 1 秒去重期内）时返回 `true`。
    pub fn on_key(&mut self, ts: u64, kind: KeyKind, vk: Option<u8>) -> bool {
        match (kind, vk) {
            (KeyKind::Backspace, _) => {
                self.backspace_after = match self.last_letter {
                    Some((t, x)) if ts.saturating_sub(t) <= BACKSPACE_WITHIN_MS => Some((ts, x)),
                    _ => None,
                };
                self.last_letter = None;
                let r1b = self
                    .invalid_at
                    .take()
                    .is_some_and(|t| ts.saturating_sub(t) <= BACKSPACE_WITHIN_MS);
                r1b && self.fire(ts)
            }
            (KeyKind::Letter, Some(y)) => {
                let hit = match self.backspace_after.take() {
                    Some((tb, x)) => ts.saturating_sub(tb) <= RETYPE_WITHIN_MS && adjacent(x, y),
                    None => false,
                };
                self.last_letter = Some((ts, y));
                hit && self.fire(ts)
            }
            _ => {
                self.last_letter = None;
                self.backspace_after = None;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacency_table() {
        assert!(adjacent(b'S', b'D'));
        assert!(adjacent(b'S', b'W'));
        assert!(adjacent(b'S', b'E'), "对角相邻");
        assert!(adjacent(b'S', b'Z'));
        assert!(adjacent(b'S', b'X'));
        assert!(!adjacent(b'S', b'S'));
        assert!(!adjacent(b'S', b'K'));
        assert!(!adjacent(b'Q', b'Z'));
        assert!(adjacent(b'G', b'B'));
        assert!(!adjacent(b'Q', b'D'));
    }

    #[test]
    fn letter_backspace_adjacent_letter_is_typo() {
        let mut d = TypoDetector::new();
        assert!(!d.on_key(0, KeyKind::Letter, Some(b'S')));
        assert!(!d.on_key(300, KeyKind::Backspace, Some(8)));
        assert!(d.on_key(600, KeyKind::Letter, Some(b'D')));
    }

    #[test]
    fn slow_backspace_or_non_adjacent_is_not_typo() {
        let mut d = TypoDetector::new();
        d.on_key(0, KeyKind::Letter, Some(b'S'));
        d.on_key(700, KeyKind::Backspace, Some(8));
        assert!(!d.on_key(800, KeyKind::Letter, Some(b'D')));

        let mut d = TypoDetector::new();
        d.on_key(0, KeyKind::Letter, Some(b'S'));
        d.on_key(200, KeyKind::Backspace, Some(8));
        assert!(!d.on_key(400, KeyKind::Letter, Some(b'K')));
    }

    #[test]
    fn backspace_soon_after_invalid_pinyin_is_r1b() {
        let mut d = TypoDetector::new();
        d.on_key(0, KeyKind::Letter, Some(b'I'));
        d.on_comp(5, true);
        assert!(d.on_key(400, KeyKind::Backspace, Some(8)));
        // 太慢、或组字已经合法了都不算
        let mut d = TypoDetector::new();
        d.on_comp(0, true);
        assert!(!d.on_key(700, KeyKind::Backspace, Some(8)));
        let mut d = TypoDetector::new();
        d.on_comp(0, true);
        d.on_comp(100, false);
        assert!(!d.on_key(200, KeyKind::Backspace, Some(8)));
    }

    #[test]
    fn dedup_within_one_second() {
        let mut d = TypoDetector::new();
        d.on_key(0, KeyKind::Letter, Some(b'S'));
        d.on_key(100, KeyKind::Backspace, Some(8));
        assert!(d.on_key(200, KeyKind::Letter, Some(b'D')));
        d.on_key(300, KeyKind::Backspace, Some(8));
        assert!(!d.on_key(400, KeyKind::Letter, Some(b'F')), "1 秒内不重复");
        d.on_key(1200, KeyKind::Letter, Some(b'F'));
        d.on_key(1300, KeyKind::Backspace, Some(8));
        assert!(d.on_key(1400, KeyKind::Letter, Some(b'G')));
    }
}
