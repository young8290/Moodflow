# 0045 产品改名为 Moodflow
- 状态：已接受（用户决定，2026-10-17）
- 日期：2026-10-17；提议人：用户；待 E（文案、合规）事后评审用户可见的改动与提示词

## 背景
用户决定把产品名从“心晴”改为 **Moodflow**（情绪随打字流动），并要求把原名“心晴”、输入法上游的名字“清风 / WindInput”
以及由它们派生的名字全部换掉，包括代码、目录与 crate 名。

第一版改动只改了用户看得到的名字；随后用户明确要求连代码里的 `wind_*` 名字一起改，晴晴保留。

## 决策
1. **名字一律换成 Moodflow。** 与汉字相邻时两边加空格（如“欢迎使用 Moodflow”“Moodflow 输入法”），英文标识按大小写用
   `Moodflow` / `moodflow` / `MOODFLOW`：
   - 产品名与文案：心晴 → Moodflow，心晴输入法 / 清风输入法 → Moodflow 输入法；
   - 目录与 crate：`wind_input/` → `moodflow_input/`、`wind_tsf/` → `moodflow_tsf/`、`xinqing_hub/` → `moodflow_hub/`、
     `wind-*` crate → `moodflow-*`、`wind-xinqing-tap` → `moodflow-tap`，以及 macOS、Linux 端；
   - 代码标识：`Wind*` 类型、`WIND_*` 常量、C++ 命名空间 `wind`、cmdbar 函数 `wind.cli` → `moodflow.cli`、
     短语文件标记 `wind:p` → `moodflow:p`、主题 `_qingfeng` → `_moodflow`、macOS bundle id `to.feng.*` → `app.moodflow.*`；
   - 协议与工具：XQP（XinQing Protocol）→ MFP（Moodflow Protocol），`crates/xqp` → `crates/mfp`，`xq-sim` → `mf-sim`，
     `xq-replay` → `mf-replay`，环境变量 `XQ_*` → `MF_*`，前端令牌 `--xq-*` → `--mf-*`、窗口事件 `xq:` → `mf:`；
   - Windows 标识：数据目录、注册表、管道、内核对象、窗口类名、exe / dll 名、Tauri identifier、自启动值名（对照表见
     `docs/moodflow/identity.md`）。TSF 的 GUID 不变。
2. **不再合并上游。** 改名后几乎每个文件都与上游不同，合并已不可行。`scripts/xinqing/rebrand.py`（合并后重做身份改造）
   换成 `scripts/moodflow/check_names.py`，CI 用它保证旧名不再出现。代码里已有的 `Moodflow：` 注释是当年标记分支改动留下的，保留即可。
3. **保留出处。** MIT 许可证要求保留原版权声明：`LICENSE` 保留 “WindInput Contributors” 一行并加上 Moodflow 一行；
   `NOTICE.md`、README、“关于”页、安装许可写明“输入法部分基于开源项目 WindInput（MIT）”（产品书 C-LAW-08）。
   上游的 logo（`pic/logo.png`）与 `BRANDING.md` 删除，README 改用本项目的占位 logo；上游的 CLA 记录
   （`CLA.md`、`signatures/`、`cla.yml`）是上游贡献者签的法律记录，不改。
4. **保留晴晴**：晴晴是角色名，不是产品名；内置桌宠 `qingqing`、logo 上的“晴”字不变。
5. **指向外部的名字不改**：GitHub 上的仓库名（`young8290/xinqing`、`young8290/xinqing-ime`、`young8290/wind-installer`、
   `huanfeng/*`）、上游网站与 issue 链接，改了链接就断；仓库改名要在 GitHub 上操作。
6. **拼音测试夹具**里的“清风”是按 `qingfeng` 打出来的词，换成同音的“轻风”，音节结构不变。

## 影响
- 按旧名部署过开发版的机器要先卸载旧版，数据不会自动迁移（见 identity.md“改名对已有安装的影响”）。
- 产品书全书称呼同步改为 Moodflow（产品书仓库另有 PR）。
- 待 E 评审：隐私说明、首次引导、关于页与三份提示词里的称呼改动（提示词改动按约定要包括 E 在内的两人评审）。
