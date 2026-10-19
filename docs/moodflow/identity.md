# Windows 上的标识

产品书 14 §1 第 4 步（FR-OPS-06、C-PLT-08）要求 Moodflow 在 Windows 上用自己的标识，能与上游输入法同时安装、互不干扰
（TC-OPS-04）。ADR 0045 起所有名字都改成了 Moodflow（代码、目录、crate 也一样），不再合并上游，
所以这里只列现状，不再有改造脚本。CI 里的 `scripts/moodflow/check_names.py` 保证改名前的名字不再出现。

## 对照表

| 类别 | 值 | 位置 |
|---|---|---|
| TSF GUID（release） | `{EF62EE3n-5ECF-413A-A476-48D1F29E827C}`（n = 0 CLSID、1 Profile、2 语言栏按钮、3/4 显示属性、5 PreservedKey） | `moodflow_tsf/src/Globals.cpp`、`direct_switch.rs`、`tsf_profile_name.rs`、`dev.ps1`、`ime-slot.ps1`、`config/app.toml` |
| TSF GUID（dev） | `{EF62DEBn-…}` | 同上 |
| 主管道 | `\\.\pipe\moodflow[_dev]_<SID>` | `Globals.cpp`、`moodflow-bridge/src/server.rs` |
| 推送管道 | `moodflow_push[_dev]` | `Globals.cpp`、`moodflow-bridge/src/push.rs` |
| 设置端 RPC 管道 | `moodflow_rpc_ctrl`、`moodflow_rpc{后缀}_events` | `moodflow-rpc`、Hub 的 `core/src/infra/imeconf.rs` |
| 数据目录 | `%LOCALAPPDATA%\Moodflow[Dev]`、`%APPDATA%\Moodflow[Dev]` | `moodflow-config/src/variant.rs`、`moodflow_tsf/include/FileLogger.h` |
| 注册表 | `HKLM\Software\Moodflow[Dev]` | `Globals.h`（`MOODFLOW_APP_REGKEY`）、`tsf_profile_name.rs`、`dev.ps1` |
| 内核对象 | `Local\Moodflow_*`、`Local\MoodflowIMEService*` | `moodflow-ipc`、`moodflow-bridge`、`apps/service`、`Globals.h` |
| 窗口类名 / 窗口消息 | `MoodflowCandidate`、`MoodflowHotkeyWnd`、`MoodflowHotkeyRetry_v1` 等 | `moodflow-ui`、`moodflow_tsf/src` |
| 显示名 | Moodflow 输入法 / Moodflow 输入法（开发版） | `Globals.h`、`LangBarItemButton.cpp`、`version.rc.in`、`build.rs`、协调器文案 |
| 安装目录 | `C:\Program Files\Moodflow[Dev]` | `dev.ps1`、`config/app.toml` |
| 核心服务 exe | `moodflow_core[_dev].exe`（构建脚本把 cargo 产出的 `moodflow_input.exe` 复制成这个名字，03 第 5.1 节） | `dev.ps1`、`dev.sh`、`remote-build.*`、`pack-installer.sh`、`Globals.h`（`MOODFLOW_SERVICE_EXE`）、`build.rs`、`config/app.toml` |
| TSF DLL | `moodflow_tsf[_x86][_dev].dll` | `CMakeLists.txt`、`Makefile`、`version.rc.in`、`moodflow_tsf.def`、上述脚本 |
| Hub | `moodflow_hub[_dev].exe`，Tauri identifier `com.moodflow.hub`，自启动值名 `MoodflowHub` | `tauri.conf.json`、`config/app.toml`、`coordinator.rs` 的 `settings_app_path` |
| 系统目录副本 | `System32\IME\Moodflow` | `config/app.toml` |
| 图标 | 波浪“M”+ 小太阳（深蓝 #1D3557、蓝 #4F9DE8、黄 #F7B940），源稿在 `assets/brand/` | `moodflow_tsf/res/moodflow_input.ico`、`assets/installer.ico`、`assets/logo.png`、`moodflow_hub/src-tauri/icons/*`、`moodflow_hub/src/components/AppLogo.vue` |

窗口类名和窗口消息与上游不同名，是因为两个输入法的 DLL 会被同一个宿主进程加载：
`TextService.cpp` 用 `FindWindowExW` 按类名找热键窗口，同名时会找到对方的窗口。

## 改名对已有安装的影响

ADR 0045 改了数据目录、注册表键、exe / dll 名和 Tauri identifier，TSF GUID 没变。按旧名部署过开发版的机器，
先用旧版本的 `dev.ps1` 卸载，或手动删掉旧的 `%LOCALAPPDATA%\XinQing[Dev]` 与 `HKLM\Software\XinQing[Dev]`，再部署新版。
旧数据不会自动迁移；需要保留的话，先在旧版 Hub 里导出备份，装好新版后在“设置 → 隐私 → 从备份导入”导回。

## 已知限制

- 安装包由独立仓库 `young8290/wind-installer` 生成（CI 检出到 `moodflow-installer/`）。仓库名没改，改名要在 GitHub 上操作。
- 图标全部由 `python3 scripts/moodflow/gen_icons.py` 从 `assets/brand/icon-1024.png` 生成；换 logo 时改 `assets/brand/` 的源稿后重跑，并同步 `AppLogo.vue` 的矢量路径。

## 验证

Linux 上能做的：`cargo test`（核心逻辑与名称断言）和 `python3 scripts/moodflow/check_names.py`。
共存验证（TC-OPS-04）必须在 Windows 上做：先装上游输入法，再用 `.\scripts\dev.ps1 p1` 部署 Moodflow，确认

1. 输入法列表里同时出现上游输入法和“Moodflow 输入法”，可分别切换、分别打字；
2. 部署或卸载 Moodflow 时，上游输入法的进程没有被停掉；
3. `%LOCALAPPDATA%\Moodflow\` 与上游的数据目录各自独立；
4. `scripts\ime-slot.ps1` 显示 Moodflow 占用的是 `EF62…` 槽位。
