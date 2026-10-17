//! moodflow-buffer：上屏前的文本工作台（纯逻辑，零依赖）。
//!
//! 开启 Buffer 后，确认的候选**先进这里而不是宿主**，用户再按块投递到绑定的输入框。设计与验收见
//! `docs/design/buffer.md`，行为契约取自 XSIME 的 Buffer（见 `docs/design/xsime-feature-port-plan.md` §2.1）。
//!
//! # 这一层管什么、不管什么
//!
//! - **管**：块的增删、目标绑定与投递授权、投递中的块不被改动、宿主拒收/结果不明时的保留、容量上限。
//! - **不管**：按键路由（哪个键是发送键）、界面、真正写进宿主。这些在协调器与各平台宿主里。
//!   本 crate 不知道 TSF 和 Android 的存在：宿主用一个 [`Target`]（目标 ID + 修订号）描述「当前是哪个输入框」。
//!
//! # 投递是两段式的
//!
//! [`Buffer::begin_next`] / [`Buffer::begin_all`] 取出要投递的文本并锁住这些块，宿主写之前用
//! [`Buffer::is_authorized`] 复核授权，写完用 [`Buffer::finish`] 报告结果：
//!
//! | 结果 | 块 | 之后 |
//! |---|---|---|
//! | [`Outcome::Accepted`] | 移除 | 可继续投递 |
//! | [`Outcome::Rejected`] | 保留 | 可重试 |
//! | [`Outcome::Unknown`] | 保留 | **暂停投递**（不自动重发，避免重复上屏），用户确认后 [`Buffer::release_hold`] |
//!
//! 换输入框、失焦、隐藏面板、切输入法都会撤销授权：在途的投递作废，旧内容绝不落到新目标。
//! 恢复投递必须显式重新 [`Buffer::bind`]，不自动跟随焦点。

use std::fmt;

/// 默认容量（UTF-16 码元），与 XSIME 一致。
pub const DEFAULT_CAPACITY_UTF16: usize = 16_384;

/// 宿主眼中的「一个输入框」。
///
/// `id` 区分不同输入框；`revision` 在同一输入框的上下文被重置时递增（如 Android 的 `onStartInput`
/// 重新进入同一字段、TSF 上下文重建）。两者都相等才算同一个目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Target {
    pub id: u64,
    pub revision: u64,
}

impl Target {
    pub const fn new(id: u64, revision: u64) -> Self {
        Self { id, revision }
    }
}

/// 输入框的种类。私密字段（密码框等）一律不启用 Buffer。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Normal,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u64);

/// 块的来源，决定退格怎么删。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// 一次确认的候选（中文词、短语等）：退格整块删除。
    Phrase,
    /// 连续直输的英文：退格逐字删除。
    Latin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    id: BlockId,
    kind: BlockKind,
    text: String,
}

impl Block {
    pub fn id(&self) -> BlockId {
        self.id
    }
    pub fn kind(&self) -> BlockKind {
        self.kind
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// 投递凭据。每次 `begin_*` 发一个新的，作废后再交回来会被拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeliveryToken(u64);

/// 一次待投递的内容。块之间的视觉间隔不会变成空格：`text` 是各块原文直接拼接。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub token: DeliveryToken,
    pub text: String,
    /// 本次包含的块数。
    pub blocks: usize,
}

/// 宿主报告的写入结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected,
    Unknown,
}

/// Buffer 的整体状态，供界面显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 没有绑定目标：可以编辑，不能投递。
    Unbound,
    /// 已绑定且授权有效。
    Ready,
    /// 焦点离开过绑定目标：可以编辑，需重新绑定才能投递。
    Suspended,
    /// 上次投递结果不明：内容保留，需用户确认后才能继续投递。
    Held,
}

/// 「发送键」按下时该做什么。哪个键是发送键由协调器按配置决定，这里只回答语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendKeyAction {
    /// 有组字：只确认组字，同一次按键**不**顺带发送。
    ConfirmComposition,
    /// 无组字且有内容：投递下一块。
    SendNext,
    /// 无组字也无内容：交给宿主。
    PassThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindError {
    PrivateField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    Empty,
    /// 超过容量；整段拒收，不截断。
    CapacityExceeded,
    /// 要改的块正在投递中。
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliverError {
    NotBound,
    Suspended,
    /// 请求方不是绑定的目标。
    TargetMismatch,
    /// 已有投递在途。
    Busy,
    Held,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishError {
    /// 凭据已作废（被撤销、已完成或不是这次发的）。
    Stale,
}

macro_rules! impl_display {
    ($($t:ty),*) => {$(
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(self, f)
            }
        }
        impl std::error::Error for $t {}
    )*};
}
impl_display!(BindError, EditError, DeliverError, FinishError);

#[derive(Debug, Clone)]
struct InFlight {
    token: DeliveryToken,
    /// 在途的块数：恒为 `blocks` 的前缀。
    count: usize,
    target: Target,
}

/// 上屏前的文本工作台。
#[derive(Debug, Clone)]
pub struct Buffer {
    blocks: Vec<Block>,
    capacity_utf16: usize,
    binding: Option<Target>,
    suspended: bool,
    held: bool,
    in_flight: Option<InFlight>,
    next_block: u64,
    next_token: u64,
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY_UTF16)
    }
}

impl Buffer {
    pub fn new(capacity_utf16: usize) -> Self {
        Self {
            blocks: Vec::new(),
            capacity_utf16,
            binding: None,
            suspended: false,
            held: false,
            in_flight: None,
            next_block: 0,
            next_token: 0,
        }
    }

    // ── 状态 ──────────────────────────────────────────────

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn len_utf16(&self) -> usize {
        self.blocks.iter().map(|b| utf16_len(&b.text)).sum()
    }

    pub fn capacity_utf16(&self) -> usize {
        self.capacity_utf16
    }

    /// 当前全部内容（用于复制兜底）。
    pub fn text(&self) -> String {
        self.blocks.iter().map(|b| b.text.as_str()).collect()
    }

    pub fn binding(&self) -> Option<Target> {
        self.binding
    }

    pub fn state(&self) -> State {
        if self.held {
            State::Held
        } else if self.binding.is_none() {
            State::Unbound
        } else if self.suspended {
            State::Suspended
        } else {
            State::Ready
        }
    }

    /// 该块是否正在投递中（界面可据此置灰）。
    pub fn is_in_flight(&self, id: BlockId) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|f| self.blocks[..f.count].iter().any(|b| b.id == id))
    }

    // ── 目标与授权 ────────────────────────────────────────

    /// 显式绑定目标。私密字段拒绝绑定，并撤销原有绑定。
    ///
    /// 绑定会作废在途投递（授权属于新的绑定），但不清除「结果不明」的暂停：
    /// 那需要用户先确认上一次到底有没有写进去。
    pub fn bind(&mut self, target: Target, field: FieldKind) -> Result<(), BindError> {
        self.in_flight = None;
        if field == FieldKind::Private {
            self.binding = None;
            self.suspended = false;
            return Err(BindError::PrivateField);
        }
        self.binding = Some(target);
        self.suspended = false;
        Ok(())
    }

    /// 宿主报告焦点落到了 `target`。离开绑定目标即暂停；回到同一目标也**不**自动恢复。
    pub fn focus_changed(&mut self, target: Target) {
        if self.binding != Some(target) {
            self.suspend();
        }
    }

    /// 撤销授权但保留内容（失焦、隐藏面板、切输入法）。
    pub fn suspend(&mut self) {
        self.in_flight = None;
        if self.binding.is_some() {
            self.suspended = true;
        }
    }

    /// 丢弃全部内容与绑定（进程销毁、维护重启、Android 换字段）。正文是临时数据，不要求用户先保存。
    pub fn discard(&mut self) {
        self.blocks.clear();
        self.binding = None;
        self.suspended = false;
        self.held = false;
        self.in_flight = None;
    }

    /// 用户确认了上次结果不明的投递后，解除暂停。
    pub fn release_hold(&mut self) {
        self.held = false;
    }

    // ── 编辑 ──────────────────────────────────────────────

    /// 追加一次确认的候选，成为独立的一块。
    pub fn push_phrase(&mut self, text: &str) -> Result<BlockId, EditError> {
        if text.is_empty() {
            return Err(EditError::Empty);
        }
        self.check_capacity(utf16_len(text))?;
        Ok(self.push_block(BlockKind::Phrase, text.to_owned()))
    }

    /// 追加一个直输的英文字符：并入末尾的英文块，否则新起一块。
    pub fn push_latin(&mut self, ch: char) -> Result<BlockId, EditError> {
        self.check_capacity(ch.len_utf16())?;
        let in_flight = self.in_flight_count();
        if self.blocks.len() > in_flight
            && let Some(last) = self.blocks.last_mut()
            && last.kind == BlockKind::Latin
        {
            last.text.push(ch);
            return Ok(last.id);
        }
        Ok(self.push_block(BlockKind::Latin, ch.to_string()))
    }

    /// 退格：英文块删末尾一个字符（删空即移除），候选块整块删除。
    pub fn backspace(&mut self) -> Result<(), EditError> {
        self.check_last_editable()?;
        let last = self.blocks.last_mut().expect("checked non-empty");
        if last.kind == BlockKind::Latin {
            last.text.pop();
            if last.text.is_empty() {
                self.blocks.pop();
            }
        } else {
            self.blocks.pop();
        }
        Ok(())
    }

    /// 整块删除最后一块。
    pub fn delete_last_block(&mut self) -> Result<(), EditError> {
        self.check_last_editable()?;
        self.blocks.pop();
        Ok(())
    }

    /// 清空内容，保留绑定。在途投递随之作废。
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.in_flight = None;
        self.held = false;
    }

    /// 发送键的语义，见 [`SendKeyAction`]。
    pub fn send_key_action(&self, composing: bool) -> SendKeyAction {
        if composing {
            SendKeyAction::ConfirmComposition
        } else if self.blocks.len() > self.in_flight_count() {
            SendKeyAction::SendNext
        } else {
            SendKeyAction::PassThrough
        }
    }

    // ── 投递 ──────────────────────────────────────────────

    /// 取出下一块待投递。
    pub fn begin_next(&mut self, target: Target) -> Result<Delivery, DeliverError> {
        self.begin(target, 1)
    }

    /// 取出当前全部内容一次投递（按下时冻结：之后再追加的块不在本次内）。
    pub fn begin_all(&mut self, target: Target) -> Result<Delivery, DeliverError> {
        let n = self.blocks.len();
        self.begin(target, n)
    }

    /// 宿主真正写入前复核：凭据有效、目标仍是绑定目标、授权未被撤销。
    pub fn is_authorized(&self, token: DeliveryToken, target: Target) -> bool {
        self.in_flight
            .as_ref()
            .is_some_and(|f| f.token == token && f.target == target)
            && self.binding == Some(target)
            && !self.suspended
    }

    /// 报告写入结果，见模块文档的表。
    pub fn finish(&mut self, token: DeliveryToken, outcome: Outcome) -> Result<(), FinishError> {
        let flight = match &self.in_flight {
            Some(f) if f.token == token => self.in_flight.take().expect("matched"),
            _ => return Err(FinishError::Stale),
        };
        match outcome {
            Outcome::Accepted => {
                self.blocks.drain(..flight.count);
            }
            Outcome::Rejected => {}
            Outcome::Unknown => self.held = true,
        }
        Ok(())
    }

    // ── 内部 ──────────────────────────────────────────────

    fn begin(&mut self, target: Target, count: usize) -> Result<Delivery, DeliverError> {
        let bound = self.binding.ok_or(DeliverError::NotBound)?;
        if self.held {
            return Err(DeliverError::Held);
        }
        if self.suspended {
            return Err(DeliverError::Suspended);
        }
        if bound != target {
            return Err(DeliverError::TargetMismatch);
        }
        if self.in_flight.is_some() {
            return Err(DeliverError::Busy);
        }
        if self.blocks.is_empty() {
            return Err(DeliverError::Empty);
        }
        let count = count.min(self.blocks.len());
        let text: String = self.blocks[..count]
            .iter()
            .map(|b| b.text.as_str())
            .collect();
        let token = DeliveryToken(self.next_token);
        self.next_token += 1;
        self.in_flight = Some(InFlight {
            token,
            count,
            target,
        });
        Ok(Delivery {
            token,
            text,
            blocks: count,
        })
    }

    fn in_flight_count(&self) -> usize {
        self.in_flight.as_ref().map_or(0, |f| f.count)
    }

    fn check_last_editable(&self) -> Result<(), EditError> {
        if self.blocks.is_empty() {
            return Err(EditError::Empty);
        }
        if self.blocks.len() <= self.in_flight_count() {
            return Err(EditError::Busy);
        }
        Ok(())
    }

    fn check_capacity(&self, extra: usize) -> Result<(), EditError> {
        if self.len_utf16() + extra > self.capacity_utf16 {
            return Err(EditError::CapacityExceeded);
        }
        Ok(())
    }

    fn push_block(&mut self, kind: BlockKind, text: String) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.blocks.push(Block { id, kind, text });
        id
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests;
