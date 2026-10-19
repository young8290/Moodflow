# 提示词模板

一张图的提示词 = **A 总规范**（每张一样）+ **B 角色设定**（按形象填一次，之后每张一样）+ **C 格子清单**（每张不同）。
这里的提示词对任何形象通用；某一只桌宠的具体设定（如内置晴晴）不写在本 skill 里，按 B 的模板单独定。
多数生图模型对英文的版式约束更听话，所以 A、C 给英文版；B 中英文都可以。每次都附**设定稿**作参考图。

---

## A 总规范（英文，原样使用）

```
A character animation sprite sheet on a single 1024x1536 portrait canvas (2:3).
Layout: an invisible 4-column x 4-row grid of equal cells, each cell exactly 256x384 px, numbered 1-16 left-to-right, top-to-bottom.
Do NOT draw any grid lines, borders, dividers, cell numbers, labels, text, captions, watermarks or signatures.
Background: one flat solid magenta color #FF00FF across the entire canvas — no gradient, no texture, no floor, no cast shadow.
Never use magenta or hot pink on the character.
In every used cell: exactly one copy of the same character, horizontally centered, feet resting on the same invisible ground line
about 64 px above the bottom edge of the cell (only jumping frames leave the ground).
Same scale in every cell — the standing character is about 200 px tall from head top to feet. No perspective, no camera zoom.
Keep everything inside its own cell with at least 20 px margin on the left and right and 24 px at the top; nothing may cross into a neighbouring cell.
Clean flat 2D vector style, bold closed outlines of one dark color, hard edges, no glow, no blur, no soft shading on the body.
Cell 15: the character in its standard front-facing standing pose (identical to the first idle frame).
Cell 16: a color swatch — three horizontal solid bars, top = body fill color, middle = outline color, bottom = main accent color (e.g. cheek blush).
Cells marked EMPTY: leave pure magenta background.
```

## B 角色设定（模板：换形象只改这一段，定稿后固定不改）

先用设定稿把形象定下来，再把定稿的特征填进下面的模板。`{…}` 是要填的地方；不变色模式可以删掉标了〔变色〕的两行。

```
Character: "{name}", {one-line concept, e.g. "a small round mascot that represents ..."}.
Proportions: {e.g. chibi, head and body merged / big head small body / about 2 heads tall}; {limbs, e.g. two short arms and two tiny feet}.
Face: {eyes}, {mouth}, {cheeks or other facial marks}.
Body fill: one single flat color {#RRGGBB} — no gradient, no highlight, no shading inside the body.      〔变色〕
Outline: {#RRGGBB}, uniform thickness, closed.
Fixed details that must appear in every cell: {e.g. a leaf on the head, a scarf; or "none"}.
Accessories and effects (sweat drops, tears, "z", stars, speed lines, props) use other colors, never the body color.   〔变色〕
Personality: {e.g. gentle and calm}; expressions stay {e.g. soft, never exaggerated}.
Readable when scaled down to 120 px tall: clear silhouette, few small details, no text or patterns.
Must look exactly like the attached reference sheet in every cell.
```

中文模板（给中文模型）：

```
角色“{名字}”：{一句话设定}。比例：{如 Q 版头身一体 / 大头小身 / 约两头身}；{四肢，如两只短短的小手、两只小脚}。
五官：{眼睛}、{嘴}、{腮红或其他脸部特征}。
身体只用一种平涂色 {#RRGGBB}，不要渐变、高光和阴影。〔变色〕
描边统一 {#RRGGBB}，粗细一致、闭合。
每一格都必须有的固定特征：{如头顶一片叶子；没有就写“无”}。
道具和特效（汗滴、眼泪、z、星星、速度线等）用别的颜色，不要用身体色。〔变色〕
性格：{如温和、安静}；表情{如柔和、不夸张}。
缩小到 120 像素高也要认得出：轮廓清楚、细节少、不要文字和花纹。每一格都要和参考设定稿是同一只。
```

## C 格子清单（每张图一段，英文）

从 `actions.md` 抄对应帧的描述，写成 `Cell n: <state> frame k — <描述>`。下面是 5 张图的现成清单。

### sheet-1.png（idle + running-right）

```
Cells 1-6: "idle" loop, facing the viewer.
Cell 1: standard standing pose, eyes open, gentle smile, arms relaxed at the sides.
Cell 2: inhale — body slightly squashed (3% wider, 3% shorter).
Cell 3: body slightly stretched up, head top about 4 px higher than cell 1.
Cell 4: standard standing pose again.
Cell 5: blink — eyes closed as two curved lines, otherwise same as cell 4.
Cell 6: eyes open, same as cell 1.
Cells 7-14: "running-right" loop, facing RIGHT (toward the right side of the image), body leaning forward ~10 degrees, arms swinging.
Cell 7: contact — right foot forward touching the ground, left foot back, body at its lowest.
Cell 8: down — body slightly squashed, weight on the right foot.
Cell 9: push off — left foot swinging forward, body 8 px higher.
Cell 10: flight — both feet slightly off the ground, body highest, two or three small speed lines behind.
Cell 11: contact — left foot forward touching the ground, right foot back, body lowest.
Cell 12: down — body slightly squashed, weight on the left foot.
Cell 13: push off — right foot swinging forward, body 8 px higher.
Cell 14: flight — both feet slightly off the ground, body highest.
Cell 15: standard standing pose. Cell 16: color swatch.
```

### sheet-2.png（failed + waiting）

```
Cells 1-8: "failed" — a small, gentle disappointment (not dramatic, no falling down, no text or symbols other than "!").
Cell 1: startled, eyes become two small dots, a tiny "!" above the head on the right.
Cell 2: a single blue sweat drop at the temple, mouth a small wavy line.
Cell 3: eyes become two small "x", sweat drop still there.
Cell 4: sad brows, eyes watery (a little blue under the eyes), both hands clasped in front of the chest.
Cell 5: one tear drop falls on one side, body curls in slightly.
Cell 6: a tear drop falls on the other side.
Cell 7: wiping the eyes with the back of a hand.
Cell 8: sniffing, brows still sad but mouth flat, about to return to normal.
Cells 9-14: "waiting" loop — dozing with eyes closed (two downward curves), mouth a short flat line; the head stays in exactly the same place in every cell.
Cell 9: a very small "z" above the head on the right.
Cell 10: the "z" a bit bigger, body sinks 1-2 px.
Cell 11: the "z" bigger and drifting up-right.
Cell 12: a second small "z" appears below the first.
Cell 13: both "z" larger and floating up, body rises back.
Cell 14: the large "z" fading out, the small one still there.
Cell 15: standard standing pose. Cell 16: color swatch.
```

### sheet-3.png（running + review）

```
Cells 1-6: "running" loop — happily jogging in place, facing the viewer, fists pumping at the sides, joyful squinting smile.
Cell 1: left foot lifted, right foot on the ground, body lowest.
Cell 2: both feet on the ground, body rising.
Cell 3: right foot lifted, left foot on the ground, body highest, two small sparkle lines beside the body.
Cell 4: left foot lifted (like cell 1 with opposite arms).
Cell 5: both feet on the ground.
Cell 6: right foot lifted.
Cells 7-12: "review" loop — concentrating: eyes narrowed, brows slightly knitted (focused, not angry), holding a small magnifying glass in the right hand in front of the body; the head stays in exactly the same place in every cell.
Cell 7: magnifier to the left, eyes looking left.
Cell 8: magnifier in the center.
Cell 9: magnifier to the right, eyes looking right.
Cell 10: magnifier back to the center, a tiny nod (head 2 px lower).
Cell 11: magnifier to the left, a very small sweat drop at the temple.
Cell 12: magnifier in the center, sweat drop gone.
Cells 13-14: EMPTY.
Cell 15: standard standing pose. Cell 16: color swatch.
```

### sheet-4.png（waving + jumping）

```
Cells 1-4: "waving" — happy crescent-shaped eyes.
Cell 1: right hand rising from the side to the chest.
Cell 2: right hand raised above the head, palm open toward the viewer.
Cell 3: hand waving outward (toward the right side), body leaning slightly left.
Cell 4: hand waving inward (toward the center), body upright.
Cells 5-9: "jumping" — a startled hop.
Cell 5: crouch — body squashed (12% wider, 12% shorter), eyes wide, feet on the ground line.
Cell 6: take off — body stretched up (10% taller), feet about 30 px above the ground line, arms up.
Cell 7: apex — feet about 90 px above the ground line, round body, wide or sparkling eyes, a tiny "!" above the head.
Cell 8: falling — feet about 35 px above the ground line, body slightly stretched.
Cell 9: landing — body squashed (8% wider, 8% shorter), feet on the ground line, smiling again.
Cells 10-14: EMPTY.
Cell 15: standard standing pose. Cell 16: color swatch.
```

### sheet-5.png（可选：running-left）

```
Cells 1-8: "running-left" loop, facing LEFT, frame-by-frame the mirror counterpart of the running-right cycle:
Cell 1 contact (left foot forward), cell 2 down, cell 3 push off, cell 4 flight with speed lines behind (on the right),
cell 5 contact (right foot forward), cell 6 down, cell 7 push off, cell 8 flight.
Cells 9-14: EMPTY.
Cell 15: standard standing pose. Cell 16: color swatch.
```

---

## 不同模型的注意点

- **一次给不出 14 格**：先只画格 1–8（其余写 EMPTY），再用同一设定稿画另一半，最后在图像软件里拼回同一张 1024 × 1536（位置按格子编号）。
- **支持参考图的模型**：参考图放设定稿和已经通过验收的 `sheet-1.png`，保持一致性；不要放旧版形象，免得被带回旧样子。
- **会自动加文字的模型**：把 “no text” 重复写在提示词开头和结尾。
- **不支持精确尺寸的模型**：按 2:3 出最大尺寸即可，工具会缩放到 1024 × 1536 再切。
- **记录**：每张图记下模型名、版本、种子（若有）和提示词，日后补帧用同一组参数。
