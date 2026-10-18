//! 打字测速的移动端门面（XSIME 吸收计划 §3.3）：与 Hub 看板的打字测速页同一套计分
//! （`moodflow-typing-practice`），宿主把练习过程中每次输入框内容变化交进来。
//!
//! 纯计算：不读库、不落盘、不打日志，范文与练习文本只在这一次调用里。

//!
//! 一次性计分用 [`score`]；宿主边打边报（Android 设置页）用 [`Practice`] 本身。

pub use moodflow_typing_practice::{Practice, Score};

/// 按事件序列给一次练习计分。`events` 是 `(从练习开始算起的毫秒, 输入框全文)`，
/// `finished_ms` 是点「结束」或打完的时刻（同样从开始算起）。
pub fn score(target: &str, events: &[(u64, String)], finished_ms: u64) -> Score {
    let mut p = Practice::new(target, 0);
    for (t, text) in events {
        p.on_text(*t, text);
    }
    p.finish(finished_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_with_the_hub_rules() {
        // 与 moodflow_hub 的 practice.rs 同一组数：30 秒打完 6 个字 = 12 字/分
        let s = score(
            "今天天气很好",
            &[(1000, "今天".into()), (2000, "今天天气很好".into())],
            31000,
        );
        assert_eq!(s.elapsed_ms, 30000);
        assert_eq!(s.final_accuracy, 1.0);
        assert_eq!(s.effective_cpm, 12.0);
    }

    #[test]
    fn mistakes_lower_process_accuracy() {
        let s = score("你好", &[(0, "你号".into()), (500, "你好".into())], 1000);
        assert!(s.process_accuracy < 1.0);
        assert_eq!(s.distance, 0);
    }
}
