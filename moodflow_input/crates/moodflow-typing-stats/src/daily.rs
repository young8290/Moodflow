//! 日常统计：只存计数。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 相邻两次输入事件的间隔不超过这个值才计入活跃时长（XSIME 口径：10 秒）。
pub const ACTIVE_GAP_MS: u64 = 10_000;

/// 默认保留天数。
pub const DEFAULT_KEEP_DAYS: usize = 366;

/// 按键类别。具体键名只对字母与数字记分布，其余只记类别，避免把快捷键习惯之类细节存下来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyClass {
    Letter,
    Digit,
    Space,
    Backspace,
    Enter,
    Punct,
    /// 选候选（数字键、鼠标或触屏点选都算这一类，而不是 Digit）。
    Select,
    Other,
}

/// 调用方报给 [`Recorder::on_key`] 的一次按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// 字母键，大小写不分。
    Letter(char),
    /// 数字键（不是选候选时）。
    Digit(char),
    Space,
    Backspace,
    Enter,
    Punct,
    Select,
    Other,
}

impl Key {
    pub fn class(self) -> KeyClass {
        match self {
            Key::Letter(_) => KeyClass::Letter,
            Key::Digit(_) => KeyClass::Digit,
            Key::Space => KeyClass::Space,
            Key::Backspace => KeyClass::Backspace,
            Key::Enter => KeyClass::Enter,
            Key::Punct => KeyClass::Punct,
            Key::Select => KeyClass::Select,
            Key::Other => KeyClass::Other,
        }
    }

    /// 计入分布的键名（只有字母和数字有）。
    fn name(self) -> Option<char> {
        match self {
            Key::Letter(c) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
            Key::Digit(c) if c.is_ascii_digit() => Some(c),
            _ => None,
        }
    }
}

/// 一天的计数。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayStats {
    /// `YYYY-MM-DD`（本地日期）。
    pub date: String,
    /// 各类按键次数。
    #[serde(default)]
    pub classes: BTreeMap<KeyClass, u64>,
    /// 字母与数字键的分布，键为单个字符。
    #[serde(default)]
    pub keys: BTreeMap<char, u64>,
    /// 上屏次数。
    #[serde(default)]
    pub commits: u64,
    /// 上屏的汉字数（CJK 统一表意文字，含扩展区）。
    #[serde(default)]
    pub cjk_chars: u64,
    /// 上屏的拉丁字母与数字数。
    #[serde(default)]
    pub latin_chars: u64,
    /// 其余可见字符数（标点、符号、表情等；空白不计）。
    #[serde(default)]
    pub other_chars: u64,
    /// 活跃时长（毫秒）。
    #[serde(default)]
    pub active_ms: u64,
}

impl DayStats {
    pub fn new(date: &str) -> Self {
        Self {
            date: date.to_owned(),
            ..Self::default()
        }
    }

    /// 按键总数。
    pub fn total_keys(&self) -> u64 {
        self.classes.values().sum()
    }

    /// 成文字数：汉字 + 拉丁 + 其他。
    pub fn total_chars(&self) -> u64 {
        self.cjk_chars + self.latin_chars + self.other_chars
    }

    /// 活跃期间的平均速度（字/分钟）；活跃不足 1 秒返回 0。
    pub fn chars_per_minute(&self) -> f64 {
        if self.active_ms < 1000 {
            return 0.0;
        }
        self.total_chars() as f64 * 60_000.0 / self.active_ms as f64
    }

    /// 每个字平均按键数（码长的粗略指标）；没有上屏返回 0。
    pub fn keys_per_char(&self) -> f64 {
        let chars = self.total_chars();
        if chars == 0 {
            return 0.0;
        }
        self.total_keys() as f64 / chars as f64
    }

    fn merge(&mut self, other: &DayStats) {
        for (k, v) in &other.classes {
            *self.classes.entry(*k).or_default() += v;
        }
        for (k, v) in &other.keys {
            *self.keys.entry(*k).or_default() += v;
        }
        self.commits += other.commits;
        self.cjk_chars += other.cjk_chars;
        self.latin_chars += other.latin_chars;
        self.other_chars += other.other_chars;
        self.active_ms += other.active_ms;
    }
}

/// 字符分类计数，正文不保留。
fn count_chars(text: &str) -> (u64, u64, u64) {
    let (mut cjk, mut latin, mut other) = (0, 0, 0);
    for c in text.chars() {
        if is_cjk(c) {
            cjk += 1;
        } else if c.is_ascii_alphanumeric() || (c.is_alphabetic() && (c as u32) < 0x2000) {
            latin += 1;
        } else if !c.is_whitespace() && !c.is_control() {
            other += 1;
        }
    }
    (cjk, latin, other)
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF   // 扩展 A
        | 0x4E00..=0x9FFF // 基本区
        | 0xF900..=0xFAFF // 兼容表意文字
        | 0x20000..=0x3134F // 扩展 B–H
    )
}

/// 多天统计，按日期排序，超出保留天数时丢最旧的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub days: Vec<DayStats>,
    #[serde(default = "default_keep")]
    pub keep_days: usize,
}

fn default_keep() -> usize {
    DEFAULT_KEEP_DAYS
}

impl Default for History {
    fn default() -> Self {
        Self {
            days: Vec::new(),
            keep_days: DEFAULT_KEEP_DAYS,
        }
    }
}

impl History {
    pub fn day(&self, date: &str) -> Option<&DayStats> {
        self.days.iter().find(|d| d.date == date)
    }

    fn day_mut(&mut self, date: &str) -> &mut DayStats {
        let pos = match self.days.binary_search_by(|d| d.date.as_str().cmp(date)) {
            Ok(i) => i,
            Err(i) => {
                self.days.insert(i, DayStats::new(date));
                i
            }
        };
        &mut self.days[pos]
    }

    fn prune(&mut self) {
        let keep = self.keep_days.max(1);
        if self.days.len() > keep {
            let drop = self.days.len() - keep;
            self.days.drain(..drop);
        }
    }

    /// 把一天的增量并进来（合并而不是覆盖，多个进程/多次落盘不丢数）。
    pub fn merge_day(&mut self, delta: &DayStats) {
        self.day_mut(&delta.date).merge(delta);
        self.prune();
    }

    /// 日期区间 `[from, to]`（含两端）的合计；`date` 字段为 `"from..to"`。
    pub fn sum(&self, from: &str, to: &str) -> DayStats {
        let mut total = DayStats::new(&format!("{from}..{to}"));
        for d in self
            .days
            .iter()
            .filter(|d| d.date.as_str() >= from && d.date.as_str() <= to)
        {
            total.merge(d);
        }
        total
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 读快照；坏文件返回空历史而不是报错（统计丢了可以重来，不能挡输入）。
    pub fn from_json(text: &str) -> Self {
        let mut h: History = serde_json::from_str(text).unwrap_or_default();
        h.days.sort_by(|a, b| a.date.cmp(&b.date));
        h.days.dedup_by(|b, a| {
            if a.date == b.date {
                a.merge(b);
                true
            } else {
                false
            }
        });
        h.prune();
        h
    }
}

/// 采集器：把事件累成当天的增量，调用方定期 [`Recorder::take`] 并进 [`History`] 落盘。
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    current: Option<DayStats>,
    /// 已经结束、还没被取走的天（跨零点时产生）。
    finished: Vec<DayStats>,
    last_event_ms: Option<u64>,
    paused: bool,
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 暂停采集（打字测速期间、用户在设置里关掉统计时）。恢复后第一次事件不与暂停前的事件连成活跃时长。
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        self.last_event_ms = None;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    fn day(&mut self, date: &str, now_ms: u64) -> &mut DayStats {
        if self.current.as_ref().is_some_and(|d| d.date != date) {
            self.finished.extend(self.current.take());
        }
        // 活跃时长：间隔不超过阈值才累计；时钟回拨（now < last）不计。
        if let Some(last) = self.last_event_ms
            && now_ms >= last
            && now_ms - last <= ACTIVE_GAP_MS
        {
            let gap = now_ms - last;
            self.current
                .get_or_insert_with(|| DayStats::new(date))
                .active_ms += gap;
        }
        self.last_event_ms = Some(now_ms);
        self.current.get_or_insert_with(|| DayStats::new(date))
    }

    /// 一次按键。
    pub fn on_key(&mut self, date: &str, now_ms: u64, key: Key) {
        if self.paused {
            return;
        }
        let day = self.day(date, now_ms);
        *day.classes.entry(key.class()).or_default() += 1;
        if let Some(n) = key.name() {
            *day.keys.entry(n).or_default() += 1;
        }
    }

    /// 一次上屏。只数字数，文本不保留；`private` 为真（私密字段等）时整条忽略。
    pub fn on_commit(&mut self, date: &str, now_ms: u64, text: &str, private: bool) {
        if self.paused || private || text.is_empty() {
            return;
        }
        let (cjk, latin, other) = count_chars(text);
        let day = self.day(date, now_ms);
        day.commits += 1;
        day.cjk_chars += cjk;
        day.latin_chars += latin;
        day.other_chars += other;
    }

    /// 取走累计的增量（含跨零点已结束的天），采集器清零但保留活跃时长的连续性。
    pub fn take(&mut self) -> Vec<DayStats> {
        let mut out = std::mem::take(&mut self.finished);
        if let Some(cur) = self.current.take() {
            let date = cur.date.clone();
            out.push(cur);
            self.current = Some(DayStats::new(&date));
        }
        out.retain(|d| *d != DayStats::new(&d.date));
        out
    }
}
