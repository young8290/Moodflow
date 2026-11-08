# Buffer（上屏前的文本工作台）

> 状态（2026-10-18）：P1 纯逻辑已实现（`moodflow_input/crates/moodflow-buffer`，27 个单测）；Windows 已接入协调器
> （`handle_buffer`）与浮窗（`moodflow-ui::buffer_panel`），在 `main`；Android 门面（`moodflow-mobile::buffer`）在 `main`，
> 键盘内 Buffer 栏换成移植自 XSIME 的 `BufferRail`（PR #100）。两端都**未真机验证**。
> 来源：[XSIME 吸收计划](xsime-feature-port-plan.md) §2.1；行为契约取自 XSIME 在 macOS / Windows / Android 上验证过的规则。

## 1. 是什么

开启 Buffer 后，确认的候选**先进 Buffer 而不是宿主**。用户在 Buffer 里攒好、改好，再按块投递到绑定的输入框。
它是二批 AI 生成、润色、翻译的地基：这些都作用于 Buffer 里的原文，结果经用户确认后再投递。
在插件体系里，它们落为 `BufferAction` 插件（[插件系统设计](plugin-system.md) §3）。

## 2. 契约

| # | 规则 | crate 里的落点 |
|---|---|---|
| 1 | 每次确认的候选是一个块；连续直输的英文合成一块 | `push_phrase` / `push_latin` |
| 2 | 退格：英文逐字删，候选整块删 | `backspace` |
| 3 | 投递授权绑定目标（输入框 ID + 修订号），**显式绑定**，不自动跟随焦点 | `bind` / `focus_changed` |
| 4 | 换框、失焦、隐藏面板、切输入法撤销授权；在途投递作废，旧内容绝不落到新目标 | `focus_changed` / `suspend` / `is_authorized` |
| 5 | 宿主拒收：保留可重试，不算已消费 | `finish(Rejected)` |
| 6 | 结果不明：保留并**暂停投递**，不自动重发（防重复上屏），用户确认后继续 | `finish(Unknown)` / `release_hold` |
| 7 | 发送键：有组字时只确认组字，同一次按键不顺带发送 | `send_key_action` |
| 8 | 「全部投递」按下时冻结队列，之后追加的块不在本次内 | `begin_all` |
| 9 | 块之间的视觉间隔不变成空格 | `Delivery::text` 直接拼接 |
| 10 | 密码框与私密字段禁用 | `bind(.., Private)` 拒绝并撤销绑定 |
| 11 | 容量上限 16,384 个 UTF-16 码元，超限整段拒收不截断 | `check_capacity` |
| 12 | 正文是临时数据，升级、重启、重置直接丢弃 | `discard` |
| 13 | 投递中的块不能被编辑；新输入另起一块 | `check_last_editable` / `push_latin` |

投递是两段式：`begin_*` 取出文本并锁住块 → 宿主写入前 `is_authorized` 复核 → 写完 `finish` 报告结果。

## 3. 两端如何接入（P2 / P3）

| | Windows（TSF） | Android（InputMethodService） |
|---|---|---|
| `Target.id` | TSF 文档/上下文身份 | `EditorInfo` 的字段身份 |
| `Target.revision` | 上下文重建时递增 | 每次 `onStartInput` 递增 |
| 绑定 | 在宿主输入框按绑定快捷键（XSIME 默认 Alt+Shift+B） | 打开键盘内 Buffer 栏时绑定当前字段 |
| 失焦时 | `suspend`：保留内容，需重新绑定 | `discard`：换字段/隐藏键盘即清空（与 XSIME Android 一致） |
| 写入 | edit session 内先 `is_authorized` 再写 | `commitText` 前 `is_authorized`，返回值映射为 Accepted / Rejected |
| 结果不明 | 写入异常且无法确认 → `Unknown` | 连接已断开 → `Unknown` |
| 私密字段 | 输入范围为密码 | `TYPE_TEXT_VARIATION_PASSWORD` 等 |

发送键、绑定键的出厂值放标量配置字段（如 `keys.toggle_buffer`、`keys.buffer_send`），不放 `keys.key_actions`，
保证用户删得掉（AGENTS.md「出厂预置的绑定/开关」）。

## 4. Windows 接入现状（2026-11-09）

- 绑定键 `keys.toggle_buffer`（出厂 Alt+Shift+B）显式绑定客户端令牌与 TSF 焦点修订号。换框、失焦、切换输入法或关闭 Buffer 撤销授权。修订号为 0 的旧宿主不允许绑定。
- 发送键 `keys.buffer_send`（出厂 Ctrl+Shift+Enter）冻结当前内容。有组字时仅确认组字；下一次发送操作才投递。
- `buffer.deliver` 精确发往绑定实例。TSF 编辑会话持有原始上下文，在实际写入前通过 `buffer.authorize` 单次复核，调用 `InsertTextAtSelection` 后才发 `buffer.receipt`。没有全局模拟输入兜底。
- Accepted 回执才移除冻结块；Rejected 保留；Unknown 或五秒超时保留并暂停。右键“我已确认上次没有写入，允许重试”由用户解除暂停。发送期间新输入另起块。
- `buffer.mode` 带客户端、修订号和单调代数；同步热键回包与异步状态推送共用，旧帧不能覆盖新状态。TSF 两处键判定共用同一捕获门，英文直输和空组字退格也送到协调器。
- 英文按 Shift/CapsLock 组合生成字符并连续成块；退格逐字删除英文，整块删除已确认候选。满容量整次拒收，状态栏明确提示。
- 截获覆盖普通提交、带光标提交、余码组合重开、智能符号后缀替换和异步命令提交。Buffer 内智能符号使用本地后缀替换，避免由宿主定时提交预览文字。
- 插件变换及宿主换行转换在最终投递时执行一次。正文不写日志、不落盘。

已完成原生 MSVC 构建和逻辑回归；真实桌面、AppContainer、LPAC 的操作验收仍待完成。

## 5. 下一步（Windows）

1. 长按发送逐块投递（默认约 1.2 秒，可配 500–3000 ms）、可选 1/2/3/5 秒节奏投递与完成后关闭。
2. 对真实 TSF 宿主验证拒收、超时、同应用换框、切换输入法及隐私状态变化。
3. 二期资料库、AI/翻译、密码库、截图、工作区等全部保留在交付范围内。Android 当前暂停。

## 6. Android 接入现状（2026-10-18，PR #100）

- 候选栏右侧的 Buffer 开关打开 `BufferRail`，绑定当前字段；确认的候选进 Buffer，宿主不收。
- 换字段、隐藏键盘：丢弃（与 XSIME Android 一致）；同一字段重启输入：暂停，需重新绑定。
- 投递两段式：先 `begin` 拿授权，`commitText` 的返回值映射为 Accepted / Rejected；被拒时保留文本可重试，连接已断开记为 Unknown，不重发。
- 密码类与 `IME_FLAG_NO_PERSONALIZED_LEARNING` 字段禁用 Buffer。
- 仍按宿主侧截获上屏指令接入，没有改走协调器的截获点（§5 第 2 条）。
- **未真机验证**：换字段 / 隐藏键盘清空、密码框禁用、宿主拒收后重试。
