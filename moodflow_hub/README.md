# moodflow_hub（Tauri 外壳 + Vue 前端）

- 职责：Hub 的界面与接口层（17 第 2、3 节）。`src-tauri/` 是 Tauri 外壳：单实例、启动参数、窗口管理、命令与事件；领域逻辑全部在 [`core/`](core/README.md)（ADR 0007）。`src/` 是 Vue 3 前端，每个窗口一个入口。
- 窗口：`widget`（小组件）、`cards`（卡片层，贴着小组件，随它一起建、有卡片才显示）、`chat`（对话）、`dashboard`（看板）、`settings`（设置）、`onboarding`（首次引导），尺寸与样式见 `src-tauri/tauri.conf.json`，与 07 第 2 节窗口清单一致；除小组件与卡片层外按需创建。
- 命令与事件：已实现 `get_status`、`state_explain`、`submit_feedback`、`self_report_set`、`self_report_list`、`pause_set`、`settings_get`、`settings_set`、`consent_get`、`consent_set`、`open_window`、`ai_config_get`、`secrets_set`、`ai_test_connection`、`ai_usage_today`、`comfort_feedback`，事件 `status:changed`、`settings:changed`、`self_report:changed`、`gateway:health`、`comfort:new`、`care:reduced`（10 第 5 节）；看板与小组件的读取命令 `comfort_list`、`day_stats`、`month_moods`、`week_stats`，`hotkey_status`、`db_rebuilt_take`、`reminder_missed_take`、`ime_config_defaults`，事件 `mood:typo`、`typing:pulse`（ADR 0036）；删除与导入 `data_delete_all`、`data_delete_mood`、`data_import`、`app_restart`（ADR 0037）。其余命令随各功能任务补上；以 `src-tauri/src/lib.rs` 的 `specta_builder` 为准。窗口之间的界面事件（`mf:` 前缀）只在前端之间传，见 `src/windows/shared/bus.ts`。
- 全局快捷键：设置键 `hotkey.chat` / `hotkey.widget` / `hotkey.dashboard`（默认 `Ctrl+Alt+Q` / `W` / `D`），外壳用 `tauri-plugin-global-shortcut` 注册（`src-tauri/src/hotkeys.rs`），注册不上的在设置“常规”里提示；无痕模式与温柔改写的快捷键在输入法核心。
- 生成的文件（不要手改，CI 会核对）：
  - `src/api/bindings.ts`：命令与事件的 TypeScript 封装，`pnpm gen:bindings`（即 `cargo run -p moodflow-hub --bin export-bindings`）；
  - `src/i18n/zh-CN.ts`：界面文案表，`pnpm gen:i18n`，来源是 `hub_templates/ui_copy.toml` 和 `explain.toml`。窗口标题也要和 `ui_copy.toml` 的 `window.*` 一致。
- 连接输入法：启动后作为 MFP 客户端连接核心（10 第 2 节），Windows 上是命名管道 `moodflow_tap`（调试构建 `moodflow_tap_dev`）；设置环境变量 `MF_MFP_TCP=127.0.0.1:18765` 时改连 `mf-sim --tcp`，任何平台都能这样联调。出厂模板默认从可执行文件旁的 `hub_templates/` 读取，调试构建回退到仓库根目录，也可用 `MF_HUB_TEMPLATES` 指定。调试构建另读 `MF_SIM_BASELINE`（固定基线）和 `MF_SIM_START_AT`（时钟起点），供 `mf-sim --baseline / --start-at` 回放复现（ADR 0021）。
- AI 服务：外壳托管 `Arc<Ai>`（`src-tauri/src/gateway.rs`，本身实现 `AiGateway`）。配置来源按优先级：调试构建的仓库根目录 `secrets.toml`（或 `MF_SECRETS_FILE` 指定）→ 设置页保存的 `secrets.bin`（DPAPI，只在 Windows）→ 调试构建连本机 mock-ai（`MF_JEV_BASE_URL` / `MF_LLM_BASE_URL` 可改）→ 离线（ADR 0012）。
- 数据目录：`%LOCALAPPDATA%\Moodflow\hub\`（调试构建为 `MoodflowDev`）；设置环境变量 `MF_HUB_DATA_DIR` 可指到别处，演示和测试时不碰真实数据。“删除全部数据”在这里留一个 `wipe-pending.json` 并重启，下次启动打开数据库之前删掉数据库文件（ADR 0037）；覆盖导入前的自动备份是 `moodflow.db.bak-<时间>`。
- 图标：`src-tauri/icons/` 由 `python3 scripts/moodflow/gen_icons.py` 从 `assets/brand/icon-1024.png` 生成，换 logo 时改源图后重跑，别手改。

## 开发

需要 Node.js 22 LTS、pnpm 9（`corepack enable`）、Rust stable；Linux 上另需 WebKitGTK（`libwebkit2gtk-4.1-dev` 等，见 CI）。

```bash
pnpm install
pnpm tauri dev            # 热重载开发；首次运行会打开引导窗口
pnpm lint                 # ESLint + Prettier + vue-tsc
pnpm test                 # Vitest（单次运行；边改边测用 pnpm vitest）
pnpm tauri build --no-bundle   # 只出 moodflow_hub.exe，不打安装包（03 第 7 节）
```

不装输入法也能看到状态变化：先启动模拟器，再带环境变量启动 Hub（Windows 上去掉 `--tcp` 和环境变量即走命名管道）：

```bash
cargo run -p mf-sim -- --script tools/mf-sim/scripts/hesitant.jsonl --speed 5 --tcp 127.0.0.1:18765
MF_MFP_TCP=127.0.0.1:18765 pnpm tauri dev
```

只看界面不需要后端时，`pnpm dev` 后在浏览器打开 `http://localhost:1420/widget/index.html` 等页面（命令调用会失败，界面按默认值显示）。

## 约定

- C-10 的晚间小结结束语可用数据目录下 `templates/evening.toml` 整体覆盖。保留正整数 `version` 和七组非空结束语；全部句子须通过出厂禁用词校验。文件缺失或读取、校验失败时使用出厂版本，修改后重启 Hub 生效。调试变体和 `MF_HUB_DATA_DIR` 规则与 C-04 一致；周信与日记模板尚未接入覆盖（ADR 0035）。

- C-04 的用户暖心话可放在 Hub 数据目录的 `templates/comfort.toml`，提示词可放在 `templates/prompts/comfort.md`（正式版 `%LOCALAPPDATA%\Moodflow\hub\templates\`，调试版 `MoodflowDev`）。从出厂文件复制后修改，重启 Hub 生效；同名文件整体替换。暖心话需保留两种风格各六组、唯一 id、6～30 个汉字的句子和简洁兜底；提示词需保留正整数版本号与三个占位符。读取或校验失败时独立回退出厂版本。用户禁用词表不生效，始终使用出厂约束。C-04 的覆盖规则详见 ADR 0034。

- C-10 日记提示词可放在同一 Hub 数据目录的 `templates/prompts/diary.md`，复制出厂文件修改并重启生效。须保留完整的正整数版本标记 `<!-- version: N -->` 和正文中的 `{summary}`、`{chat_digest}`；读取或校验失败回退出厂版本。固定文案 `ui_copy.toml` 仍只认出厂版本，详见 ADR 0047。

- C-03 / FR-AIG-08 的网关指标通过 `commands.aiGatewayMetrics()` 读取：`apis` 是各接口的调用次数、成功率、P50/P95 与熔断状态，`models` 是按配置优先级排列的模型状态。读取不发网络请求，不消耗预算，不含密钥或正文。指标仅在内存中，Hub 重启或更换网关配置后清空；未调用的接口不出现在 `apis`，未知的成功率与延迟为 `null`。设置页“AI 服务”的接口状态表已接入，演示面板待接入（ADR 0033）。

- 前端只显示后端给的状态，不自行推断（17 第 3.2 节）；窗口打开时取快照，之后只跟随事件。
- 固定文案只能来自 `ui_copy.toml`，组件里写裸字符串会被 ESLint（`vue/no-bare-strings-in-template`）拦下。
- 颜色、字号、间距只用 `src/styles/tokens.css` 里的 `--mf-*` 令牌（07 第 1 节）。
- C-09 可覆盖 Hub 数据目录的 `templates/prompts/rewrite.md`：须保留正整数版本和 `{style}`、`{text}`，重启加载；失败回退出厂版本（ADR 0048）。
- C-10 日记可覆盖 `templates/ui_copy.toml` 的 `diary.blank`，要求顶层正整数版本和非空、无禁用词的正文；用户 `diary.safety_session` 不生效，求助标题只读出厂（ADR 0052）。
- C-10 周信允许覆盖 `templates/prompts/letter.md` 与 `templates/letter_tips.toml`（同一 Hub 数据目录）。复制出厂文件修改后重启 Hub：提示词须有正整数版本号并保留 `{style_block}`、`{weekly_stats_json}`；配套文案须保留三档评语与 `late` / `rest_low` / `hard` / `default` 各一条建议，内容非空且通过出厂禁用词校验。读取或校验失败逐文件回退；`letter_fallback.md` 尚未接入用户覆盖，详见 ADR 0046。
- C-10 周信正文可覆盖 `templates/letter_fallback.md`，保留版本号；仅支持已知统计变量和配对、非嵌套条件块，可空统计须包在对应条件块内。重启加载，非法内容回退出厂版本（ADR 0051）。
