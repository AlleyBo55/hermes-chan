"""Compose effect-specific action clips from the approved colored Hermes-chan art.

The character pixels come from the existing 192x208 sprites. Only the small
hand-attached effect marks are drawn here, so her identity stays consistent.
Run after pack_assets.py; the script also refreshes the overlay's sprite copy.
"""
from __future__ import annotations

import json
import math
import shutil
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
SPRITES = ROOT / "sprites/hermeschan-v1"
OVERLAY = ROOT / "overlay/src/hermeschan"
CELL = (192, 208)
SCALE = 4

# Frames 0-1 anticipate, frame 2 causes the app effect, 3 sustains it, 4
# recovers. All artwork below is composed from the approved original sprites.
EFFECTS = {
    "crack": {
        "sources": ["grab_window_throw/00", "grab_window_throw/01",
                    "grab_window_throw/02", "grab_window_throw/03", "glitch/03"],
        "anchor": (151, 105), "duration": 1400,
    },
    "portal": {
        "sources": ["point/00", "point/01", "point/02", "point/03", "glitch/03"],
        "anchor": (151, 108), "duration": 2200,
    },
    "text_shatter": {
        "sources": ["type/00", "type/01", "type/02", "type/03", "glitch/03"],
        "anchor": (145, 109), "duration": 1650,
    },
    "ripple": {
        "sources": ["scroll/00", "scroll/01", "scroll/02", "scroll/03", "glitch/03"],
        "anchor": (150, 107), "duration": 1400,
    },
    "pixel_dissolve": {
        "sources": ["glitch/00", "glitch/01", "glitch/02", "glitch/03", "point/03"],
        "anchor": (154, 102), "duration": 1600,
    },
    "lightning": {
        "sources": ["grab_window_shake/00", "grab_window_shake/01",
                    "grab_window_shake/02", "grab_window_shake/03", "glitch/03"],
        "anchor": (153, 105), "duration": 1300,
    },
    "tear": {
        "sources": ["grab_window_bend/00", "grab_window_bend/01",
                    "grab_window_bend/02", "grab_window_bend/03", "glitch/03"],
        "anchor": (148, 110), "duration": 1700,
    },
}


def line(draw: ImageDraw.ImageDraw, points, fill, width=1):
    draw.line([(round(x * SCALE), round(y * SCALE)) for x, y in points],
              fill=fill, width=max(1, round(width * SCALE)), joint="curve")


def ellipse(draw: ImageDraw.ImageDraw, box, outline, width=1):
    draw.ellipse(tuple(round(value * SCALE) for value in box),
                 outline=outline, width=max(1, round(width * SCALE)))


def mark(draw: ImageDraw.ImageDraw, effect: str, frame: int, anchor):
    if frame == 0:
        return
    x, y = anchor
    strength = [0, 0.42, 1, 0.85, 0.34][frame]
    radius = [0, 4, 13, 17, 6][frame]
    cyan = (91, 231, 255, round(245 * strength))
    violet = (167, 117, 255, round(235 * strength))
    white = (247, 251, 255, round(255 * strength))
    pink = (255, 108, 204, round(235 * strength))

    if effect == "crack":
        for angle, length in ((-2.35, 17), (-1.45, 23), (-0.55, 20),
                              (0.44, 19), (1.35, 17)):
            length *= strength
            point = (x + math.cos(angle) * length, y + math.sin(angle) * length)
            turn = (point[0] + math.cos(angle + 0.55) * length * 0.35,
                    point[1] + math.sin(angle + 0.55) * length * 0.35)
            line(draw, [(x, y), point, turn], (30, 88, 170, round(190 * strength)), 2.6)
            line(draw, [(x, y), point, turn], white, 1.1)
        ellipse(draw, (x - 2, y - 2, x + 2, y + 2), white, 2)
    elif effect == "portal":
        for offset, color, width in ((0, violet, 3.4), (3, cyan, 2.1), (-3, white, 1.2)):
            r = radius + offset
            if r > 1:
                ellipse(draw, (x - r * 0.75, y - r, x + r * 0.75, y + r), color, width)
        for index in range(5):
            angle = index * 1.27 + frame * 0.58
            dot_x = x + math.cos(angle) * radius * 0.84
            dot_y = y + math.sin(angle) * radius * 1.05
            ellipse(draw, (dot_x - 1.1, dot_y - 1.1, dot_x + 1.1, dot_y + 1.1), white, 2)
    elif effect == "text_shatter":
        font = ImageFont.truetype("/System/Library/Fonts/Supplemental/Arial Bold.ttf", 8 * SCALE)
        for index, glyph in enumerate("Aa!?"):
            angle = -1.2 + index * 0.68
            distance = radius * (0.6 + (index % 2) * 0.3)
            gx = x + math.cos(angle) * distance
            gy = y + math.sin(angle) * distance
            draw.text((round(gx * SCALE), round(gy * SCALE)), glyph,
                      font=font, fill=white if index % 2 else cyan,
                      stroke_width=SCALE, stroke_fill=(40, 72, 130, round(220 * strength)))
        for dx, dy in ((-6, -9), (8, -5), (13, 6)):
            line(draw, [(x + dx * strength, y + dy * strength),
                        (x + (dx + 3) * strength, y + (dy - 3) * strength)], pink, 1.6)
    elif effect == "ripple":
        for factor, color in ((0.45, white), (0.72, cyan), (1.0, violet)):
            r = max(2, radius * factor)
            ellipse(draw, (x - r, y - r * 0.55, x + r, y + r * 0.55), color, 1.7)
    elif effect == "pixel_dissolve":
        for index in range(14):
            dx = ((index * 17) % 35 - 11) * strength
            dy = ((index * 11) % 29 - 13) * strength
            size = 1.4 + index % 3
            fill = [cyan, violet, pink, white][index % 4]
            draw.rectangle(tuple(round(v * SCALE) for v in
                         (x + dx, y + dy, x + dx + size, y + dy + size)), fill=fill)
    elif effect == "lightning":
        for branch in (-1, 0, 1):
            end_x = x + (7 + branch * 3) * strength
            end_y = y + (branch * 15 - 8) * strength
            points = [(x, y), (x + 5 * strength, y - 5 * strength),
                      (x + 2 * strength, y - 6 * strength),
                      (end_x, end_y)]
            line(draw, points, (32, 96, 217, round(210 * strength)), 4.2)
            line(draw, points, white if branch == 0 else cyan, 2)
    elif effect == "tear":
        h = radius * 1.55
        points = [(x, y - h), (x + 5 * strength, y - h * 0.52),
                  (x - 2 * strength, y - h * 0.08),
                  (x + 6 * strength, y + h * 0.35), (x, y + h)]
        line(draw, points, (47, 43, 114, round(235 * strength)), 5)
        line(draw, points, pink, 2.8)
        line(draw, [(px - 2, py) for px, py in points], cyan, 0.8)


def draw_frame(source: Image.Image, effect: str, frame: int, anchor):
    if source.size != CELL:
        raise ValueError(f"Unexpected sprite size {source.size}")
    mark_layer = Image.new("RGBA", (CELL[0] * SCALE, CELL[1] * SCALE))
    mark(ImageDraw.Draw(mark_layer), effect, frame, anchor)
    mark_layer = mark_layer.resize(CELL, Image.Resampling.LANCZOS)
    result = source.copy().convert("RGBA")
    result.alpha_composite(mark_layer)
    return result


def build(destination: Path):
    manifest_path = destination / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    atlas = Image.new("RGBA", (CELL[0] * 5, CELL[1] * len(EFFECTS)))
    frames_dir = destination / "frames"
    frames_dir.mkdir(exist_ok=True)
    for row, (effect, spec) in enumerate(EFFECTS.items()):
        duration = spec["duration"]
        durations = [120, 120, round(duration * 0.45), round(duration * 0.35), 0]
        durations[-1] = duration - durations[2] - durations[3]
        clip_frames = []
        for index, source_name in enumerate(spec["sources"]):
            source_path = frames_dir / f"action__{source_name.replace('/', '__')}.png"
            source = Image.open(source_path).convert("RGBA")
            image = draw_frame(source, effect, index, spec["anchor"])
            image.save(frames_dir / f"action__{effect}__{index:02}.png")
            atlas.alpha_composite(image, (index * CELL[0], row * CELL[1]))
            clip_frames.append({
                "atlas": "effect_action",
                "rect": [index * CELL[0], row * CELL[1], *CELL],
                "duration": durations[index],
                "event": "effect_start" if index == 2 else None,
            })
        manifest["clips"][f"action/{effect}"] = {
            "loop": False, "after": "core/idle", "frames": clip_frames,
        }
    manifest["atlases"]["effect_action"] = "atlases/hermeschan-effect-actions.png"
    (destination / "atlases").mkdir(exist_ok=True)
    atlas.save(destination / manifest["atlases"]["effect_action"])
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    return atlas


def main():
    atlas = build(SPRITES)
    OVERLAY.mkdir(parents=True, exist_ok=True)
    shutil.copy2(SPRITES / "manifest.json", OVERLAY / "manifest.json")
    (OVERLAY / "atlases").mkdir(exist_ok=True)
    atlas.save(OVERLAY / "atlases/hermeschan-effect-actions.png")

    preview = Image.new("RGB", (CELL[0] * 5, (CELL[1] + 28) * len(EFFECTS)), "#ece8e0")
    draw = ImageDraw.Draw(preview)
    for row, effect in enumerate(EFFECTS):
        for index in range(5):
            image = Image.open(SPRITES / "frames" / f"action__{effect}__{index:02}.png")
            preview.paste(image, (index * CELL[0], row * (CELL[1] + 28)), image)
            draw.text((index * CELL[0] + 8, row * (CELL[1] + 28) + CELL[1] + 5),
                      f"{effect} / {index:02}" + ("  IMPACT" if index == 2 else ""),
                      fill="#263a3a")
    preview.save(SPRITES / "effect-actions-preview.png")
    print(f"Built {len(EFFECTS) * 5} effect frames in {len(EFFECTS)} clips")


if __name__ == "__main__":
    main()
