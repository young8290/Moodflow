//! 候选矩阵导航（纯状态机）。
//!
//! 多于一页候选时按 `↓` 展开：视口最多显示 [`MatrixNav::VIEW_ROWS`] 行，每行是引擎的一页；
//! 选中项往下走时继续取页。契约取自 XSIME `CANDIDATE-MATRIX.md`
//! （见 `docs/design/xsime-feature-port-plan.md` §2.2）：
//!
//! - 左右在整行内移动，到行首/行尾停住，不换行。
//! - 上下 / PageUp / PageDown 在行间移动；首行按上或按 Esc 收起并**保留组字**，回到展开时的那一页。
//!   收起状态下再按 Esc 不由本模块处理，走普通取消。
//! - 空格上屏高亮项；1–9 只选当前行的项；鼠标点击保留真实页号与下标。
//! - 失焦、上屏、新输入、密码框都要调 [`MatrixNav::reset`]，旧的投影与点击作废。
//!
//! 本模块不碰按键码：协调器把按键经 `keymap::NavKeys` 归一成 [`NavInput`] 再交进来（AGENTS.md「候选导航键」）。

/// 按页查询候选数。`None` 表示该页不存在（已超过最后一页）。
pub trait PageSource {
    fn page_len(&self, page: usize) -> Option<usize>;
}

impl<F: Fn(usize) -> Option<usize>> PageSource for F {
    fn page_len(&self, page: usize) -> Option<usize> {
        self(page)
    }
}

/// 归一后的导航输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavInput {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    /// 选择键 1–9（`n` 从 1 起）。
    Digit(u8),
    Space,
    Escape,
}

/// 一次导航的结果，由协调器执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavEffect {
    /// 不归矩阵处理，交回普通路径（如收起状态下的 Esc、单页时的 ↓）。
    NotHandled,
    /// 矩阵状态变了，需要重绘。
    Moved,
    /// 收起矩阵，回到展开时的页并保留组字。
    Collapsed { page: usize },
    /// 上屏引擎第 `page` 页的第 `index` 个候选。
    Commit { page: usize, index: usize },
    /// 按键属于矩阵但当前无事可做（如越界的数字键），吞掉不出字。
    Ignored,
}

/// 矩阵的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixRow {
    /// 引擎页号。
    pub page: usize,
    /// 该页候选数。
    pub len: usize,
}

/// 当前视口，供界面绘制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixView {
    pub rows: Vec<MatrixRow>,
    /// 高亮所在行在 `rows` 中的下标。
    pub active_row: usize,
    /// 高亮列。
    pub column: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MatrixNav {
    expanded: bool,
    origin_page: usize,
    /// 已加载的行，从展开时的页起连续排列。
    rows: Vec<MatrixRow>,
    active: usize,
    column: usize,
    top: usize,
}

impl MatrixNav {
    pub const VIEW_ROWS: usize = 3;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// 失焦、上屏、新输入、密码框：回到收起状态。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 处理一个导航输入。`page` / `column` 是收起状态下当前的页与高亮列（展开时忽略）。
    pub fn input(
        &mut self,
        input: NavInput,
        page: usize,
        column: usize,
        source: &impl PageSource,
    ) -> NavEffect {
        if !self.expanded {
            return match input {
                NavInput::Down => self.expand(page, column, source),
                _ => NavEffect::NotHandled,
            };
        }
        match input {
            NavInput::Down | NavInput::PageDown => self.down(source),
            NavInput::Up | NavInput::PageUp => self.up(),
            NavInput::Left => {
                if self.column > 0 {
                    self.column -= 1;
                    NavEffect::Moved
                } else {
                    NavEffect::Ignored
                }
            }
            NavInput::Right => {
                if self.column + 1 < self.rows[self.active].len {
                    self.column += 1;
                    NavEffect::Moved
                } else {
                    NavEffect::Ignored
                }
            }
            NavInput::Digit(n) => {
                let row = self.rows[self.active];
                let n = usize::from(n);
                if (1..=row.len).contains(&n) {
                    let effect = NavEffect::Commit {
                        page: row.page,
                        index: n - 1,
                    };
                    self.reset();
                    effect
                } else {
                    NavEffect::Ignored
                }
            }
            NavInput::Space => {
                let effect = NavEffect::Commit {
                    page: self.rows[self.active].page,
                    index: self.column,
                };
                self.reset();
                effect
            }
            NavInput::Escape => self.collapse(),
        }
    }

    /// 鼠标点击视口里的某一项。只认当前已加载的行，作废的投影点不中。
    pub fn click(&mut self, page: usize, index: usize) -> NavEffect {
        if !self.expanded {
            return NavEffect::NotHandled;
        }
        match self.rows.iter().find(|r| r.page == page) {
            Some(row) if index < row.len => {
                self.reset();
                NavEffect::Commit { page, index }
            }
            _ => NavEffect::Ignored,
        }
    }

    /// 当前视口；收起时为 `None`。
    pub fn view(&self) -> Option<MatrixView> {
        if !self.expanded {
            return None;
        }
        let end = (self.top + Self::VIEW_ROWS).min(self.rows.len());
        Some(MatrixView {
            rows: self.rows[self.top..end].to_vec(),
            active_row: self.active - self.top,
            column: self.column,
        })
    }

    fn expand(&mut self, page: usize, column: usize, source: &impl PageSource) -> NavEffect {
        let Some(len) = source.page_len(page).filter(|&n| n > 0) else {
            return NavEffect::NotHandled;
        };
        // 只有一页时不展开，↓ 交回普通路径。
        if source.page_len(page + 1).is_none_or(|n| n == 0) {
            return NavEffect::NotHandled;
        }
        self.expanded = true;
        self.origin_page = page;
        self.rows = vec![MatrixRow { page, len }];
        self.active = 0;
        self.top = 0;
        self.column = column.min(len - 1);
        self.fill(source);
        NavEffect::Moved
    }

    fn down(&mut self, source: &impl PageSource) -> NavEffect {
        if self.active + 1 >= self.rows.len() {
            self.load_next(source);
        }
        if self.active + 1 >= self.rows.len() {
            return NavEffect::Ignored;
        }
        self.active += 1;
        self.column = self.column.min(self.rows[self.active].len - 1);
        if self.active >= self.top + Self::VIEW_ROWS {
            self.top = self.active + 1 - Self::VIEW_ROWS;
        }
        self.fill(source);
        NavEffect::Moved
    }

    fn up(&mut self) -> NavEffect {
        if self.active == 0 {
            return self.collapse();
        }
        self.active -= 1;
        self.column = self.column.min(self.rows[self.active].len - 1);
        if self.active < self.top {
            self.top = self.active;
        }
        NavEffect::Moved
    }

    fn collapse(&mut self) -> NavEffect {
        let page = self.origin_page;
        self.reset();
        NavEffect::Collapsed { page }
    }

    /// 让视口尽量填满。
    fn fill(&mut self, source: &impl PageSource) {
        while self.rows.len() < self.top + Self::VIEW_ROWS && self.load_next(source) {}
    }

    fn load_next(&mut self, source: &impl PageSource) -> bool {
        let next = self.rows.last().map_or(self.origin_page, |r| r.page + 1);
        match source.page_len(next) {
            Some(len) if len > 0 => {
                self.rows.push(MatrixRow { page: next, len });
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 页：每页 9 个，最后一页 4 个。
    fn pages(p: usize) -> Option<usize> {
        match p {
            0..=3 => Some(9),
            4 => Some(4),
            _ => None,
        }
    }

    fn single(p: usize) -> Option<usize> {
        (p == 0).then_some(5)
    }

    fn expanded(page: usize, col: usize) -> MatrixNav {
        let mut m = MatrixNav::new();
        assert_eq!(m.input(NavInput::Down, page, col, &pages), NavEffect::Moved);
        m
    }

    #[test]
    fn down_expands_only_with_more_than_one_page() {
        let mut m = MatrixNav::new();
        assert_eq!(
            m.input(NavInput::Down, 0, 0, &single),
            NavEffect::NotHandled
        );
        assert!(!m.is_expanded());
        let m = expanded(0, 2);
        let v = m.view().unwrap();
        assert_eq!(v.rows.len(), 3);
        assert_eq!((v.active_row, v.column), (0, 2));
        assert_eq!(v.rows[0].page, 0);
    }

    #[test]
    fn collapsed_keys_other_than_down_are_not_handled() {
        let mut m = MatrixNav::new();
        for k in [
            NavInput::Up,
            NavInput::Escape,
            NavInput::Space,
            NavInput::Digit(1),
        ] {
            assert_eq!(m.input(k, 0, 0, &pages), NavEffect::NotHandled);
        }
    }

    #[test]
    fn viewport_scrolls_and_loads_pages_as_selection_advances() {
        let mut m = expanded(0, 0);
        for _ in 0..3 {
            assert_eq!(m.input(NavInput::Down, 0, 0, &pages), NavEffect::Moved);
        }
        let v = m.view().unwrap();
        assert_eq!(v.rows.iter().map(|r| r.page).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(v.active_row, 2);
        m.input(NavInput::Down, 0, 0, &pages);
        assert_eq!(
            m.input(NavInput::Down, 0, 0, &pages),
            NavEffect::Ignored,
            "最后一页之后不再动"
        );
    }

    #[test]
    fn column_is_clamped_on_a_shorter_row() {
        let mut m = expanded(3, 8);
        m.input(NavInput::Down, 0, 0, &pages); // 第 4 页只有 4 个
        assert_eq!(m.view().unwrap().column, 3);
    }

    #[test]
    fn left_right_stay_within_the_row() {
        let mut m = expanded(0, 0);
        assert_eq!(m.input(NavInput::Left, 0, 0, &pages), NavEffect::Ignored);
        for _ in 0..8 {
            m.input(NavInput::Right, 0, 0, &pages);
        }
        assert_eq!(m.input(NavInput::Right, 0, 0, &pages), NavEffect::Ignored);
        assert_eq!(m.view().unwrap().column, 8);
    }

    #[test]
    fn up_on_first_row_and_escape_collapse_to_origin_page() {
        let mut m = expanded(2, 0);
        assert_eq!(
            m.input(NavInput::Up, 0, 0, &pages),
            NavEffect::Collapsed { page: 2 }
        );
        assert!(!m.is_expanded());
        let mut m = expanded(2, 0);
        m.input(NavInput::Down, 0, 0, &pages);
        assert_eq!(
            m.input(NavInput::Escape, 0, 0, &pages),
            NavEffect::Collapsed { page: 2 }
        );
        assert_eq!(
            m.input(NavInput::Escape, 2, 0, &pages),
            NavEffect::NotHandled,
            "第二次 Esc 走普通取消"
        );
    }

    #[test]
    fn page_keys_move_between_rows_like_arrows() {
        let mut m = expanded(0, 0);
        m.input(NavInput::PageDown, 0, 0, &pages);
        assert_eq!(m.view().unwrap().active_row, 1);
        m.input(NavInput::PageUp, 0, 0, &pages);
        assert_eq!(
            m.input(NavInput::PageUp, 0, 0, &pages),
            NavEffect::Collapsed { page: 0 }
        );
    }

    #[test]
    fn space_commits_highlight_with_real_page_and_index() {
        let mut m = expanded(1, 4);
        m.input(NavInput::Down, 0, 0, &pages);
        assert_eq!(
            m.input(NavInput::Space, 0, 0, &pages),
            NavEffect::Commit { page: 2, index: 4 }
        );
        assert!(!m.is_expanded());
    }

    #[test]
    fn digits_select_only_in_active_row() {
        let mut m = expanded(3, 0);
        m.input(NavInput::Down, 0, 0, &pages); // 第 4 页 4 个候选
        assert_eq!(
            m.input(NavInput::Digit(5), 0, 0, &pages),
            NavEffect::Ignored
        );
        assert_eq!(
            m.input(NavInput::Digit(4), 0, 0, &pages),
            NavEffect::Commit { page: 4, index: 3 }
        );
    }

    #[test]
    fn click_uses_real_page_and_rejects_stale_targets() {
        let mut m = expanded(0, 0);
        assert_eq!(m.click(9, 0), NavEffect::Ignored);
        assert_eq!(m.click(1, 9), NavEffect::Ignored);
        assert_eq!(m.click(2, 3), NavEffect::Commit { page: 2, index: 3 });
        assert_eq!(m.click(2, 3), NavEffect::NotHandled, "上屏后旧投影作废");
    }

    #[test]
    fn reset_drops_the_projection() {
        let mut m = expanded(0, 0);
        m.reset();
        assert!(m.view().is_none());
    }
}
