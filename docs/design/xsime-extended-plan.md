# XSIME 吸收二期：工作区、Capsule、同步、LaTeX 与文献引用

> 状态：**计划稿，全部未开工**（2026-10-18）。决定见 [ADR 0057](../adr/0057-扩大XSIME吸收范围.md)，修订
> [ADR 0056](../adr/0056-吸收XSIME基础功能与Android端.md) 第 9 条。一期（Buffer、矩阵、并击、剪贴板、导入、九宫格、Android）
> 见 [xsime-feature-port-plan.md](xsime-feature-port-plan.md)。
> 参考：XSIME `main`（[scholay/xsime](https://github.com/scholay/xsime)，Apache-2.0）的 `MAILBOX-WORKSPACES.md`、`CAPSULE.md`、
> `CAPTURE.md`、`SCHOLAY-BUFFER.md`、`SYSTEM-ARCHITECTURE.md`、`platforms/android/README.md`、`platforms/linux/capsule/SPEC.md`、
> `Sources/RimeBuffer/`。文中「XSIME：」开头的句子是它的行为，「Moodflow：」开头的是我们的落点。

## 0. 结论先行

1. **范围**：除摩斯电码与电音演奏外，原「明确不做」的 XSIME 功能全部纳入（§2）。librime、XSIME 的 GPL 数据、插件原生界面、
   macOS / Linux 宿主这四条规则不变。
2. **这一期基本不能移植代码。** 这些功能在 XSIME 里只有 macOS（Swift / AppKit / ScreenCaptureKit / Vision）实现，Windows 端明确没有
   Capsule 和 Mailbox 前端（XSIME `platforms/windows/native/README.md`），Android 路线图排除了 Mailbox、Capsule 和云同步；LaTeX 与文献检索
   的代码在未公开的官方插件子模块里。所以以**继承行为契约、自己实现**为主；能直接移植的只有零依赖的小块（如文献单条格式化器）。
3. **安全上比 XSIME 更严**：密码库真正静态加密；同步默认端到端加密；外部文字永不自动上屏（§3）。
4. **先做低风险、两端共用的纯逻辑**（资料库、文献格式化、词典翻译），**再做平台重活**（截图录屏、终端），最后做同步。
5. 工作量粗估约 1,400 小时（§5），是一期的三倍左右；截图录屏与同步是两个最大的块。

## 1. XSIME 现状调研（2026-10-18）

| 功能 | XSIME 已有平台 | 关键实现（macOS） | 说明 |
|---|---|---|---|
| Mailbox 工作区（终端 / 对话 / 收件） | 仅 macOS；Linux 只有计划 | `Sources/RimeBuffer/Mailbox/`；SwiftTerm PTY；本机回环网关 `127.0.0.1:47700` + Bearer token；`mailbox.json` | 窗口 1000×700、最小 680×500；三个内置插件 `builtin.mailbox.terminal/chat/inbox`；会话上限 200 × 每会话 200 条 |
| Capsule 捕获 | 仅 macOS；Linux 计划 | `Sources/RimeBuffer/Capture/`；ScreenCaptureKit、Vision（OCR + 二维码）、AVAssetWriter；索引 GRDB / SQLite | 区域 / 全屏 / 窗口 / 滚动 / 延时 / 取字 / 录屏；临时资产 30 天、10 GB 上限，使用中的资产有租约；锁屏 / 睡眠 / 安全输入即停；滚动截图上限 32768 px / 6400 万像素；GIF ≤2 分钟、15 fps |
| Capsule 资料库（笔记 / 资源 / 文献 / 密码） | macOS 完整；Linux 只有笔记底栏；Android 只有本地剪贴板 | 每条一个 Markdown 文件（front matter + 正文），Obsidian 可读；写入用 SHA-256 revision 防并发覆盖；资源只记本机路径 | 标签：最近（剪贴板历史）、捕获、笔记、图片、视频、文献、技能、密码 |
| 密码库 | macOS | ChaCha20-Poly1305，AAD = id + title；**主密钥明文存 0600 文件**；并击口令与自定义口令只是界面门禁 | 单字段显示 15 秒、详情开放 60 秒；复制带 Concealed / Transient 标记，60 秒后清剪贴板；TOTP（RFC 6238）；无自动填充、不模拟粘贴、不进 iCloud |
| iCloud 同步 | 仅 macOS（`CapsuleCloudSync.swift`） | 同步到用户自选的 iCloud Drive 文件夹，本机为权威副本 | 同时修改时云端版本获胜、与设备时钟无关，本机败者存 `conflicts/`；删除用永久墓碑；媒体暂时读不到只延后、绝不当作删除；**明文上云**；密码、主密钥、本机路径从不同步 |
| LaTeX | 仅 macOS（下载型 Buffer 插件） | 实现不在公开仓库 | 纯 AI：文字描述 → LaTeX、公式图片 → LaTeX（≤3 张、每张 ≤4 MiB）；无本地渲染、无 Unicode 转换 |
| 文献引用 | 仅 macOS | Reference：AI 提取检索词 → 第三方付费网关 Minicod 检索 Semantic Scholar → 取有摘要的前 4 篇 → AI 写带夹注的句子；Capsule 文献：约 140 行的单条格式化器 `CapsuleReference.swift`（Apache-2.0，无 CSL 引擎） | 引用号无法验证时拒绝输出；六种格式；默认 GB/T 7714；无 DOI 在线查询、无 BibTeX 导入 |
| Android AI + Buffer 插件快捷栏 | Android | 候选栏空闲时显示 翻译 / 快问 / 润色 / 作诗 / 画画 五个入口；OpenAI 兼容服务（HTTPS），密钥用 Keystore AES-GCM | 在线 AI 需用户开启；换主机须重填密钥；10 s 连接 / 20 s 读取 / 60 s 总时限；输出 ≤2048 token、≤16384 UTF-16；SSE 须完整校验后才能发送；无请求历史、无自动重试、不跟随重定向；关闭在线 AI 时有明确标注的本地 Mock |
| 本机翻译词典 | Android | CC-CEDICT（125,166 条，CC BY-SA 4.0），构建机预编译紧凑基数索引，手机懒加载 | 词 / 短语双向查词，不是句子翻译；确认输入 400 ms 后自动查，新编辑取消旧结果；私密 / 密码框无入口 |
| 插件商店页与社区页 | macOS | 官方插件目录与社区入口 | Moodflow 的市场客户端 `moodflow-market` 已有 |
| 宠物轮换 | Android | 键盘左上角宠物，点按轮换 18 套皮肤，长按打开选择器；无动画 | Moodflow 有自己的桌宠素材（晴晴，ADR 0044） |

## 2. 范围与优先级

| # | 功能 | Windows | Android | 优先级 | 阶段 |
|---|---|---|---|---|---|
| 1 | 资料库：笔记 | Hub 窗口 + 输入时底栏插入 | 主应用管理 + 键盘面板插入 | P1 | Q1 |
| 2 | 资料库：资源（图片 / 视频 / 文件路径） | 同上 | 同上（SAF 授权的文件） | P2 | Q1 |
| 3 | 资料库：文献条目 + 单条格式化 | 同上 | 同上 | P1 | Q1 |
| 4 | 本机中英词典翻译（Buffer 插件） | Buffer 上的翻译动作 | 键盘快捷栏「翻译」 | P1 | Q1 |
| 5 | 密码库 | Hub 窗口，Windows Hello 可选 | 主应用，生物识别可选 | P1 | Q2 |
| 6 | Capsule 捕获：截图、滚动截图、取字（OCR）、二维码、贴图、标注 | 辅助进程 `moodflow-capture` | 对已有图片 OCR / 识码；截屏走 MediaProjection（后做） | P1 | Q3 |
| 7 | Capsule 捕获：录屏、GIF | 辅助进程 | 不做第一版 | P2 | Q3 |
| 8 | 工作区：对话 | Hub 窗口，共用 AI 网关与危机检测 | 主应用页面 | P1 | Q4 |
| 9 | 工作区：收件（本机推送、MCP 推送、分享） | 回环网关 | 系统分享 / 通知 | P2 | Q4 |
| 10 | 工作区：终端 | ConPTY | 不做 | P2 | Q4 |
| 11 | 同步（用户自选文件夹 / WebDAV，默认端到端加密） | 文件夹 | SAF 目录或 WebDAV | P2 | Q5 |
| 12 | LaTeX：文字 / 图片 → LaTeX（AI）、LaTeX → Unicode、本地预览 | Buffer 动作 + Hub 预览 | Buffer 快捷栏 | P2 | Q6 |
| 13 | 文献引用：DOI 查询、AI 写带引文的句子 | Buffer 动作 | Buffer 快捷栏 | P2 | Q6 |
| 14 | Android AI 接入 + Buffer 快捷栏（快问 / 润色 / 作诗 / 画画 / AI 翻译） | Windows 已有 Hub AI（改写），快捷动作做成 `BufferAction` 插件 | 键盘候选栏空闲时的五个入口 | P1 | Q6 |
| 15 | 插件商店页与社区页 | Hub | 设置应用 | P2 | Q7 |
| 16 | 键盘宠物轮换 | 不适用（Windows 已有桌宠） | 键盘左上角，用晴晴等桌宠素材 | P2 | Q7 |

## 3. 共同原则（所有二期功能都要满足）

1. **隐私闸门照旧**：密码框、私密字段、无痕模式、名单应用、安全桌面、锁屏期间，资料库底栏不弹、截图停止或暂停、AI 入口不出现、
   插件收不到内容。
2. **外部文字永不自动上屏**：工作区推送、OCR、AI 结果、翻译结果、文献句子一律只进 Buffer，由用户明确发送；Buffer 的投递授权规则
   （换目标作废）照旧。
3. **本机优先、无账号、无服务器**：所有数据默认只在本机；同步只到用户自选的存储（§4.6）。
4. **出网须同意**：会把内容发出本机的功能（AI 对话、LaTeX、文献检索、AI 翻译、DOI 查询）在 09 第 2.1 节有各自的同意项，
   用户同意前不发送；只发用户这一次明确选中的内容；AI 配置沿用 Hub 网关（ADR 0012、0055：换主机须重填密钥）。
5. **危机安全不可绕开**：任何 AI 对话界面都经过同一套危机检测（08）。
6. **日志不含正文**：笔记、密码、OCR 结果、对话、终端输出都不进日志。
7. **有界存储、原子写入**：每类数据有条数 / 体积上限；写入先写临时文件再原子替换，替换前复验 revision。
8. **纯逻辑先行**：数据模型、加密、同步协议、格式化、映射表进零平台依赖的 crate，必须过 `cargo check-android`。
9. **不进 TSF DLL**：窗口在 Hub，截图录屏在独立辅助进程；C++ TSF 只负责输入。

## 4. 各功能的契约与落点

### 4.1 资料库：笔记、资源、文献条目

- **XSIME**：每条一个 Markdown 文件（YAML front matter + 正文），Obsidian 可读；写入时用正文与元数据的 SHA-256 作 revision，
  发现磁盘上的 revision 变了就拒绝覆盖（防两个窗口互相覆盖）。资源条目只记本机路径，不复制、不同步。
- **Moodflow**：
  - 新 crate **`moodflow-capsule`**（纯逻辑）：条目模型（笔记 / 资源 / 文献 / 捕获 / 密码引用）、front matter 读写（与 XSIME 格式兼容，
    方便导入导出）、revision CAS、搜索索引（标题、正文、标签、文献字段）、条数与体积上限。
  - 存储：Windows `%LOCALAPPDATA%\Moodflow\capsule\`（不放漫游目录，避免域策略把内容同步走），目录 DACL 只给当前用户；
    Android `filesDir/capsule/`。
  - 文献条目：类型（图书、期刊论文、会议论文、报告）、作者、年份、期刊 / 会议、出版社、卷、期、页码、DOI、URL、ISBN；
    **允许没有 PDF 附件的纯元数据条目**（XSIME 不允许，这是我们补的）。
  - 输入时插入：Windows 在 Moodflow 为当前输入法时按快捷键（暂定 `Ctrl+Shift+K`，标量字段）弹底栏，回车插入；Android 键盘面板。
    插入走 Buffer 或直接上屏由设置决定，密码框里不弹。
- **验收**：与 XSIME 生成的 Markdown 互相读写不丢字段；两个窗口同时改同一条时后写者被拒并提示；10,000 条下搜索 < 100 ms。

### 4.2 文献格式化与引用

- **单条格式化**：移植 XSIME `CapsuleReference.swift`（约 140 行，Apache-2.0，保留版权头）为 Rust crate **`moodflow-cite`**，
  GB/T 7714（按类型标 `[M]/[J]/[C]/[R]`）为默认；必填项缺失时不排版。正式样式（APA、MLA、Chicago 等）用 `hayagriva`
  （MIT / Apache-2.0，支持 CSL）加 CSL 样式（CC BY-SA，单独登记）。
- **DOI 查询（新增，XSIME 没有）**：经 doi.org 内容协商或 Crossref REST 取 CSL-JSON，补全条目；只发 DOI 本身。BibTeX 导入导出。
- **AI 写带引文的句子（Reference）**：不用 XSIME 的 Minicod 付费网关，改用 OpenAlex（CC0）/ Crossref / Semantic Scholar 官方 API 检索，
  只取有摘要的结果；照搬 XSIME 的防编造闸门：句中引用号必须对应真实检索结果，越界即拒绝输出；正文与参考文献表原子交付到 Buffer。
- **验收**：GB/T 7714 用例与 XSIME 输出一致；伪造的引用号被拒；断网时格式化照常可用。

### 4.3 本机中英词典翻译

- **XSIME**：CC-CEDICT（125,166 条）在构建机预编译成紧凑基数索引，手机懒加载；词 / 短语双向查词，不是句子翻译；
  确认输入 400 ms 后自动查，新编辑取消旧结果；未覆盖的片段保留原文，部分覆盖要标明；私密 / 密码框无入口。
- **Moodflow**：构建时从 MDBG 官方下载并锁定版本与哈希（同 rime-frost 的下载机制），生成索引进数据目录；查词逻辑放 crate
  **`moodflow-translate`**（纯逻辑，两端共用）；做成 Buffer 动作（Windows 的 Buffer 浮窗菜单、Android 快捷栏「翻译」）。
  词典数据按 **CC BY-SA 4.0** 单独分发，原始头部、署名、修改说明与许可全文随包。
- **验收**：离线可用；部分覆盖有标注；私密字段没有入口；APK 体积增量记录在案。

### 4.4 密码库

- **继承 XSIME**：条目字段（标题、账号、密码、网址、备注、TOTP 密钥）；单字段显示 15 秒、详情开放 60 秒后自动遮蔽；
  复制时标记为敏感并在 60 秒后清剪贴板；TOTP（RFC 6238）；**不做自动填充、不模拟粘贴到其他程序**；不进剪贴板历史。
- **比 XSIME 更严（必须）**：
  - 加密：每条 XChaCha20-Poly1305，AAD 含条目 id；主密钥随机生成。
  - 主密钥保护：Windows 用 DPAPI（`CryptProtectData`，用户范围）包装，可选 Windows Hello（`UserConsentVerifier`）解锁；
    Android 用 Keystore 不可导出密钥包装，可选 BiometricPrompt，存 `noBackupFilesDir`；另可设主口令，经 Argon2id 派生密钥再包装一层。
  - 剪贴板：Windows 设置 `ExcludeClipboardContentFromMonitorProcessing`、`CanIncludeInClipboardHistory=0`、`CanUploadToCloudClipboard=0`；
    Android 设置 `ClipDescription.EXTRA_IS_SENSITIVE`。
  - 新 crate **`moodflow-vault`**（纯逻辑：格式、加解密、TOTP、自动锁定计时；平台密钥保护以 trait 注入）。
- **待产品决定**：Android 上能否把密码插入密码框（XSIME 一律禁止；更合规的路线是另做 AutofillService）。
- **验收**：拿到磁盘上的文件但没有平台密钥时解不开；锁定后内存中不留明文；剪贴板 60 秒后清空且不进系统剪贴板历史；E 的安全评审通过。

### 4.5 Capsule 捕获

- **继承 XSIME**：区域 / 全屏 / 窗口 / 滚动 / 延时截图、取字（OCR）、二维码、录屏；冻结画面、十字线、像素放大镜、固定比例；
  截图只排除捕获工具自己的界面；结果卡片可复制 / 保存 / 贴图 / 编辑；贴图可调透明度、可锁定穿透；标注与美化；
  临时资产保留 30 天、总量 10 GB，使用中的资产有租约不被清理；**锁屏、睡眠、会话失活、安全输入任一出现就停止或暂停，
  解除后不自动恢复录制**；OCR 结果只能复制、存笔记或显式送入 Buffer。
- **Windows**：独立辅助进程 **`moodflow-capture`**（Rust，Per-Monitor DPI v2），由 Hub 拉起：
  截图用 Windows.Graphics.Capture（Win10 1903+），失败回退 DXGI Desktop Duplication；自身界面用
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` 排除；OCR 用 `Windows.Media.Ocr`（离线，需中文语言包，没装时提示）；
  二维码用 `rqrr` 或 zxing-cpp（Apache-2.0）；录屏用 Media Foundation（H.264 / AAC）+ WASAPI loopback，GIF 用 `gif` crate；
  滚动截图用 `SendInput` 滚轮并拼接。快捷键标量字段（暂定 `keys.capture = "ctrl+shift+a"`），出厂可清空。
  索引与保留规则放 `moodflow-capsule`。
- **Android**：第一版只做**对已有图片 OCR / 识码**（照片选择器、分享进来的图片、剪贴板图片）：OCR 用 PaddleOCR / Tesseract
  （Apache-2.0，随包模型）或 ML Kit（需评估 Google 条款与 Play 服务依赖），二维码用 ZXing。截屏需 MediaProjection（每次用户同意、
  前台服务），放在后续阶段；贴图（需悬浮窗权限）与录屏不做第一版。
- **风险**：工作量最大；多屏、负坐标、DPI 混排；UAC 安全桌面与 DRM 窗口拍不到；截到密码框的隐私风险。
- **验收**：三屏混合 DPI 下区域截图像素对齐；锁屏时截图任务取消；OCR 送入 Buffer 后不自动发送；临时资产超 30 天或 10 GB 被清理、
  使用中的不被清理。

### 4.6 同步（取代 iCloud）

- **继承 XSIME 的协议**（它只依赖文件，可以原样换成任意同步文件夹）：条目是 UUID 命名的 Markdown；删除用永久墓碑；
  资产按内容寻址；三方 revision 基线；同时修改时**同步端版本获胜、与设备时钟无关**，本机败者存 `conflicts/`；
  媒体暂时读不到（占位文件、未下载）只延后这一条，**绝不当作删除**；不做垃圾回收（以后另议）。
- **Moodflow 的改动**：
  - 目标：用户自选文件夹（OneDrive、Dropbox、Syncthing、坚果云盘符等）或 WebDAV（坚果云、Nextcloud）；
  - **默认端到端加密**：恢复短语经 Argon2id 派生密钥，文件用 XChaCha20-Poly1305 加密，文件名不泄露标题；明文同步只作为用户显式选项；
  - 密码库与主密钥只在启用端到端加密并设置恢复短语后才允许同步；本机路径、剪贴板历史、捕获临时资产从不同步；
  - 识别 OneDrive 占位属性与第三方网盘自己生成的冲突副本；
  - 新 crate **`moodflow-sync`**（纯逻辑：协议、冲突判定、墓碑、加密封装；文件系统与 WebDAV 访问以 trait 注入）；
    Android 用 SAF 目录树授权或 WebDAV + WorkManager。
- **产品书冲突**：01 第 6.2 节排除「多设备同步」。按 ADR 0057 改为「不设账号、不建服务器；允许同步到用户自己的存储，默认端到端加密」。
- **验收**：两台设备交替离线编辑后合并无丢失、败者进 `conflicts/`；网盘未下载的文件不被当作删除；同步目录里看不到明文标题与正文（加密模式）。

### 4.7 工作区（XSIME 的 Mailbox）

- **命名**：Hub 已有「信箱」（FR-DSH-06，晴晴的周信），XSIME 的 Mailbox 在 Moodflow 里叫「**工作区**」。
- **继承 XSIME**：一个窗口里三个模块；导航切换与关窗不结束终端进程，「结束」才终止；终端不伪装成聊天消息；
  收取文件是用户明确操作，以会话 id 关联，不按目录或文件名推测；收件输入框只加本地备注，不调用发送接口；外部消息加入 Buffer 必须经过
  审核；流式内容只在内存，完成后才落盘；对话的模型在首次发送时冻结，不因全局设置改变被静默替换；会话上限 200 × 每会话 200 条。
- **对话**：Windows 在 Hub 新窗口，Android 在主应用页面；**共用 Hub 的 AI 网关与危机安全检测**；与晴晴对话分开存储。
- **收件**：本机回环网关只监听 `127.0.0.1`，Bearer token 用 DPAPI / Credential Manager 保管；MCP 只提供推送与流式写入，
  **没有任何读取工具**（「只给不看不发」）；Android 的来源换成系统分享（`ACTION_SEND`）与通知。
- **终端（仅 Windows）**：ConPTY（`CreatePseudoConsole`）+ VT 解析（`alacritty_terminal` 或 `vte`，Apache-2.0 / MIT）自绘；
  第一版也可以先只做「在 Windows Terminal 中打开」加手动收取产物。Android 不做终端。
- **新 crate**：**`moodflow-mailbox`**（纯逻辑：会话与消息模型、上限、归档、未读、原子落盘；回环网关的协议定义）。
- **风险**：回环端口被本机其他进程滥用（同用户进程默认可信，靠 token）；外部文字绕过审核直达上屏；终端子进程树清理。
- **验收**：MCP 客户端无法读取任何内容；外部推送进 Buffer 后不自动发送；关窗后终端进程仍在、「结束」后子进程全部退出。

### 4.8 LaTeX

- **继承 XSIME**：两种 AI 模式——文字描述 → LaTeX、公式图片 → LaTeX（≤3 张、每张 ≤4 MiB）；图片块不可投递；输入变化后旧结果作废。
- **Moodflow 补充（XSIME 没有）**：离线的 LaTeX → Unicode 符号转换（自建映射表，放 `moodflow-transform` 或新 crate）、
  本地预览（KaTeX，MIT；Windows 在 Hub 的 WebView2，Android 在 WebView）。
- 做成 `BufferAction` 插件（ADR 0054 的联网规则：只在用户显式触发时联网、只访问清单列出的主机）。
- **验收**：`\alpha + \beta` 离线转成 `α + β`；AI 结果有预览可核对；图片超限被拒。

### 4.9 Android AI 接入与 Buffer 快捷栏

- **继承 XSIME**：候选栏空闲时显示 翻译 / 快问 / 润色 / 作诗 / 画画 五个入口（68 × 30 dp），点按开启 Buffer 并选中插件，长按打开设置；
  OpenAI 兼容服务、HTTPS、换主机须重填密钥；密钥用 Keystore AES-GCM；10 s / 20 s / 60 s 时限；输出 ≤2048 token、≤16384 UTF-16、
  报文 ≤1 MiB；SSE 须完整校验（`finish_reason` + `[DONE]`）后才能发送；无请求历史、无自动重试、不跟随重定向；
  网络错误保留原文、不回落到 Mock；关闭在线 AI 时 Mock 必须明确标注；私密 / 密码框无入口。
- **Moodflow**：AI 配置格式与 Hub 网关一致（同一套 `secrets` 规则），但 Android 没有 Hub，网关逻辑需要一个移动端可用的实现
  （把 `moodflow_hub/gateway` 的纯逻辑部分抽成可在 Android 编译的 crate，或在 Kotlin 侧按同一契约实现）——这是本项最大的设计点，开工前要定。
  Windows 上同样的五个动作做成 `BufferAction` 插件，复用 Hub AI。
- 「画画」：XSIME Android 只产出文字提示词，不生成图片；Moodflow 照此。
- **验收**：私密字段无入口；断网保留原文；关闭在线 AI 时 Mock 有标注；换主机不带旧密钥。

### 4.10 插件商店页与社区页

- **Moodflow**：两端插件页的「商店」标签页基于 `moodflow-market`（验签索引、更新与撤回、原子安装）；「社区」页只放链接
  （讨论区、提交插件的说明、市场索引仓库），不内嵌第三方网页登录。依赖插件计划的 `moodflow-plugins` 索引仓库与签名密钥
  （[plugin-system.md](plugin-system.md) §11 P5–P8）。
- **验收**：安装时列出能力并要求同意；撤回后客户端停用；Android 上的安装路线先过 Google Play 政策核查。

### 4.11 Android 键盘宠物轮换

- **继承 XSIME**：键盘左上角一只宠物，点按轮换皮肤，长按打开选择器（选择器里也能进「键位与方案」）；Buffer 打开时只显示一个宠物控件。
- **Moodflow**：素材用 Hub 的桌宠（晴晴与导入的 Codex / petdex 桌宠，ADR 0044）的静态帧；XSIME 也没有动画，动画留到以后。
  可选：宠物表情随 Moodflow 的天气变化——但 Android 没有心情感知，**第一版不做**。
- **验收**：不影响键盘高度与触摸热区；皮肤选择持久化。

## 5. 阶段与工作量（粗估，单位：小时）

| 阶段 | 内容 | 主要产出 | 估算 |
|---|---|---|---|
| **Q0 立约** | ADR 0057（已写）；产品书 V1.7；许可证清单（§6）；Android AI 网关的实现路线；密码库与同步加密方案交 E 评审 | 文档 | 30 |
| **Q1 资料库** | `moodflow-capsule`、`moodflow-cite`（单条格式化 + DOI / BibTeX）、`moodflow-translate`（CC-CEDICT 索引与构建下载）；Hub 资料库窗口与输入时底栏；Android 主应用页面与键盘面板 | 笔记、资源、文献、翻译两端可用 | 220 |
| **Q2 密码库** | `moodflow-vault`；Windows DPAPI / Hello、Android Keystore / 生物识别；两端界面 | 密码库两端可用，安全评审通过 | 120 |
| **Q3 捕获** | `moodflow-capture` 辅助进程（截图、滚动、OCR、二维码、贴图、标注、录屏、GIF）；Android 图片 OCR / 识码 | Windows 捕获全套；Android OCR | 380 |
| **Q4 工作区** | `moodflow-mailbox`；对话（两端）、收件（回环网关 + MCP / 分享）、终端（Windows ConPTY） | 工作区可用 | 220 |
| **Q5 同步** | `moodflow-sync`；文件夹与 WebDAV；端到端加密；冲突界面 | 两设备同步 | 200 |
| **Q6 AI 动作** | Android AI 网关与五个快捷入口；LaTeX（AI + 离线 Unicode + 预览）；文献 Reference（检索 + 防编造） | AI 动作两端可用 | 180 |
| **Q7 商店与宠物** | 两端插件商店页与社区页（依赖插件 M3）；Android 键盘宠物 | — | 60 |
| **合计** | | | **约 1,410** |

建议顺序：Q0 → Q1 → Q2 → Q6 → Q4 → Q3 → Q5 → Q7。资料库是笔记、文献、捕获、同步的共同地基，先做；截图录屏与同步风险最高，放后面，
不阻塞其他项。每个阶段同样分「代码通过 / 真机可用 / 可发版」三段验收。

## 6. 许可证与第三方

| 来源 | 许可 | 用途 | 处理 |
|---|---|---|---|
| XSIME `CapsuleReference.swift` | Apache-2.0 | 移植为 `moodflow-cite` | 保留版权头，登记 `NOTICE.md` |
| XSIME 其余功能 | Apache-2.0 | 只参考行为契约 | 不复制代码则无需登记；若移植片段同上 |
| CC-CEDICT | CC BY-SA 4.0 | 本机中英词典 | 数据与派生索引单独按 CC BY-SA 分发，随包附原始头部、署名、修改说明、许可全文；构建时下载并锁定哈希 |
| CSL 样式 | CC BY-SA 3.0 | 文献样式 | 单独登记 |
| `hayagriva` | MIT / Apache-2.0 | CSL 引擎 | 依赖登记 |
| `alacritty_terminal` / `vte` | Apache-2.0 / MIT | 终端解析 | 依赖登记（XSIME 的 SwiftTerm 署名缺口提醒我们必须登记） |
| zxing-cpp / ZXing / `rqrr` | Apache-2.0 / MIT 等 | 二维码 | 依赖登记 |
| PaddleOCR / Tesseract 模型 | Apache-2.0 | Android OCR | 模型文件随包，登记 |
| KaTeX | MIT | LaTeX 预览 | 依赖登记 |
| Crossref / OpenAlex / Semantic Scholar API | 各自条款（OpenAlex 数据 CC0） | 文献检索 | 遵守限流，Semantic Scholar 需申请 key |

## 7. 待定问题

1. Android 的 AI 网关：抽出可在 Android 编译的 Rust crate，还是 Kotlin 侧按契约重写？（建议抽 crate，两端一份规则。）
2. Android 密码能否插入密码框（XSIME 禁止）？还是另做 AutofillService？
3. 同步的明文选项要不要提供？（建议提供但默认关，开启时明确提示。）
4. Windows OCR 依赖系统中文语言包，没装时是引导安装还是随包带 PaddleOCR？（体积与质量取舍。）
5. 工作区终端第一版做自绘终端还是先「在 Windows Terminal 中打开」？
6. 文献检索默认用哪家公开 API（OpenAlex 免 key，Semantic Scholar 质量好但要 key）？

## 8. 进度（2026-10-18）

全部 ⬜ 未开工。Q0 中的 ADR 0057 与本计划已写；产品书 V1.7 同步。
