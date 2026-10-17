# 0045 产品改名为 Moodflow
- 状态：已接受（用户决定，2026-10-17）
- 日期：2026-10-17；提议人：用户；待 E（文案、合规）事后评审用户可见的改动

## 背景
用户决定把产品名从“心晴”改为 **Moodflow**（情绪随打字流动）。“心晴”出现在三类地方：用户看得到的文字（界面文案、窗口标题、
输入法在系统语言栏里的名字、隐私说明）；代码注释与文档（包括 AGENTS.md 规定的 `心晴：` 改动标记）；Windows 上的身份标识
（数据目录 `%LOCALAPPDATA%\XinQing`、注册表、管道、内核对象、exe / dll 名、Tauri identifier、autostart 值名，见 identity.md）。

## 决策
1. **只改用户看得到的名字。** 写法：英文 `Moodflow`，与汉字相邻时两边加空格（与界面里“AI 生成”的写法一致），
   如“欢迎使用 Moodflow”“Moodflow 输入法”。改了：
   - Hub：`hub_templates/ui_copy.toml` 的文案值（注释不动）、`privacy_notice.md` 正文、`tauri.conf.json` 的 `productName` 与窗口标题、
     “导出运行信息”里的说明头；
   - 输入法：语言栏名称与提示（`Globals.h`、`LangBarItemButton.cpp`、`version.rc.in`、`build.rs`、`tsf_profile_name.rs`、
     `push_config.rs`、协调器语言栏文案、`theme.rs` 的升级提示）、托盘里的“Moodflow 设置 / 开启·关闭 Moodflow 功能”、
     暂停 / 恢复与组件未运行的气泡、改写的“要先在 Moodflow 里同意”；`config/app.toml`、`installer-license.txt`、`dev.ps1` 的显示名；
   - `scripts/xinqing/rebrand.py` 的显示名规则改成 `Moodflow 输入法`，合并上游后照旧一键改好。
2. **身份标识不改。** 目录、注册表、管道、exe / dll 名、Tauri identifier、autostart 值名、仓库名、crate 名都保持 XinQing：
   改了已经装过的用户会丢数据、两份输入法同时注册，需要迁移步骤，等真正发布前再单独决定。TSF 的 GUID 不变，
   所以改名后在系统里仍是同一个输入法，只是显示名变了。
3. **注释、文档、日志里的“心晴”不批量改。** `心晴：` 是 AGENTS.md 规定的改动标记，改了就找不到；日志不面向用户。
   产品书与 ADR 正文后续按需改称呼。
4. **AI 提示词暂不改。** `prompts/chat.md`、`comfort.md`、`letter.md` 里写着“你是晴晴，心晴里的 AI 陪伴者”。提示词的改动按
   约定要包括 E 在内的两人评审，留给 E：改成“Moodflow 里的 AI 陪伴者”即可，晴晴这个名字不变。
5. 内置宠物“晴晴”、logo 上的“晴”字不变：晴晴是角色名，不是产品名。

## 影响
- 产品书 00、01（产品名）、07（窗口标题、托盘与语言栏文案）、21（隐私说明）随后同步称呼；本 ADR 不改需求编号与行为。
- E 评审：隐私说明与首次引导里的称呼改动、三份提示词。
