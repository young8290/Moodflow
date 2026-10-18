//! 打字测速（XSIME 吸收计划 §3.3）：练习页把每次输入框内容变化交上来，这里用输入法核心的
//! `moodflow-typing-practice` 计分，与 Android 端同一套口径。纯计算，不读库、不落盘、不出网，
//! 范文与练习文本只在这一次调用里。

use moodflow_typing_practice::Practice;
use serde::{Deserialize, Serialize};
use specta::Type;

/// 练习过程中的一次输入框内容变化。`t_ms` 是从练习开始算起的毫秒数。
#[derive(Debug, Clone, Deserialize, Type)]
pub struct PracticeEvent {
    #[specta(type = specta_typescript::Number)]
    pub t_ms: f64,
    pub text: String,
}

/// 一次练习的成绩（口径见 `moodflow-typing-practice`）。
#[derive(Debug, Clone, Serialize, Type)]
pub struct PracticeScore {
    #[specta(type = specta_typescript::Number)]
    pub elapsed_ms: f64,
    pub target_chars: u32,
    pub typed_chars: u32,
    pub distance: u32,
    /// 0..=1
    #[specta(type = specta_typescript::Number)]
    pub final_accuracy: f64,
    /// 0..=1
    #[specta(type = specta_typescript::Number)]
    pub process_accuracy: f64,
    /// 有效速度，字/分钟
    #[specta(type = specta_typescript::Number)]
    pub effective_cpm: f64,
}

/// 按事件序列给一次练习计分。`finished_ms` 是点「结束」或打完的时刻（同样从练习开始算起）。
#[tauri::command]
#[specta::specta]
pub fn practice_score(
    target: String,
    events: Vec<PracticeEvent>,
    finished_ms: u32,
) -> PracticeScore {
    score(&target, &events, f64::from(finished_ms))
}

fn score(target: &str, events: &[PracticeEvent], finished_ms: f64) -> PracticeScore {
    let ms = |v: f64| {
        if v.is_finite() && v > 0.0 {
            v as u64
        } else {
            0
        }
    };
    let mut p = Practice::new(target, 0);
    for e in events {
        p.on_text(ms(e.t_ms), &e.text);
    }
    let s = p.finish(ms(finished_ms));
    PracticeScore {
        elapsed_ms: s.elapsed_ms as f64,
        target_chars: s.target_chars as u32,
        typed_chars: s.typed_chars as u32,
        distance: s.distance as u32,
        final_accuracy: s.final_accuracy,
        process_accuracy: s.process_accuracy,
        effective_cpm: s.effective_cpm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(t: f64, text: &str) -> PracticeEvent {
        PracticeEvent {
            t_ms: t,
            text: text.into(),
        }
    }

    #[test]
    fn scores_with_the_core_rules() {
        let s = score(
            "今天天气很好",
            &[ev(1000.0, "今天"), ev(2000.0, "今天天气很好")],
            31000.0,
        );
        assert_eq!(s.elapsed_ms, 30000.0);
        assert_eq!(s.final_accuracy, 1.0);
        assert_eq!(s.effective_cpm, 12.0);
    }

    #[test]
    fn garbage_times_do_not_panic() {
        let s = score("ab", &[ev(f64::NAN, "a"), ev(-5.0, "ab")], f64::INFINITY);
        assert_eq!(s.distance, 0);
    }
}
