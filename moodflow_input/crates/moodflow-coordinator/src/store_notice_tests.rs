//! 用户词库打不开时的提示（XSIME 吸收计划 §3.2）。

use moodflow_config::Config;
use moodflow_ui_types::UiCommand;

use crate::Coordinator;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mf-store-notice-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn unopenable_store_is_reported_once_and_left_untouched() {
    let dir = temp_dir("bad");
    // 用一个同名目录占住 userdata.redb，打开必然失败。
    let blocker = dir.join("userdata.redb");
    std::fs::create_dir_all(&blocker).unwrap();
    std::fs::write(blocker.join("keep.txt"), "原样保留").unwrap();

    let (c, rx) = Coordinator::new_headless_with_ui_at(Config::default(), None, Some(&dir));
    assert!(!c.user_store_available());
    assert!(c.notify_store_unavailable());
    let toast = rx.try_iter().find_map(|cmd| match cmd {
        UiCommand::ShowToast { text, .. } if text.contains("用户词库打不开") => Some(text),
        _ => None,
    });
    assert!(toast.is_some(), "应发出提示");
    assert_eq!(
        std::fs::read_to_string(blocker.join("keep.txt")).unwrap(),
        "原样保留",
        "不删除、不重建原库"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn healthy_store_and_no_user_dir_do_not_warn() {
    let dir = temp_dir("ok");
    let (c, _rx) = Coordinator::new_headless_with_ui_at(Config::default(), None, Some(&dir));
    assert!(c.user_store_available());
    assert!(!c.notify_store_unavailable());
    let (c, _rx) = Coordinator::new_headless_with_ui(Config::default(), None);
    assert!(c.user_store_available(), "无用户目录（测试形态）不算失败");
    let _ = std::fs::remove_dir_all(&dir);
}
