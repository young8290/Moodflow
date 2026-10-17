#!/usr/bin/env python3
"""产品名检查（ADR 0045）：仓库里不应再出现改名前的名字。

旧名：心晴 / XinQing / 清风 / WindInput / wind_* 代码前缀 / XQP / xq- 前缀。
例外只有出处声明：LICENSE、NOTICE.md、CLA 记录、指向上游仓库或网站的链接，
以及少数写明“基于 WindInput”的署名句（见 ALLOWED_LINE）。

    python3 scripts/moodflow/check_names.py   # 有残留则列出并以退出码 1 结束（CI 用）
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

OLD = re.compile(
    r"心晴|清风|[Xx]in[Qq]ing|XINQING|[Ww]ind[Ii]nput|WINDINPUT|XQP|[Xx]qp"
    r"|(?<![A-Za-z0-9])[Ww]ind[-_][A-Za-z0-9]|(?<![A-Za-z0-9])WIND_"
    r"|(?<![A-Za-z0-9_])(?:xq|XQ)(?=[-_:.][A-Za-z*])"
)

SKIP_FILES = re.compile(
    r"^(LICENSE|NOTICE\.md|CLA\.md|signatures/|docs/release-notes/|docs/archive/"
    r"|docs/adr/0045-|scripts/moodflow/check_names\.py$)"
)

# 整行放行：上游仓库 / 网站链接、GitHub Actions 检出的上游仓库、署名句
ALLOWED_LINE = re.compile(
    r"github\.com/huanfeng/|young8290/xinqing|huanfeng/[A-Za-z0-9_.-]+|young8290/wind-installer"
    r"|windinput\.com|hellogithub\.com"
    r"|开源项目 WindInput|基于的 WindInput|所基于的上游|WindInput 以 MIT"
    r"|原名心晴|原名 心晴|之前叫心晴|手动删掉旧的"
)

TEXT_SUFFIXES = {
    ".rs", ".toml", ".ts", ".vue", ".js", ".mjs", ".json", ".md", ".py", ".ps1", ".sh",
    ".bat", ".yml", ".yaml", ".cpp", ".h", ".hpp", ".c", ".in", ".rc", ".def", ".swift",
    ".plist", ".strings", ".txt", ".css", ".html", ".desktop", ".xml", ".conf", ".cmake",
}


def main() -> int:
    files = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, capture_output=True, check=True
    ).stdout.decode().split("\0")
    hits = []
    for f in files:
        if not f or SKIP_FILES.match(f):
            continue
        p = ROOT / f
        if p.suffix not in TEXT_SUFFIXES and p.name not in {"Makefile", "CMakeLists.txt"}:
            continue
        try:
            lines = p.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeDecodeError):
            continue
        for i, line in enumerate(lines, 1):
            if OLD.search(line) and not ALLOWED_LINE.search(line):
                hits.append(f"{f}:{i}: {line.strip()[:120]}")
        if OLD.search(f):
            hits.append(f"{f}: 文件名含旧名")
    if hits:
        print("发现改名前的名字（ADR 0045）：")
        print("\n".join(hits))
        return 1
    print("产品名检查通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
