# overlay — Hermes-chan's body (Rust / Tauri)

The visible character. A transparent, always-on-top window that only takes clicks on her
sprite, bubble and buttons, renders Hermes-chan, and connects to the plugin over the duplex
bridge. Runs on macOS, Windows and Linux (X11, or XWayland on Wayland).

## Why Tauri

Tauri 2 gives the three window properties we need on every desktop — transparency,
always-on-top, and click-through — which `winit` alone does not. The renderer is a small
canvas in the system webview (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux).

## Modules

- `src-tauri/src/bridge.rs` — client of the plugin's bridge: an AF_UNIX socket on macOS and
  Linux, `127.0.0.1` plus a per-run token on Windows; newline JSON both ways (see
  `../ARCHITECTURE.md` for the message table).
- `src-tauri/src/winctl/` — her view of the desktop, one file per OS: the active window, the
  on-screen window list (her ledges), window pranks, idle time, snapshots.
- `src-tauri/src/input/` — native input for the `computer_use` backend, one file per OS.
- `src-tauri/src/overlay.rs` — window setup, follow/perch, hit testing, effects, and the
  Ctrl+Shift+click `deactivate` command.
- `src-tauri/src/watchdog.rs` — she leaves when the Hermes process that started her exits.
- `src/` — the webview: `sprite-player.js`, `wander.js` (roaming brain), `main.js` (states,
  bubble, chat, gestures), `effect*.js` (prank effects).

## Gestures

| Gesture | Does |
| --- | --- |
| drag | pick her up; let go and she falls |
| Ctrl+click / right-click | open or close the chat box |
| Ctrl+Shift+click | send her away: her process ends until `/hermeschan on`, a plugin off/on or a Hermes restart |
| 📌 button | follow the active window, or roam |

## Build

See [BUILD.md](BUILD.md). Do **not** commit a compiled binary: the body is built from this
source on the user's machine (catalog rule 17).
