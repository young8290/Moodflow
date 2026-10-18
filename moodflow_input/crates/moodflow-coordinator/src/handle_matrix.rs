//! 候选矩阵接入（XSIME 吸收计划 §2.2，导航状态机见 `moodflow_candidate::matrix`）。
//!
//! 只在 `ui.candidate.matrix` 打开、横排、主输入路（无特殊模式）时生效：
//!
//! - 导航键仍经 `keymap::NavKeys` 归一（AGENTS.md「候选导航键」）：高亮下移 → ↓、上移 → ↑、
//!   翻页 → PageDown / PageUp。收起状态下只有 ↓ 会展开；单页时 ↓ 交回普通路径。
//! - 展开后 ←→、空格、1–9、Esc 由 [`Coordinator::matrix_raw_key`] 截住（这些键在矩阵里有专门语义，
//!   收起时完全不碰）。
//! - 绘制：展开时候选窗收到视口里各行（每行一页）拼成的列表和 `SetCandidateGrid { columns }`，
//!   按每行一页排成网格；序号每行从 1 重编。
//! - 候选变化（新输入、上屏、换方案）经 `reset_candidate_view` 收起矩阵，旧投影与点击作废。

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use moodflow_bridge::handler::{KeyAction, KeyEventData};
use moodflow_candidate::matrix::{MatrixNav, MatrixView, NavEffect, NavInput};
use moodflow_keys::keymap;
use moodflow_ui_types::UiCommand;

use crate::coordinator::{Coordinator, State};

const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;

#[derive(Default)]
pub(crate) struct MatrixState {
    nav: Mutex<MatrixNav>,
    /// 上次下发给候选窗的列数（0 = 收起），只在变化时发。
    grid_sent: AtomicUsize,
}

impl Coordinator {
    fn matrix_applies(&self, state: &State) -> bool {
        if !self.rt().config.ui.candidate.matrix || state.active.is_some() {
            return false;
        }
        let o = self.desired_orientation(state);
        !o.vertical && !o.rotated()
    }

    pub(crate) fn matrix_reset(&self) {
        self.matrix
            .nav
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reset();
    }

    /// 当前视口；收起时为 `None`。
    pub(crate) fn matrix_view(&self) -> Option<MatrixView> {
        self.matrix
            .nav
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view()
    }

    /// 让候选列表至少覆盖到第 `pages` 页（矩阵往下走时继续取页）。
    fn matrix_ensure_pages(&self, state: &mut State, pages: usize) {
        let pp = self.per_page(state.active).max(1);
        while state.has_more && state.candidates.len() < pages * pp {
            let before = state.candidates.len();
            self.expand_candidates(state);
            if state.candidates.len() <= before {
                break;
            }
        }
    }

    fn matrix_input(&self, state: &mut State, input: NavInput) -> Option<KeyAction> {
        if !self.matrix_applies(state) || state.candidates.is_empty() {
            return None;
        }
        let pp = self.per_page(state.active).max(1);
        let expanded = self
            .matrix
            .nav
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_expanded();
        // 往下走之前把后面几页取进来（视口 3 行 + 再往下一行的判断）。
        let lookahead = match self.matrix_view() {
            Some(v) => v.rows.last().map_or(0, |r| r.page) + 2,
            None => state.current_page + 2,
        };
        self.matrix_ensure_pages(state, lookahead + 1);
        let len = state.candidates.len();
        let source = |page: usize| {
            let start = page * pp;
            (start < len).then(|| (len - start).min(pp))
        };
        let effect = {
            let mut nav = self.matrix.nav.lock().unwrap_or_else(|e| e.into_inner());
            nav.input(input, state.current_page, state.selected_index, &source)
        };
        match effect {
            NavEffect::NotHandled => {
                // 收起状态下 Esc / 方向键等不归矩阵：普通路径照常处理。
                let _ = expanded;
                None
            }
            NavEffect::Moved | NavEffect::Ignored => {
                self.notify_ui_update(state);
                Some(KeyAction::Consumed)
            }
            NavEffect::Collapsed { page } => {
                state.current_page = page;
                state.selected_index = state.selected_index.min(pp - 1);
                self.notify_ui_update(state);
                Some(KeyAction::Consumed)
            }
            NavEffect::Commit { page, index } => {
                state.current_page = page;
                state.selected_index = index;
                self.matrix_reset();
                self.commit_highlighted(state).or(Some(KeyAction::Consumed))
            }
        }
    }

    /// 导航动作（已经 `NavKeys` 归一）交给矩阵。返回 `None` = 不归矩阵，走普通高亮/翻页。
    pub(crate) fn matrix_nav(
        &self,
        state: &mut State,
        nav: keymap::NavAction,
    ) -> Option<KeyAction> {
        let input = match nav {
            keymap::NavAction::HighlightDown => NavInput::Down,
            keymap::NavAction::HighlightUp => NavInput::Up,
            keymap::NavAction::PageNext => NavInput::PageDown,
            keymap::NavAction::PagePrev => NavInput::PageUp,
        };
        self.matrix_input(state, input)
    }

    /// 展开状态下的 ←→、空格、1–9、Esc。收起时一律返回 `None`，不碰这些键。
    pub(crate) fn matrix_raw_key(
        &self,
        state: &mut State,
        data: &KeyEventData,
    ) -> Option<KeyAction> {
        if data.modifiers != 0 || self.matrix_view().is_none() {
            return None;
        }
        let input = match data.key_code {
            VK_LEFT => NavInput::Left,
            VK_RIGHT => NavInput::Right,
            VK_SPACE => NavInput::Space,
            VK_ESCAPE => NavInput::Escape,
            vk @ 0x31..=0x39 => NavInput::Digit((vk - 0x30) as u8),
            _ => return None,
        };
        self.matrix_input(state, input)
    }

    /// 鼠标点了展开视口里的第 `i` 项（视口内的连续下标）。收起时返回 `None`，走普通点选。
    pub(crate) fn matrix_click(&self, i: usize) -> Option<usize> {
        let view = self.matrix_view()?;
        let pp = {
            let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            self.per_page(s.active).max(1)
        };
        let first = view.rows.first()?.page;
        let (page, index) = (first + i / pp, i % pp);
        let effect = self
            .matrix
            .nav
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .click(page, index);
        match effect {
            NavEffect::Commit { page, index } => Some(page * pp + index),
            _ => None,
        }
    }

    /// 绘制范围：展开时为视口各行拼起来的 `(start, end, 选中项在其中的下标)`，收起时 `None`。
    pub(crate) fn matrix_draw_range(&self, state: &State) -> Option<(usize, usize, usize)> {
        let view = self.matrix_view()?;
        let pp = self.per_page(state.active).max(1);
        let first = view.rows.first()?;
        let last = view.rows.last()?;
        let start = (first.page * pp).min(state.candidates.len());
        let end = (last.page * pp + last.len).min(state.candidates.len());
        Some((start, end, view.active_row * pp + view.column))
    }

    /// 把列数（展开 = 每页候选数，收起 = 0）下发候选窗，只在变化时发。
    pub(crate) fn sync_candidate_grid(&self, state: &State) {
        let want = if self.matrix_view().is_some() {
            self.per_page(state.active).max(1)
        } else {
            0
        };
        if self.matrix.grid_sent.swap(want, Ordering::Relaxed) != want {
            let _ = self
                .ui_tx
                .send(UiCommand::SetCandidateGrid { columns: want });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moodflow_candidate::Candidate;

    fn coord(matrix: bool) -> std::sync::Arc<Coordinator> {
        let mut cfg = moodflow_config::Config::default();
        cfg.ui.candidate.matrix = matrix;
        cfg.ui.candidate.per_page = 3;
        cfg.ui.candidate.layout = "horizontal".into();
        Coordinator::new_headless_with_ui(cfg, None).0
    }

    fn fill(state: &mut State, n: usize) {
        state.input_buffer = "x".into();
        state.candidates = (0..n)
            .map(|i| Candidate {
                text: format!("c{i}"),
                ..Default::default()
            })
            .collect();
        state.current_page = 0;
        state.selected_index = 0;
        state.has_more = false;
    }

    #[test]
    fn disabled_matrix_never_takes_keys() {
        let c = coord(false);
        let mut s = c.state.lock().unwrap();
        fill(&mut s, 9);
        assert!(
            c.matrix_nav(&mut s, keymap::NavAction::HighlightDown)
                .is_none()
        );
    }

    #[test]
    fn down_expands_rows_and_space_commits_the_cell() {
        let c = coord(true);
        let mut s = c.state.lock().unwrap();
        fill(&mut s, 9);
        assert!(matches!(
            c.matrix_nav(&mut s, keymap::NavAction::HighlightDown),
            Some(KeyAction::Consumed)
        ));
        let v = c.matrix_view().expect("已展开");
        assert!(v.rows.len() >= 2);
        // 展开后 → 移一格，↓ 到第二行，范围覆盖视口各行。
        let right = KeyEventData {
            key_code: VK_RIGHT,
            scan_code: 0,
            modifiers: 0,
            event_type: 0,
            toggles: 0,
            event_seq: 0,
            prev_char: 0,
        };
        c.matrix_raw_key(&mut s, &right);
        c.matrix_nav(&mut s, keymap::NavAction::HighlightDown);
        let (start, end, sel) = c.matrix_draw_range(&s).unwrap();
        assert_eq!(start, 0);
        assert!(end >= 6);
        assert_eq!(sel, 3 + 1, "第二行第二个");
        // 空格上屏后矩阵收起。
        let space = KeyEventData {
            key_code: VK_SPACE,
            ..right.clone()
        };
        c.matrix_raw_key(&mut s, &space);
        assert!(c.matrix_view().is_none());
    }

    #[test]
    fn single_page_and_collapsed_raw_keys_fall_through() {
        let c = coord(true);
        let mut s = c.state.lock().unwrap();
        fill(&mut s, 2);
        assert!(
            c.matrix_nav(&mut s, keymap::NavAction::HighlightDown)
                .is_none(),
            "单页时 ↓ 交回普通路径"
        );
        let esc = KeyEventData {
            key_code: VK_ESCAPE,
            scan_code: 0,
            modifiers: 0,
            event_type: 0,
            toggles: 0,
            event_seq: 0,
            prev_char: 0,
        };
        assert!(
            c.matrix_raw_key(&mut s, &esc).is_none(),
            "收起时 Esc 不归矩阵"
        );
    }

    #[test]
    fn escape_collapses_and_keeps_composition() {
        let c = coord(true);
        let mut s = c.state.lock().unwrap();
        fill(&mut s, 9);
        c.matrix_nav(&mut s, keymap::NavAction::HighlightDown);
        let esc = KeyEventData {
            key_code: VK_ESCAPE,
            scan_code: 0,
            modifiers: 0,
            event_type: 0,
            toggles: 0,
            event_seq: 0,
            prev_char: 0,
        };
        assert!(matches!(
            c.matrix_raw_key(&mut s, &esc),
            Some(KeyAction::Consumed)
        ));
        assert!(c.matrix_view().is_none());
        assert_eq!(s.input_buffer, "x", "组字保留");
    }
}
