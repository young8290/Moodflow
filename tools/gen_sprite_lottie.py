#!/usr/bin/env python3
"""生成天气小精灵的 lottie 动画（07 DS-MOTION-02、16 D-03）。

每种天气一个文件，输出到 xinqing_hub/src/assets/sprite/<weather>.json，画布 96 × 96、30 帧 / 秒，
用标记（markers）分出五段，由 xinqing_hub/src/components/WeatherSprite.vue 按段播放：

    loop      4 秒   循环：呼吸、眨眼，加上各天气配饰的动作（DS-MOTION-02 要求 2–4 秒）
    shake     0.5 秒 晃一下（打错字）
    approach  0.6 秒 靠近（出现暖心话）
    close     0.3 秒 闭眼（无痕 / 密码框 / 暂停）；倒着播就是睁眼
    sleep     4 秒   闭着眼的循环：呼吸更轻，配饰不动

颜色不写死：每个图层只有一种样式（一种填充或一种描边），图层带 class（lottie 的 `cl`），
由组件里的 CSS 按设计令牌上色（--xq-w-*、--xq-on-weather、--xq-text-2），深浅色与高对比度跟着令牌走。
JSON 里的颜色只是占位。形状与原来的 SVG 小精灵一致（坐标单位同 viewBox 0 0 96 96）。

只用标准库：

    python3 tools/gen_sprite_lottie.py           # 重新生成
    python3 tools/gen_sprite_lottie.py --check   # 只核对已提交的文件是不是最新
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "xinqing_hub" / "src" / "assets" / "sprite"
WEATHERS = ["sunny", "wind", "cloudy", "rain", "storm", "night"]

FPS = 30
LOOP = 120  # 4 秒
SEGMENTS = [("loop", LOOP), ("shake", 15), ("approach", 18), ("close", 9), ("sleep", 120)]
START = {}
_t = 0
for _name, _dur in SEGMENTS:
    START[_name] = _t
    _t += _dur
TOTAL = _t

# 缓动：DS-MOTION-01 的 cubic-bezier(.2,.8,.2,1)、线性、ease-in
EASE = ((0.2, 0.8), (0.2, 1.0))
LINEAR = ((0.0, 0.0), (1.0, 1.0))
EASE_IN = ((0.42, 0.0), (1.0, 1.0))

# 占位颜色（运行时由 CSS 覆盖）
PLACEHOLDER = {
    "xq-fill-w": [0.96, 0.76, 0.27, 1],
    "xq-line-on": [0.17, 0.16, 0.16, 1],
    "xq-fill-on": [0.17, 0.16, 0.16, 1],
    "xq-line-text": [0.42, 0.40, 0.37, 1],
}


# ───────────────────────── SVG 路径 → 三次贝塞尔 ─────────────────────────

Point = tuple[float, float]


def _arc_to_cubics(p0: Point, rx: float, ry: float, phi_deg: float, large: int, sweep: int, p1: Point):
    """SVG 椭圆弧 → 若干段三次贝塞尔（每段不超过 90°）。返回 [(c1, c2, end), ...]。"""
    if p0 == p1:
        return []
    if rx == 0 or ry == 0:
        return [(p0, p1, p1)]
    phi = math.radians(phi_deg)
    cos_phi, sin_phi = math.cos(phi), math.sin(phi)
    dx, dy = (p0[0] - p1[0]) / 2, (p0[1] - p1[1]) / 2
    x1p = cos_phi * dx + sin_phi * dy
    y1p = -sin_phi * dx + cos_phi * dy
    rx, ry = abs(rx), abs(ry)
    lam = (x1p**2) / (rx**2) + (y1p**2) / (ry**2)
    if lam > 1:
        rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
    num = rx**2 * ry**2 - rx**2 * y1p**2 - ry**2 * x1p**2
    den = rx**2 * y1p**2 + ry**2 * x1p**2
    coef = math.sqrt(max(0.0, num / den)) if den else 0.0
    if large == sweep:
        coef = -coef
    cxp = coef * rx * y1p / ry
    cyp = -coef * ry * x1p / rx
    cx = cos_phi * cxp - sin_phi * cyp + (p0[0] + p1[0]) / 2
    cy = sin_phi * cxp + cos_phi * cyp + (p0[1] + p1[1]) / 2

    def angle(ux: float, uy: float, vx: float, vy: float) -> float:
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    t1 = angle(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not sweep and dt > 0:
        dt -= 2 * math.pi
    elif sweep and dt < 0:
        dt += 2 * math.pi
    n = max(1, math.ceil(abs(dt) / (math.pi / 2) - 1e-9))
    step = dt / n
    k = 4 / 3 * math.tan(step / 4)
    out = []

    def pt(t: float) -> Point:
        x, y = rx * math.cos(t), ry * math.sin(t)
        return (cos_phi * x - sin_phi * y + cx, sin_phi * x + cos_phi * y + cy)

    def deriv(t: float) -> Point:
        x, y = -rx * math.sin(t), ry * math.cos(t)
        return (cos_phi * x - sin_phi * y, sin_phi * x + cos_phi * y)

    t = t1
    for i in range(n):
        a, b = t, t + step
        pa, pb = pt(a), pt(b)
        da, db = deriv(a), deriv(b)
        c1 = (pa[0] + k * da[0], pa[1] + k * da[1])
        c2 = (pb[0] - k * db[0], pb[1] - k * db[1])
        end = p1 if i == n - 1 else pb
        out.append((c1, c2, end))
        t = b
    return out


def parse_path(d: str) -> list[dict]:
    """SVG 路径（M L H V Q T C A Z，大小写都认）→ 子路径列表，每个是 {start, segs: [(c1, c2, end)], closed}。"""
    tokens = re.findall(r"[MmLlHhVvQqTtCcAaZz]|-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?", d)
    subpaths: list[dict] = []
    cur: Point = (0.0, 0.0)
    start: Point = (0.0, 0.0)
    prev_q: Point | None = None
    i = 0
    cmd = ""

    def num() -> float:
        nonlocal i
        v = float(tokens[i])
        i += 1
        return v

    def flag() -> int:
        # 弧线的两个标志位可能紧挨着写（如 "0 1 0"），按单个数字取
        return int(num())

    while i < len(tokens):
        if re.fullmatch(r"[A-Za-z]", tokens[i]):
            cmd = tokens[i]
            i += 1
        rel = cmd.islower()
        c = cmd.upper()
        ox, oy = (cur if rel else (0.0, 0.0))
        if c == "M":
            cur = (num() + ox, num() + oy)
            start = cur
            subpaths.append({"start": cur, "segs": [], "closed": False})
            cmd = "l" if rel else "L"
            prev_q = None
        elif c == "L":
            p = (num() + ox, num() + oy)
            subpaths[-1]["segs"].append((cur, p, p))
            cur, prev_q = p, None
        elif c == "H":
            p = (num() + (cur[0] if rel else 0.0), cur[1])
            subpaths[-1]["segs"].append((cur, p, p))
            cur, prev_q = p, None
        elif c == "V":
            p = (cur[0], num() + (cur[1] if rel else 0.0))
            subpaths[-1]["segs"].append((cur, p, p))
            cur, prev_q = p, None
        elif c in ("Q", "T"):
            if c == "Q":
                q = (num() + ox, num() + oy)
            else:
                q = (2 * cur[0] - prev_q[0], 2 * cur[1] - prev_q[1]) if prev_q else cur
            p = (num() + ox, num() + oy)
            c1 = (cur[0] + 2 / 3 * (q[0] - cur[0]), cur[1] + 2 / 3 * (q[1] - cur[1]))
            c2 = (p[0] + 2 / 3 * (q[0] - p[0]), p[1] + 2 / 3 * (q[1] - p[1]))
            subpaths[-1]["segs"].append((c1, c2, p))
            cur, prev_q = p, q
        elif c == "C":
            c1 = (num() + ox, num() + oy)
            c2 = (num() + ox, num() + oy)
            p = (num() + ox, num() + oy)
            subpaths[-1]["segs"].append((c1, c2, p))
            cur, prev_q = p, None
        elif c == "A":
            rx, ry, phi = num(), num(), num()
            large, sweep = flag(), flag()
            p = (num() + ox, num() + oy)
            subpaths[-1]["segs"].extend(_arc_to_cubics(cur, rx, ry, phi, large, sweep, p))
            cur, prev_q = p, None
        elif c == "Z":
            if cur != start:
                subpaths[-1]["segs"].append((cur, start, start))
            subpaths[-1]["closed"] = True
            cur, prev_q = start, None
        else:  # pragma: no cover
            raise ValueError(f"不支持的路径命令 {cmd}")
    return subpaths


def r(v: float) -> float:
    return round(v, 3) + 0.0


def lottie_path(sub: dict) -> dict:
    """一个子路径 → lottie 的 sh（顶点 v、入切线 i、出切线 o，都是相对顶点的偏移）。"""
    pts = [sub["start"]] + [s[2] for s in sub["segs"]]
    segs = sub["segs"]
    closed = sub["closed"]
    if closed and len(pts) > 1 and math.dist(pts[-1], pts[0]) < 1e-6:
        pts = pts[:-1]
    n = len(pts)
    ins = [[0.0, 0.0] for _ in range(n)]
    outs = [[0.0, 0.0] for _ in range(n)]
    for k, (c1, c2, _end) in enumerate(segs):
        a = k
        b = (k + 1) % n if closed else k + 1
        if b >= n:
            continue
        outs[a] = [c1[0] - pts[a][0], c1[1] - pts[a][1]]
        ins[b] = [c2[0] - pts[b][0], c2[1] - pts[b][1]]
    return {
        "ty": "sh",
        "d": 1,
        "ks": {
            "a": 0,
            "k": {
                "i": [[r(x), r(y)] for x, y in ins],
                "o": [[r(x), r(y)] for x, y in outs],
                "v": [[r(x), r(y)] for x, y in pts],
                "c": closed,
            },
        },
    }


def shapes_of(d: str) -> list[dict]:
    return [lottie_path(s) for s in parse_path(d)]


def ellipse(cx: float, cy: float, rad: float) -> dict:
    return {"ty": "el", "d": 1, "p": {"a": 0, "k": [cx, cy]}, "s": {"a": 0, "k": [2 * rad, 2 * rad]}}


# ───────────────────────── 关键帧与图层 ─────────────────────────


def static(v):
    return {"a": 0, "k": v}


def keys(frames: list[tuple[float, list[float] | float, tuple | None, bool]]) -> dict:
    """[(帧, 值, 到下一帧的缓动, 是否保持)] → lottie 动画属性。最后一帧的缓动不用填。"""
    out = []
    for t, v, ease, hold in frames:
        k: dict = {"t": r(t), "s": [r(x) for x in v] if isinstance(v, list) else [r(v)]}
        if hold:
            k["h"] = 1
        elif ease is not None:
            (ox, oy), (ix, iy) = ease
            k["o"] = {"x": [ox], "y": [oy]}
            k["i"] = {"x": [ix], "y": [iy]}
        out.append(k)
    return {"a": 1, "k": out}


def tiled(cycle: list[tuple[float, float, tuple]], period: float, offset: float, end: float) -> list:
    """把一个周期的关键帧（相位 0–1、值、缓动）铺满 [0, end]，`offset` 是起始相位（帧）。

    裁到 0 和 end 的那两段按线性插值补一个关键帧；因为 end 是周期的整数倍，首尾两帧的值相同，循环无缝。
    """

    def value_at(phase_frames: float) -> float:
        p = (phase_frames % period) / period
        for (p0, v0, _e), (p1, v1, _e2) in zip(cycle, cycle[1:]):
            if p0 <= p <= p1:
                return v0 if p1 == p0 else v0 + (v1 - v0) * (p - p0) / (p1 - p0)
        return cycle[-1][1]

    frames = []
    first = -((offset % period))
    c = first
    while c < end:
        for p, v, e in cycle:
            t = c + p * period
            if 0 < t < end:
                frames.append((t, v, e))
        c += period
    frames.append((0.0, value_at(offset), LINEAR))
    frames.append((end, value_at(offset + end), LINEAR))
    frames.sort(key=lambda f: f[0])
    # 去掉同一帧的重复（周期首尾相接处）
    dedup = []
    for f in frames:
        if dedup and abs(dedup[-1][0] - f[0]) < 1e-6:
            dedup[-1] = f
        else:
            dedup.append(f)
    return [(t, v, e, False) for t, v, e in dedup]


class Comp:
    def __init__(self, weather: str):
        self.weather = weather
        self.layers: list[dict] = []
        self.next_ind = 1

    def null(self, name: str, pivot: Point, parent: int | None = None, **ks) -> int:
        ind = self.next_ind
        self.next_ind += 1
        layer = {
            "ddd": 0,
            "ind": ind,
            "ty": 3,
            "nm": name,
            "sr": 1,
            "ks": self._ks(pivot, ks),
            "ao": 0,
            "ip": 0,
            "op": TOTAL,
            "st": 0,
            "bm": 0,
        }
        if parent is not None:
            layer["parent"] = parent
        self.layers.append(layer)
        return ind

    def shape(self, name: str, cls: str, items: list[dict], parent: int, pivot: Point = (48, 48),
              width: float = 0, **ks) -> int:
        """一个只有一种样式的形状图层：cls 决定是填充（xq-fill-*）还是描边（xq-line-*）。"""
        ind = self.next_ind
        self.next_ind += 1
        color = static(PLACEHOLDER[cls])
        if cls.startswith("xq-fill"):
            style = {"ty": "fl", "c": color, "o": static(100), "r": 1, "bm": 0}
        else:
            style = {"ty": "st", "c": color, "o": static(100), "w": static(width), "lc": 2, "lj": 2, "bm": 0}
        group = {
            "ty": "gr",
            "nm": name,
            "it": items
            + [
                style,
                {
                    "ty": "tr",
                    "p": static([0, 0]),
                    "a": static([0, 0]),
                    "s": static([100, 100]),
                    "r": static(0),
                    "o": static(100),
                    "sk": static(0),
                    "sa": static(0),
                },
            ],
        }
        layer = {
            "ddd": 0,
            "ind": ind,
            "ty": 4,
            "nm": name,
            "cl": cls,
            "parent": parent,
            "sr": 1,
            "ks": self._ks(pivot, ks),
            "ao": 0,
            "shapes": [group],
            "ip": 0,
            "op": TOTAL,
            "st": 0,
            "bm": 0,
        }
        self.layers.append(layer)
        return ind

    @staticmethod
    def _ks(pivot: Point, ks: dict) -> dict:
        base = {
            "o": static(100),
            "r": static(0),
            "p": static([pivot[0], pivot[1], 0]),
            "a": static([pivot[0], pivot[1], 0]),
            "s": static([100, 100, 100]),
        }
        base.update(ks)
        return base

    def to_json(self) -> dict:
        # lottie 按数组顺序从上往下画：后加的图层盖在上面，所以倒过来
        return {
            "v": "5.7.4",
            "fr": FPS,
            "ip": 0,
            "op": TOTAL,
            "w": 96,
            "h": 96,
            "nm": f"xq-sprite-{self.weather}",
            "ddd": 0,
            "assets": [],
            "layers": list(reversed(self.layers)),
            "markers": [{"tm": START[name], "cm": name, "dr": dur} for name, dur in SEGMENTS],
        }


def offset_pos(pivot: Point, dx: float, dy: float) -> list[float]:
    return [pivot[0] + dx, pivot[1] + dy, 0]


def build(weather: str) -> dict:
    c = Comp(weather)
    shake, approach, close, sleep = START["shake"], START["approach"], START["close"], START["sleep"]

    # 整体：晃一下（左右摆两下回原位）、靠近（放大一点再回来），支点在脚下 (48, 80)
    foot = (48.0, 80.0)
    figure = c.null(
        "figure",
        foot,
        r=keys(
            [
                (shake, 0, EASE, False),
                (shake + 15 * 0.25, -6, EASE, False),
                (shake + 15 * 0.5, 5, EASE, False),
                (shake + 15 * 0.75, -3, EASE, False),
                (shake + 15, 0, None, False),
            ]
        ),
        s=keys(
            [
                (approach, [100, 100, 100], EASE, False),
                (approach + 18 * 0.45, [108, 108, 100], EASE, False),
                (approach + 18, [100, 100, 100], None, False),
            ]
        ),
    )

    # 配饰：画在身体后面，先加
    add_accessory(c, weather, figure)

    # 呼吸：睁眼时 4 秒一次；闭眼的循环更轻
    breath = c.null(
        "breath",
        foot,
        parent=figure,
        s=keys(
            [
                (0, [100, 100, 100], EASE, False),
                (LOOP / 2, [103, 98, 100], EASE, False),
                (LOOP, [100, 100, 100], LINEAR, False),
                (sleep, [100, 100, 100], EASE, False),
                (sleep + 60, [102, 99, 100], EASE, False),
                (sleep + 120, [100, 100, 100], None, False),
            ]
        ),
    )
    body_center = (48.0, 50.0)
    c.shape("body", "xq-fill-w", [ellipse(48, 50, 30)], breath, body_center)
    c.shape("body-line", "xq-line-on", [ellipse(48, 50, 30)], breath, body_center, width=1.5)

    eye_pivot = (48.0, 47.0)
    # 睁着的眼睛：眨眼（4 秒一次，第 92%–100% 之间）；close 段压扁后隐藏
    c.shape(
        "eyes-open",
        "xq-fill-on",
        [ellipse(38, 47, 3), ellipse(58, 47, 3)],
        breath,
        eye_pivot,
        s=keys(
            [
                (0, [100, 100, 100], LINEAR, False),
                (LOOP * 0.92, [100, 100, 100], LINEAR, False),
                (LOOP * 0.96, [100, 10, 100], LINEAR, False),
                (LOOP, [100, 100, 100], LINEAR, False),
                (close, [100, 100, 100], EASE, False),
                (close + 6, [100, 10, 100], None, False),
            ]
        ),
        o=keys([(0, 100, None, True), (close + 6, 0, None, True)]),
    )
    c.shape(
        "eyes-closed",
        "xq-line-on",
        shapes_of("M34 48q4 4 8 0") + shapes_of("M54 48q4 4 8 0"),
        breath,
        eye_pivot,
        width=2,
        o=keys([(0, 0, None, True), (close + 6, 100, None, True)]),
    )
    c.shape("mouth", "xq-line-on", shapes_of("M42 60q6 5 12 0"), breath, (48, 60), width=2)
    return c.to_json()


def add_accessory(c: Comp, weather: str, figure: int) -> None:
    """各天气的配饰（DS-COLOR-02：状态除颜色外还要有不同的形状）。只在 loop 段动，其余段停在第 0 帧的样子。"""
    if weather == "sunny":
        # 8 道光芒，4 秒转 45°（对称，首尾看起来一样）
        rays = []
        for k in range(1, 9):
            a = math.radians(k * 45)

            def rot(x: float, y: float) -> Point:
                dx, dy = x - 48, y - 48
                return (48 + dx * math.cos(a) - dy * math.sin(a), 48 + dx * math.sin(a) + dy * math.cos(a))

            p0, p1 = rot(48, 4), rot(48, 12)
            rays.append(lottie_path({"start": p0, "segs": [(p0, p1, p1)], "closed": False}))
        c.shape(
            "rays",
            "xq-line-text",
            rays,
            figure,
            (48, 48),
            width=2,
            r=keys([(0, 0, LINEAR, False), (LOOP, 45, None, False)]),
        )
    elif weather == "cloudy":
        d = "M8 34a9 9 0 0 1 9-9 12 12 0 0 1 22 2 8 8 0 0 1 0 16H17a9 9 0 0 1-9-9z"
        pivot = (24.0, 34.0)
        drift = keys(
            [
                (0, offset_pos(pivot, 0, 0), EASE, False),
                (LOOP / 2, offset_pos(pivot, 4, 0), EASE, False),
                (LOOP, offset_pos(pivot, 0, 0), None, False),
            ]
        )
        c.shape("cloud", "xq-fill-w", shapes_of(d), figure, pivot, p=drift)
        c.shape("cloud-line", "xq-line-on", shapes_of(d), figure, pivot, width=1.5, p=drift)
    elif weather == "rain":
        # 三滴雨，2 秒落一次，彼此错开 0.7 秒
        for n, (x, y) in enumerate([(22, 80), (48, 84), (74, 80)]):
            d = f"M{x} {y}q-3 5 0 7t0-7z"
            pivot = (float(x), float(y))
            offset = n * 21  # 0.7 秒
            # 落到底的那一刻跳回起点并变透明（与原来的 CSS 关键帧一致）
            fall = tiled([(0.0, -4.0, EASE_IN), (0.99, 0.0, LINEAR), (1.0, -4.0, LINEAR)], 60, offset, LOOP)
            fade = tiled([(0.0, 0.0, LINEAR), (0.2, 100.0, LINEAR), (0.99, 100.0, LINEAR), (1.0, 0.0, LINEAR)], 60, offset, LOOP)
            pos = keys([(t, offset_pos(pivot, 0, v), e, h) for t, v, e, h in fall])
            op = keys(fade)
            c.shape(f"drop-{n}", "xq-fill-w", shapes_of(d), figure, pivot, p=pos, o=op)
            c.shape(f"drop-{n}-line", "xq-line-on", shapes_of(d), figure, pivot, width=1.5, p=pos, o=op)
    elif weather == "storm":
        # 闪电每 4 秒暗一下再亮回来（远低于 DS-MOTION-04 的每秒 3 次）
        d = "M76 6 64 28h9l-7 18 18-24h-9l6-16z"
        flash = keys(
            [
                (0, 100, LINEAR, False),
                (LOOP * 0.8, 100, LINEAR, False),
                (LOOP * 0.85, 35, LINEAR, False),
                (LOOP * 0.9, 100, LINEAR, False),
                (LOOP, 100, None, False),
            ]
        )
        c.shape("bolt", "xq-fill-w", shapes_of(d), figure, (76, 26), o=flash)
        c.shape("bolt-line", "xq-line-on", shapes_of(d), figure, (76, 26), width=1.5, o=flash)
    elif weather == "night":
        d = "M84 10a12 12 0 1 0 6 20 10 10 0 1 1-6-20z"
        pivot = (84.0, 20.0)
        bob = keys(
            [
                (0, offset_pos(pivot, 0, 0), EASE, False),
                (LOOP / 2, offset_pos(pivot, 0, -3), EASE, False),
                (LOOP, offset_pos(pivot, 0, 0), None, False),
            ]
        )
        c.shape("moon", "xq-fill-w", shapes_of(d), figure, pivot, p=bob)
        c.shape("moon-line", "xq-line-on", shapes_of(d), figure, pivot, width=1.5, p=bob)
    elif weather == "wind":
        # 三道风线，2 秒摆一次，彼此错开 0.3 秒
        for n, d in enumerate(["M4 30h20a5 5 0 1 0-5-5", "M4 40h28", "M4 50h18a5 5 0 1 1-5 5"]):
            pivot = (16.0, 30.0 + 10 * n)
            sway = tiled([(0.0, 0.0, EASE), (0.5, 3.0, EASE), (1.0, 0.0, LINEAR)], 60, n * 9, LOOP)
            pos = keys([(t, offset_pos(pivot, v, 0), e, h) for t, v, e, h in sway])
            c.shape(f"gust-{n}", "xq-line-text", shapes_of(d), figure, pivot, width=2, p=pos)
    else:  # pragma: no cover
        raise ValueError(weather)


def render(weather: str) -> str:
    return json.dumps(build(weather), ensure_ascii=False, separators=(",", ":")) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="只核对已提交的文件，不写")
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    stale = []
    for w in WEATHERS:
        path = OUT / f"{w}.json"
        text = render(w)
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(path.name)
        else:
            path.write_text(text, encoding="utf-8")
    if stale:
        print("小精灵动画不是最新的，请运行 python3 tools/gen_sprite_lottie.py：" + "、".join(stale))
        return 1
    if not args.check:
        print(f"已生成 {len(WEATHERS)} 个小精灵动画到 {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
