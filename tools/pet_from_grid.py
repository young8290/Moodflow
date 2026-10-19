#!/usr/bin/env python3
"""把生图模型画的 4×4 动作网格图切成 Codex 桌宠精灵图（ADR 0044；规范见 .claude/skills/codex-pet/SKILL.md）。

输入：一个文件夹，里面是 layout.json 列出的 sheet-1.png … sheet-4.png（可选 sheet-5.png），每张 2:3，
4×4 网格（格 1–14 动作、格 15 标准站姿、格 16 色卡），纯品红背景。

输出到 --out：

    pet.json          id、displayName、description、spritesheetPath
    spritesheet.png   8 列 × 9 行，每格 192 × 208，透明底（Codex 与 Moodflow 都能直接用）
    body.png          只有身体的白色蒙版（小组件按天气色给身体上色）；--no-tint 时不出
    lines.png         身体以外的部分（描边、五官、道具、特效、影子）；--no-tint 时不出
    report.txt        每帧的检查结果
    preview/          （--preview）每个动作一张 GIF + 一张总览图

用法：

    python3 tools/pet_from_grid.py --in <图所在文件夹> --out <输出文件夹> [--preview] [--no-tint]
        [--id qingqing --name 晴晴 --desc "…"] [--layout .claude/skills/codex-pet/layout.json]
        [--bg-tol 60] [--body-tol 48] [--no-shadow]

需要 Pillow 与 numpy。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LAYOUT = ROOT / ".claude" / "skills" / "codex-pet" / "layout.json"


def hex_rgb(s: str) -> np.ndarray:
    s = s.lstrip("#")
    return np.array([int(s[i : i + 2], 16) for i in (0, 2, 4)], dtype=np.float32)


def key_out(rgb: np.ndarray, bg: np.ndarray, tol: float) -> np.ndarray:
    """按与背景色的距离算透明度：tol 以内全透明，tol..2·tol 之间渐变（抗锯齿边）。返回 0..1。"""
    d = np.sqrt(((rgb - bg) ** 2).sum(axis=-1))
    return np.clip((d - tol) / tol, 0.0, 1.0)


def despill(rgb: np.ndarray, alpha: np.ndarray, bg: np.ndarray) -> np.ndarray:
    """半透明边缘去掉背景色的“光晕”：把颜色当作 alpha 混合的结果反解出前景色。"""
    a = alpha[..., None]
    fg = np.where(a > 0.02, (rgb - bg * (1 - a)) / np.maximum(a, 0.02), rgb)
    return np.clip(fg, 0, 255)


def bbox(mask: np.ndarray, thr: float = 0.5):
    ys, xs = np.where(mask > thr)
    if len(xs) == 0:
        return None
    return xs.min(), ys.min(), xs.max() + 1, ys.max() + 1


class Sheet:
    def __init__(self, path: Path, layout: dict, bg_tol: float):
        cw, ch = layout["cell"]
        gw, gh = layout["grid"]
        W, H = cw * gw, ch * gh
        img = Image.open(path).convert("RGB")
        if abs(img.width / img.height - W / H) > 0.01:
            raise SystemExit(f"{path.name}: 比例 {img.width}×{img.height} 不是 {W}:{H}")
        if img.size != (W, H):
            img = img.resize((W, H), Image.LANCZOS)
        rgb = np.asarray(img, dtype=np.float32)
        # 背景色：取四条边的中位数（规范是 #FF00FF，模型可能略有偏差）
        border = np.concatenate([rgb[0], rgb[-1], rgb[:, 0], rgb[:, -1]])
        self.bg = np.median(border, axis=0)
        expect = hex_rgb(layout["source"]["background"])
        self.bg_warn = float(np.sqrt(((self.bg - expect) ** 2).sum())) > 60
        self.alpha = key_out(rgb, self.bg, bg_tol)
        self.rgb = despill(rgb, self.alpha, self.bg)
        self.cw, self.ch, self.gw = cw, ch, gw
        self.name = path.name

    def cell(self, n: int):
        r, c = divmod(n - 1, self.gw)
        y0, x0 = r * self.ch, c * self.cw
        return self.rgb[y0 : y0 + self.ch, x0 : x0 + self.cw], self.alpha[y0 : y0 + self.ch, x0 : x0 + self.cw]


def swatch_colors(rgb: np.ndarray, alpha: np.ndarray):
    """色卡格：上中下三条，各取不透明像素的中位色。"""
    h = rgb.shape[0]
    out = []
    for i in range(3):
        band = slice(i * h // 3, (i + 1) * h // 3)
        m = alpha[band] > 0.9
        out.append(np.median(rgb[band][m], axis=0) if m.sum() > 50 else None)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--in", dest="inp", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--layout", type=Path, default=DEFAULT_LAYOUT)
    ap.add_argument("--id", default="qingqing")
    ap.add_argument("--name", default="晴晴")
    ap.add_argument("--desc", default="Moodflow 的小精灵，身体颜色跟着当前的“心情天气”变。")
    ap.add_argument("--no-tint", action="store_true", help="不出 body/lines（形象不是平涂身体时用）")
    ap.add_argument("--no-shadow", action="store_true", help="不在脚下加影子")
    ap.add_argument("--preview", action="store_true")
    ap.add_argument("--bg-tol", type=float, default=60.0)
    ap.add_argument("--body-tol", type=float, default=48.0)
    a = ap.parse_args()

    L = json.loads(a.layout.read_text(encoding="utf-8"))
    T = L["target"]
    TW, TH = T["cell"]
    COLS, ROWS = T["columns"], T["rows"]
    src = L["source"]
    states = L["states"]
    airborne = {k: set(v) for k, v in L.get("airborne", {}).items()}
    rest = set(L.get("rest_states", []))

    sheet_rgba = np.zeros((ROWS * TH, COLS * TW, 4), dtype=np.float32)
    body_mask = np.zeros((ROWS * TH, COLS * TW), dtype=np.float32)
    filled: dict[tuple[str, int], bool] = {}
    report: list[str] = []
    body_color = None

    for sh in L["sheets"]:
        path = a.inp / sh["file"]
        if not path.exists():
            if sh.get("optional"):
                report.append(f"[跳过] {sh['file']}（可选，未提供）")
                continue
            raise SystemExit(f"缺少 {path}")
        S = Sheet(path, L, a.bg_tol)
        if S.bg_warn:
            report.append(f"[警告] {S.name}: 背景色 {S.bg.astype(int).tolist()} 不像 {src['background']}")
        # 色卡
        sw = swatch_colors(*S.cell(L["swatch_cell"]))
        if sw[0] is None:
            report.append(f"[警告] {S.name}: 格 {L['swatch_cell']} 读不出色卡")
        elif body_color is None:
            body_color = sw[0]
            report.append(f"[信息] 身体色 {'#%02X%02X%02X' % tuple(int(v) for v in body_color)}（来自 {S.name} 色卡）")
        # 标准站姿 → 比例与地面
        _, aa = S.cell(L["anchor_cell"])
        bb = bbox(aa)
        if bb is None:
            raise SystemExit(f"{S.name}: 格 {L['anchor_cell']}（标准站姿）是空的")
        ax0, ay0, ax1, ay1 = bb
        scale = T["stand_height"] / (ay1 - ay0)
        anchor_cx, anchor_floor = (ax0 + ax1) / 2, ay1
        report.append(f"[信息] {S.name}: 标准站姿高 {ay1 - ay0}px（规范约 {src['stand_height']}px）→ 缩放 {scale:.3f}")

        for c in sh["cells"]:
            st, fr = c["state"], c["frame"]
            rgb, al = S.cell(c["cell"])
            b = bbox(al)
            tag = f"{S.name} 格{c['cell']} {st}#{fr + 1}"
            if b is None:
                if not c.get("optional"):
                    report.append(f"[错误] {tag}: 空格")
                continue
            x0, y0, x1, y1 = b
            if x0 < 4 or y0 < 4 or x1 > S.cw - 4 or y1 > S.ch - 4:
                report.append(f"[警告] {tag}: 内容贴到格子边上，可能被切掉")
            # 水平：按身体中心对齐（举手、道具不影响居中）；有身体色就用身体色的范围
            cx = (x0 + x1) / 2
            if body_color is not None and not a.no_tint:
                bm = (np.sqrt(((rgb - body_color) ** 2).sum(-1)) < a.body_tol) & (al > 0.5)
                bbb = bbox(bm.astype(np.float32))
                if bbb is not None:
                    cx = (bbb[0] + bbb[2]) / 2
            # 垂直：接地帧把脚底贴到地面；离地帧按标准站姿的地面保留高度
            if fr in airborne.get(st, set()):
                floor_src = anchor_floor
            else:
                floor_src = y1
            dst_w, dst_h = max(1, round(S.cw * scale)), max(1, round(S.ch * scale))
            rgba = np.dstack([rgb, al * 255]).astype(np.uint8)
            small = Image.fromarray(rgba, "RGBA").resize((dst_w, dst_h), Image.LANCZOS)
            ox = round(T["center_x"] - cx * scale)
            oy = round(T["floor_y"] - floor_src * scale)
            frame = Image.new("RGBA", (TW, TH))
            frame.paste(small, (ox, oy), small)
            fa = np.asarray(frame, dtype=np.float32)
            fb = bbox(fa[..., 3] / 255)
            if fb and (fb[0] <= 0 or fb[1] <= 0 or fb[2] >= TW or fb[3] >= TH):
                report.append(f"[警告] {tag}: 缩放后超出 {TW}×{TH} 的格子，边缘被裁")
            row, col = states[st]["row"], fr
            Y, X = row * TH, col * TW
            sheet_rgba[Y : Y + TH, X : X + TW] = fa
            if body_color is not None and not a.no_tint:
                d = np.sqrt(((fa[..., :3] - body_color) ** 2).sum(-1))
                bmask = np.clip((a.body_tol * 1.5 - d) / (a.body_tol * 0.5), 0, 1) * (fa[..., 3] / 255)
                body_mask[Y : Y + TH, X : X + TW] = bmask
                share = bmask.sum() / max(1.0, (fa[..., 3] / 255).sum())
                if share < 0.25:
                    report.append(f"[警告] {tag}: 身体色只占 {share:.0%}，身体是不是不是平涂色？")
                if st in rest:
                    bb2 = bbox(bmask)
                    if bb2:
                        hcx, htop = (bb2[0] + bb2[2]) / 2, bb2[1]
                        rh = T["rest_head"]
                        exp_top = rh["center"][1] - rh["radius"]
                        if abs(hcx - rh["center"][0]) > rh["tolerance"] or abs(htop - exp_top) > rh["tolerance"]:
                            report.append(
                                f"[警告] {tag}: 头部位置 (中心 x {hcx:.0f}, 顶 y {htop:.0f}) 偏离换装锚点 (x {rh['center'][0]}, 顶 y {exp_top})"
                            )
            filled[(st, fr)] = True

    # 镜像补帧
    for dst, srcname in L.get("mirror", {}).items():
        for fr in range(states[dst]["frames"]):
            if (dst, fr) in filled:
                continue
            if (srcname, fr) not in filled:
                continue
            sr, dr = states[srcname]["row"] * TH, states[dst]["row"] * TH
            X = fr * TW
            sheet_rgba[dr : dr + TH, X : X + TW] = sheet_rgba[sr : sr + TH, X : X + TW][:, ::-1]
            body_mask[dr : dr + TH, X : X + TW] = body_mask[sr : sr + TH, X : X + TW][:, ::-1]
            filled[(dst, fr)] = True
        report.append(f"[信息] {dst} 缺的帧由 {srcname} 镜像")

    missing = [f"{st}#{fr + 1}" for st, s in states.items() for fr in range(s["frames"]) if (st, fr) not in filled]
    if missing:
        report.append("[错误] 缺帧：" + "、".join(missing))

    # 影子：每格脚下一个淡椭圆，离地越高越小越淡
    shadow = np.zeros_like(body_mask)
    if not a.no_shadow:
        for st, s in states.items():
            for fr in range(s["frames"]):
                Y, X = s["row"] * TH, fr * TW
                al = sheet_rgba[Y : Y + TH, X : X + TW, 3] / 255
                b = bbox(al)
                if b is None:
                    continue
                lift = max(0.0, T["floor_y"] - b[3])
                k = max(0.35, 1 - lift / 120)
                rx, ry = 40 * k, 7 * k
                yy, xx = np.mgrid[0:TH, 0:TW]
                e = ((xx - T["center_x"]) / rx) ** 2 + ((yy - (T["floor_y"] + 2)) / ry) ** 2
                shadow[Y : Y + TH, X : X + TW] = np.clip(1 - e, 0, 1) ** 0.6 * 0.16 * k

    def under(rgba: np.ndarray, sh: np.ndarray, ink=(43, 42, 40)) -> np.ndarray:
        """把影子垫在下面。"""
        a_top = rgba[..., 3:4] / 255
        a_sh = sh[..., None]
        a_out = a_top + a_sh * (1 - a_top)
        col = (rgba[..., :3] * a_top + np.array(ink) * a_sh * (1 - a_top)) / np.maximum(a_out, 1e-6)
        return np.dstack([col, a_out[..., 0] * 255])

    a.out.mkdir(parents=True, exist_ok=True)
    full = under(sheet_rgba, shadow)
    Image.fromarray(np.clip(full, 0, 255).astype(np.uint8), "RGBA").save(a.out / "spritesheet.png", optimize=True)
    if not a.no_tint and body_color is not None:
        h, w = body_mask.shape
        body = np.zeros((h, w, 4), dtype=np.uint8)
        body[..., :3] = 255
        body[..., 3] = np.clip(body_mask * 255, 0, 255).astype(np.uint8)
        Image.fromarray(body, "RGBA").save(a.out / "body.png", optimize=True)
        lines = sheet_rgba.copy()
        lines[..., 3] = lines[..., 3] * (1 - np.clip(body_mask, 0, 1))
        Image.fromarray(np.clip(under(lines, shadow), 0, 255).astype(np.uint8), "RGBA").save(a.out / "lines.png", optimize=True)
    elif not a.no_tint:
        report.append("[错误] 没读到身体色，未出 body/lines；检查格 16 色卡，或用 --no-tint")
    (a.out / "pet.json").write_text(
        json.dumps({"id": a.id, "displayName": a.name, "description": a.desc, "spritesheetPath": "spritesheet.png"}, ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )

    if a.preview:
        pv = a.out / "preview"
        pv.mkdir(exist_ok=True)
        sheet_img = Image.open(a.out / "spritesheet.png")
        for st, s in states.items():
            frames = []
            for fr in range(s["frames"]):
                cellimg = sheet_img.crop((fr * TW, s["row"] * TH, (fr + 1) * TW, (s["row"] + 1) * TH))
                bgc = Image.new("RGBA", (TW, TH), (246, 243, 236, 255))
                bgc.alpha_composite(cellimg)
                frames.append(bgc.convert("P", palette=Image.ADAPTIVE))
            frames[0].save(pv / f"{st}.gif", save_all=True, append_images=frames[1:], duration=int(s["ms"] / s["frames"]), loop=0, disposal=2)
        over = Image.new("RGBA", (COLS * TW + 160, ROWS * TH), (246, 243, 236, 255))
        over.alpha_composite(sheet_img, (160, 0))
        d = ImageDraw.Draw(over)
        try:
            font = ImageFont.truetype("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 20)
        except OSError:
            font = ImageFont.load_default()
        for st, s in states.items():
            d.text((12, s["row"] * TH + TH // 2 - 10), st, fill=(43, 42, 40, 255), font=font)
        over.save(pv / "overview.png")

    errors = [r for r in report if r.startswith("[错误]")]
    (a.out / "report.txt").write_text("\n".join(report) + "\n", encoding="utf-8")
    print("\n".join(report))
    print(f"→ {a.out}（{'有错误' if errors else '完成'}）")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
