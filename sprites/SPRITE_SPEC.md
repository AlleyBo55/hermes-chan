# Sprite spec

Hermechan's art is **original**. The placeholder in this folder
(`hermeschan-placeholder.svg`) is an original, simple chibi-maid design drawn
from basic shapes — she is **not** the Nous Research logo mascot and **not** any
existing anime character. Final art can be hand-drawn to this spec or generated
with Hermes' own `/hatch` pipeline, which already emits a compatible atlas.

## Atlas format (petdex / Codex compatible)

- One PNG atlas, **8 columns × 9 rows**, each cell **192 × 208 px** (matches the
  format Hermes' `/hatch` produces, so a generated sheet drops straight in).
- One **row per animation state**; columns are frames (left→right, loop).
- Transparent background. Trim-safe margin inside each cell.

## States (one row each)

| Row | State | Meaning | Facing |
| --- | --- | --- | --- |
| 0 | `idle` | standing, breathing, blink | front |
| 1 | `thinking` | finger to chin, "…" | 3/4 |
| 2 | `working` | sleeves up, doing a task | 3/4 |
| 3 | `speaking` | mouth moving for the bubble | front |
| 4 | `listening` | leaning in, attentive | front |
| 5 | `done` | small wave / thumbs up | front |
| 6 | `celebrate` | jump, arms up | front |
| 7 | `failed` | flinch / sweatdrop | front |
| 8 | `blocked` | waiting, tapping foot | front |

## Facing & extra sheets

The character must be able to **turn her back to you to watch the active
window**. Ship a second atlas `hermeschan-back.png` with at least `idle`,
`peek`, `working`, `point` rows drawn from behind. Side-facing (`left`/`right`)
is used while walking between windows.

## Emotions (expression overlay)

`hermeschan_emote` sets one of:
`neutral, happy, shy, surprised, sulk, sleepy, proud, panic, love`.

Implement emotions either as:
- separate face-only overlay frames composited on the body, **or**
- dedicated columns appended to the `idle`/`speaking` rows.

## Hand anchors (for carrying / pointing / grabbing)

For `point`, `grab_window`, and `tidy_desktop`, the body needs a **hand anchor
point** per frame so a carried icon or a window handle sticks to her hand.
Provide `hermeschan-anchors.json`:

```json
{ "working": [{"frame":0,"hand":[150,120]}, ...],
  "point":   [{"frame":0,"hand":[172,96]},  ...] }
```

## Drop-in checklist

1. Export `hermeschan.png` (front atlas), `hermeschan-back.png`, optional
   `hermeschan-anchors.json` into this folder.
2. Keep the 192×208 cell size and row order above.
3. The overlay reads row index by state name from `states.py`.
