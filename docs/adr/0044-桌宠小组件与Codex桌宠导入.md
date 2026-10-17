# 0044 桌宠小组件与 Codex 桌宠导入
- 状态：已接受（用户提出，2026-10-17）
- 日期：2026-10-17；提议人：D（按用户要求实现）；待 E（文案、伦理）、A（安装包）事后评审，不同意的可直接改

## 背景
用户希望小组件不再是一张卡片，而是一只像 Codex 桌宠那样站在桌面上的宠物：会动、有表情，并且能直接导入 Codex 桌宠的资源换宠物。
07 FR-WGT-01～08 写的是 320 × 168 的圆角卡片（小精灵在左、状态行与一句话区在右）。Codex 桌宠是一个文件夹：`pet.json`
（`id`、`displayName`、`description`、`spritesheetPath`）加一张精灵图：8 列，每格 192 × 208，9 行（v1）或 11 行（v2，多出的两行
留给别的应用），九行依次是 idle、running-right、running-left、waving、jumping、failed、waiting、running、review，帧数与一轮时长固定
（6/1100 ms、8/1060、8/1060、4/700、5/840、8/1220、6/1010、6/820、6/1030）。petdex 用同样的格式，装在 `~/.petdex/pets/`。

## 决策
1. **小组件就是桌宠，不另开窗口。** 透明窗口里，宠物站在底部；原来卡片上的东西（状态行与 ✎、角标、一句话区、底栏）放进宠物头顶的
   **气泡**。气泡平时收着（仍在页面里，读屏照样读得到），鼠标停在宠物或气泡上、键盘聚焦、来了新消息（亮 8 秒）、开着解释 / 自评 /
   专注 / 换装面板时出现。窗口 300 × (156 + 宠物高)；鼠标不在宠物和气泡上时窗口**点穿**到桌面（每 80 ms 读一次光标位置，
   `setIgnoreCursorEvents`）。贴边隐藏、吸附、按显示器记住位置、全屏时隐藏、右键菜单、`Ctrl+Alt+W` 都照旧。
2. **精灵图格式统一用 Codex 的。** 内置的“晴晴”由 `tools/gen_pet_sheet.py`（Pillow）画出，放在 `hub_templates/pets/qingqing/`：
   标准 `spritesheet.png`（拿到 Codex 里也能用）之外，多给 `body.png`（身体蒙版）和 `lines.png`（线条、五官、影子），小组件用
   `mask-image` 把身体按当前天气的设计令牌（`--xq-w-*`）上色——“看颜色知道心情天气”（DS-COLOR）不丢。导入的宠物不随天气变色，
   旁边挂一个 24 px 的小天气精灵。换装（FR-WGT-08）只叠在内置晴晴身上，且只在平时的三种姿态里显示。
3. **状态 → 动画。** 平时：专注中 review，闭眼（暂停 / 无痕 / 密码框）waiting，雨天与夜里 waiting，其他 idle；**不会因为心情不好播
   failed**（C-ETH：不评判用户）。反应：打错字 jumping 一次（取代“晃一下”）；新的暖心话、提醒、周信 waving 两轮（取代“靠近”）；
   单击 waving；扔出去落地 jumping；我们自己的操作失败 failed 一次；专注结束 running 两轮；**求助入口出现时不表演**；闭眼时一律不表演。
4. **动起来。** 按住拖动（往哪边拖播哪边跑）；甩出去（松手速度 ≥ 0.6 px/ms）受重力落到工作区底边、撞墙反弹、落地弹几下再停，
   轻轻放下就停在原处；停下后照旧吸附、记住位置。在地上时每 25–70 秒沿底边散步 80–320 px（0.07 px/ms）；专注、闭眼、悬停、
   开着面板或菜单、没在地上、系统要求减少动态效果时不散步，散步途中这些情况一出现就停。减少动态效果时只画每段第一帧、不播反应、
   不扔。
5. **导入。** 设置“常规 → 桌宠”：选 `pet.json`、宠物 zip 包或一张精灵图导入；Codex（`$CODEX_HOME/pets`，默认 `~/.codex/pets`）和
   petdex（`~/.petdex/pets`）里装过的宠物列出来一键导入。外壳（`src-tauri/src/pets.rs`）只读文件头认尺寸（PNG / WebP 的 VP8、VP8L、
   VP8X），要求 8 列、9 或 11 行、可整体等比缩放但每格落在整像素上，单张 ≤ 24 MB；zip 最多看 256 个条目，只取 `pet.json` 和它旁边
   的精灵图；清单里的 `spritesheetPath` 只认同一文件夹里的文件。复制到数据目录 `hub\pets\<编号>\`，只留 `pet.json` 和
   `spritesheet.<png|webp>`，先写临时文件夹再整个换掉。编号整理成小写字母、数字、`-`、`_`；不能顶替内置的 `qingqing`。
   图片以 data URL 交给前端（CSP 已允许 `img-src data:`）。宠物包是用户自己选的图片、不含个人数据，“删除全部数据”时与模板覆盖一样保留。
6. **设置键**（`settings.rs`）：`pet.id`（文字，格式 `pet_id`，出厂 `qingqing`；找不到时用晴晴）、`pet.size`（small / medium / large
   = 0.5 / 0.75 / 1 倍，出厂 medium）、`pet.roam`（出厂开）。命令 `pet_list`、`pet_sheet(id)`、`pet_import(path)`、`pet_discover`、
   `pet_remove(id)`（删的是正在用的就换回晴晴并推 `settings:changed`）。小组件权限加 `set-ignore-cursor-events`、`cursor-position`。

## 影响
- 07 第 2 节小组件窗口尺寸、FR-WGT-01（窗口形态、点穿、扔与散步）、FR-WGT-02～05（内容进气泡）、FR-WGT-03（天气用身体颜色或
  小天气精灵表示；“晃一下”“靠近”换成 jumping / waving）、FR-WGT-08（换装只在内置晴晴上）、FR-SET-03（桌宠一组）；
  10 第 5.1 节加五个命令、第 6.2 节加三个键；15 第 1 节加 `hub_templates/pets/qingqing/`；09 删除范围写明 `hub\pets\` 保留。
- 原来的 `WeatherStage` 不再用于小组件（看板等处的 `WeatherSprite` 照旧）；精灵图读不到时小组件退回 `WeatherSprite`。
- 风险：点穿靠轮询光标，80 ms 内的点击可能落在透明区；Windows 上的效果需要真机确认（A-11 兼容测试一并看）。
