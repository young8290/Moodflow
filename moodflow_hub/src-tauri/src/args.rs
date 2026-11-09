//! 启动参数（17 第 2.1 节）：`--background`（核心拉起）、`--open <target>`（打开某个窗口）、
//! `--demo`（演示模式，FR-DMO-03，ADR 0023；只在第一个实例启动时生效）。

use crate::windows::WindowTarget;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LaunchArgs {
    pub background: bool,
    pub open: Option<WindowTarget>,
    pub demo: bool,
}

impl LaunchArgs {
    /// A core/watchdog background wake must not reopen an existing Hub window.
    /// Explicit destinations still apply, including when combined with background.
    pub fn reopen_target(&self) -> Option<WindowTarget> {
        self.open.or_else(|| (!self.background).then_some(WindowTarget::Widget))
    }

    /// 不认识的参数忽略：第二个实例转交参数时，旧版本核心可能带上新版本不再使用的参数。
    pub fn parse<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = LaunchArgs::default();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            match a.as_ref() {
                "--background" => out.background = true,
                "--demo" => out.demo = true,
                "--open" => out.open = it.next().and_then(|t| WindowTarget::from_label(t.as_ref())),
                other => {
                    if let Some(t) = other.strip_prefix("--open=") {
                        out.open = WindowTarget::from_label(t);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_flags_and_ignores_the_rest() {
        assert_eq!(
            LaunchArgs::parse(Vec::<String>::new()),
            LaunchArgs::default()
        );
        let a = LaunchArgs::parse(["--background", "--open", "dashboard", "--future-flag"]);
        assert!(a.background);
        assert!(!a.demo);
        assert!(LaunchArgs::parse(["--demo"]).demo);
        assert_eq!(a.open, Some(WindowTarget::Dashboard));
        assert_eq!(
            LaunchArgs::parse(["--open=chat"]).open,
            Some(WindowTarget::Chat)
        );
        assert_eq!(LaunchArgs::parse(["--open", "nope"]).open, None);
        assert_eq!(LaunchArgs::parse(["--open"]).open, None);
    }

    #[test]
    fn a_background_watchdog_wake_does_not_open_windows() {
        assert_eq!(LaunchArgs::parse(["--background"]).reopen_target(), None);
        assert_eq!(
            LaunchArgs::default().reopen_target(),
            Some(WindowTarget::Widget)
        );
        assert_eq!(
            LaunchArgs::parse(["--background", "--open", "dashboard"]).reopen_target(),
            Some(WindowTarget::Dashboard)
        );
    }
}
