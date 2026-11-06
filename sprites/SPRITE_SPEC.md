# Sprite spec

Hermes-chan's final art follows the user-supplied Nous Girl portrait: a cute
colored chibi with blunt glossy black bangs, outward-curled black hair, white
headphones, large blue eyes, a white collared shirt, black tie and straps,
black skirt, and little shoes. The older geometric chibi-maid SVG remains a
placeholder only. Sakurasou's Maid-chan is a behavior reference for the desktop
companion, not a costume reference. See `production-plan/README.md` for the
254-frame allocation and `hermeschan-v1/` for final exports.

## Atlas format (petdex / Codex compatible)

- Core PNG atlas: **8 columns × 9 rows**, each cell **192 × 208 px** (matches the
  format Hermes' `/hatch` produces). Additional back, movement, action and
  emotion atlases are packaged separately. The current Codex UI avatar size
  was not independently measured.
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

The current 254-frame collection uses full-body emotion variants. These play
as front-facing expressive idle clips. A future face rig would be needed for
arbitrary emotions during every action and facing.

## Hand anchors (for carrying / pointing / grabbing)

For `point`, `grab_window`, and `tidy_desktop`, the body needs a **hand anchor
point** per frame so a carried icon or a window handle sticks to her hand.
Provide `hermeschan-anchors.json` after measuring final pixels:

```json
{ "working": [{"frame":0,"hand":[150,120]}, ...],
  "point":   [{"frame":0,"hand":[172,96]},  ...] }
```

## Drop-in checklist

1. Export `hermeschan.png` (front atlas), `hermeschan-back.png`, other atlases,
   254 base PNGs plus 35 effect-action PNGs, and the runtime manifest into
   `hermeschan-v1/`.
2. Keep the 192×208 cell size and row order above.
3. The overlay reads row index by state name from `states.py`.

The seven effect clips in `hermeschan-effect-actions.png` have five frames
each. Frame 2 carries `effect_start`; that is when the native effect window
appears. Their character art is composed from the approved 254-frame set, with
hand-attached marks added by `build_effect_sprites.py`.
