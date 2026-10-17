//! XSIME 五笔对照用例（XSIME 吸收计划 §3.5）。
//!
//! XSIME `docs/wubi.md` 里「五笔 86」与「犀术五笔」（五笔拼音混输）的行为，用最小内存词典复刻，
//! 锁住 Moodflow 混输与之一致的部分：
//!
//! 1. 混输下五笔码 `wq` 与全拼 `ni` 都能找到「你」，无需切换模式。
//! 2. 相同字母时，完整五笔匹配排在拼音候选前面。
//! 3. 四码唯一自动上屏只对**五笔词条**生效；拼音候选不触发，长拼音不被四码截断。
//!
//! 「`z` 引导全拼查字并以括号显示五笔码」在 Moodflow 是临时拼音模式 + 编码提示，走协调器，不在本文件。

use moodflow_dict::cached::CachedDict;
use moodflow_dict::codetable::CodetableDict;
use moodflow_dict::{DictManager, SystemDictLayer};
use moodflow_engine::codetable::{CodeTableEngine, CommitOptions};
use moodflow_engine::mixed::{MixConfig, MixedEngine};
use moodflow_engine::pinyin::Config as PinyinConfig;
use moodflow_engine::{Engine, PinyinEngine};
use std::sync::Arc;

fn wubi_engine() -> Box<dyn Engine> {
    let mut d = CodetableDict::empty();
    d.merge_single("wq".into(), "你".into(), 1000, 0);
    d.merge_single("wqvb".into(), "你好".into(), 900, 1);
    // 与拼音 `ai` 同字母的五笔全码
    d.merge_single("ai".into(), "工".into(), 800, 2);
    let dm = DictManager::new();
    dm.register_layer(Box::new(SystemDictLayer::new(CachedDict::Memory(d), "sys")));
    Box::new(CodeTableEngine::new(
        4,
        CommitOptions {
            clear_on_empty_max: true,
            auto_commit_at_full: true,
            ..Default::default()
        },
        Arc::new(dm),
    ))
}

fn pinyin_engine() -> PinyinEngine {
    let mut d = CodetableDict::empty();
    d.merge_single("ni".into(), "你".into(), 1000, 0);
    d.merge_single("hao".into(), "好".into(), 1000, 1);
    d.merge_single("ni hao".into(), "你好".into(), 900, 2);
    d.merge_single("ai".into(), "爱".into(), 1000, 3);
    PinyinEngine::new(PinyinConfig::default(), CachedDict::Memory(d))
}

fn mixed() -> MixedEngine {
    MixedEngine::new(
        wubi_engine(),
        Some(Box::new(pinyin_engine())),
        None,
        MixConfig::default(),
    )
}

fn texts(r: &moodflow_engine::ConvertResult) -> Vec<String> {
    r.candidates.iter().map(|c| c.text.clone()).collect()
}

#[test]
fn wubi_code_and_full_pinyin_both_find_the_same_char() {
    let e = mixed();
    let by_wubi = e.convert("wq", 20).unwrap();
    assert!(
        texts(&by_wubi).contains(&"你".to_string()),
        "{:?}",
        texts(&by_wubi)
    );
    let by_pinyin = e.convert("ni", 20).unwrap();
    assert!(
        texts(&by_pinyin).contains(&"你".to_string()),
        "{:?}",
        texts(&by_pinyin)
    );
}

#[test]
fn exact_wubi_match_ranks_before_pinyin_on_shared_letters() {
    let r = mixed().convert("ai", 20).unwrap();
    let t = texts(&r);
    let wubi = t.iter().position(|x| x == "工").expect("应有五笔「工」");
    if let Some(py) = t.iter().position(|x| x == "爱") {
        assert!(wubi < py, "完整五笔匹配应在拼音前：{t:?}");
    }
}

#[test]
fn unique_four_code_wubi_entry_auto_commits() {
    let r = mixed().convert("wqvb", 20).unwrap();
    assert!(
        r.should_commit,
        "四码唯一五笔词条应自动上屏：{:?}",
        texts(&r)
    );
    assert_eq!(r.commit_text, "你好");
}

#[test]
fn pinyin_never_triggers_four_code_auto_commit_nor_gets_truncated() {
    let e = mixed();
    let four = e.convert("niha", 20).unwrap();
    assert!(!four.should_commit, "四个字母的拼音不触发自动上屏");
    assert!(!four.should_clear, "拼音还没打完，不清空");
    let five = e.convert("nihao", 20).unwrap();
    assert!(
        texts(&five).contains(&"你好".to_string()),
        "长拼音不被四码截断：{:?}",
        texts(&five)
    );
    assert!(!five.should_commit);
}
