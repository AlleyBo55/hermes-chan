# overlay — Hermechan's body (Rust / Tauri)

The visible character. A transparent, always-on-top, click-through-except-sprite
window that renders Hermechan and connects to the plugin over the duplex socket.

## Why Tauri

Tauri 2 gives the three window properties we need for free — transparency,
always-on-top, and per-pixel click-through — which `winit` alone does not on
macOS. The renderer is a small canvas; all world/physics logic is Rust.

## Modules (to build)

- `bridge`   — `AF_UNIX` client to `HERMESCHAN_SOCKET`; newline JSON both ways
  (see `../ARCHITECTURE.md` for the message table).
- `world`    — desktop snapshot: windows (macOS Accessibility + CGWindowList),
  desktop icons (Finder / NSWorkspace), monitors with scale factors. Refresh
  before each action and ~10 Hz during one. Convert Cocoa / AX / CG coordinate
  systems at this boundary only.
- `body`     — sprite state machine (rows from `sprites/SPRITE_SPEC.md`), 2D
  kinematics (gravity, surfaces = screen bottom + window top edges), facing
  (front / back / left / right), hand anchors.
- `render`   — canvas draw loop; faces the active window (back to the user)
  while peeking.
- `bubble`   — speech bubble + a text input; typed text → `{"type":"chat"}`.

## macOS permissions (request at first run; never hang)

- **Accessibility** — read/move windows. Stand holding a "grant me access" sign
  until granted; do not block the agent.
- **Screen Recording** — only for the `bend`/`glitch` window-snapshot effect.
- **Automation (Finder)** — only for `tidy_desktop` real icon moves.

## Build & attestation (catalog rule 17)

Do **not** commit a compiled binary. The release binary is built by this repo's
public GitHub Actions and carries an `actions/attest-build-provenance`
attestation the catalog verifies against the pinned commit. `cargo tauri dev`
for local iteration.

## Status

Not started — this is the next build step after the plugin scaffold. Start with
the Tauri shell + `bridge` + rendering one sprite whose state is driven by the
live plugin hooks.
