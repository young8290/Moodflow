#!/usr/bin/env python3
"""生成内置桌宠“晴晴”的精灵图（ADR 0044），格式与 Codex 桌宠一致，导入的宠物可以直接替换。

输出到 hub_templates/pets/qingqing/：

    pet.json         清单：id、displayName、description、spritesheetPath（与 Codex 的 pet.json 字段相同）
    spritesheet.png  标准精灵图：8 列 × 9 行，每格 192 × 208，透明底；身体按晴天色上色。导入别处时照样能用
    body.png         只有身体（含手脚）的蒙版，白色不透明；小组件用它按当前天气色（--mf-w-*）给身体上色
    lines.png        身体以外的部分：描边、五官、影子、汗滴、星星等；叠在上了色的身体上面

九行依次是 Codex 的九种状态（帧数 / 一轮时长与 Codex 相同，见 moodflow_hub/src/windows/widget/pet.ts）：

    0 idle 6   1 running-right 8   2 running-left 8   3 waving 4   4 jumping 5
    5 failed 8 6 waiting 6         7 running 6        8 review 6

先在 4 倍画布上画再缩小（抗锯齿）。需要 Pillow：

    python3 tools/gen_pet_sheet.py
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path(__file__).resolve().parents[1] / "hub_templates" / "pets" / "qingqing"

COLS, ROWS = 8, 9
CW, CH = 192, 208
SS = 4  # 超采样倍数

# 颜色与设计令牌一致（tokens.css 浅色）：身体用 --mf-w-sunny，线条用 --mf-text-1
SUNNY = (0xF6, 0xC3, 0x44, 255)
INK = (0x2B, 0x2A, 0x28, 255)
CHEEK = (0xF2, 0x8C, 0x7A, 110)
SHADOW = (0x2B, 0x2A, 0x28, 40)
TEAR = (0x7F, 0xA7, 0xC9, 255)
WHITE = (255, 255, 255, 255)
SPARK = (0xF6, 0xC3, 0x44, 255)

BASE_X, BASE_Y, R = 96.0, 122.0, 54.0  # 静止时身体的圆心与半径
FLOOR = 186.0  # 地面（脚底）的 y


@dataclass
class Pose:
    dx: float = 0.0  # 身体相对静止位置的偏移
    dy: float = 0.0
    sx: float = 1.0  # 横向 / 纵向缩放（挤压与拉伸）
    sy: float = 1.0
    eyes: str = "open"  # open / blink / happy / wide / x / sleepy / squint / sad
    look: float = 0.0  # 瞳孔左右看，-1..1
    mouth: str = "smile"  # smile / open / o / flat / wobble / grin
    feet: tuple[float, float] = (0.0, 0.0)  # 两只脚抬起的高度
    feet_dx: tuple[float, float] = (0.0, 0.0)
    hand: tuple[float, float] | None = None  # 右手相对身体圆心的位置
    hand2: tuple[float, float] | None = None  # 左手
    extras: list[tuple] = field(default_factory=list)
    shadow: float = 1.0  # 影子大小（离地越高越小）
    cheeks: bool = True


def body_geom(p: Pose):
    cx = BASE_X + p.dx
    rx, ry = R * p.sx, R * p.sy
    # 挤压时身体贴着地面，圆心随纵向缩放下沉
    cy = BASE_Y + p.dy + R * (1 - p.sy)
    return cx, cy, rx, ry


def s(v: float) -> float:
    return v * SS


def ellipse(d: ImageDraw.ImageDraw, cx, cy, rx, ry, **kw):
    d.ellipse([s(cx - rx), s(cy - ry), s(cx + rx), s(cy + ry)], **kw)


def draw_body(d: ImageDraw.ImageDraw, p: Pose, fill):
    cx, cy, rx, ry = body_geom(p)
    # 脚：两个小椭圆，在身体底部
    for i, side in enumerate((-1, 1)):
        fx = cx + side * rx * 0.42 + p.feet_dx[i]
        fy = cy + ry - 2 - p.feet[i]
        ellipse(d, fx, fy, 13, 8, fill=fill)
    ellipse(d, cx, cy, rx, ry, fill=fill)
    for h in (p.hand, p.hand2):
        if h is not None:
            ellipse(d, cx + h[0], cy + h[1], 11, 11, fill=fill)


def draw_lines(d: ImageDraw.ImageDraw, p: Pose):
    cx, cy, rx, ry = body_geom(p)
    w = int(s(3))
    # 影子
    sw = 46 * p.shadow
    ellipse(d, cx, FLOOR + 3, sw, 6 * p.shadow, fill=SHADOW)
    # 脚、身体、手的描边
    for i, side in enumerate((-1, 1)):
        fx = cx + side * rx * 0.42 + p.feet_dx[i]
        fy = cy + ry - 2 - p.feet[i]
        ellipse(d, fx, fy, 13, 8, outline=INK, width=w)
    ellipse(d, cx, cy, rx, ry, outline=INK, width=w)
    for h in (p.hand, p.hand2):
        if h is not None:
            ellipse(d, cx + h[0], cy + h[1], 11, 11, outline=INK, width=w)
    # 五官
    ex, ey = 19 * p.sx, cy - 8 * p.sy
    look = p.look * 5
    for side in (-1, 1):
        x = cx + side * ex + look
        if p.eyes == "open":
            ellipse(d, x, ey, 5.5, 7.5, fill=INK)
            ellipse(d, x + 1.8, ey - 2.6, 1.8, 1.8, fill=WHITE)
        elif p.eyes == "wide":
            ellipse(d, x, ey, 8.5, 9.5, fill=WHITE, outline=INK, width=w)
            ellipse(d, x, ey, 3.5, 3.5, fill=INK)
        elif p.eyes == "blink":
            d.line([s(x - 6), s(ey), s(x + 6), s(ey)], fill=INK, width=w)
        elif p.eyes == "happy":
            d.arc([s(x - 7), s(ey - 5), s(x + 7), s(ey + 7)], 200, 340, fill=INK, width=w)
        elif p.eyes == "x":
            d.line([s(x - 6), s(ey - 6), s(x + 6), s(ey + 6)], fill=INK, width=w)
            d.line([s(x - 6), s(ey + 6), s(x + 6), s(ey - 6)], fill=INK, width=w)
        elif p.eyes == "sleepy":
            d.arc([s(x - 7), s(ey - 7), s(x + 7), s(ey + 5)], 20, 160, fill=INK, width=w)
        elif p.eyes == "squint":
            ellipse(d, x, ey, 6, 3, fill=INK)
            # 小眉毛：认真
            d.line([s(x - 7), s(ey - 11 + side * 2), s(x + 7), s(ey - 11 - side * 2)], fill=INK, width=w)
        elif p.eyes == "sad":
            ellipse(d, x, ey + 1, 5, 6.5, fill=INK)
            d.line([s(x - 7), s(ey - 10 - side * 3), s(x + 6), s(ey - 12 + side * 3)], fill=INK, width=w)
    # 腮红
    if p.cheeks:
        for side in (-1, 1):
            ellipse(d, cx + side * 31 * p.sx, cy + 8 * p.sy, 8, 4.5, fill=CHEEK)
    # 嘴
    my = cy + 12 * p.sy
    mx = cx + look * 0.6
    if p.mouth == "smile":
        d.arc([s(mx - 8), s(my - 7), s(mx + 8), s(my + 5)], 20, 160, fill=INK, width=w)
    elif p.mouth == "grin":
        d.chord([s(mx - 11), s(my - 7), s(mx + 11), s(my + 9)], 0, 180, fill=INK)
    elif p.mouth == "open":
        ellipse(d, mx, my + 2, 7, 6, fill=INK)
    elif p.mouth == "o":
        ellipse(d, mx, my + 3, 5, 6.5, outline=INK, width=w)
    elif p.mouth == "flat":
        d.line([s(mx - 6), s(my + 2), s(mx + 6), s(my + 2)], fill=INK, width=w)
    elif p.mouth == "wobble":
        pts = [(s(mx - 9 + i * 3), s(my + 3 + (2 if i % 2 else -1))) for i in range(7)]
        d.line(pts, fill=INK, width=w, joint="curve")
    # 配饰
    for e in p.extras:
        kind = e[0]
        if kind == "sweat":
            _, x, y, k = e
            x, y = cx + x, cy + y
            d.polygon([(s(x), s(y - 9 * k)), (s(x - 5 * k), s(y)), (s(x + 5 * k), s(y))], fill=TEAR)
            ellipse(d, x, y + 1, 5 * k, 5 * k, fill=TEAR)
        elif kind == "tear":
            _, x, y = e
            ellipse(d, cx + x, cy + y, 3.5, 5, fill=TEAR)
        elif kind == "speed":
            _, x, y, n = e
            for i in range(n):
                yy = cy + y + i * 16
                d.line([s(cx + x), s(yy), s(cx + x - 22 + i * 4), s(yy)], fill=INK, width=int(s(2.5)))
        elif kind == "z":
            _, x, y, k = e
            x, y = cx + x, cy + y
            z = 7 * k
            d.line([s(x - z), s(y - z), s(x + z), s(y - z), s(x - z), s(y + z), s(x + z), s(y + z)], fill=INK, width=int(s(2.5)))
        elif kind == "spark":
            _, x, y, k = e
            x, y = cx + x, cy + y
            for a in range(4):
                ang = a * math.pi / 2
                d.line([s(x), s(y), s(x + math.cos(ang) * 8 * k), s(y + math.sin(ang) * 8 * k)], fill=SPARK, width=int(s(3)))
        elif kind == "glass":
            _, x, y = e
            x, y = cx + x, cy + y
            ellipse(d, x, y, 15, 15, outline=INK, width=int(s(4)))
            ellipse(d, x, y, 12, 12, fill=(255, 255, 255, 70))
            d.line([s(x + 10), s(y + 10), s(x + 24), s(y + 24)], fill=INK, width=int(s(6)))
        elif kind == "dust":
            _, x, y, k = e
            for i in range(3):
                ellipse(d, cx + x - i * 9, cy + y - i * 3, 4 * k, 4 * k, outline=INK, width=int(s(2)))
        elif kind == "bang":
            _, x, y = e
            x, y = cx + x, cy + y
            d.line([s(x), s(y - 14), s(x), s(y + 2)], fill=INK, width=int(s(4)))
            ellipse(d, x, y + 8, 2.5, 2.5, fill=INK)


def bob(t: float) -> float:
    return abs(math.sin(t * math.pi))


def idle(n=6):
    out = []
    for i in range(n):
        b = math.sin(i / n * 2 * math.pi)
        out.append(Pose(sx=1 - 0.02 * b, sy=1 + 0.03 * b, eyes="blink" if i == 4 else "open"))
    return out


def run_right(n=8):
    out = []
    for i in range(n):
        ph = i / n * 2 * math.pi
        up = abs(math.sin(ph))
        land = 1 - up
        out.append(
            Pose(
                dx=4,
                dy=-12 * up,
                sx=1 + 0.06 * land,
                sy=1 - 0.07 * land,
                look=1.0,
                mouth="open" if i % 4 < 2 else "smile",
                feet=(8 * max(0, math.sin(ph)), 8 * max(0, -math.sin(ph))),
                feet_dx=(10 * math.cos(ph), -10 * math.cos(ph)),
                extras=[("speed", -R - 8, -14, 3)] if i % 2 == 0 else [("dust", -R + 2, R - 4, 0.8 + 0.2 * land)],
                shadow=1 - 0.2 * up,
            )
        )
    return out


def waving(n=4):
    out = []
    for i in range(n):
        a = math.radians(-60 + 35 * math.sin(i / n * 2 * math.pi))
        hand = (R * 0.78 + 12 * math.cos(a), -R * 0.42 + 18 * math.sin(a))
        out.append(Pose(eyes="happy", mouth="grin", hand=hand, sy=1 + 0.02 * (i % 2), extras=[("spark", -R - 6, -R + 6, 0.8 + 0.3 * (i % 2))]))
    return out


def jumping():
    return [
        Pose(sx=1.16, sy=0.8, eyes="squint", mouth="flat", shadow=1.1),
        Pose(dy=-34, sx=0.88, sy=1.14, eyes="open", mouth="open", feet=(6, 6), shadow=0.75),
        Pose(dy=-60, eyes="happy", mouth="grin", hand=(R * 0.9, -R * 0.8), hand2=(-R * 0.9, -R * 0.8), feet=(4, 4), shadow=0.55, extras=[("spark", R + 10, -R, 1.0), ("spark", -R - 12, -R + 10, 0.7)]),
        Pose(dy=-26, sx=0.9, sy=1.12, eyes="open", mouth="o", feet=(6, 6), shadow=0.8),
        Pose(sx=1.12, sy=0.86, eyes="happy", mouth="smile", shadow=1.08, extras=[("dust", -R + 6, R - 6, 0.9), ("dust", R + 22, R - 6, 0.9)]),
    ]


def failed():
    return [
        Pose(dy=-10, sx=0.94, sy=1.08, eyes="wide", mouth="o", extras=[("bang", R + 14, -R)]),
        Pose(dy=-4, eyes="wide", mouth="wobble", extras=[("sweat", R * 0.75, -R * 0.55, 1.0)]),
        Pose(sx=1.06, sy=0.92, eyes="x", mouth="wobble", extras=[("sweat", R * 0.78, -R * 0.45, 1.2)]),
        Pose(sx=1.14, sy=0.82, eyes="sad", mouth="wobble", hand=(R * 0.7, R * 0.4), hand2=(-R * 0.7, R * 0.4)),
        Pose(dx=-3, sx=1.16, sy=0.8, eyes="sad", mouth="wobble", hand=(R * 0.72, R * 0.42), hand2=(-R * 0.72, R * 0.42), extras=[("tear", -20, 4), ("tear", 20, 4)]),
        Pose(dx=3, sx=1.16, sy=0.8, eyes="sad", mouth="wobble", hand=(R * 0.72, R * 0.42), hand2=(-R * 0.72, R * 0.42), extras=[("tear", -21, 12), ("tear", 21, 12)]),
        Pose(dx=-3, sx=1.16, sy=0.8, eyes="sad", mouth="wobble", hand=(R * 0.72, R * 0.42), hand2=(-R * 0.72, R * 0.42), extras=[("tear", -22, 20), ("tear", 22, 20)]),
        Pose(sx=1.1, sy=0.86, eyes="sad", mouth="flat", hand=(R * 0.7, R * 0.4), hand2=(-R * 0.7, R * 0.4)),
    ]


def waiting(n=6):
    out = []
    for i in range(n):
        b = math.sin(i / n * 2 * math.pi)
        out.append(Pose(dx=4 * b, sy=1 + 0.02 * b, eyes="sleepy", mouth="flat", cheeks=False, extras=[("z", R * 0.8, -R - 6 - 3 * i, 0.6 + 0.1 * i)]))
    return out


def running(n=6):
    out = []
    for i in range(n):
        ph = i / n * 2 * math.pi
        up = abs(math.sin(ph))
        out.append(
            Pose(
                dy=-14 * up,
                sx=1 + 0.05 * (1 - up),
                sy=1 - 0.06 * (1 - up),
                eyes="squint",
                mouth="grin" if i % 3 == 0 else "open",
                feet=(10 * max(0, math.sin(ph)), 10 * max(0, -math.sin(ph))),
                hand=(R * 0.92, -6 + 10 * math.sin(ph)),
                hand2=(-R * 0.92, -6 - 10 * math.sin(ph)),
                extras=[("sweat", R * 0.8, -R * 0.6, 0.8)] if i in (2, 3) else [],
                shadow=1 - 0.2 * up,
            )
        )
    return out


def review(n=6):
    out = []
    for i in range(n):
        lk = math.sin(i / n * 2 * math.pi)
        out.append(Pose(eyes="squint", look=lk, mouth="flat", hand=(R * 0.55 + 12 * lk, R * 0.5), extras=[("glass", 14 * lk + 30, 18)]))
    return out


def mirror(img: Image.Image) -> Image.Image:
    return img.transpose(Image.FLIP_LEFT_RIGHT)


def render(pose: Pose, layer: str) -> Image.Image:
    big = Image.new("RGBA", (CW * SS, CH * SS), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)
    if layer == "body":
        draw_body(d, pose, WHITE)
    elif layer == "lines":
        draw_lines(d, pose)
    else:
        draw_body(d, pose, SUNNY)
        draw_lines(d, pose)
    return big.resize((CW, CH), Image.LANCZOS)


def main() -> None:
    rr = run_right()
    rows = [idle(), rr, rr, waving(), jumping(), failed(), waiting(), running(), review()]
    OUT.mkdir(parents=True, exist_ok=True)
    for layer, name in (("full", "spritesheet.png"), ("body", "body.png"), ("lines", "lines.png")):
        sheet = Image.new("RGBA", (COLS * CW, ROWS * CH), (0, 0, 0, 0))
        for r, poses in enumerate(rows):
            for c, pose in enumerate(poses):
                img = render(pose, layer)
                if r == 2:  # 向左跑：向右跑的镜像
                    img = mirror(img)
                sheet.paste(img, (c * CW, r * CH))
        sheet.save(OUT / name, optimize=True)
    manifest = {
        "id": "qingqing",
        "displayName": "晴晴",
        "description": "Moodflow 的小精灵，身体颜色跟着当前的“心情天气”变。",
        "spritesheetPath": "spritesheet.png",
    }
    (OUT / "pet.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"已生成 {OUT}")


if __name__ == "__main__":
    main()
