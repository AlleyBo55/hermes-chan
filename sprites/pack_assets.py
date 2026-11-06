"""Register generated art, pack atlases, and validate the 250-frame collection.

Run from any directory with Python + Pillow. This does not call an image API.
"""
from __future__ import annotations

import hashlib
import json
import shutil
from collections import Counter
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "output/imagegen/hermeschan-chibi/raw"
DEST = ROOT / "sprites/hermeschan-v1"
PLAN = ROOT / "sprites/production-plan/manifest.planned.json"
CELL = (192, 208)


def clean_alpha(image):
    image = image.convert("RGBA")
    alpha = image.getchannel("A")
    peak = alpha.getextrema()[1]
    if peak < 128:
        raise ValueError("Empty or almost transparent generated image")
    # Remove near-invisible model residue; keep the antialiased silhouette.
    image.putalpha(alpha.point(lambda value: max(0, round((value - 8) * 255 / (peak - 8)))))
    return image


def contact_bottom(frame):
    if frame["clip"] == "core/celebrate":
        return [196, 196, 192, 184, 181, 184, 196, 196][frame["index"]]
    if frame["clip"] == "movement/fall":
        return 180
    if frame["clip"] == "movement/land" and frame["index"] == 0:
        return 186
    return 196


def estimate_hands(image):
    """Locate separate skin regions; estimates are explicitly marked in output."""
    pixels = image.load()
    candidates = set()
    for y in range(CELL[1]):
        for x in range(CELL[0]):
            r, g, b, a = pixels[x, y]
            if a > 200 and r > 150 and g > 95 and b > 65 and 9 < r - g < 80 and 2 < g - b < 70:
                candidates.add((x, y))
    regions = []
    while candidates:
        seed = candidates.pop()
        pending, region = [seed], [seed]
        while pending:
            x, y = pending.pop()
            for point in ((x-1,y),(x+1,y),(x,y-1),(x,y+1)):
                if point in candidates:
                    candidates.remove(point)
                    pending.append(point)
                    region.append(point)
        if len(region) >= 3:
            regions.append(region)
    regions.sort(key=len, reverse=True)
    # The largest region is normally the face. Other large regions can be legs.
    hands = [r for r in regions[1:] if len(r) < 160 and sum(p[1] for p in r)/len(r) < 180]
    hands = sorted(hands, key=len, reverse=True)[:2]
    centers = sorted([[round(sum(p[0] for p in r)/len(r)), round(sum(p[1] for p in r)/len(r))] for r in hands])
    return {"screen_left": centers[0] if centers else None,
            "screen_right": centers[-1] if len(centers) > 1 else None,
            "method": "skin-region centroid estimate; not a verified fingertip/contact anchor"}


def main():
    plan = json.loads(PLAN.read_text())
    missing = [Path(f["path"]).name for f in plan["frames"] if not (RAW / Path(f["path"]).name).is_file()]
    if missing:
        raise SystemExit(f"Waiting for {len(missing)} / 250 source frames; first: {missing[0]}")
    for directory in ("frames", "atlases", "contact-sheets"):
        (DEST / directory).mkdir(parents=True, exist_ok=True)

    master = clean_alpha(Image.open(RAW / "core__idle__00.png"))
    master_box = master.getchannel("A").point(lambda a: 255 if a >= 32 else 0).getbbox()
    scale = 176 / (master_box[3] - master_box[1])
    master_x = (master_box[0] + master_box[2]) / 2
    atlases = {name: Image.new("RGBA", tuple(info["size_px"])) for name, info in plan["atlases"].items()}
    runtime = {"version": 1, "character": "Hermes-chan", "cell": list(CELL),
               "display": [96, 104], "atlases": {}, "clips": {},
               "states": plan["state_map"], "emotions": plan["emotion_map"],
               "actions": plan["action_map"], "windowGestures": plan["window_gesture_map"],
               "points": plan["point_map"], "backReactions": plan["back_reactions"]}
    frames, hashes, anchors = [], {}, {}
    for planned in plan["frames"]:
        filename = Path(planned["path"]).name
        source = RAW / filename
        original = Image.open(source)
        if original.mode != "RGBA" or original.getchannel("A").histogram()[0] == 0:
            raise ValueError(f"Missing genuine transparent alpha: {filename}")
        image = clean_alpha(original)
        box = image.getchannel("A").point(lambda a: 255 if a >= 32 else 0).getbbox()
        small = image.resize((round(image.width*scale), round(image.height*scale)), Image.Resampling.LANCZOS)
        offset = [round(96-master_x*scale), round(contact_bottom(planned)-box[3]*scale)]
        frame = Image.new("RGBA", CELL)
        frame.alpha_composite(small, tuple(offset))
        alpha = frame.getchannel("A")
        bounds = alpha.point(lambda a: 255 if a >= 16 else 0).getbbox()
        if not bounds or bounds[0] <= 0 or bounds[1] <= 0 or bounds[2] >= CELL[0] or bounds[3] >= CELL[1]:
            raise ValueError(f"Empty/clipped sprite: {filename}, {bounds}")
        digest = hashlib.sha256(frame.tobytes()).hexdigest()
        if digest in hashes:
            raise ValueError(f"Duplicate artwork: {filename} and {hashes[digest]}")
        hashes[digest] = filename
        frame.save(DEST / "frames" / filename)
        atlas = planned["atlas"]
        atlases[atlas["group"]].alpha_composite(frame, (atlas["column"]*192, atlas["row"]*208))
        info = dict(planned, path=f"frames/{filename}", status="generated", sha256=digest,
                    source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
                    registration={"uniform_scale": scale, "offset_px": offset, "bounds_px": bounds})
        if planned["requires_measured_hand_anchors"]:
            anchors[planned["id"]] = estimate_hands(frame)
        frames.append(info)
    for name, image in atlases.items():
        file = plan["atlases"][name]["file"]
        image.save(DEST / "atlases" / file)
        runtime["atlases"][name] = f"atlases/{file}"
    indexed = {frame["id"]: frame for frame in frames}
    for name, animation in plan["animations"].items():
        runtime["clips"][name] = {
            "loop": animation["loop"], "after": animation["after"],
            "frames": [{"atlas": indexed[f]["atlas"]["group"],
                        "rect": indexed[f]["atlas"]["rect_px"],
                        "duration": indexed[f]["duration_ms"],
                        "event": indexed[f]["contact_event"]} for f in animation["frames"]]}
    (DEST / "manifest.json").write_text(json.dumps(runtime, indent=2)+"\n")
    shutil.copy2(ROOT / "overlay/src/sprite-player.js", DEST / "sprite-player.js")
    (DEST / "frames.json").write_text(json.dumps(frames, indent=2)+"\n")
    (DEST / "hand-anchor-estimates.json").write_text(json.dumps(anchors, indent=2)+"\n")

    font = ImageFont.load_default(size=13)
    for group, atlas in atlases.items():
        columns = plan["atlases"][group]["columns"]
        rows = plan["atlases"][group]["rows"]
        sheet = Image.new("RGB", (columns*192, rows*238), "#ece8e0")
        draw = ImageDraw.Draw(sheet)
        for frame in [f for f in frames if f["atlas"]["group"] == group]:
            col, row = frame["atlas"]["column"], frame["atlas"]["row"]
            art = Image.open(DEST / frame["path"])
            sheet.paste(art, (col*192, row*238), art)
            draw.text((col*192+8, row*238+210), f"{frame['clip'].split('/')[1]} / {frame['index']:02}", fill="#263a3a", font=font)
        sheet.save(DEST / "contact-sheets" / f"{group}.jpg", quality=95)

    overview = Image.new("RGB", (960, 6*150), "#ede9e1")
    draw = ImageDraw.Draw(overview)
    for index, (name, animation) in enumerate(plan["animations"].items()):
        frame = indexed[animation["frames"][0]]
        art = Image.open(DEST / frame["path"])
        art.thumbnail((108, 117), Image.Resampling.LANCZOS)
        x, y = index%8*120, index//8*150
        overview.paste(art, (x+6, y+2), art)
        label = name.replace('grab_window_', 'window_')
        a, b = label.split('/')
        draw.text((x+5,y+120), a, font=font, fill="#65807b")
        draw.text((x+5,y+134), b, font=ImageFont.load_default(size=11), fill="#263a3a")
    overview.save(DEST / "preview.png")

    report = {"frame_count": len(frames), "clip_count": len(runtime["clips"]),
              "size_px": list(CELL), "all_rgba": True, "all_have_transparency": True,
              "unique_pixel_hashes": len(hashes), "clipped_frames": 0,
              "group_counts": dict(Counter(f["atlas"]["group"] for f in frames)),
              "hand_anchors": "Estimates only; not suitable for exact OS targeting without review",
              "visual_review": "pending"}
    (DEST / "validation.json").write_text(json.dumps(report, indent=2)+"\n")
    overlay = ROOT / "overlay/src/hermeschan"
    (overlay / "atlases").mkdir(parents=True, exist_ok=True)
    shutil.copy2(DEST / "manifest.json", overlay / "manifest.json")
    for file in (DEST / "atlases").glob("*.png"):
        shutil.copy2(file, overlay / "atlases" / file.name)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
