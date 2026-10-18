# A 输入法 · 交接文档

> 负责范围（产品书 13 第 1 节）：Moodflow 分支的身份改造、MFP 协议与核心侧服务端、采集钩子、隐私闸门与无痕、Hub 守护、
> 输入法里的 Moodflow 入口（菜单、工具栏、气泡）、温柔改写的核心一侧、DLL 全量按键时序、安装包、上游同步；
> 另是 MFP 协议（`protocol/mfp.schema.json`、`crates/mfp`）的契约负责人（13 第 3.1 节）。
> 任务清单与估算见产品书 16 第 2.1 节。本文件随 A 的每个 PR 更新，任务中途换人时按产品书 13 第 3.1 节“交接”直接看这里。
> 最后更新：2026-10-18（A-13～A-20：XSIME 吸收、插件、Android，ADR 0054 / 0056）

## 1. 任务状态

| 编号 | 任务 | 状态 | 代码 / PR | 还差什么 |
|---|---|---|---|---|
| A-01 | fork、身份改造、全构建验证 | 代码完成 | [#1](https://github.com/young8290/xinqing-ime/pull/1)；说明见 [identity.md](../identity.md) | 在真 Windows 上与上游输入法并装验证（`scripts/dev.ps1` 的 p1）还没人做；这里的开发环境没有 Windows，只能靠 CI 编译 |
| A-02 | MFP v1 schema 与 Rust 类型 | 完成 | `protocol/mfp.schema.json`、`crates/mfp/`（Hub、mf-sim、核心三方共用） | 改协议要走契约 PR（A 负责） |
| A-03 | `moodflow-tap`：队列、MFP 服务端、心跳、背压 | 代码完成 | [#4](https://github.com/young8290/xinqing-ime/pull/4)，`moodflow_input/crates/moodflow-tap/`，ADR 0009 第 1–9 条 | 命名管道与 ACL 没在真 Windows 上跑过；ADR 0009 待评审 |
| A-04 | 核心钩子：按键、上屏、焦点、组字、候选 | 完成 | [#5](https://github.com/young8290/xinqing-ime/pull/5)，`moodflow-coordinator/src/moodflow.rs` | 组字长度只看 `input_buffer`，临时拼音等独占模式的缓冲不计 |
| A-05 | 隐私闸门、无痕模式（菜单、`Ctrl+Alt+P`） | 完成 | [#8](https://github.com/young8290/xinqing-ime/pull/8)，配置段 `[moodflow]`、state.toml `moodflow_paused` | 安全桌面闸门见第 3 节（[#24](https://github.com/young8290/xinqing-ime/pull/24)） |
| A-06 | Hub 守护、总开关、下行处理 | 完成 | [#9](https://github.com/young8290/xinqing-ime/pull/9)，`moodflow-tap/src/guard.rs`，Hub 收到 `bye{disabled}` 后退出（本 PR），ADR 0009 第 10 条 | 命名管道与 ACL 的真 Windows 验证见 A-03 |
| A-07 | 主菜单“Moodflow”分组、工具栏天气按钮、光标旁气泡 | 完成 | [#10](https://github.com/young8290/xinqing-ime/pull/10)、[#13](https://github.com/young8290/xinqing-ime/pull/13)；`docs/design/toolbar-customization.md` 第十三节 | 图标大小、小圆点位置、菜单与气泡要在真机上看一眼 |
| A-08 | 温柔改写（核心一侧） | 大部分完成 | [#18](https://github.com/young8290/xinqing-ime/pull/18)（已合并），`moodflow-coordinator/src/moodflow/rewrite.rs`，ADR 0009 第 11 条 | 见第 4 节 |
| A-09 | DLL 全量按键时序 `CMD_MF_KEY_TRACE` | 暂不做 | — | 按 16 推荐的方案甲，FR-SEN-08 在 13 第 2.1 节降级清单第 1 项，提前降为 P2；团队改选别的方案再做。要改 C++（`moodflow_tsf/`） |
| A-10 | 安装包、卸载 | **进行中** | 第一部分 [#24](https://github.com/young8290/xinqing-ime/pull/24)（已合并）：`config/app.toml`、`scripts/dev.ps1` 的 `Build-Hub`、`.github/workflows/moodflow-package.yml` | 见第 4 节 |
| A-11 | typer 打字脚本、兼容矩阵测试支援 | **进行中** | `tools/typer`（[README](../../../tools/typer/README.md)）；核心耗时埋点 `moodflow-coordinator/src/moodflow/perf.rs` | `typer run` 与埋点都没在真机上跑过，TC-PERF-01、TC-STA-04 还没出数；兼容矩阵与 TC-RWR-09 要真机 |
| A-12 | 每 2 周上游同步 | 未开始 | 基线 `moodflow/base` = `df6f966`，`upstream` 远端已设 | 步骤见 [identity.md](../identity.md)；2026-10-04 查过，上游 main 仍是 `df6f966`，没有要合的 |
| A-13 | XSIME 吸收：纯逻辑（Buffer、候选矩阵、并击、剪贴板历史、Rime 导入、九宫格、数据清单、用户库异常提示、五笔对照、联想历史上限） | 完成 | [#96](https://github.com/young8290/xinqing-ime/pull/96)；计划 [xsime-feature-port-plan.md](../../design/xsime-feature-port-plan.md) §9 | — |
| A-14 | XSIME 吸收：Windows 接入（剪贴板历史、Buffer、并击、候选矩阵、LPAC 放行） | 代码完成 | [#97](https://github.com/young8290/xinqing-ime/pull/97)、[#98](https://github.com/young8290/xinqing-ime/pull/98) | **一项都没真机验收**；缺口见计划 §10.1、§10.2 |
| A-15 | 打字测速（`moodflow-typing-practice`，Hub 看板页） | 代码完成 | #97、#98 | Hub 页要在真机上看；Android 上练习仍计入日常统计（计划 §10.3 A2） |
| A-16 | 插件 M1～M3 核心（api / host / market / registry，协调器接入） | 核心完成 | #96、#97；设计 [plugin-system.md](../../design/plugin-system.md) §10–§11 | 插件管理页、市场仓库、签名密钥、内容包安装等，见设计 §11 |
| A-17 | Android 骨架（`moodflow_android/`、`moodflow-mobile` 门面、JNI、CI） | 完成 | #97 | — |
| A-18 | Android 打包完整词库 | 完成 | `main` 2840af6f | APK 解包时按清单校验未接 |
| A-19 | Android 键盘与设置换成 XSIME 界面（引擎仍是 Moodflow 核心） | **评审中** | [#100](https://github.com/young8290/xinqing-ime/pull/100)（CI 全绿，未合入） | 真机验收（计划 §10.1 V5）；缺口见计划 §10.3 |
| A-20 | Windows TSF 端到端自动测试（含 AppContainer / LPAC） | 未开始 | — | 需要 Windows 环境（计划 §10.2 W5） |

## 2. 代码地图（A 负责的部分）

```
protocol/mfp.schema.json            MFP 契约（A 负责）
crates/mfp/                         消息类型与帧编解码，三方共用
moodflow_input/crates/moodflow-tap/ 核心侧 MFP 服务端（进程级单例 Tap）
├─ src/lib.rs                       Tap：钩子（只做闸门判断 + try_send）、改写收发、无痕
├─ src/gate.rs                      隐私闸门（同意、无痕、密码框、名单、安全桌面）
├─ src/recent.rs                    “最近上屏”缓冲（只在内存里）
├─ src/guard.rs                     Hub 守护
└─ src/{server,pipe,transport}.rs   写线程、读线程、命名管道 / TCP
moodflow_input/crates/moodflow-coordinator/
├─ src/moodflow.rs                   钩子接线、菜单、工具栏格、气泡、下行分派
├─ src/moodflow/rewrite.rs           温柔改写模式
├─ src/moodflow/perf.rs              dev 构建的按键与钩子耗时埋点（NFR-PERF-01）
└─ tests/moodflow_tap.rs             端到端测试（测试扮演 Hub，A-04～A-08 全在这一个测试里）
moodflow_input/crates/moodflow-config/      [moodflow] 配置段、热键动作 moodflow_pause / moodflow_rewrite
moodflow_input/crates/moodflow-ui(-types)/  工具栏天气格与 weather_*.svg
tools/typer/                        节奏打字脚本（SendInput 发键；gen perf / gen r1 / check / run）
```

改 Moodflow 原有文件时用 `Moodflow：` 开头的注释标出来，方便合并上游。各 crate 的细节在它们自己的 AGENTS.md。

验证（在 `moodflow_input/`）：`cargo test -p moodflow-tap`、`cargo test -p moodflow-coordinator --test moodflow_tap`、
`cargo test -p moodflow-coordinator --lib moodflow`；与 Hub 联调：两边都设 `MF_MFP_TCP=127.0.0.1:18765`，或用 `mf-sim --tcp` 扮演一方。
提交规则（只 `git add` 显式路径、`cargo fmt` 单独提交、提交信息不加 AI 署名）见根目录 AGENTS.md。

## 3. 安全桌面闸门（A-05 遗留，#24 已合并）

C-PLT-05：登录、锁屏、UAC 提权框处于安全桌面，输入法不采集也不显示 Moodflow 界面。协调器在每次焦点事件里先判断
是否在安全桌面（`moodflow::on_secure_desktop`：进程名是 `LogonUI.exe` / `consent.exe`，或 `OpenInputDesktop`
被拒绝、桌面名不是 `Default`），再调 `Tap::set_secure_desktop` 并报焦点。闸门关着时焦点、按键都不发，最近上屏清空；
回到普通桌面的第一个焦点事件打开闸门并补发焦点。测试只覆盖进程名这条路，`OpenInputDesktop` 这条要在真机上锁屏验证。
本仓现在可以在 Linux 上做 Windows 编译检查：`cargo check -p moodflow-coordinator --target x86_64-pc-windows-gnu`（`rustup` 里已有该目标）。

## 4. 进行中：A-10 安装包

`scripts/dev.ps1 8` 出包，清单是 `config/app.toml`。安装器是通用生成器 moodflow-installer（清单驱动），Moodflow 用自己的 fork
[young8290/wind-installer](https://github.com/young8290/wind-installer)：上游缺的四项能力在 fork 里补（fork 的 PR #1），
`moodflow-package.yml` 检出 fork 的 main。Actions 里手动跑 “Moodflow Package” 就能拿到 `Moodflow-Setup-<版本>-dev.<提交号>.exe`
去真机试装；手动跑时可填 `installer_ref`，用 fork 里还没合并的分支打包。本地打包要把 fork 克隆到本仓同级的 `moodflow-installer` 目录。

| 部分 | 需求 | 状态 |
|---|---|---|
| Hub 进安装包：`Build-Hub` 用 pnpm 构建 `moodflow_hub.exe`，连同 `hub_templates/` 放进 `build/` | FR-OPS-01 | 完成（没装 pnpm 时跳过；`MF_REQUIRE_HUB=1` 时报错） |
| 开始菜单“Moodflow”快捷方式（打开 Hub） | FR-OPS-01 | 完成 |
| 升级、卸载前结束 Hub（`process_names` 加 `moodflow_hub`） | FR-OPS-02 | 完成 |
| 卸载时可选删除 Moodflow 数据（`%LOCALAPPDATA%\Moodflow\hub`） | FR-OPS-02 | 完成：并进“删除 Moodflow 数据、用户词库和配置数据”这一个勾选，默认不选；产品书写的是两个选项 |
| 不自动设为默认输入法 | FR-OPS-01 | 安装器本来就不设 |
| 安装包名 | FR-OPS-01 | `Moodflow-Setup-<版本>.exe`（打包器固定“名称-版本”的顺序），产品书写的是 `Moodflow-<版本>-Setup.exe` |
| 检测 WebView2、缺失时下载微软引导程序 | FR-OPS-01、C-PLT-06 | 已写，未实测：清单 `[[prerequisite]]`。打包时 `Add-WebView2Bootstrapper` 下载微软在线引导程序到 `redist\`、校验微软签名；装完检测，缺了静默运行它（需联网，最长等 20 分钟），仍缺则完成页给下载链接。便携版只提示 |
| 快捷方式写应用标识（通知要用） | FR-OPS-01、FR-NTF-01 | 已写，未实测：“Moodflow”快捷方式带 `app_user_model_id = "com.moodflow.hub"`，须与 `tauri.conf.json` 的 identifier 一致。Hub 的系统通知已接（D-09，`tauri-winrt-notification`） |
| 完成页提示、许可页放 MIT 全文与“基于 Moodflow 输入法”声明 | FR-OPS-01、FR-OPS-06 | 已写，未实测：完成页两行说明；“我已阅读并同意《许可协议与开源声明》”点开是 `config/installer-license.txt` 加根目录 `LICENSE` 拼成的全文。`LICENSE`、`NOTICE.md` 随包装进安装目录 |
| 卸载时删掉 Hub 的“登录时后台启动”启动项 | FR-OPS-02 | 已写，未实测：清单 `[runtime_autostart]` 认领值名 `MoodflowHub` 和 `Moodflow`。Hub 一侧已做（ADR 0039 第 2 条）：设置“常规”的开关写 Run 键值名 `MoodflowHub`，参数 `--background`；调试构建用 `MoodflowHubDev`，卸载清单可一并认领 |

这四项依赖 fork 的 PR #1：要先合并它，再合并本仓的对应 PR，否则打包时旧安装器会忽略这些清单字段（不报错，但功能不生效）。
fork 刚建时 Actions 默认关着，要在 fork 的 Actions 页点一次启用，PR #1 的 Windows CI 才会跑。

## 5. A-08 剩余事项

| 部分 | 需求 | 状态 |
|---|---|---|
| 快捷键 `moodflow.rewrite_hotkey`（出厂 `Ctrl+Alt+R`，`none` 不绑） | FR-RWR-01 | 完成 |
| 前置条件：Moodflow 开着、同意 ⑥、不在密码框 / 名单应用 / 无痕、没有组字；每种不满足都有气泡；没同意时请 Hub 打开设置问一次 | FR-RWR-01 | 完成 |
| 来源：最近上屏（`ReplaceBackward`，UTF-16 长度）→ 剪贴板（≤ 300 字，插入） | FR-RWR-01、FR-RWR-04 | 完成。光标移动、其他按键、焦点切换都会清空最近上屏 |
| 候选框：标题行“AI 改写 · 风格”、编码区原文前 20 字、候选折成每行 18 字最多 3 行、竖排 | FR-RWR-03、FR-RWR-06 | 完成；真机上的宽度没看过 |
| 按键：`1`–`3` / 空格上屏、`Ctrl+1`–`3` 复制、`Tab` 换风格（5 分钟缓存）、`Esc` 取消、其他键退出后照常处理并由 C++ 重放 | FR-RWR-02、FR-RWR-03 | 完成 |
| 等待 10 秒超时、失败提示 1.5 秒后退出；危机内容直接退出并邀请聊聊 | FR-RWR-03、FR-RWR-05 | 完成；危机气泡点不了（ADR 0009 第 11 条） |
| 退出时发 `rewrite_done`（不含文字） | FR-RWR-04 | 完成 |
| `ReplaceBackward` 表现异常的宿主改为“插入 + 复制原文” | FR-RWR-04 | 完成（ADR 0041）：输入法配置 `moodflow.rewrite_insert_apps`（出厂空），命中的程序里只插入、不替换；兼容测试发现问题的宿主再加进出厂值 |
| 首次使用说明 | FR-RWR-06 | 完成（ADR 0041）：放在 Hub 给出同意 ⑥ 的地方（首次引导、设置“温柔改写”），核心气泡不加 |
| 选中文字来源 | FR-RWR-01（P2） | 未做：DLL 不上报选区 |

Hub 一侧（提示词、保真校验、隐私占位符、每日上限、`rewrite_log`）是 C 的 C-09，不在这里。

## 6. 关键决定与待评审

- ADR 0009（核心侧 MFP 服务端的实现约定，11 条）：状态“提议”，等 A 自己以外的人评审（建议 C 看第 6、7、10 条，E 看第 11 条）。
  接受后要改产品书 17 第 1.1–1.6 节、10 第 2.1 节、06 FR-RWR-03、04 FR-SEN-01，并写 00 第 5 节修订记录。
- 范围按产品书 16 推荐的方案甲在做，团队还没正式确认。

## 7. 已知问题

- 本仓的开发会话没有 Windows：命名管道、TSF 吃键与重放、`ReplaceBackward`、工具栏图标都只在 Linux 无头测试和 CI 的 Windows 编译里验证过。
- “最近上屏”按 10 第 2.4 节最多留 300 字；06 FR-RWR-01 写的是 200 字，两处要统一。
- 改写模式里的 `1`–`3`、`Tab`、`Esc` 仍照常报 `key`，Hub 的感知会看到这些键。
- 出厂默认方案是全拼（`data/config.toml` 的 `[schema]`，2026-10-05 起；之前是五笔 86）。用户配置里写过 `schema.active` 的老安装不受影响。打包流水线没有构建 Moodflow 的设置程序（`moodflow-setting` 是独立仓库），装好后没有“Moodflow 输入法设置”，用户只能在输入法菜单里切换列表里已有的方案。
- 改写快捷键在中英文两种状态下都能用；英文状态下单按 Shift 切中英的逻辑仍会生效（与快捷加词一样）。

## 8. 下一步（按优先级）

0. （2026-10-18 起最优先）XSIME 吸收与 Android 的验收：评审合入 PR #100；Windows 真机验收 A-14（计划 §10.1 V1–V4）；
   Android 真机验收 A-19（计划 §10.1 V5）。之后按计划 §10.2 / §10.3 补功能缺口；
1. A-10 剩余：在真机上试装一次 Actions 产出的安装包，重点看 Win10 无 WebView2 时的代装、完成页、协议全文、开始菜单快捷方式；
2. A-08 收尾：首次使用说明的归属定下来后实现；与 C 联调真实的 `rewrite_result`；
3. 真机验证清单（需要有 Windows 的组员）：并装、命名管道、改写替换（含 emoji 和扩展区汉字）、锁屏与 UAC、工具栏；
4. A-11 剩余：真机上用 typer 跑 TC-PERF-01、TC-STA-04 出数，兼容矩阵；A-12 每两周查一次上游。

## 9. 修订记录

| 日期 | 改动 |
|---|---|
| 2026-10-04 | 初版：盘点 A-01～A-12 现状；A-08 温柔改写核心一侧与 ADR 0009 第 11 条 |
| 2026-10-04 | A-08 已合并；安全桌面闸门；A-09 按方案甲暂不做 |
| 2026-10-04 | A-10 第一部分：Hub 进安装包、开始菜单快捷方式、卸载结束 Hub 与删除 Moodflow 数据、试装用的打包流水线 |
| 2026-10-04 | A-10 第一部分已合并，安装器缺项定为 fork moodflow-installer；A-11 第一部分：`tools/typer`；A-12 查过上游无更新 |
| 2026-10-04 | A-11：核心耗时埋点（dev 构建，每 2,000 键记一行汇总） |
| 2026-10-05 | A-10 第二部分：安装器改用 fork，补 WebView2、通知标识、完成页与许可全文、卸载清 Hub 自启项 |
| 2026-10-05 | 出厂默认方案改为全拼（五笔与五笔拼音混输保留在切换列表里） |
| 2026-10-18 | 新增 A-13～A-20：XSIME 吸收（纯逻辑、Windows 接入）、打字测速、插件核心、Android 骨架与 XSIME 界面移植（ADR 0054、0056） |
