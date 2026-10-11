# D 前端与视觉 · 交接文档

> 负责范围（产品书 13 第 1 节）：WGT 小组件、DSH 看板、SET 设置、ONB 引导、NTF 系统通知、REV-04 晴天收集的界面、设计规范；
> 另是前后端绑定 `xinqing_hub/src/api/bindings.ts` 的契约负责人（13 第 3.1 节）。
> 任务清单与估算见产品书 16 第 2.4 节。本文件随 D 的每个 PR 更新，任务中途换人时按产品书 13 第 3.1 节“交接”直接看这里。
> 最后更新：2026-10-11（[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86)：D-02～D-09 收尾——看板六页、卡片层、设置九个分类、对话补齐、全局快捷键、无障碍走查）

## 1. 任务状态

| 编号 | 任务 | 状态 | 代码 / PR | 还差什么 |
|---|---|---|---|---|
| D-01 | 前端骨架：Vite + Vue 3、设计令牌、类型生成封装、多窗口 | 完成 | `xinqing_hub/src/`：`styles/tokens.css`、`api/`、`i18n/`、`windows/shared/mount.ts`、`vite.config.ts` | DS-ICON 要求的 Lucide 图标库还没引入：目前十几处图标都是内联 SVG（1.5 px 圆头描边，与 Lucide 同风格），需要成套图标时再引入，记得在“关于”页列许可 |
| D-02 | 小组件（布局、小精灵、一句话区、底栏、交互、贴边） | 完成（P2 项除外） | 第一～五部分见第 3 节（#14、#16 等已合并）；一句话区消息队列、底栏、菜单“待确认日程与待办（n）”、研究 / 演示模式角标、“晃一下 / 靠近”、`Ctrl+Alt+W`：[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86) | 菜单里的开始 / 结束专注、换装，底栏的专注倒计时：FR-RST-10、FR-REV-04 都是 P2，后端还没有 |
| D-03 | 小精灵插画与 lottie 动画（6 天气 + 3 一次性动作）、logo | 代码部分完成 | `components/WeatherSprite.vue`：SVG + CSS 动画——每种天气 2–4 秒循环（呼吸、眨眼，加上光芒转、云飘、雨落、闪电每 3 秒闪一下、月亮浮、风线摆），一次性动作 `play('shake' \| 'approach')`，“闭眼”即 `eyesClosed`；`WeatherStage.vue` 交叉淡化。“晃一下”接 `mood:typo`、“靠近”接新暖心话（[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86)）。logo 矢量版 `components/AppLogo.vue`（与 `tools/gen_hub_icons.py` 同一套几何），用在“关于”和引导页标题旁（[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86)） | 正式插画、lottie 素材与正式应用图标要设计稿（Claude 做不了插画定稿）；素材到位后只换 `WeatherSprite.vue` 的内部实现，对外接口（`weather` / `eyesClosed` / `play()`）不变 |
| D-04 | 卡片层（日程 / 待办 / 提醒 / 自评 / 小结 / 周信等 8 类） | 完成 | `windows/cards/`（[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86)）：新窗口 `cards`（ADR 0036 第 7 条），贴着小组件上方（小组件在上半屏时放下方），最多 2 张、其余排队，不抢焦点，Enter 主操作、Esc 收起；日程 / 待办卡片（识别中、AI 识别、标记、时间重叠，60 秒收起）、到点提醒与错过的提醒（`reminder_missed_take`，只显示一次）、休息提醒（从小组件挪来）、负面自评后的回应与“和晴晴聊聊”、晚间小结、周信提示。日程卡片的“修改”在看板日程页打开编辑框 | 卡片层跟随小组件的位置只在浏览器里看过，要在 Windows 真机上和吸附、贴边、多显示器一起走查 |
| D-05 | 首次引导与同意 | 完成 | `windows/onboarding/`：FR-ONB-01～04；FR-ONB-05（xinqing-ime#64 已合并）——`AiStep.vue`（Jev 与大模型的地址和密钥，已保存的只显示末 4 位、留空不改，“测试连接”先保存再逐个模型测，可跳过进离线模式）、`PrefsStep.vue`（小组件放哪个角、`care.level`、四种休息提醒开关，改了就存）；完成后小组件由晴晴打招呼（`shared/firstRun.ts`） | 引导页不让改大模型的模型列表（沿用已有的或用默认），完整的放设置页“AI 服务”（FR-SET-08）；Jev 没有测试接口，只做说明 |
| D-06 | 对话窗口（流式、标识、求助卡片 UI） | 完成 | `windows/chat/`：`useChat.ts`（会话、流式事件、先到事件暂存）、`App.vue`（AI 说明、历史抽屉、气泡与 `AI 生成` 标签、复制、停止 / 重试）、`SafetyCard.vue`（求助卡片，折叠成一行不可移除）；与 C-07/C-08 同一个 PR，ADR 0018；快捷指令（吐槽 / 理一理 / 呼吸，`BreathingGuide.vue`）与“记住”确认条由 C 补上（#57），D 已走查样式；[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86)：快捷指令“📝 写成情绪日记”（共用看板的 `shared/DiaryEditor.vue`，带当前会话起草）、历史抽屉的本地搜索（`chat_search`）与保留期限（`chat.retention_days`）、标题栏置顶切换（`capabilities/chat.json`）、看板点开某段对话（`xq:chat-open`）；`Ctrl+Alt+Q` 由全局快捷键打开 | —（求助卡片上的学校心理中心电话已接设置 `safety.school_phone`） |
| D-07 | 情绪看板 | 完成（P2 项除外） | `windows/dashboard/`：左侧六页导航（#70 已合并）；[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86) 补齐——今日（四张概要卡片、打字心电图 `typing:pulse`、状态时间线（点一段看解释，自评实心圆点，逐段列表）、今天的暖心话、写日记）、情绪日历（主导天气 / “—”、日记 📓，点某天看详情）、周报（一句话总结、休息完成率与饮水环比、新增日程与完成待办、状态分布、7 × 24 热力图、每日输入时长、按天查看表格、作息洞察）、日程与待办（待确认 / 已添加 / 已过期 / 已忽略、编辑、加入我的日历）、信箱、对话与日记。读取命令见 ADR 0036。每张图有读屏概括与表格视图 | 周报的专注总时长（FR-RST-10）与晴天值（FR-REV-04）是 P2，后端有数据后加数字卡片；状态分布用横条图而非 07 写的环形图（ADR 0036 第 11 条），待改产品书 |
| D-08 | 设置中心（schema 表单生成器 + 9 个分类） | 完成 | `windows/settings/`：九个分类按 FR-SET-01 排序。此前已合并：输入法（#36、#39、#41）、关怀（#67）、AI 服务（#66）、隐私（#69）、关于。[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86) 补齐：常规（总开关、小组件、颜色模式、全局快捷键与冲突提示，替换原“外观”）、感知（同意 ② / ⑤、不感知 / 只感知的程序、记住无痕状态、无痕模式快捷键、基线进度与重新熟悉）、提醒、日程与待办（同意 ③）、温柔改写（同意 ⑥、快捷键、今日次数）；关怀补晚间小结与周信，AI 服务补接口状态表，隐私与关于补参加研究与开发者选项；每页“恢复默认”（先确认，只写与出厂值不同的项；输入法里的项用新命令 `ime_config_defaults`，ADR 0036 第 10 条）与底部“已保存” | “隐私”的删除全部数据（FR-DAT-04）、导入（FR-DAT-07）、隐私说明全文要 E 的命令和文本；黑 / 白名单的“从正在运行的程序中选择”要外壳新增列进程的命令；温柔改写的默认风格与文本来源开关还没有设置键（核心按 Tab 轮换风格、先取最近上屏再取剪贴板，页面照实说明）；常规的“晴天收集”、提醒的专注设置是 P2 |
| D-09 | 系统通知、无障碍与高对比度、DPI 走查 | 代码部分完成，真机走查待做 | 系统通知：休息提醒（D，`src-tauri/src/notify.rs`）、日程 / 待办 / 错过的提醒（C-05 接入同一个 `Notifier`）；令牌里有 `forced-colors` 高对比度和 `prefers-reduced-motion`，不可见时暂停动画；[xinqing-ime#86](https://github.com/young8290/xinqing-ime/pull/86) 用 axe-core（WCAG 2.1 A/AA + best-practice）在浅色、深色下走查了全部窗口、看板六页和设置九个分类，修掉小组件 / 卡片层缺读屏标题、看板小字深色对比度不足，现在没有违规项 | 150% 文本大小与 100%–200% DPI、讲述人、Windows 高对比度主题、通知弹出都要在 Windows 真机上走一遍；深色模式 `--xq-text-3` 在卡片底上只有 4.46:1，07 第 1.2 节的令牌值待调（看板小字已改用 `--xq-text-2`） |

## 2. 代码地图（D 负责的部分）

```
xinqing_hub/
├─ src/styles/tokens.css        设计令牌（07 第 1 节），深浅色、高对比度
├─ src/styles/base.css          公共样式：焦点环、减少动画、不可见暂停动画
├─ src/api/                     bindings.ts（生成，D 是契约负责人）+ unwrap / CommandError
├─ src/i18n/                    zh-CN.ts（由 hub_templates/ui_copy.toml 生成）+ t / errorText
├─ src/components/EChart.vue    ECharts 薄封装：按需注册、SVG 渲染、主题 / 尺寸变化重画、减少动态效果时不放动画
├─ src/components/chartTheme.ts 图表用色：从设计令牌解析（--xq-chart-1、--xq-surface、--xq-border、--xq-text-*）
├─ src/stores/                  status（状态快照 + status:changed）、settings（设置缓存 + settings:changed）
├─ src/components/              AppLogo（logo 矢量版）、WeatherSprite（静态小精灵）、WeatherStage（交叉淡化）、ExplainPanel（状态解释，`actions` 插槽放“准 / 不准”）
├─ src/weather.ts               天气图标、名称、不确定说法
├─ src/selfReport.ts            自评选项文案、覆盖期推算（看板时间线也会用）
├─ src/explain.ts               状态解释的拼句（state_explain 的结构 → 标题 / 说明 / 来源），看板时间线也用它
├─ src/format.ts                时长、日期、钟点的显示格式（看板、底栏、卡片共用）
├─ src/windows/<label>/         每个窗口一个入口：widget / cards / chat / dashboard / settings / onboarding
│  └─ widget/
│     ├─ App.vue                小组件界面与交互
│     ├─ statusLine.ts          状态行（特殊情形优先级）
│     ├─ menu.ts                右键菜单项（规格顺序）
│     ├─ useExplain.ts          悬停 / 聚焦状态行时取解释、开关面板
│     ├─ useSelfReport.ts       自评：开关面板、提交、跟踪“你说的”覆盖期
│     ├─ SelfReportPanel.vue    “我现在…”面板
│     ├─ placement.ts           位置计算纯函数：默认位置、吸附、按显示器记住、小标签
│     ├─ useWidgetWindow.ts     调 Tauri 窗口接口：恢复位置、拖动后吸附、贴边隐藏、置顶
│     ├─ messages.ts            一句话区消息队列的纯函数：优先级、暖心话 2 小时后淡出为今日一句（FR-WGT-04）
│     ├─ useMessages.ts         一句话区接事件：comfort:new、care:reduced、求助、周信提示、db_rebuilt_take
│     ├─ MessageLine.vue        一句话区：逐字出现、AI 生成标签、悬停 👍 / 👎 / 🔕
│     ├─ footer.ts / useFooter.ts  底栏：今日输入时长、最近的日程或未完成待办（FR-WGT-05）
│  └─ cards/                    卡片层窗口（FR-WGT-07，ADR 0036 第 7 条）
│     ├─ queue.ts / cards.ts    卡片排队（最多 2 张）与各类卡片的数据；rest.ts 休息提醒卡片的文案
│     ├─ useCards.ts            接日程 / 待办 / 提醒 / 休息 / 自评回应 / 小结 / 周信事件，reminder_missed_take
│     ├─ useCardsWindow.ts      贴着小组件摆放、按卡片高度调窗口大小、没卡片就隐藏
│     └─ CardShell.vue 等       卡片外框与 Schedule / Reminder / Rest / Note 四种卡片
│  └─ shared/bus.ts             窗口之间的 xq: 前端事件（卡片数、周信提示、看板跳页、对话窗口打开某段对话）
│  └─ shared/DiaryEditor.vue    日记编辑框（起草、来源标识），看板与对话窗口共用
│  └─ onboarding/
│     ├─ App.vue                引导步骤：年龄 → 告知 → 同意 → 增强模式 → AI 服务 → 偏好
│     ├─ AiStep.vue             AI 服务：地址与密钥、测试连接、可跳过（ADR 0012）
│     └─ PrefsStep.vue          偏好：小组件的角、关怀频率、休息提醒
│  └─ shared/aiConfig.ts        AI 服务表单 ↔ secrets_set 入参（引导页与设置页共用）
│  └─ shared/NoticeTable.vue    告知表（FR-ONB-03），引导页与设置“隐私”共用
│  └─ shared/consent.ts         同意项编号 ①～⑥、哪些会发给第三方（撤回时要说明）
│  └─ shared/firstRun.ts        引导交给小组件的偏好：放哪个角、要不要打招呼（localStorage）
│  └─ dashboard/
│     ├─ App.vue                看板左侧六页导航（FR-DSH-01），#weekly 等直接打开对应页，也听 xq:dashboard-nav
│     ├─ TodayPage.vue          今日：概要卡片、TypingEcg（打字心电图）、StateTimeline（状态时间线）、ComfortList、写日记
│     ├─ today.ts               今日的纯函数：概要卡片、时间线几何、心电图的 ECharts option
│     ├─ CalendarPage.vue / calendar.ts  情绪日历（FR-DSH-03）
│     ├─ WeeklyPage.vue / weekly.ts      周报（FR-DSH-04）：状态分布横条、热力图、每日输入时长、按天表格
│     ├─ RoutineInsight.vue / routine.ts 作息洞察（FR-REV-03）
│     ├─ SchedulePage.vue / agenda.ts    日程与待办（FR-SCH-09/13），ScheduleEditor / TodoEditor 编辑框
│     ├─ MailboxPage.vue        信箱（FR-DSH-06）
│     ├─ ChatDiaryPage.vue      对话与日记（FR-DSH-05、FR-DIA-03）
│     └─ dashboard.css          看板共用版式（前缀 d-）
│  └─ settings/
│     ├─ App.vue                左侧分类（FR-SET-01 的顺序），默认“输入法”，#about 直接打开“隐私与关于”
│     ├─ ImeSection.vue         输入法：取 schema 和配置、修改即保存、跳过原因 / 重启横幅 / 核心没运行；
│     │                         收到 ime_config:changed、窗口获得焦点、保存后都重取配置（合并重复的刷新）
│     ├─ ImeFieldControl.vue    一项配置按类型出控件（开关 / 下拉 / 数字 / 文本 / 列表；map、array 只读）
│     ├─ imeForm.ts             常用项清单（改时同步 ui_copy.toml 的 [ime.field]）、高级分组、按点分键名取值
│     ├─ hotkey.ts              快捷键：录制规则、键名与别名（hotkey.test.ts 对照清风 hotkey.rs）、用录制框的配置键清单
│     ├─ HotkeyRecorder.vue     快捷键录制框：点一下录制，Esc 取消、退格清除，重复时提示
│     ├─ GeneralSection.vue     常规（FR-SET-03）：总开关、小组件、颜色模式、全局快捷键
│     ├─ SenseSection.vue       感知（FR-SET-04）：同意 ② / ⑤、程序名单、无痕、基线
│     ├─ CareSection.vue        关怀：频率、说话风格、晚间小结、周信、同意 ④、安静时段、勿扰应用、学校电话、晴晴记住的事（FR-SET-05）
│     ├─ RestSection.vue        提醒（FR-SET-06）；AgendaSection.vue 日程与待办（FR-SET-07）；RewriteSection.vue 温柔改写（FR-SET-10）
│     ├─ care.ts                设置页的输入校验（与后端 SettingItem::accepts、memory_entry 同一套规则，含研究编号）
│     ├─ AiSection.vue          AI 服务：地址与密钥、大模型优先级、测试连接、每日上限与今日用量、接口状态（FR-SET-08）
│     ├─ AboutSection.vue       隐私与关于：PrivacySection、ResearchDevSection（参加研究、演示模式），下面是“关于”
│     ├─ PrivacySection.vue     隐私：告知表、同意与撤回、最近出网记录、导出我的数据（FR-SET-09）
│     ├─ SectionHead.vue        分类标题与“恢复默认”（先确认）；settings.css 各分类共用版式（前缀 s-）
│     ├─ useHubSettings.ts      Hub 设置键：读、修改即保存、“已保存”提示、按 settings_schema 恢复默认
│     ├─ useImeKeys.ts          存在输入法里的几项（xinqing.*）：读写、ime_config_defaults 恢复默认
│     ├─ useConsents.ts         分类页里的单独同意开关，撤回时如实说明
│     └─ licenses.ts            开源许可条目；新增随包分发的组件 / 字体 / 素材 / 图标库时在这里加
├─ core/src/infra/imeconf.rs   wind-rpc 客户端（读写输入法配置，ADR 0016）
├─ src-tauri/src/commands/ime.rs  ime_schema / ime_config_get / ime_config_defaults / ime_config_set
├─ core/src/domain/dashboard.rs  看板统计的纯函数（主导天气、时间线、周统计、一句话总结），ADR 0036
├─ core/src/domain/hotkey.rs     Hub 全局快捷键的格式与校验；src-tauri/src/hotkeys.rs 注册与冲突
├─ src-tauri/src/commands/dashboard.rs  comfort_list / day_stats / month_moods / week_stats
├─ src-tauri/src/pulse.rs        mood:typo、typing:pulse 两个瞬时事件
├─ src/api/bindings.test.ts      守住生成的绑定：时间戳导出成 number（ADR 0027）
├─ core/src/domain/notify.rs     系统通知的文案与按钮（FR-NTF-01），读 ui_copy.toml 的 [notify]、[rest]
├─ src-tauri/src/notify.rs       系统通知：Windows toast + 按钮回调交回休息服务；非 Windows 不弹（退回小组件卡片）
├─ src-tauri/src/ime_events.rs  常驻线程读 wind-rpc 事件通道，转成 ime_config:changed（ADR 0017）
└─ src-tauri/
   ├─ capabilities/              default.json（各窗口共用）、widget.json / cards.json（挪动、缩放、显示、置顶）、chat.json（置顶）、settings.json（另存为）
   ├─ src/fullscreen.rs          前台全屏时自动隐藏小组件（判断逻辑 AutoHide 平台无关，探测只在 Windows）
   └─ src/windows.rs             建窗口；小组件建好先不显示，由前端定位后 show()
```

验证（在 `xinqing_hub/`）：`pnpm lint && pnpm test && pnpm build`；改了 `ui_copy.toml` 后 `pnpm gen:i18n`，并在仓库根目录跑
`python3 tools/check_templates.py hub_templates`（禁用词）。看界面：`pnpm dev` 后浏览器打开 `http://localhost:1420/widget/index.html`
（没有后端，窗口接口会失败并打警告，界面照常显示）；真窗口行为要 `pnpm tauri dev`（Linux 需要 WebKitGTK）。

## 3. D-02 小组件明细

| 部分 | 需求 | 状态 |
|---|---|---|
| 布局、状态行（暂停 / 未连接 / 冷启动优先级）、离线角标、单击打开对话、拖动 | FR-WGT-02/03/06 | 完成（骨架阶段） |
| 默认位置（主显示器右下角 16 px）、拖到边缘 24 px 内吸附、按显示器记住位置、显示器拔掉回默认位置 | FR-WGT-01 | 完成（本 PR）。位置存在 WebView 的 localStorage（`xq.widget.placement`），只是本机界面偏好 |
| 贴边隐藏：吸附在左右边缘、开了 `widget.autohide` 时，鼠标离开 3 秒收成 24 × 72 小标签，移入或聚焦展开 | FR-WGT-01 | 完成（本 PR） |
| 置顶跟随 `widget.topmost`、透明度跟随 `widget.opacity` | FR-WGT-01 | 完成（本 PR） |
| 右键菜单（右键 / `Shift+F10` / 菜单键）：暂停·恢复感知、打开看板、设置、隐藏小组件 | FR-WGT-06 | 完成（本 PR）。其余项见下一行 |
| 菜单里的开始 / 结束专注、换装、待确认日程与待办 | FR-WGT-06 | “待确认日程与待办（n）”已加（xinqing-ime#86，点了打开看板日程页）；开始 / 结束专注（FR-RST-10）、换装（FR-REV-04）是 P2，后端有了再按 `menu.ts` 顶部注释的顺序插入 |
| 天气切换 400 ms 交叉淡化 | FR-WGT-03 | 完成（本 PR） |
| 一句话区悬停显示全文 | FR-WGT-04 | 完成（本 PR） |
| 一句话区的消息优先级队列（求助 > 自评回应 > 卡片 > 休息提醒 > 暖心话 > 小结 > 今日一句 > 空闲问候） | FR-WGT-04 | 完成（xinqing-ime#86）：`messages.ts` 排序——求助入口 > 出错提示 > 暖心话 > 提示（降档、数据库重建）> 周信提示 > 今日一句 > 空闲问候。自评回应、日程卡片、休息提醒、小结按 FR-WGT-07 是卡片，挪进卡片层，不再占一句话区。暖心话逐字出现（减少动态效果时整句）、`AI 生成` 标签单独一行、2 小时后淡出为今日一句，悬停出现 👍 / 👎 / 🔕 |
| 悬停状态行显示解释 | FR-WGT-06、FR-STA-09 | 完成（第二部分）：鼠标停留 300 ms 或键盘聚焦状态行时，面板盖住右侧整列，标题（不确定说法 + 可能性）、每条说明一行、来源与冷启动附注弱化；`Esc`、失焦、移开、状态变化都会收起。每次打开都重新取 `state_explain(null)`；它和当前快照不是同一个状态时不用，退回只显示标题。可能性百分比从状态行的悬停提示挪进了面板 |
| 解释面板里的“准 / 不准” | FR-STA-07 | 完成（第四部分）：两个按钮放在来源那一行右侧，点了调 `submit_feedback('mood_state', null, …)`（记到最近一条状态记录，即当前显示的状态），换成“谢谢，我记下了”，状态变了再重新问。为了放下按钮，面板从只盖右侧一列改为盖住整个内容区（约 286 × 134 px），小精灵会被暂时盖住；按钮仍守 32 × 32 点击目标。文案在 `ui_copy.toml` 的 `[widget.feedback]` |
| 状态行 ✎ 与自评面板、自评后 60 分钟显示“你说的：…” | FR-WGT-02/03/06、FR-STA-10 | 完成（第五部分）：状态行右侧 ✎ 或右键菜单第一项“我现在…”打开面板（`SelfReportPanel.vue`，盖住内容区）——六个选项点一下就提交，备注选填（≤ 50 字，提示只存本地）；收到 `self_report:changed` 后状态行显示“🌙 你说的：有点累”，到期由本地定时器收尾；窗口打开时没有覆盖期快照，就由 `self_report_list(今天)` 的最后一条推出（`src/selfReport.ts`）。覆盖期内不弹解释面板。离线角标改为状态行内联，与 ✎ 并排 |
| 底栏：今日输入时长、下一个日程 / 待办数、专注倒计时 | FR-WGT-05 | 完成（xinqing-ime#86）：今日输入时长（`day_stats`）、未来 24 小时最近的日程或未完成待办数，点了打开看板日程页；专注倒计时是 P2 |
| 前台全屏应用时自动隐藏，退出后恢复 | FR-WGT-01 | 完成（第三部分）：外壳每秒调一次 `SHQueryUserNotificationState`（全屏应用、全屏 D3D 游戏、演示模式都算），进全屏时把可见的小组件藏起来，退出时只恢复自己藏的；用户自己隐藏的不会被叫回来，全屏期间用户叫出来的也不再藏回去 |
| 首次启动不在默认位置闪一下 | FR-WGT-01 | 完成（第三部分）：小组件配置成 `visible: false`，外壳建好后不显示，前端挪到记住的位置再 `show()`；定位失败也照样显示 |
| “晃一下”（打错字） | FR-WGT-03、DS-MOTION-02 | 完成（xinqing-ime#86）：瞬时事件 `mood:typo`（外壳限频 1.5 秒，ADR 0036 第 5 条）；新暖心话时“靠近” |
| `Ctrl+Alt+W` 显示 / 隐藏 | FR-WGT-01、FR-ENT-04 | 完成（xinqing-ime#86）：`tauri-plugin-global-shortcut`，与 `Ctrl+Alt+Q` / `D` 一起由设置键 `hotkey.*` 控制，注册不上的在设置“常规”提示（ADR 0036 第 6 条） |

## 4. 关键决定与待评审

- **右键菜单用系统原生菜单**（`@tauri-apps/api/menu` 的 `Menu.popup`），不在网页里画：小组件窗口只有 320 × 168，菜单项补齐后有 8 项，网页菜单画不下；原生菜单还自带讲述人与高对比度支持（DS-A11Y-02/04）。代价是菜单外观不走设计令牌。07 没规定菜单样式，不算偏离产品书，未写 ADR。
- **小组件位置存在 WebView 的 localStorage**，没有进 Hub 数据库：它是本机界面偏好，不是用户数据，也不需要随导出导入走；放进 `settings` 表要新增设置键（C 的契约）。以后要让“删除全部数据”也清掉它，再挪进数据库。
- **ADR 0016（输入法设置的读写与表单生成，D 提议）**：心晴不带清风的设置程序（托盘“设置”打开的是 Hub），而 wind-rpc 的 `config.schema` 只有键名、类型和枚举值、没有中文名称和范围，所以“输入法”分类改为常用项手写名称、其余按 schema 放进“高级”；客户端不依赖清风的 crate，协议版本号由测试核对。待 A、C 评审，接受后改产品书 07 FR-SET-02、10 第 5.1 节、17 第 3.4 节。
- **ADR 0017（输入法配置变更事件，D 提议）**：外壳常驻一个线程连 `xinqing_rpc{后缀}_events`（后缀在 `_events` 之前，和控制通道相反），把 `config.changed` 转成 `ime_config:changed {reason, needs_restart}`；每次连上先推一条 `connected`，核心没运行时 1～30 秒退避重连。核心自己的语言栏 / 菜单写配置不广播，所以设置窗口获得焦点时也要重新取。待 A、C 评审，接受后改产品书 10 第 5.2 节、07 FR-SET-02。
- **ADR 0027（前端绑定的数字类型，D 提议）**：specta 把所有 `f64` 导出成 `number | null`（`serde_json` 把 NaN 写成 `null`）。不会是 NaN 的字段（时间戳、`SettingValue::Number`）
  标 `#[specta(type = specta_typescript::Number)]` 导出成 `number`，真的可空的 `Option` 保持 `| null`；`src/api/bindings.test.ts` 守住 `ts` / `*_ts` 字段。
  `SettingValue` 属 C 的注册表契约、`SelfReportItem` 属 B，待 C、B 评审。**新加 `f64` 字段时按 ADR 0027 第 3 条处理。**
- **`bindings.ts` 契约评审（2026-10-05）**：ADR 0017 之后别的 PR 往绑定里加了对话、记忆、快捷指令、数据导出、出网记录、作息洞察、演示 / 研究模式的命令和事件，
  共约 240 行，D 逐项对照 10 第 5.1 / 5.2 节和各 ADR 看过，在 ADR 0018 / 0019 / 0022 / 0023 / 0026 的“评审”里写了**同意**和不阻塞的建议。
  `chat_search`、`ai_net_log_recent` 没有 ADR 登记，D 同意它们的形状，改 10 第 5.1 节时补上。D 自己的后续：① 所有 `f64` 导出成 `number | null`
  （时间戳其实不会为空）——已由 ADR 0027 处理；② 设置页做“关怀”“AI 服务”前，提 `settings_schema()` 只读命令（ADR 0019 评审）。
- **ADR 0012（AI 服务配置，C 提议）D 的评审意见：同意**（写在 ADR 的“评审”一节）。E 要求 release 只收 `https://`，落地时 `error.ai_config_invalid` 文案要同改；建议 `ai_config_get` 带上两侧健康状态，FR-SET-08 页面要用。
- **ADR 0010（状态解释的信号挑选与拼句）D 的评审意见：同意**（B 在 xinqing-ime#11 请 D 看界面拼句）。结构化的
  `Explanation { state, prob, signals[{kind, value}], source, cold_start }` 够界面用：`kind` 的序列化名就是
  `explain.signal.<kind>.text` 的键，`value` 统一传给 `{p}` / `{n}` / `{m}` 即可（`t()` 不认识的变量原样保留）。
  三点不阻塞的建议：① 小组件悬停时界面会排成多行（不确定说法 + 可能性、每条说明一行、来源和冷启动附注用 `--xq-text-3`），
  `note.format` 那种单行格式只给回放和评测用，建议 ADR 第 5 条写明“界面可以按自己的版式排”；② `StatusSnapshot.prob`
  是 0–1 小数，`Explanation.prob` 是 0–100 整数，同一个量两种单位，加 `state_explain` 命令时最好统一（或至少在字段名上区分）；
  ③ 实时解释最好带上它对应的窗口或时间，界面拿到后能确认它和当前快照是同一次切换，避免悬停时显示上一个状态的解释。（界面暂时用“解释的状态 = 快照的状态”判断，同一状态两次切换之间分不出来。）
- **图表用 ECharts 5（03 第 5 节技术栈）**：`echarts/core` 按需注册（目前只有折线、网格、提示框、参考线）+ SVG 渲染；看板窗口的包因此约 500 kB（gzip 167 kB），
  只在看板窗口加载。图表遵守 dataviz 规范：单系列不放图例、2 px 线、8 px 点带 2 px 底色外圈、网格一档灰实线、轴文字用文字令牌、悬停十字准线 + 提示框、
  每张图下有表格视图、读屏有一句话概括。
- **新令牌 `--xq-chart-1`**（浅 `#c47a1f`、深 `#c98128`、高对比度 `CanvasText`）：主色 `#f29e38` 作图表线对白底只有 2.2:1，深色模式又太亮（OKLCH L 0.80），
  图表另取同色相深一档，用 dataviz 的校验脚本核过（对比 ≥ 3:1、L 在图表带内）。**07 第 1.2 节的令牌表待补这一行**（改产品书时一并做）。
- **系统通知用 `tauri-winrt-notification`（仅 Windows 依赖）**：官方 `tauri-plugin-notification` 在桌面端不支持按钮，FR-NTF-01 要“知道了”/“5 分钟后”。
  只在小组件窗口看不见时弹（`rest.rs` 的 `show()` 判断窗口可见性，B 在那里留了接入点）；弹出来就不再推送 `rest:due`，免得小组件再出现时又看到同一张卡片；
  弹不出来（非 Windows、文案没加载、系统拒绝）照旧推送 `rest:due`。“知道了”等同卡片上的“已完成”（该类计时清零），点通知正文不算任何操作。
  专注助手由系统处理（C-PLT-09）。应用标识：发行版用 `identifier`（`com.xinqing.hub`），要靠安装包建开始菜单快捷方式登记（A-10）；调试构建借用 PowerShell 的标识。
- **系统“另存为”对话框用官方插件 `tauri-plugin-dialog`**（`@tauri-apps/plugin-dialog`）：`data_export(path)` 要前端给保存路径（E 的 xinqing-ime#47）。
  权限只给设置窗口、只开 `dialog:allow-save`（`src-tauri/capabilities/settings.json`）；对话框只把用户选的路径交回前端，读写文件仍在 Rust 端。
  以后“导入”要选文件时再加 `dialog:allow-open`。插件属 Tauri 项目，许可同 Tauri（“关于”页已列）。不涉及 `bindings.ts` 等契约，未写 ADR。
- **ADR 0036（看板与小组件的读取命令和瞬时事件，D 提议，xinqing-ime#86）**：四个只读命令（`comfort_list` / `day_stats` / `month_moods` / `week_stats`）、
  `comfort:new` 加 `trigger`、瞬时事件 `mood:typo` / `typing:pulse`（只有键间间隔与是否退格，不含键值）、全局快捷键设置键、卡片层窗口、
  两个“只取一次”的命令、设置页“恢复默认”的 `ime_config_defaults`、周报状态分布用横条图。没有新迁移。待 B、C、E 评审，接受后改 07、10、15。
- **状态分布不用环形图**（ADR 0036 第 11 条）：天气色按图表配色规则校验在正常视力下就分不开（多云 / 小雨 ΔE 7.0），扇区又只能靠颜色区分；
  改为单色横条、按占比排序、直接标名称与百分比。天气色只用在小精灵和图标上，不拿来区分图表系列。
- **设置“恢复默认”的范围**：只改偏好。总开关 `xinqing.enabled`、各项同意、学校电话、晴晴记住的事、AI 地址与密钥、研究编号与演示模式都不随它改；
  所以“隐私与关于”没有“恢复默认”按钮（这一页没有偏好类的项）。
- **对话窗口的置顶记在 WebView 的 localStorage**：和小组件位置一样是本机界面偏好，不进数据库。
- **卡片层是独立窗口**：卡片 320 × ≤ 220、最多 2 张，放进小组件窗口（320 × 168）会盖住小精灵；独立窗口不抢焦点、能贴着小组件上下摆放。
- **小标签 24 × 72**：07 只规定了宽 24 px，高度是这里定的（够露出 20 px 的小精灵）。24 px 宽低于 DS-A11Y-03 的 32 px 点击目标，但小标签不需要点击——鼠标移入或键盘聚焦就展开，所以没有按 32 px 做。如果评审认为要守 32 px，07 的 24 px 也要一起改。

## 5. 已知问题

- `status.paused` 同时表示“用户暂停”和“无痕 / 密码框 / 黑名单”，菜单在密码框里也会显示“恢复感知”，点了不会解除闸门；要区分需要后端在快照里分开两个字段（B / A）。
- 吸附靠“最后一次移动事件后 400 ms 没再动”判断拖动停下。拖着不松手停顿超过 400 ms，窗口会先吸附一次，继续拖时 Windows 会把它拉回鼠标处，只是一下跳动。Tauri 没有“拖动结束”事件，暂时这样。
- 全屏检测只在进出全屏的那一刻动手：如果小组件恰好是在全屏期间才建出来的（例如在全屏游戏里完成了引导），它会一直显示到下次进出全屏。
- 自评覆盖期跨过午夜时（23:30 自评、00:10 重开小组件），当天的 `self_report_list` 里没有那一条，重开后不再显示“你说的”；要彻底解决得让快照带上覆盖期（B 的契约）。
- 快捷键录制框在 Hub 窗口里录：输入法自己的快捷键（例如 `Ctrl+Shift+E` 轮换方案）可能先被核心吃掉，录不到；`Win` 组合多数被系统占用。都要在 Windows 真机上走一遍。
- “输入法”分类：wind-rpc 的 schema 没有数值范围和说明文字，高级区只显示键名；方案名来自 ui_copy 的 `[ime.schema]`，核心新增方案时显示原 id，需要补文案。没在真机上连过清风核心。
- 系统通知只在 Linux 上编过、在 MSVC 目标上单独核对过 toast 的调用，没有在 Windows 真机上弹过；发行版要确认安装包登记了应用标识，否则 toast 不显示。
  小组件“贴边隐藏”时窗口仍算看得见，提醒走卡片（贴边小标签不会自动展开）——要不要也改走通知，真机试过再定。
- 设置“隐私”：撤回 ① 时 09 FR-DAT-05 要“询问是否同时删除已有的情绪数据”，还没有删除情绪数据的命令（E-01），目前只确认一次再撤回；
  删除全部数据（FR-DAT-04）、导入（FR-DAT-07）、隐私说明全文（E-06）都还没有，页面底部写了“还在准备中”。
- “关怀”页的输入校验（`settings/care.ts`）抄了后端 `SettingItem::accepts` 的规则，后端只回笼统的 `settings.out_of_range`；后端规则改了要同步改这里（后端仍会拒绝，只是提示变笼统）。
- 安静时段用 `<input type="time">`，显示 12 / 24 小时制跟系统区域设置走；存进去的始终是 `HH:MM`。
- 窗口行为（吸附、贴边隐藏、多显示器、卡片层跟随小组件）只有纯函数单测和组件测试，还没在 Windows 真机上走查（100%–200% 混合缩放、任务栏在左 / 上）。
- 设置“常规”的总开关、“感知”的名单与无痕、“温柔改写”的快捷键存在输入法配置里：核心没运行时这几项不可改（页面有说明），恢复默认也只在核心运行时出现；
  `ime_config_defaults`（wind-rpc `config.getDefaults`）只在单测里对过请求形状，没连过真核心。
- Hub 全局快捷键与输入法快捷键的重复检测只比设置里的值；被别的软件占用要靠注册结果（`hotkey_status`），只在 Linux 上跑过注册逻辑的单测。
- 打字心电图在看板可见时每秒最多收 10 次 `typing:pulse`、≤ 5 Hz 重画；低配电脑上的 CPU 占用没量过（DS-PERF）。
- 深色模式 `--xq-text-3` 在卡片底（`--xq-surface`）上是 4.46:1，12 px 小字差一点到 4.5:1；看板已改用 `--xq-text-2`，其余用 text-3 的地方（日期、计数）
  在页面底色上过了 axe 检查。令牌值待 07 第 1.2 节一起调。

## 6. 下一步（按优先级）

1. Windows 真机走查（D-09）：小组件与卡片层的窗口行为、全局快捷键与录制框、系统通知、100%–200% DPI 与 150% 文本大小、讲述人、高对比度主题；
2. ADR 0036 评审（B、C、E），接受后改产品书：07 FR-DSH-03/04（主导天气口径、状态分布横条图）、07 第 1.2 节（`--xq-chart-1`、深色 `--xq-text-3`）、
   10 第 5.1 / 5.2 / 6.2 节（命令、事件、`hotkey.*`）、15 第 2 节（`weekly_line.toml` 已加载）；
3. 设计稿到位后替换小精灵插画、lottie 动画与应用图标（D-03）；
4. E 的删除全部数据、导入、隐私说明全文到位后补“隐私”（FR-SET-09）；外壳加列进程的命令后补“从正在运行的程序中选择”（FR-SET-04）；
5. P2：专注（小组件菜单与底栏、周报、设置“提醒”）、晴天收集与换装（FR-REV-04）。

## 7. 修订记录

| 日期 | 改动 |
|---|---|
| 2026-10-04 | 初版：盘点 D-01～D-09 现状；D-02 第一部分（小组件窗口行为、右键菜单、交叉淡化） |
| 2026-10-04 | D-02 第二部分：悬停状态行显示解释（接 B 的 `state_explain`） |
| 2026-10-04 | D-02 第三部分：前台全屏时自动隐藏小组件、首帧不闪 |
| 2026-10-04 | D-02 第四部分：解释面板里的“准 / 不准”（接 B 的 `submit_feedback`） |
| 2026-10-04 | D-03：小精灵 SVG + CSS 循环动画与一次性动作（lottie 前的占位） |
| 2026-10-04 | 修 `Explanation.prob` → `prob_pct` 改名后 main 上的类型错误（xinqing-ime#23）；设置中心加左侧分类与“关于”、logo 矢量版 |
| 2026-10-04 | D-02 第五部分：“我现在…”自评面板、状态行“你说的”（接 B 的 `self_report_*`） |
| 2026-10-05 | D-08 契约：ADR 0016、wind-rpc 客户端与 `ime_*` 三个命令 |
| 2026-10-05 | D-08 第二部分：设置中心“输入法”分类（常用项 + 高级区，修改即保存） |
| 2026-10-05 | D-08 契约：ADR 0017、wind-rpc 事件通道读线程与 `ime_config:changed` 事件 |
| 2026-10-05 | D-08 第三部分：“输入法”页接 `ime_config:changed` 与窗口焦点刷新 |
| 2026-10-05 | D-06 第一版：对话窗口（随 C-07/C-08，ADR 0018） |
| 2026-10-05 | D-08 第四部分：快捷键录制框（键名对照清风 hotkey.rs）、中英切换键五选一、快捷键中文名称 |
| 2026-10-05 | D-06：C 补上快捷指令按钮、呼吸引导与“记住”确认条（FR-CHT-06/07），待 D 评审样式 |
| 2026-10-05 | D-06 走查：对话窗口的点击目标、AI 标签样式、输入中动画、键盘焦点与高对比度 |
| 2026-10-05 | `bindings.ts` 契约评审：ADR 0018 / 0019 / 0022 / 0023 / 0026 写入 D 的意见 |
| 2026-10-05 | `bindings.ts` 契约：ADR 0027，不会是 NaN 的 f64（时间戳、设置数值）导出成 number |
| 2026-10-05 | D-05：首次引导补上 FR-ONB-05（AI 服务、偏好、完成后晴晴打招呼） |
| 2026-10-05 | D-08：设置中心“AI 服务”分类（FR-SET-08）；AI 服务表单文案与逻辑由引导页、设置页共用 |
| 2026-10-05 | D-08：设置中心“关怀”分类（FR-SET-05）；求助卡片显示设置里的学校心理中心电话 |
| 2026-10-05 | D-08：设置“隐私”部分（FR-SET-09）：告知表、同意与撤回、出网记录、导出；引入 tauri-plugin-dialog |
| 2026-10-06 | D-07：情绪看板六页导航、今日（当前天气、自评）、周报作息洞察；引入 ECharts 与图表令牌 `--xq-chart-1` |
| 2026-10-06 | D-09：系统通知——小组件看不见时休息提醒改弹 Windows toast（“知道了”/“5 分钟后”） |
| 2026-10-11 | xinqing-ime#86：ADR 0036；D-02 收尾（一句话区队列、底栏、晃一下、全局快捷键）、D-04 卡片层、D-07 看板六页、D-08 设置九个分类与恢复默认、D-06 对话补齐（写日记、搜索、置顶、保留期限）、D-03 引导页 logo、D-09 axe 无障碍走查 |
