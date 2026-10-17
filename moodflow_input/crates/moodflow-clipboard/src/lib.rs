//! moodflow-clipboard：剪贴板历史（纯逻辑）。
//!
//! XSIME Capsule「临时」模块的最小子集（见 `docs/design/xsime-feature-port-plan.md` §2.4）：
//! 最近复制的**文本与链接**，去重、置顶收藏、按保留期与条数清理、搜索。首批只存本机。
//!
//! # 边界
//!
//! - **采集**不在这里：Windows 由剪贴板监听、Android 由键盘可见期间的剪贴板读取产生事件，
//!   宿主把文本与敏感标记交给 [`History::record`]（平台分层：有事件推送通路的状态走注入）。
//! - **落盘**不在这里：[`History::to_json`] / [`History::from_json`] 给出可存的快照，由调用方写入
//!   用户数据目录。**日志里不得出现条目正文**（AGENTS.md 日志规范）。
//! - 时间是 Unix 秒，由调用方给。

use serde::{Deserialize, Serialize};

/// 条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Text,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub kind: EntryKind,
    pub text: String,
    /// 最近一次复制的时间（Unix 秒）。
    pub copied_at: u64,
    /// 收藏的条目不过期、不计入条数上限、不被「清空」删除。
    pub pinned: bool,
}

/// 宿主随文本一起报告的来源信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Source {
    /// 平台标记为敏感（Windows 的排除监听格式、Android 的 `EXTRA_IS_SENSITIVE`）或来自私密字段。
    pub sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    /// 未收藏条目的上限。
    pub max_entries: usize,
    /// 未收藏条目的保留天数；0 表示不按时间清理。
    pub retention_days: u64,
    /// 单条最大字符数，超出不收录（大段文本不是剪贴板历史的用途）。
    pub max_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 200,
            retention_days: 30,
            max_chars: 20_000,
        }
    }
}

/// 一次收录的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recorded {
    New(u64),
    /// 与已有条目相同：移到最前并更新时间。
    Refreshed(u64),
    Skipped(SkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Sensitive,
    Blank,
    TooLong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    limits: Limits,
    /// 最新的在前。
    entries: Vec<Entry>,
    next_id: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(Limits::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "剪贴板历史解析失败（第 {} 行第 {} 列）",
            self.line, self.column
        )
    }
}

impl std::error::Error for LoadError {}

impl History {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: Vec::new(),
            next_id: 1,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// 改上限后立即按新上限清理。
    pub fn set_limits(&mut self, limits: Limits, now: u64) {
        self.limits = limits;
        self.prune(now);
    }

    /// 收录一次复制。
    pub fn record(&mut self, text: &str, source: Source, now: u64) -> Recorded {
        if source.sensitive {
            return Recorded::Skipped(SkipReason::Sensitive);
        }
        if text.trim().is_empty() {
            return Recorded::Skipped(SkipReason::Blank);
        }
        if text.chars().count() > self.limits.max_chars {
            return Recorded::Skipped(SkipReason::TooLong);
        }
        if let Some(pos) = self.entries.iter().position(|e| e.text == text) {
            let mut e = self.entries.remove(pos);
            e.copied_at = now;
            let id = e.id;
            self.entries.insert(0, e);
            return Recorded::Refreshed(id);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.insert(
            0,
            Entry {
                id,
                kind: classify(text),
                text: text.to_owned(),
                copied_at: now,
                pinned: false,
            },
        );
        self.prune(now);
        Recorded::New(id)
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.pinned = pinned;
                true
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// 清空未收藏的条目。
    pub fn clear_unpinned(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// 按保留期与条数清理未收藏条目。宿主在启动和收录时调用即可。
    pub fn prune(&mut self, now: u64) {
        let limits = self.limits;
        if limits.retention_days > 0 {
            let max_age = limits.retention_days * 86_400;
            self.entries
                .retain(|e| e.pinned || now.saturating_sub(e.copied_at) <= max_age);
        }
        let mut unpinned = 0usize;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            unpinned += 1;
            unpinned <= limits.max_entries
        });
    }

    /// 不区分大小写的子串搜索，按新到旧返回。
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q))
            .collect()
    }

    pub fn filter(&self, kind: EntryKind) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("History 只含可序列化的字段")
    }

    /// 读回快照。解析失败只报位置，不回显内容。
    pub fn from_json(json: &str) -> Result<Self, LoadError> {
        let mut h: History = serde_json::from_str(json).map_err(|e| LoadError {
            line: e.line(),
            column: e.column(),
        })?;
        // 防御：快照被手改过时，保证 id 不重复分配。
        let max_id = h.entries.iter().map(|e| e.id).max().unwrap_or(0);
        h.next_id = h.next_id.max(max_id + 1);
        Ok(h)
    }
}

/// 整段是一个 http(s) 链接（允许首尾空白）才算链接。
fn classify(text: &str) -> EntryKind {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let is_url = (lower.starts_with("https://") || lower.starts_with("http://"))
        && !t.chars().any(char::is_whitespace)
        && t.len() > "https://".len();
    if is_url {
        EntryKind::Link
    } else {
        EntryKind::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn rec(h: &mut History, t: &str, now: u64) -> Recorded {
        h.record(t, Source::default(), now)
    }

    #[test]
    fn newest_first_and_links_are_classified() {
        let mut h = History::default();
        rec(&mut h, "你好", 1);
        rec(&mut h, "https://example.com/a?b=1", 2);
        rec(&mut h, "see https://example.com", 3);
        let kinds: Vec<_> = h.entries().iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [EntryKind::Text, EntryKind::Link, EntryKind::Text]);
        assert_eq!(h.entries()[0].text, "see https://example.com");
    }

    #[test]
    fn duplicate_moves_to_top_and_keeps_id() {
        let mut h = History::default();
        let Recorded::New(id) = rec(&mut h, "a", 1) else {
            panic!()
        };
        rec(&mut h, "b", 2);
        assert_eq!(rec(&mut h, "a", 3), Recorded::Refreshed(id));
        assert_eq!(h.entries()[0].text, "a");
        assert_eq!(h.entries()[0].copied_at, 3);
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn skips_sensitive_blank_and_too_long() {
        let mut h = History::new(Limits {
            max_chars: 5,
            ..Limits::default()
        });
        assert_eq!(
            h.record("密码123", Source { sensitive: true }, 1),
            Recorded::Skipped(SkipReason::Sensitive)
        );
        assert_eq!(rec(&mut h, "  \n", 1), Recorded::Skipped(SkipReason::Blank));
        assert_eq!(
            rec(&mut h, "123456", 1),
            Recorded::Skipped(SkipReason::TooLong)
        );
        assert_eq!(
            rec(&mut h, "一二三四五", 1),
            Recorded::New(1),
            "按字符而非字节计"
        );
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn count_limit_drops_oldest_unpinned_only() {
        let mut h = History::new(Limits {
            max_entries: 2,
            ..Limits::default()
        });
        let Recorded::New(first) = rec(&mut h, "1", 1) else {
            panic!()
        };
        h.set_pinned(first, true);
        rec(&mut h, "2", 2);
        rec(&mut h, "3", 3);
        rec(&mut h, "4", 4);
        let texts: Vec<_> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["4", "3", "1"]);
    }

    #[test]
    fn retention_expires_unpinned_entries() {
        let mut h = History::default();
        let Recorded::New(keep) = rec(&mut h, "old pinned", 0) else {
            panic!()
        };
        h.set_pinned(keep, true);
        rec(&mut h, "old", 0);
        rec(&mut h, "fresh", 29 * DAY);
        h.prune(31 * DAY);
        let texts: Vec<_> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["fresh", "old pinned"]);
    }

    #[test]
    fn zero_retention_keeps_by_count_only() {
        let mut h = History::new(Limits {
            retention_days: 0,
            ..Limits::default()
        });
        rec(&mut h, "x", 0);
        h.prune(1000 * DAY);
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn clear_and_remove() {
        let mut h = History::default();
        let Recorded::New(a) = rec(&mut h, "a", 1) else {
            panic!()
        };
        let Recorded::New(b) = rec(&mut h, "b", 2) else {
            panic!()
        };
        h.set_pinned(a, true);
        h.clear_unpinned();
        assert_eq!(h.entries().len(), 1);
        assert!(!h.remove(b));
        assert!(h.remove(a));
        assert!(h.entries().is_empty());
    }

    #[test]
    fn search_is_case_insensitive_and_filter_by_kind() {
        let mut h = History::default();
        rec(&mut h, "Hello World", 1);
        rec(&mut h, "https://moodflow.example", 2);
        assert_eq!(h.search("hello").len(), 1);
        assert_eq!(h.search("").len(), 2);
        assert_eq!(h.filter(EntryKind::Link).len(), 1);
    }

    #[test]
    fn json_round_trip_and_id_safety() {
        let mut h = History::default();
        rec(&mut h, "a", 1);
        rec(&mut h, "b", 2);
        let mut back = History::from_json(&h.to_json()).unwrap();
        assert_eq!(back, h);
        assert_eq!(rec(&mut back, "c", 3), Recorded::New(3));
        assert!(History::from_json("{oops").is_err());
    }

    #[test]
    fn shrinking_limits_prunes_immediately() {
        let mut h = History::default();
        for i in 0..5 {
            rec(&mut h, &i.to_string(), i);
        }
        h.set_limits(
            Limits {
                max_entries: 2,
                ..Limits::default()
            },
            10,
        );
        assert_eq!(h.entries().len(), 2);
    }
}
