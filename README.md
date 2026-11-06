# Hermes-chan: the desktop companion who actually lives on your desktop

**Your AI agent finally has a body.** Hermes-chan is a chibi, Nous Girl-inspired
character who walks your screen, climbs your windows, watches YouTube with you,
pranks your apps, and does the agent's real clicks and typing with her own hands.
She is a plugin for [Hermes Agent](https://hermes-agent.nousresearch.com/) with a
small native macOS body written in Rust (Tauri).

<p align="center">
  <img src="sprites/hermeschan-v1/preview.png" width="420" alt="Hermes-chan sprite sheet preview">
</p>

<p align="center">
  <img src="sprites/hermeschan-v1/walk-preview.gif" alt="Walk cycle">
  <img src="sprites/hermeschan-v1/climb-preview.gif" alt="Climb cycle">
  <img src="sprites/hermeschan-v1/hop-preview.gif" alt="Hopping up the screen one step at a time">
</p>

> **Status: working, macOS only, built from source.** The plugin, the Rust/Tauri
> overlay and a 52-clip sprite pack (289 frames) are wired up and running. Catalog
> validation passes against Hermes 0.23.

## What she can do

### She lives on your screen like a shimeji
- **Walks** the bottom of your screen with an 8-frame walk cycle whose speed is matched to
  her stride (60 px/s), so her feet do not slide.
- **Hops up the scene one step at a time.** Each hop is about 5% of the screen height. She
  lands, stands on that height as if it were a step, walks along it, then hops up again
  or back down. Never above the top of the screen, never below the floor.
- **Climbs** the left and right edges of the screen and the sides of the window you are
  using, to a random height. She hangs on, climbs higher, or lets go and falls.
- **Stands on windows and rides them.** Every visible window's top edge is a ledge. Move
  the window and she moves with it; close it and she falls.
- **Falls with gravity.** Drag her (she dangles in her own "dragged" pose) and let go: she
  plays her fall animation all the way down and lands with a little crouch.
- **Goes anywhere, even over the menu bar.** Her window sits at the menu bar's level, so
  she can climb to the very top, and she follows you onto other Spaces and into
  full-screen apps.

<p align="center">
  <img src="sprites/hermeschan-v1/dragged-preview.png" width="360" alt="Dragged pose">
</p>

### She notices what you are doing
- **Watches videos with you.** Focus a YouTube, Netflix, Twitch, Prime Video, Disney+,
  Vimeo, Bilibili or Crunchyroll tab, or IINA, VLC, QuickTime, Infuse, mpv or the TV app,
  and she walks over, sits under the video with her back to you, and watches. Now and
  then she turns round to react ("popcorn time!", "did you see that?"), then goes back
  to watching. Switch away and she leaves.
- **Works along with you.** In a code editor or terminal (VS Code, Cursor, Xcode, Zed,
  JetBrains, Terminal, iTerm, Warp, Ghostty, kitty, WezTerm, Alacritty) she stands by
  the window in her "working" pose.
- **Visits the window you switch to.** Most of the time when you change windows she heads
  over to peek at it, point at it or climb its side.
- **Mumbles to herself.** 510 idle lines and 219 video reactions, shown in her speech
  bubble every minute or two, each list shuffled so nothing repeats until all have
  played. Local and free: no model call.

### She plays pranks on your apps
Every prank has its own casting animation, and its effect fires on that animation's
impact frame, starting at her hand. All effects are drawn on a separate click-through
layer above the app, so the app itself is never changed and looks normal again a moment
later.

| Prank | What you see |
| --- | --- |
| Crack | the screen cracks from her fist |
| Portal | a swirling portal opens in front of her |
| Break font | the window's text shatters into flying glyphs |
| Collapse | a snapshot of the window cracks and falls apart into a dark void, then the real app is back |
| Tear | the window rips open |
| Ripple, pixel dissolve, lightning | ripples, pixels and bolts burst from her hand |
| Glitch, bend | scanlines, colour split and blocks, or the window's border bends |

<p align="center">
  <img src="sprites/hermeschan-v1/effect-actions-preview.png" width="720" alt="The casting clip for each effect">
</p>

Try every effect in [the local effects preview](overlay/src/effects-preview.html), and
every sprite clip in [the sprite review page](sprites/hermeschan-v1/index.html).

**She also moves real windows, carefully.** On her own she can shake the window you are
using, nudge it sideways, fling it, or pull it down, timed to her grab frame. She only
does this after 20 seconds with no keyboard or mouse input, only on the active window,
and she puts it back 2.5 seconds later unless you moved it yourself. While you are
working she bends the window instead.

### She is the agent's body
- **Real computer use, performed by her.** Set `computer_use.backend: hermes-chan` and
  every click, drag and keystroke the agent makes is animated by her: she walks to the
  window, reaches out, and her Rust body sends the native macOS input on the sprite's
  contact frame. Approval stays in Hermes, above her.
- **Lives the agent's state.** Thinking, working, blocked, done and celebrating each have
  their own animation.
- **Talks with you.** Ctrl+click her (or right-click) to open the chat box above her
  head. What you type becomes a normal user turn in Hermes, and the reply shows in her
  bubble. Enter sends, Esc closes.
- **Has a life between turns.** An optional heartbeat asks the model for one tiny
  in-character reaction (cheap, not a full turn), so she can comment on what you are
  doing.

## Evidence

Measured on a 1512x982 MacBook screen while building these features:

| What | Measured |
| --- | --- |
| Window motion while walking | a position update every 16.7 ms (60 fps), 1-2 px per step |
| Walking speed | about 63 px/s, matching a 22 px step twice per 720 ms cycle |
| Hops | 49 px steps: floor (733) to 684 to 635, walked along it, back down to the floor |
| Climb | from the floor to y = -133, above the menu bar, then a fall and a clean landing |
| Video watching | walked to the video's centre in 4 s, watched for 31 s, left 1 s after the tab changed |
| Window ledges | reads every on-screen window (owner, layer, bounds) once a second, no Screen Recording needed |
| Sprite pack | 52 clips, 289 frames; standing clips share one feet line (feet at row 197 of 208) |
| Lines | 510 idle mumbles and 219 video reactions, all unique, at most 49 characters |

## Install

You need macOS, [Hermes Agent](https://hermes-agent.nousresearch.com/), the
[Rust toolchain](https://rustup.rs) and the Xcode Command Line Tools
(`xcode-select --install`).

```bash
git clone <this repo> hermes-chan && cd hermes-chan
./run.sh          # links the plugin into ~/.hermes/plugins and builds her body
```

The first build downloads and compiles the Rust crates, which takes a few minutes; later
builds take seconds. Then start Hermes and wake her:

```
/hermeschan wake
```

Optional, to make her the agent's acting body, add to `~/.hermes/config.yaml`:

```yaml
computer_use:
  backend: hermes-chan
```

### macOS permissions

| Permission | What it is for | Without it |
| --- | --- | --- |
| Accessibility | reading and moving the active window | she still roams, but can't perch, follow or move windows |
| Screen Recording | the "collapse" snapshot and the heartbeat's optional glance | she does a different prank instead |
| Automation (System Events, Finder) | the active window's title; real `tidy_desktop` moves | no app-aware reactions; desktop tidying stays a plan |

If a permission is missing she fails cleanly and keeps animating.

### Run the body on its own

```bash
cd overlay/src-tauri && cargo build && ./target/debug/hermes-chan-overlay
```

To quit her: `pkill -f hermes-chan-overlay` (or Ctrl+C in that terminal). She has no
menu bar icon yet.

## Commands

```
/hermeschan status | come | sleep | wake | hide | say <text>
/hermeschan effect <name>   # crack, portal, text_shatter, ripple, pixel_dissolve, lightning, tear, glitch, bend
/hermeschan mischief on|off # her own pranks, effects and window moves
/hermeschan watch on|off | react
/hermeschan heartbeat on|off|status [interval]
```

The 📌 button above her switches between **follow** (she perches on the active window
and watches it) and **roam** (everything in "What she can do").

## Config

| Env | Default | Meaning |
| --- | --- | --- |
| `HERMESCHAN_SOCKET` | `~/.hermes/hermes-chan.sock` | duplex socket between the plugin and her body |
| `HERMESCHAN_AUTOSTART` | `true` | start her body with the plugin |
| `HERMESCHAN_BUILD` | unset | build her body on first use if no binary is found |

## Safety

- Her pranks are drawn on a click-through overlay. They never change an app's memory,
  content or files.
- She moves a real window only when you have been idle for 20 seconds, only the active
  window, and puts it back. `/hermeschan mischief off` turns off all her pranks.
- As the `computer_use` backend she acts only through Hermes' approval system and adds
  no bypass.
- `tidy_desktop` is plan-first: running it needs a short-lived plan token plus
  `confirm=true`, journals each move first, and can be undone.
- She makes no network calls of her own, sends no telemetry and never reads credentials.
  Window snapshots stay in memory on your Mac.

## License

Code is MIT. The sprite artwork draws on the supplied Nous Girl portrait; the code
license is not a claim of independent rights to the Nous Research character or the
source illustration. See [the sprite spec](sprites/SPRITE_SPEC.md).
