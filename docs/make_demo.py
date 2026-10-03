#!/usr/bin/env python3
"""Render docs/demo.gif: a test run without tokencat, then the same run with it.

    python3 docs/make_demo.py BEFORE.log AFTER.md docs/demo.gif \
        --before-cmd "pytest -v" --after-cmd "tokencat run -- pytest -v"

BEFORE.log is the raw output of the test command, AFTER.md what
`tokencat run -- <cmd>` printed for the same run. Needs Pillow and a
monospace TrueType font (DejaVu Sans Mono by default).
"""

import argparse
import re

from PIL import Image, ImageDraw, ImageFont

COLS, ROWS = 100, 42
FONT_SIZE = 15
PAD = 16
BAR = 36

BG = (13, 17, 23)
CHROME = (22, 27, 34)
FG = (201, 209, 217)
DIM = (125, 133, 144)
GREEN = (63, 185, 80)
RED = (248, 81, 73)
YELLOW = (210, 153, 34)
BLUE = (88, 166, 255)
PURPLE = (188, 140, 255)
BADGE_BG = (48, 54, 61)

LIGHTS = ((255, 95, 86), (255, 189, 46), (39, 201, 63))

FALLBACK_CHARS = "⋮"


def tokens_label(n):
    return f"{n:,} tokens"


def color_before(line):
    """Spans for a raw pytest line: PASSED green, FAILED red, and so on."""
    if line.startswith("E "):
        return [(line, RED)]
    if line.startswith("=") or line.startswith("_"):
        return [(line, DIM)]
    for word, color in (("PASSED", GREEN), ("FAILED", RED), ("ERROR", RED),
                        ("SKIPPED", YELLOW), ("XFAIL", YELLOW)):
        i = line.find(word)
        if i >= 0:
            return [(line[:i], FG), (word, color), (line[i + len(word):], DIM)]
    return [(line, FG)]


def color_after(line, in_code):
    if line.startswith("## "):
        return [(line, RED if "✗" in line else GREEN)]
    if line.startswith("### "):
        return [(line, BLUE)]
    if line.startswith("```"):
        return [(line, DIM)]
    if line.startswith("[tokencat:"):
        return [(line, PURPLE)]
    if in_code:
        m = re.match(r"^(>?\s*\d+ \| )(.*)$", line)
        if m:
            gutter_color = RED if line.startswith(">") else DIM
            return [(m.group(1), gutter_color), (m.group(2), FG)]
        return [(line, DIM)]
    if line.startswith("at ") or line.startswith("  at "):
        return [(line, DIM)]
    return [(line, FG)]


class Term:
    def __init__(self, font_path, fallback_path):
        self.font = ImageFont.truetype(font_path, FONT_SIZE)
        self.fallback = ImageFont.truetype(fallback_path, FONT_SIZE)
        box = self.font.getbbox("M")
        self.cw = self.font.getlength("M")
        self.lh = int((box[3] - box[1]) * 1.55)
        self.w = int(PAD * 2 + self.cw * COLS)
        self.h = BAR + PAD * 2 + self.lh * ROWS

    def frame(self, title, rows, badge=None, badge_color=FG):
        img = Image.new("RGB", (self.w, self.h), BG)
        d = ImageDraw.Draw(img)
        d.rectangle([0, 0, self.w, BAR], fill=CHROME)
        for i, c in enumerate(LIGHTS):
            x = 18 + i * 20
            d.ellipse([x - 6, BAR // 2 - 6, x + 6, BAR // 2 + 6], fill=c)
        tw = d.textlength(title, font=self.font)
        d.text(((self.w - tw) / 2, BAR // 2), title, font=self.font, fill=DIM, anchor="lm")
        y = BAR + PAD
        for spans in rows[-ROWS:]:
            x = PAD
            used = 0
            for text, color in spans:
                text = text[: max(0, COLS - used)]
                # Glyphs the monospace font lacks are drawn with the fallback.
                for part in re.split(f"([{FALLBACK_CHARS}])", text):
                    font = self.fallback if part and part in FALLBACK_CHARS else self.font
                    d.text((x, y), part, font=font, fill=color)
                    x += self.cw * len(part)
                used += len(text)
            y += self.lh
        if badge:
            bw = d.textlength(badge, font=self.font) + 20
            x0 = self.w - bw - 10
            d.rounded_rectangle([x0, 5, x0 + bw, BAR - 5], radius=7, fill=BADGE_BG)
            d.text((x0 + 10, BAR // 2), badge, font=self.font, fill=badge_color, anchor="lm")
        return img


def build_palette():
    """Every text color blended over every background, as anti-aliasing does."""
    inks = [FG, DIM, GREEN, RED, YELLOW, BLUE, PURPLE] + list(LIGHTS)
    colors = []
    for bg in (BG, CHROME, BADGE_BG):
        colors.append(bg)
        for ink in inks:
            for i in range(1, 8):
                t = i / 7
                colors.append(tuple(round(b + (c - b) * t) for b, c in zip(bg, ink)))
    colors = list(dict.fromkeys(colors))[:256]
    return [v for c in colors for v in c]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("before")
    ap.add_argument("after")
    ap.add_argument("out")
    ap.add_argument("--before-cmd", default="pytest -v")
    ap.add_argument("--after-cmd", default="tokencat run -- pytest -v")
    ap.add_argument("--font", default="/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")
    ap.add_argument("--fallback-font", default="/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")
    args = ap.parse_args()

    before = open(args.before, encoding="utf-8", errors="replace").read().splitlines()
    after = open(args.after, encoding="utf-8").read().rstrip("\n").splitlines()
    footer = next((l for l in after if l.startswith("[tokencat:")), "")
    m = re.search(r"([\d,]+) -> ([\d,]+) tokens", footer)
    tok_before, tok_after = (int(x.replace(",", "")) for x in m.groups())

    term = Term(args.font, args.fallback_font)
    frames, durations = [], []

    def add(img, ms):
        frames.append(img)
        durations.append(ms)

    def type_cmd(title, cmd, prefix_rows=()):
        for i in range(0, len(cmd) + 1, 2):
            rows = list(prefix_rows) + [[("$ ", GREEN), (cmd[:i] + "▌", FG)]]
            add(term.frame(title, rows), 45)
        add(term.frame(title, list(prefix_rows) + [[("$ ", GREEN), (cmd, FG)]]), 400)

    # Act 1: the raw run scrolls past while the context counter climbs.
    title1 = "without tokencat"
    type_cmd(title1, args.before_cmd)
    prompt1 = [[("$ ", GREEN), (args.before_cmd, FG)]]
    total = len(before)
    shown = 0
    steps = [3, 6, 10, 16, 24] + [max(1, total // 28)] * 40
    for step in steps:
        shown = min(total, shown + step)
        rows = prompt1 + [color_before(l) for l in before[:shown]]
        badge = "context: " + tokens_label(tok_before * shown // total)
        add(term.frame(title1, rows, badge, RED), 70)
        if shown == total:
            break
    add(term.frame(title1, prompt1 + [color_before(l) for l in before],
                   f"context: {tokens_label(tok_before)}  ({total:,} lines)", RED), 2600)

    # Act 2: the same run through tokencat.
    title2 = "with tokencat"
    type_cmd(title2, args.after_cmd)
    prompt2 = [[("$ ", GREEN), (args.after_cmd, FG)]]
    colored, in_code = [], False
    for l in after:
        colored.append(color_after(l, in_code))
        if l.startswith("```"):
            in_code = not in_code
    add(term.frame(title2, prompt2), 500)
    for i in range(1, len(colored) + 1, 3):
        add(term.frame(title2, prompt2 + colored[:i]), 40)
    add(term.frame(title2, prompt2 + colored, "context: " + tokens_label(tok_after), GREEN), 6000)

    # One shared palette keeps colors stable and the file small.
    palette = Image.new("P", (1, 1))
    palette.putpalette(build_palette())
    quantized = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in frames]
    quantized[0].save(args.out, save_all=True, append_images=quantized[1:],
                      duration=durations, loop=0, optimize=True, disposal=1)


if __name__ == "__main__":
    main()
