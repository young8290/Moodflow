#!/usr/bin/env python3
"""从品牌母版生成 Moodflow 的全部图标（logo 源文件见 assets/brand/，说明见 assets/README.md）。

输入：assets/brand/icon-1024.png（应用图标母版，1024×1024，白色圆角底 + 品牌原图 logo-original.png 里的标志）
输出：
  moodflow_tsf/res/moodflow_input.ico        TSF DLL 与核心 exe 的图标，10 层 16→256
  assets/installer.ico                       安装程序图标，同上
  assets/logo.png                            安装向导 logo，144×144
  moodflow_hub/src-tauri/icons/32x32.png、128x128.png、128x128@2x.png（256）、icon.png（512）、icon.ico（10 层）

缩放一律先预乘 alpha 再缩、之后反预乘，避免透明区的黑色渗进圆角边缘。需要 Pillow：

    python3 scripts/moodflow/gen_icons.py
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
MASTER = ROOT / "assets" / "brand" / "icon-1024.png"
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
HUB = ROOT / "moodflow_hub" / "src-tauri" / "icons"


def scaled(src: Image.Image, size: int) -> Image.Image:
    """预乘 alpha 后用 LANCZOS 缩放，再反预乘。"""
    rgba = src.convert("RGBA")
    pre = Image.new("RGBA", rgba.size)
    r, g, b, a = rgba.split()
    from PIL import ImageChops

    pre = Image.merge("RGBA", (ImageChops.multiply(r, a), ImageChops.multiply(g, a), ImageChops.multiply(b, a), a))
    small = pre.resize((size, size), Image.LANCZOS)
    px = small.load()
    for y in range(size):
        for x in range(size):
            pr, pg, pb, pa = px[x, y]
            if pa:
                px[x, y] = (min(255, pr * 255 // pa), min(255, pg * 255 // pa), min(255, pb * 255 // pa), pa)
            else:
                px[x, y] = (0, 0, 0, 0)
    return small


def write_ico(src: Image.Image, path: Path) -> None:
    layers = [scaled(src, s) for s in ICO_SIZES]
    layers[-1].save(path, format="ICO", sizes=[(s, s) for s in ICO_SIZES], append_images=layers[:-1])


def main() -> None:
    src = Image.open(MASTER)
    write_ico(src, ROOT / "moodflow_tsf" / "res" / "moodflow_input.ico")
    write_ico(src, ROOT / "assets" / "installer.ico")
    scaled(src, 144).save(ROOT / "assets" / "logo.png", optimize=True)
    for name, size in (("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)):
        scaled(src, size).save(HUB / name, optimize=True)
    write_ico(src, HUB / "icon.ico")
    print("已生成：TSF / 安装包 / Hub 图标")


if __name__ == "__main__":
    main()
