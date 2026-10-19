# 产品展示视频

[`moodflow-showcase.mp4`](moodflow-showcase.mp4)：Moodflow 产品展示视频，1920×1080、30 fps、H.264，时长 5 分 0.5 秒，已混入音效轨（打字、卡片弹出、转场、组合键、logo 铃声），**不含配乐**。

- 内容以情绪功能为重点：情绪部分是一张连续的桌面，在聊天、文档窗口里真实打字，右下角晴晴卡片实时跟着变天，依次演示日程待办、打字心电图与判断理由、主动报告心情、对话与危机安全、温柔改写、休息与喝水、密码框闭眼与隐私设置、晚间小结与周信、情绪看板；之后是输入法、效率工具，最后是 Android 预览。
- 画面是依据产品文档做的演示动画，按功能全部完成的样子展示，不是真机录屏。
- 配乐建议、分段卡点和后期注意事项见 [配乐与剪辑说明.md](配乐与剪辑说明.md)，每场的起止秒数见 [timeline.json](timeline.json)。
- 换装段演示导入社区桌宠（Roxy、初音未来、悟空、蕾姆、胡桃、流萤，来自 codex-pets.net），`source/assets/skin-*.png` 由它们的 GIF 转成；形象版权归各自原作者，公开发布或商用前请先确认授权。
- logo 用的是品牌原图（`assets/brand/`），`source/assets/` 里是它的裁切副本；晴晴的图来自 `tools/gen_pet_sheet.py` 的输出，按天气预先着色。

## 重新生成

画面写在 `source/video.html` 里（每个场景一段 `scene({...})`，`window.renderAt(秒)` 画出任意时刻），由 Playwright 逐帧截图，再用 ffmpeg 编码。需要 Node + Playwright（Chromium）、ffmpeg、Python 3 + numpy，以及系统里的中文字体（文泉驿正黑）和彩色 emoji 字体。

```bash
cd docs/showcase/source
npx http-server -p 8765 -s -c-1 . &   # 页面要通过 http 打开，file:// 下 CSS 遮罩会被拦
node render.js                        # → video-only.mp4、sfx.json、timeline.json（约 8 分钟）
python3 sfx.py                        # → sfx.wav，并混成 moodflow-showcase.mp4
```

只改了音效、不想重出画面时，用 `node collect.js` 重新收集音效时间点，再跑 `python3 sfx.py`。
`render.js`、`collect.js` 里 Playwright 的路径写的是 `/opt/node22/lib/node_modules/playwright`，换环境时改成本机的安装位置。
生成的 mp4 / wav 默认留在 `source/` 里，确认后把成片复制到本目录覆盖。
