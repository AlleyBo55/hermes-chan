# Hermes-chan sprite production

Target: **254 transparent 192 × 208 PNGs across 45 animations**. The four
additional frames cover the user-requested Shimeji drag gesture.

## Appearance

The final user-approved design is `output/imagegen/hermeschan-chibi/master-front.png`:
a very cute two-head-tall chibi with a huge expressive head, straight glossy
black bangs, outward-curled black hair, broad white over-ear headphones, white
collared shirt, black tie and straps, black skirt and shoes. The final version
**recolors that exact design**: warm skin, blue eyes and restrained blue accents
while keeping the shirt white. The user's screenshot of the accepted character
is the visual acceptance reference. The actual Nous Research portrait remains
an identity reference for hair and headphones. Maid-chan from Sakurasou is a
reference for desktop-companion behavior only.

Earlier maid, adult-proportion, hoodie, turtleneck and sailor-blouse explorations
are rejected drafts. They are not part of the delivered collection.

## Exact allocation

| Group | Clips | PNG frames |
| --- | ---: | ---: |
| Core: idle, thinking, working, speaking, listening, done, celebrate, failed, blocked | 9 | 72 |
| Back: idle, peek, working, point | 4 | 32 |
| Movement: walk left/right, climb left/right, fall, land, dragged | 7 | 52 |
| Actions: click, drag, type, key, scroll, point, window drag/shake/throw/bend, glitch, tidy, mischief, sleep, wake, sit | 16 | 62 |
| Emotions: neutral, happy, shy, surprised, sulk, sleepy, proud, panic, love | 9 | 36 |
| **Total** | **45** | **254** |

All 11 states from `hermeschan/states.py` and all nine emotions from
`hermeschan/tools.py` are represented. Back-facing watch reactions and
screen-facing action poses keep her attention on the active window during video
watching, typing, clicking, scrolling, window handling and desktop cleanup.
The sprites are app-independent, so Kiro, a browser, or another installed app
can use the same gestures. Emotion clips are full-body front-facing
variants, not a face rig that can change expressions on every physical action.

## Walk and climb revision

The four directional walk/climb loops use two cohesive eight-pose sheets instead
of independently generated poses. `sprites/extract_motion_sheets.py` cuts each
left-facing sheet into source frames and reflects it for right-facing motion,
keeping the head, outfit and scale consistent. Walking plays at 90 ms per frame;
climbing plays at 110 ms. Climb frames register by head position so the body can
compress and extend without a head-size jump. The overlay also keeps an active
movement clip running when repeated active-window events arrive, and lets the
roaming controller retain ownership of its own walk and climb cycles.

The source sheets are `output/imagegen/hermeschan-chibi/walk-sheet-v2.png` and
`climb-sheet-v2.png`; the exact prompts are in `motion-prompts.md`. The earlier
individual movement sources are preserved under `output/imagegen/hermeschan-chibi/motion-originals/`.

## Dimensions and delivery

- Source-cell size is **192 × 208**, as specified in `sprites/SPRITE_SPEC.md`.
  The existing spec calls this petdex / Codex compatible; exact dimensions in
  the current Codex UI were not independently measured.
- Compact display uses **96 × 104 CSS px**, with native-resolution PNGs retained.
- Shared ground/pivot: **[96, 196]**. Registration preserves a uniform character
  scale, grounded feet and intentional airborne offsets. `movement/dragged`
  hovers while held, then plays `movement/land` when released.
- Individual PNGs are canonical. Five atlases, a playback manifest, contact
  sheets and the interactive preview are additional packaging.
- The atlases and runtime manifest live only in `overlay/src/hermeschan/`, which
  her body embeds; the review page loads them from there with the same canvas player.
- Hands point to or hold empty space. Windows, icons, speech bubbles and UI
  effects remain separate runtime objects.

## Production files

`manifest.planned.json` contains frame names, phases, timings and message
mappings. `prompts.jsonl` contains the exact per-frame appearance and action
prompts. `frame-list.md` is the readable checklist. Source images are retained
under `output/imagegen/hermeschan-chibi/`. Final exports go to
`sprites/hermeschan-v1/`; this final collection excludes rejected drafts.

`sprites/pack_assets.py` is not an API runner. Run it with Python and Pillow once all 254 source
images exist. It registers the art, creates atlases and reports file count,
alpha, pixel uniqueness and clipping. Every contact sheet and animation still
requires visual inspection before declaring the pack reviewed.

Generation uses the bundled imagegen CLI, with one distinct image edit per
frame. API authentication is working. Credentials are read from the user's
local configuration and are never stored in this project or shown in output.
