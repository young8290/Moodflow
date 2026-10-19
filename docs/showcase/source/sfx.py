#!/usr/bin/env python3
"""按 sfx.json 里的事件时间合成音效轨（打字、弹出、转场、组合键、logo 叮），混进视频。

    python3 sfx.py            # 生成 sfx.wav，并合成 moodflow-showcase.mp4（视频 + 音效）

音效全部用程序合成，没有外部素材，可商用。轻音乐另走一轨，后期叠加。
"""
import json
import subprocess
import wave
from pathlib import Path

import numpy as np

HERE = Path(__file__).parent
SR = 48000
rng = np.random.default_rng(7)


def env(n, attack, decay):
    t = np.arange(n) / SR
    return np.minimum(1, t / max(attack, 1e-4)) * np.exp(-t / decay)


def bandnoise(n, lo, hi):
    x = rng.standard_normal(n)
    f = np.fft.rfft(x)
    freqs = np.fft.rfftfreq(n, 1 / SR)
    f[(freqs < lo) | (freqs > hi)] = 0
    y = np.fft.irfft(f, n)
    return y / (np.abs(y).max() + 1e-9)


def key_click():
    """轻薄的键盘声：高频咔嗒 + 一点低频“嗒”。"""
    n = int(0.06 * SR)
    t = np.arange(n) / SR
    click = bandnoise(n, 1800, 7000) * env(n, 0.0005, 0.006)
    thock = np.sin(2 * np.pi * rng.uniform(150, 210) * t) * env(n, 0.001, 0.018)
    return 0.55 * click + 0.45 * thock


def combo():
    n = int(0.12 * SR)
    a = np.zeros(n)
    for off in (0, 0.045):
        k = key_click() * 1.1
        i = int(off * SR)
        a[i : i + len(k)] += k[: n - i]
    return a


def pop():
    """卡片弹出：柔和的上扬短音。"""
    n = int(0.12 * SR)
    t = np.arange(n) / SR
    f = 520 + 380 * (1 - np.exp(-t / 0.03))
    ph = 2 * np.pi * np.cumsum(f) / SR
    return np.sin(ph) * env(n, 0.003, 0.04) * 0.9


def whoosh():
    """转场：一阵轻风。"""
    n = int(0.5 * SR)
    t = np.arange(n) / SR
    shape = np.sin(np.pi * np.clip(t / 0.5, 0, 1)) ** 2
    return bandnoise(n, 300, 2500) * shape * 0.7


def chime():
    """logo 出现：铃音。"""
    n = int(1.8 * SR)
    t = np.arange(n) / SR
    out = np.zeros(n)
    for f, a, d in ((1046.5, 1.0, 0.9), (1568.0, 0.55, 0.7), (2093.0, 0.3, 0.5), (523.25, 0.4, 1.2)):
        out += a * np.sin(2 * np.pi * f * t) * env(n, 0.004, d)
    return out / np.abs(out).max()


GAIN = {"key": 0.32, "combo": 0.4, "pop": 0.16, "whoosh": 0.12, "chime": 0.3}
MAKE = {"key": key_click, "combo": combo, "pop": pop, "whoosh": whoosh, "chime": chime}


def main():
    data = json.loads((HERE / "sfx.json").read_text())
    total = data["total"]
    track = np.zeros(int((total + 2) * SR))
    last = {}
    for e in data["events"]:
        typ, at = e["type"], e["t"]
        # 同类音效太密时跳过（同一时刻多个弹出只响一次）
        gap = {"key": 0.045, "pop": 0.12, "whoosh": 1.0, "combo": 0.3, "chime": 1.0}[typ]
        if at - last.get(typ, -9) < gap:
            continue
        last[typ] = at
        snd = MAKE[typ]() * GAIN[typ] * (rng.uniform(0.8, 1.1) if typ == "key" else 1)
        i = int(at * SR)
        if i < 0 or i >= len(track):
            continue
        j = min(len(track), i + len(snd))
        track[i:j] += snd[: j - i]
    track = track[: int(total * SR)]
    peak = np.abs(track).max()
    if peak > 0.89:
        track *= 0.89 / peak
    pcm = (track * 32767).astype(np.int16)
    with wave.open(str(HERE / "sfx.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    counts = {}
    for e in data["events"]:
        counts[e["type"]] = counts.get(e["type"], 0) + 1
    print("事件：", counts)
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(HERE / "video-only.mp4"), "-i", str(HERE / "sfx.wav"),
         "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ac", "2", "-shortest", "-movflags", "+faststart",
         str(HERE / "moodflow-showcase.mp4")],
        check=True,
    )
    print("→ moodflow-showcase.mp4、sfx.wav")


if __name__ == "__main__":
    main()
