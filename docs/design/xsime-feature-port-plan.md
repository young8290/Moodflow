# 从 XSIME 吸收基础功能：Windows + Android 双平台计划

> 状态：**计划稿**（2026-10-17），尚未开工。参考对象为 [scholay/xsime](https://github.com/scholay/xsime)
> `main`（Apache-2.0，macOS/iOS 为主线，另有 Windows/Android/Linux 端）。
> 本文只回答「拿什么、放哪、按什么顺序做、怎么验收」；每个功能开工时另写各自的设计文档。

## 0. 结论先行

1. **吸收的是功能与交互契约，不是 XSIME 的引擎。** XSIME 的中文输入全部建在 librime 上；
   Moodflow 有自研 Rust 引擎（`moodflow-engine` / `moodflow-dict` / `moodflow-store`）。
   把 librime 拉进来等于并行维护两套引擎、两套用户词库，**不做**。
2. **先进纯逻辑 crate，再接两个宿主。** 每个功能的状态机、数据格式、失效规则写在零平台依赖的
   crate 里，必须过 `cargo check-android`；Windows（TSF + `moodflow-ui`）与 Android
   （`moodflow-mobile` → 平台仓 UniFFI）只做薄薄的呈现与投递。这是本仓「平台分层策略」的
   直接应用（见 AGENTS.md 同名一节）。
3. **首批只拿六项基础功能**（§2），其余 XSIME 能力（Mailbox 终端、截图录屏、音乐、摩斯、
   密码库、iCloud 等）明确不在范围内（§6）。

## 1. XSIME 功能盘点与取舍

XSIME 的结构：librime 引擎 + 三个「槽位」界面（Buffer / Capsule / Mailbox）+ 插件目录。
按「是否是输入法的基础能力」「Moodflow 是否已有」逐项判断：

| XSIME 功能 | 本质 | Moodflow 现状 | 取舍 |
|---|---|---|---|
| **Buffer**（上屏前的文本工作台，按块投递） | 输入核心交互 | 无 | ✅ 首批 |
| **候选矩阵**（`↓` 展开最多 3 行连续页、方向键/数字键导航） | 候选交互 | 有横/竖排与翻页，无矩阵 | ✅ 首批 |
| **并击**（同批组键 / 先左后右，键位方案 JSON 导入导出） | 输入方式 | 无 | ✅ 首批 |
| **剪贴板历史**（Capsule「临时」模块的最小子集） | 周边基础能力 | 只有 `HostServices` 读写剪贴板 | ✅ 首批（仅文本/链接） |
| **跨平台方案导入**（同一份 Rime ZIP / UTF-8 TXT·TSV 词库五端导入） | 数据迁移 | 有 `.wpkg` 方案包，无 Rime 包导入 | ✅ 首批（转换到 `.wpkg`，见 §2.5） |
| **九宫格拼音**（移动端） | 移动端输入方式 | 无（UI 里的「九宫格」是九宫格图片拉伸，与 T9 无关） | ✅ 首批（Android 专属） |
| 学习词库健康检查与恢复提示 | 数据安全 | redb，无专门检查 | 🔶 二批（形态不同：检查 redb 而非 LevelDB） |
| 日常统计 / 打字测速 | 周边 | `coordinator/src/stats.rs` 有采集 | 🔶 二批（扩展既有统计，不另起数据集） |
| Buffer 插件：AI 生成 / 润色 / 实时翻译 | 依赖 Buffer | Hub 已有 AI 服务与温柔改写（ADR 0012/0028） | 🔶 二批，复用 Hub 的 AI 配置 |
| XSIME 五笔（五笔码与拼音免切换） | 方案 | 已有混输引擎 | ⛔ 已覆盖，只对照行为差异 |
| Mailbox（终端 / 对话 / 收件） | 桌面应用 | Hub 有对话 | ⛔ 不做 |
| Capsule 截图、录屏、OCR、笔记、资源、密码、iCloud | 桌面应用 | — | ⛔ 不做 |
| 电音演奏、摩斯电码、LaTeX、文献引用（Scholay） | 趣味/垂直插件 | — | ⛔ 不做 |

## 2. 首批六项：要实现的契约

下面每项列出**从 XSIME 继承的行为契约**（已在其 macOS/Windows/Android 上验证过的规则），
以及在 Moodflow 里的落点。契约是验收依据，实现由我们自己写。

### 2.1 Buffer（缓冲工作台）

契约（取自 XSIME `ARCHITECTURE.md`、Android README、`Shared/Sources/RimesCore/DefaultBuffer.swift` 的行为描述）：

- 开启后，确认的候选**先进 Buffer 而不是宿主**；每次确认的字符串是一个块，英文按词成块。
- 「插入下一块」「插入全部」「删除最后一块」；Return 只在无组字时发送 Buffer，**同一次按键
  不能既确认组字又发送**。
- **投递授权绑定目标**：换输入框、失焦、隐藏键盘/面板、切输入法、进程销毁都会撤销待投递
  内容的授权，旧内容绝不投递到新目标。宿主拒收时保留文本可重试，不算已消费。
- 密码框与私密字段禁用 Buffer。
- 正文是临时数据：升级、重启、重置可直接丢弃，不要求用户保存（XSIME 2026-10-10 维护策略）。
- 容量上限（XSIME 为 16,384 UTF-16 码元）。

落点：

- 新 crate **`moodflow-buffer`**（纯逻辑）：块模型、编辑操作、目标修订号与投递授权、容量上限。
  不知道 TSF/Android 的存在；宿主以「目标 ID + 修订号」描述焦点。
- 协调器：Buffer 开启时把上屏路由改到 buffer；新增 `UiCommand::UpdateBuffer`（在
  `moodflow-ui-types`，遵守其三条红线）。
- 开关：出厂快捷键放**标量字段**（如 `keys.toggle_buffer`），不要放进 `keys.key_actions`
  ——否则用户删不掉（AGENTS.md「出厂预置的绑定/开关」一条）。

### 2.2 候选矩阵

契约（XSIME `CANDIDATE-MATRIX.md`，Windows/Linux 共用 `Shared/CandidateMatrix/navigation.hpp`）：

- 多于一页时 `↓` 展开，视口最多 3 行、每行一页，选中项前进时继续取页。
- 左右在整行内移动；行太长时滚动到选中列，不把候选挤成小格。
- 上下 / PageUp/PageDown / `-` `=` 在行间移动；首行 `↑` 或 Esc 收起并**保留组字**，再按 Esc
  才走普通取消。
- 空格上屏高亮项；1–9 只选当前行可见项；鼠标选择保留真实页号与下标。
- 失焦、上屏、新输入、密码框都会使旧矩阵投影和点击回调失效。

落点：导航状态机放 `moodflow-candidate`（纯逻辑，可单测）；键路由**必须**走现有
`keymap::NavKeys` + `apply_nav_key`，不新写硬编码分支（AGENTS.md「候选导航键」）。
`UpdateCandidates` 增加矩阵行数据；Windows 由 `moodflow-ui` 候选窗画，Android 键盘候选区按宽度排版。

### 2.3 并击

契约（XSIME `CHORD-KEYMAPS.md`）：

- 统一规则同时支持「同批按下」和「先左后右分开敲」；**一组键全部松开时结算**，组键间隔可配。
- 键位方案 = 左右键区 + 音节边界策略 + 映射表（组合 → 完整音节或拼音片段）；
  可复制内置方案、新建、导入/导出 JSON；空映射表可存草稿，不可应用。
- 启用并击扩展**不替换**当前普通方案。

落点：

- 新 crate **`moodflow-chord`**：按键批次结算、键位方案格式与校验、组合 → 拼音片段；输出喂给
  现有拼音引擎，不另起候选路径。
- ⚠ **Windows 风险最大的一项**：并击需要 keydown + keyup。按 AGENTS.md「C++ 吃键集必须 ⊆ Rust
  出字集」，`moodflow_tsf` 在并击方案下吃掉的每个键，Rust 侧必须在同条件下产出；结算时机在
  keyup，要先确认 TSF 现有 keyup 转发机制（目前只用于修饰键轻敲）能扩展到字母键。
- Android 触屏无真正的多键同按，首批只在**物理键盘**上提供并击；触屏并击另议。
- 内置键位数据（XSIME「飞耀」427 条映射）：先确认其许可证再决定是否收录（§5）。

### 2.4 剪贴板历史（Capsule 最小子集）

契约（XSIME Capsule「临时」模块）：最近复制的文本/链接列表，点击选中、回车或双击粘贴；
保存到长期收藏、按类型筛选。首批只做**文本与链接**、只存本机。

落点：

- 纯逻辑放新 crate **`moodflow-clipboard`**：历史条目、去重、保留期限、条数上限、敏感内容排除。
- 平台采集走注入：`HostServices` 现有剪贴板三方法只覆盖读写；「监听变化」属于有事件推送通路
  的平台状态，按分层策略**走事件注入**，不在 trait 里加轮询。
- Windows：`AddClipboardFormatListener`，尊重 `ExcludeClipboardContentFromMonitorProcessing`
  等敏感格式标记（密码管理器会设）。
- Android：10+ 只有**获得焦点的默认输入法**能读剪贴板，因此历史只在键盘可见期间采集；
  遵守 `ClipDescription.EXTRA_IS_SENSITIVE`。
- 存储：redb（`moodflow-store`），**不写日志正文**（日志规范）。

### 2.5 方案与词库导入

XSIME 五端导入同一份标准 Rime ZIP（含 Lua/OpenCC/多方案依赖）。Moodflow 没有 librime，
**不能**完整执行 Rime 方案（Lua、speller 代数规则无法等价），因此目标收窄为：

- **词库导入**：Rime `.dict.yaml`（含 `import_tables`）与 UTF-8 TXT/TSV → Moodflow 用户词库或
  码表层。`moodflow-dict` 已能读 `.dict.yaml`，复用其解析。
- **方案转换**：从 Rime 方案中抽取可等价表达的部分（码表、码长、简码、拼音词库）生成 `.wpkg`，
  无法表达的部分（Lua、复杂 speller、filter）**明确列为诊断项**，不静默丢弃。
- 继承 XSIME 的安全规则：忽略 `__MACOSX`、`build/`、`sync/`、`*.userdb*`，不导入他人设备私有状态；
  同一份 ZIP 两端（Windows/Android）结果一致，以 SHA-256 标识。
- 落点：`moodflow-transfer`（`.wpkg` 格式已在这里），导入后的数据文件一律经覆盖解析函数定位。

### 2.6 九宫格拼音（Android）

契约（XSIME `NineKeyPinyin.swift` 与 Android 端行为）：数字键序列 → 所有可能拼音音节组合 →
候选；选九宫格布局时自动用拼音，选其他中文方案则回到全键盘。

落点：T9 序列到拼音的展开放 `moodflow-engine`（纯逻辑，以现有拼音音节表为准）；
Android 平台仓只做键盘布局。Windows 不提供。

## 3. 双平台分工

| 层 | Windows | Android |
|---|---|---|
| 纯逻辑（共用） | `moodflow-buffer` / `moodflow-chord` / `moodflow-clipboard` / `moodflow-candidate` 矩阵 / `moodflow-transfer` 导入 / `moodflow-engine` T9 | 同左 |
| 协调器接入 | `Coordinator`（`desktop-ui` feature） | headless `Coordinator`，经 `moodflow-mobile` 门面 |
| 呈现 | `moodflow-ui`：Buffer 浮窗、矩阵候选窗、剪贴板历史条 | 平台仓：键盘内 Buffer 栏、候选区矩阵、剪贴板面板、九宫格布局 |
| 投递 | TSF edit session（`moodflow_tsf`） | `InputConnection.commitText`，失败保留 |
| 焦点/目标失效 | TSF 上下文切换、焦点事件 | `onStartInput` / `onFinishInput` / `onWindowHidden` |
| 设置界面 | 邻仓 `moodflow-setting`（新增配置键要跑 `wind_setting_assets` 对账） | 平台仓设置页 |

`moodflow-mobile` 只新增**与绑定框架无关**的能力（Buffer/剪贴板/矩阵的门面方法）；
UniFFI 类型投影仍在平台仓。

## 4. 阶段与验收

每阶段分别记录：源提交、构建命令、测试命令、设备/宿主、通过项、**未验证项**。
代码通过、真机可用、可发版分开验收，不用百分比代替证据（沿用 XSIME 的交付记录习惯）。

| 阶段 | 内容 | 验收 |
|---|---|---|
| **P0 准备**（约 1 周） | 许可证核查（§5）；六项功能各写一页设计文档放 `docs/design/`；确认 TSF keyup 能否扩展到字母键 | 设计文档合入；并击 Windows 可行性有结论 |
| **P1 纯逻辑**（约 3 周） | 四个新/扩展 crate + T9 + 导入转换器，全部带单测 | `cargo test` 全绿且耗时合理（防「数据缺失假绿」）；`cargo check-headless`、`cargo check-android` 通过；CI `cargo tree` 断言不破 |
| **P2 Windows**（约 3 周） | 协调器接入 + `moodflow-ui` 呈现 + TSF 投递；配置键与设置页 | 编译机原生 MSVC 产物（不用交叉产物验收）；真实宿主（记事本、Word、浏览器、企业微信）连续输入；换框/失焦/密码框不串投；`dev.ps1 ci` 通过 |
| **P3 Android**（约 3 周，可与 P2 并行） | `moodflow-mobile` 门面 + 平台仓 UI + 九宫格 | 真机：Buffer 换字段/隐藏键盘清理、密码框禁用、剪贴板仅前台采集、九宫格与全键盘切换、物理键盘并击 |
| **P4 二批** | 词库健康检查、统计/测速扩展、Buffer 上的 AI 生成/润色/翻译（复用 Hub AI 配置） | 另立计划 |

建议顺序：**Buffer → 候选矩阵 → 剪贴板历史 → 导入 → 九宫格 → 并击**。
Buffer 是 AI/翻译二批的地基；并击风险最高（TSF keyup），放最后，不阻塞其他项。

## 5. 许可证与归属

- XSIME 自有代码为 **Apache-2.0**，本仓为 **MIT**。默认做法是**按契约自行实现、不复制代码**。
  确需移植某段代码时：保留 Apache-2.0 版权头，在 `NOTICE.md` 登记来源，并标注修改——
  Apache-2.0 代码可以进 MIT 项目，但那部分仍按 Apache-2.0 分发。
- XSIME 的方案、词库、键位数据各有许可证（雾凇拼音等为 GPL-3.0，见其 `THIRD_PARTY_NOTICES.md`
  与 `rime-data/licenses/`）。**不直接收录其数据文件**；需要的数据回到原始上游按现有构建时下载
  机制处理（同 rime-frost 的做法）。
- 「飞耀」并击键位在收录前单独确认来源与许可。

## 6. 明确不做

- 集成 librime 或执行 Rime Lua。
- Mailbox、截图/录屏/OCR、笔记/资源/密码库、iCloud 同步。
- 电音演奏、摩斯电码、LaTeX、文献引用等插件。
- 对 macOS/Linux 宿主做任何改动（本计划只覆盖 Windows 与 Android；纯逻辑 crate 天然可被
  macOS 复用，但接入不在本期）。

## 7. 待定问题

1. Android 平台仓（MoodflowAndroid）不在本仓，P3 的 UI 工作要在那边开分支；是否需要先把
   `moodflow-mobile` 的接口变更单独发版？
2. 剪贴板历史在 Windows 上是否要做成全局快捷键（跨输入法可用）？XSIME 的做法是唤出时先切到
   自己的输入法，这在 Windows 上需要 TSF 激活配置文件，复杂度高，首批建议只在 Moodflow 为当前
   输入法时可用。
3. 二批的翻译能力：XSIME 只用 Apple 本机翻译，Windows/Android 都没有对等物，需要单独做
   隐私与可用性评估（本机模型 vs. 走 Hub 的 AI 渠道）。
