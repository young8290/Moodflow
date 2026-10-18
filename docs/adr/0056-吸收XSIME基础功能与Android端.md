# 0056 吸收 XSIME 基础功能，新增 Android 端
- 状态：已接受（用户决定方向，2026-10-17；Android 界面移植方式 2026-10-18 定）
- 日期：2026-10-18；提议人：用户（要求把 [XSIME](https://github.com/scholay/xsime) 的基础功能吸收进 Moodflow，
  Windows 与 Android 都支持，同类功能吸收 XSIME 做得更好的地方）；待 A（输入法）、E（产品与质量）事后评审
- 关联：ADR 0054（插件信任模型）、ADR 0055（换 AI 地址须重填密钥）；完整计划与进度见
  [docs/design/xsime-feature-port-plan.md](../design/xsime-feature-port-plan.md)

## 背景
产品书 01 第 6.2 节把「移动端（Android / iOS）」列为 V1.0 明确不包含，第 8 节把 Android 版放在 V2 设想里；
04 FR-IME-01 只要求「继承输入法原有能力」。2026-10-17 用户提出三件事：

1. 吸收 XSIME（Apache-2.0，基于 librime 的多端输入法）的基础功能，Windows 与 Android 都要；
2. 两边都有的功能，吸收 XSIME 做得更好的地方（例如换 AI 地址不带旧密钥、用户库打不开时要提示）；
3. 像 Obsidian 一样可扩展、带插件市场（另见 ADR 0054）。

用户试用第一版 Android APK 后指出：键盘与 XSIME、与 Windows 端差距太大，并明确「可以直接复制 XSIME 的部分代码」。
在「引入 librime」与「保留 Moodflow 自研 Rust 引擎、只移植界面」之间，用户选了后者。

## 决策
1. **引擎不换。** 中文输入仍由 Moodflow 自研引擎（`moodflow-engine` / `moodflow-dict` / `moodflow-store`）完成，
   不集成 librime、不执行 Rime Lua。XSIME 的方案通过**导入转换**进来（Rime ZIP → `.wpkg`）。
2. **首批吸收六项基础功能**：Buffer（上屏前文本工作台）、候选矩阵、并击、剪贴板历史（仅文本与链接）、
   Rime 方案与词库导入、九宫格拼音（Android）。二批：打字测速（已做）、Buffer 上的 AI 生成 / 润色 / 翻译（未做）。
3. **先纯逻辑 crate，再接两个宿主。** 每项功能的状态机、数据格式与失效规则写在零平台依赖的 crate 里，
   必须过 `cargo check-android`；Windows（TSF + `moodflow-ui`）与 Android（`moodflow-mobile` → JNI）只做呈现与投递。
4. **新增 Android 端**，与 Windows 同仓同主线（`moodflow_android/`），进程内直接调用 headless 协调器，
   配置格式三端统一。Android 只做输入法与设置应用，**不含 Hub**（陪伴、感知、AI 对话仍只在 Windows）。
5. **Android 界面允许直接移植 XSIME 代码**（键位几何、键盘绘制、候选栏、Buffer 栏、剪贴板面板、触屏并击、
   配色、设置页导航）。移植文件保留 Apache-2.0 版权头并在 `NOTICE.md` 逐个登记；**不复制 XSIME 的方案与词库数据**
   （雾凇拼音等为 GPL-3.0），词库仍按现有构建时下载机制从上游取得。
6. **XSIME 内置的「飞耀」并击键位**（427 条 + 展开 14 条）可以收录：其出处说明记载为发布者提供、声明不属受版权保护的
   功能性映射，按 XSIME 仓库许可证分发；转格式后放在 `moodflow-mobile/data/chord-builtin.json`，登记见 `NOTICE.md`。
7. **两端共用的新数据走协调器。** 剪贴板历史在两端都经 `UiEvent::ClipboardChanged` → 协调器，存同一种
   `clipboard_history.json`，受同一个 `clipboard.enabled` 开关与同一道隐私闸门约束。
8. **出厂默认偏保守**：剪贴板历史、并击、候选矩阵出厂关；三个新快捷键放标量字段（`keys.clipboard_panel`、
   `keys.toggle_buffer`、`keys.buffer_send`），用户清空即禁用。
9. **明确不做**（**已被 ADR 0057 修订**：除摩斯电码与电音演奏外，本条列的功能全部移入二期计划；librime、GPL 数据、插件原生界面、macOS / Linux 宿主四条规则不变）：XSIME 的 Mailbox（终端 / 对话 / 收件）、截图录屏 OCR、笔记 / 资源 / 密码库、iCloud、电音演奏、
   摩斯电码、LaTeX、文献引用；Android 上 XSIME 的 AI、本机翻译词典、官方插件商店与社区页、宠物轮换动画。
10. **版本与标签**：产品版本以 `docs/VERSION` 为唯一真源；Android 另有 `moodflow_android/VERSION`，主次版本号必须与产品版本一致，
    补丁号可独立递增；标签 `android-vX.Y.Z`，发布时不设为 latest。

## 备选方案与取舍
- **集成 librime**：能直接用 XSIME 的方案与 Lua 扩展，但要并行维护两套引擎、两套用户词库，候选排序规则
  （「排序必须落到 weight」）也无法统一。没有采用。
- **Android 只按契约重写界面、不复制代码**：许可证最干净，但用户试用后认为与 XSIME 差距太大；Apache-2.0 代码可以进 MIT 项目，
  只要保留版权头并登记。没有采用。
- **Android 独立仓库**：XSIME 的做法是单仓单主线，核心改动与两端接入能在一个 PR 里验证。没有采用独立仓。
- **Android 带 Hub**：陪伴功能依赖桌面进程模型（命名管道、桌宠窗口），移植成本远超本期范围。留到以后另行决定。

## 影响
- 新 crate：`moodflow-buffer`、`moodflow-chord`、`moodflow-clipboard`、`moodflow-typing-practice`、`moodflow-manifest`；
  扩展：`moodflow-candidate::matrix`、`moodflow-engine::pinyin::t9`、`moodflow-transfer::rime_import`、`moodflow-mobile`；
  新目录 `moodflow_android/`（Gradle 工程 + JNI 绑定 crate），CI 新增 `android.yml`。
- 新配置：`[clipboard]`、`[chord]`、`ui.candidate.matrix`、`keys.clipboard_panel` / `toggle_buffer` / `buffer_send`。
- Hub：看板新增「打字测速」页（命令 `practice_score`），练习时核心不记日常统计。
- 产品书（V1.6 采纳）：00 修订记录与术语；01 第 6、8 节（范围与版本规划，Android 由 V2 设想提前为 V1.x 的输入法预览）；
  03 组件视图与部署；04 新增 FR-IME-03 起的输入扩展需求；07 打字测速页与 Android 设置应用；09 剪贴板历史与插件数据；
  10 新配置键与命令；12 / 18 测试用例；13 里程碑与风险；14 Android 开发环境；16 任务清单；19 用户手册；20 Android 发版；
  21 隐私说明补剪贴板历史。
- 验收：代码通过、真机可用、可发版分开记录。截至 2026-10-18，Windows 与 Android 的接入都**未在真机验证**，见计划 §9。
