"""Cut the approved motion sheets into registered left/right source PNGs.

The two sheets keep all poses for each motion in one drawing, which makes the
character scale and outfit much steadier across frames. Right-facing frames are
reflections of the left-facing cycle. Run before ``pack_assets.py``.
"""
from __future__ import annotations

import shutil
from pathlib import Path

from PIL import Image, ImageOps


ROOT = Path(__file__).resolve().parent.parent
ART = ROOT / "output/imagegen/hermeschan-chibi"
RAW = ART / "raw"
ORIGINALS = ART / "motion-originals"
CELL = (384, 512)
CANVAS = (1024, 1024)
SHEETS = {"walk": ART / "walk-sheet-v2.png", "climb": ART / "climb-sheet-v2.png"}


def subject(cell: Image.Image) -> Image.Image:
    cell = cell.convert("RGBA")
    alpha = cell.getchannel("A")
    # Generated sheets can have invisible near-zero residue between cells.
    alpha = alpha.point(lambda a: a if a >= 8 else 0)
    cell.putalpha(alpha)
    bounds = alpha.point(lambda a: 255 if a >= 32 else 0).getbbox()
    if not bounds or bounds[0] < 8 or bounds[1] < 4 or bounds[2] > 376 or bounds[3] > 500:
        raise ValueError(f"Subject touches a cell boundary: {bounds}")
    return cell.crop((max(0, bounds[0] - 2), max(0, bounds[1] - 2),
                      min(CELL[0], bounds[2] + 2), min(CELL[1], bounds[3] + 2)))


def main() -> None:
    master = Image.open(RAW / "core__idle__00.png").convert("RGBA")
    bounds = master.getchannel("A").point(lambda a: 255 if a >= 32 else 0).getbbox()
    center_x = round((bounds[0] + bounds[2]) / 2)
    ORIGINALS.mkdir(parents=True, exist_ok=True)
    for motion, sheet_path in SHEETS.items():
        sheet = Image.open(sheet_path).convert("RGBA")
        if sheet.size != (1536, 1024):
            raise ValueError(f"Unexpected sheet size: {sheet_path}: {sheet.size}")
        poses = []
        for index in range(8):
            column, row = index % 4, index // 4
            cell = sheet.crop((column * CELL[0], row * CELL[1],
                               (column + 1) * CELL[0], (row + 1) * CELL[1]))
            poses.append(subject(cell))
        # Uniform scale across all eight phases. Smaller poses stay smaller.
        scale = min(980 / max(p.height for p in poses), 800 / max(p.width for p in poses))
        for index, pose in enumerate(poses):
            size = (round(pose.width * scale), round(pose.height * scale))
            pose = pose.resize(size, Image.Resampling.LANCZOS)
            for side in ("left", "right"):
                filename = f"movement__{motion}_{side}__{index:02}.png"
                target = RAW / filename
                backup = ORIGINALS / filename
                if target.exists() and not backup.exists():
                    shutil.copy2(target, backup)
                art = pose if side == "left" else ImageOps.mirror(pose)
                canvas = Image.new("RGBA", CANVAS)
                canvas.alpha_composite(art, (center_x - art.width // 2, 1008 - art.height))
                canvas.save(target)
        print(f"{motion}: 8 cohesive poses × 2 directions; source scale {scale:.3f}")


if __name__ == "__main__":
    main()
