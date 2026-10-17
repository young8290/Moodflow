# Buffer（上屏前的文本工作台）

> 状态：P1 纯逻辑已实现（`moodflow_input/crates/moodflow-buffer`）；Android 门面（`moodflow-mobile::buffer`）与
> 键盘内 Buffer 栏已搭好（未真机验证）；协调器与 Windows 宿主尚未接入。
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

## 4. 下一步

1. 协调器：Buffer 开启时把上屏路由到 `Buffer`；新增 `UiCommand::UpdateBuffer`（`moodflow-ui-types`）。
2. Windows：`moodflow-ui` Buffer 浮窗（不激活、焦点留在宿主）；TSF 投递与授权复核。
3. ~~Android：`moodflow-mobile` 门面方法；`moodflow_android/` 的键盘内 Buffer 栏。~~ 已搭好，待真机验收
   （Android 上 Buffer 直接截获宿主侧的上屏指令，不经协调器路由；协调器接入后改走第 1 条）。
4. 长按发送逐块投递（默认约 1.2 秒，可配 500–3000 ms）、可选 1/2/3/5 秒节奏投递：在协调器里用 `begin_next` 循环实现，本 crate 不管计时。
