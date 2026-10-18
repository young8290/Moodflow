# 0057 扩大 XSIME 吸收范围：除摩斯电码与电音演奏外，原「明确不做」的功能全部纳入计划
- 状态：已接受（用户决定，2026-10-18）
- 日期：2026-10-18；提议人：用户（「明确不做中，除摩斯密码、电音演奏，其他都做」）；待 A（输入法）、C（AI 与后端）、
  E（隐私、安全）事后评审；**修订 ADR 0056 第 9 条**
- 完整计划：[docs/design/xsime-extended-plan.md](../design/xsime-extended-plan.md)

## 背景
ADR 0056 第 9 条把 XSIME 的一批功能列为「明确不做」：Mailbox（终端 / 对话 / 收件）、Capsule 的截图 / 录屏 / OCR、
笔记 / 资源 / 密码库、iCloud 同步、电音演奏、摩斯电码、LaTeX、文献引用（Scholay），以及 Android 上 XSIME 的 AI、
本机翻译词典与 Buffer 插件快捷栏、官方插件商店与社区页、宠物轮换动画。

2026-10-18 用户决定：除摩斯电码与电音演奏外，其余都做。同时确认四条**规则性**的「不做」保持不变（见决策第 2 条）。

调研结论（XSIME 源码，详见计划 §1）：这些功能在 XSIME 里**只有 macOS 实现**，Windows、Android 都没有，
Linux 只有笔记底栏；LaTeX 与文献检索的实现在未公开的官方插件子模块里，只能按文档复刻。所以这一批**不能像首批那样移植代码**，
主要是按 XSIME 的行为契约自己实现。

## 决策
1. **纳入计划（V1.x 二期）**：
   - Mailbox 工作区：终端（仅 Windows）、对话、收件；
   - Capsule 捕获：截图、滚动截图、录屏、OCR、二维码、贴图、标注（Android 先做「对图片 OCR / 识码」）；
   - Capsule 资料库：笔记、资源、文献条目、密码库；
   - 同步（取代 iCloud）：同步到用户自选的文件夹（OneDrive、Dropbox、Syncthing、坚果云等）或 WebDAV；
   - LaTeX（文字 / 图片 → LaTeX，另加离线的 LaTeX → Unicode 与本地预览）；
   - 文献引用：单条格式化（GB/T 7714、APA 等）、DOI 查询、AI 生成带引文的句子（防编造闸门）；
   - Android：OpenAI 兼容的 AI 接入与 Buffer 插件快捷栏（翻译、快问、润色、作诗、画画）、本机中英词典翻译、
     插件商店页与社区页、键盘宠物轮换（用 Moodflow 自己的桌宠素材）。
2. **规则不变**：不集成 librime / 不执行 Rime Lua；不收录 XSIME 的 GPL 方案与词库数据；插件不带原生界面（ADR 0054 第 7 条）；
   macOS / Linux 宿主不在本计划内。摩斯电码、电音演奏仍不做。
3. **比 XSIME 更严的安全要求**：
   - **密码库必须真正静态加密**。XSIME 把 32 字节主密钥明文存在权限 0600 的文件里，解锁口令只是界面门禁。
     Moodflow 的主密钥必须由平台密钥保护（Windows DPAPI / 可选 Windows Hello，Android Keystore / 可选生物识别），
     或由口令经 Argon2id 派生的密钥包装。
   - **同步默认端到端加密**（XSIME 上云的是明文）；密码库与主密钥只在有恢复短语包装主密钥时才允许同步。
   - **外部文字永不自动上屏**（继承 XSIME）：Mailbox 推送、OCR、AI 结果、翻译结果只能进 Buffer，由用户明确发送。
4. **与产品原则的衔接**：
   - 01 第 6.2 节「云端账号、多设备同步、任何服务器端存储用户数据」改为：仍然**不设账号、不建服务器**；
     允许用户把数据同步到**自己选择的存储**，默认端到端加密。
   - 所有会把内容发出本机的功能（AI 对话、LaTeX、文献检索、AI 翻译、DOI 查询）都要在 09 第 2.1 节加同意项，用户同意前不发送。
   - Mailbox 的「对话」与 Hub 的晴晴对话共用 AI 网关与危机安全检测（08），不能绕开。
   - Hub 已有「信箱」（FR-DSH-06，晴晴的周信）；XSIME 的 Mailbox 在 Moodflow 里叫「工作区」，避免同名。
5. **落点**：纯逻辑仍先进零平台依赖的 crate（资料库、密码库加密、同步协议、文献格式化、LaTeX 映射、词典索引、Mailbox 存储），
   过 `cargo check-android`；Windows 的窗口放在 Hub（Tauri）里，截图 / 录屏用独立的辅助进程，**都不进 TSF DLL**；
   Android 放在主应用的页面里，键盘内只放插入用的面板。

## 备选方案与取舍
- **照搬 XSIME 的密码库**：实现快，但等于没有静态加密，同一用户下任何进程都能读出全部密码。没有采用。
- **同步用明文（照 XSIME）**：实现简单，但第三方网盘能读到用户的笔记与资料。没有采用为默认，可作为用户显式选择的选项。
- **文献检索用 XSIME 的 Minicod 网关**：第三方付费服务，条款不明。改用 Crossref / doi.org / OpenAlex 等公开接口。
- **Android 截屏照 macOS 做全套**：键盘内不能静默截屏，MediaProjection 每次都要用户同意，贴图需要悬浮窗权限。
  第一版只做对已有图片的 OCR / 识码。

## 影响
- 计划：[xsime-extended-plan.md](../design/xsime-extended-plan.md)（分 Q0～Q7 阶段，**全部未开工**）；
  [xsime-feature-port-plan.md](../design/xsime-feature-port-plan.md) §1、§7 相应更新；ROADMAP 新增一节。
- 第三方许可要登记（计划 §6）：CC-CEDICT（CC BY-SA 4.0，词典数据单独分发）、CSL 样式（CC BY-SA）、终端解析库、二维码库、
  KaTeX 等；移植 XSIME 的 `CapsuleReference.swift` 时保留 Apache-2.0 版权头。
- 产品书（V1.7 采纳）：00 修订记录；01 第 6.1.1、6.2、6.3、8 节；03 组件；06 新增二期功能需求；04 新增 Android 的 AI 与翻译等需求；
  09 新数据项、同意项与安全措施；10 计划中的接口；12 / 18 验收与用例；13 阶段与风险；16 任务。
