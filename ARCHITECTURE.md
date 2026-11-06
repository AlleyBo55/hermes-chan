# Architecture

Two processes, one duplex bridge. Runs on macOS, Windows and Linux (X11/XWayland).

```
┌──────────────────────────── Hermes agent process ────────────────────────────┐
│  plugin: hermes-chan  (Python, register(ctx))                                │
│                                                                              │
│   register_computer_use_provider ─► HermechanProvider ─► HermechanBackend    │
│       (selected by computer_use.backend: hermes-chan)                        │
│   register_hook  × pre/post_llm, pre/post_tool, approval, session            │
│   register_tool  × say / emote / point / grab / glitch / effect / tidy       │
│   register_command  /hermeschan  (on | off | status | ...)                   │
│   ctx.llm.complete_structured  ◄── autonomy (heartbeat idle brain)           │
│   ctx.inject_message           ◄── chat bubble                               │
│   Body: on/off switch ─► OverlayRunner (builds, spawns, supervises)          │
│                                                                              │
│   Bridge server: AF_UNIX (macOS, Linux) | 127.0.0.1 + token (Windows) ─────┐ │
└────────────────────────────────────────────────────────────────────────────┼─┘
                                                                             │
                                                        newline JSON, duplex │
                                                                             │
┌──────────────────────────── Rust / Tauri overlay ──────────────────────────┼─┐
│  transparent, always-on-top, clicks only on her sprite/bubble/buttons      ▼ │
│   bridge client ─ state / say / action / point / config in                   │
│                 ─ chat / intent / action events out                          │
│   winctl   : active window, window list, window moves, idle, snapshots       │
│              (one module each for macOS, Windows, Linux)                     │
│   input    : native input for computer use (CoreGraphics/SendInput/XTest)    │
│   watchdog : leaves when the Hermes process that started her exits           │
│   webview  : sprite state machine, roaming brain, effects, chat bubble       │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Lifecycle: on, off, and leaving with Hermes

- `register(ctx)` builds a `Body` (`hermeschan/body.py`). When her persisted `active` setting is
  on, it starts the bridge and the `OverlayRunner` two seconds later, so short-lived Hermes
  commands that load plugins (`hermes plugins install --enable`) do not flash a body.
- One body per profile: only the process that owns the bridge starts her. A second Hermes process
  waits and takes over when the first lets go.
- **Ctrl+Shift+click** (or closing her window) makes the overlay send
  `{"type":"intent","name":"deactivate"}` and exit with code 0. The plugin persists `active:
  false` and stops supervising; exit code 0 is never restarted. `/hermeschan on` and `off` flip
  the same switch from a chat.
- Disabling the plugin in Hermes runs its `on_unload` callback, which stops the heartbeat, the
  overlay process and the bridge. Interpreter exit does the same through `atexit`, and the overlay
  also watches its parent PID (`HERMESCHAN_PARENT_PID`) so it leaves even after a crash.

## Why a computer-use backend (not a parallel controller)

Hermes' `computer_use` tool talks to exactly one backend through the
`ComputerUseBackend` ABC. By registering ours, **the agent's own decisions flow
through Hermes-chan's body**: `click/drag/type/scroll/key/focus_app` waits
for the matching sprite contact frame, then sends one native-input request to
the Rust overlay. The overlay uses CoreGraphics/System Events on macOS,
SendInput on Windows and XTest on Linux, and returns a structured result. Approval gating, screenshot dedup and vision routing stay
in Hermes' public `computer_use` tool *above* us; no private Hermes driver is
imported. If the body is absent, actions fail cleanly as `body_disconnected`.

## The bridge protocol

Newline-delimited JSON. The plugin is the server.

- **macOS, Linux:** an `AF_UNIX` socket, `0600`, at `HERMESCHAN_SOCKET` or
  `<Hermes home>/hermes-chan.sock`. A live socket owned by another process is never unlinked.
- **Windows:** Python has no `AF_UNIX` there, so the plugin listens on `127.0.0.1` (port 0,
  `SO_EXCLUSIVEADDRUSE`) and hands the overlay it starts `HERMESCHAN_TCP` plus a random
  `HERMESCHAN_TOKEN`. The same values sit in `<Hermes home>/hermes-chan/bridge.json` for an
  overlay started by hand. A client whose first line is not the matching `hello` is dropped.

| Dir | Message |
| --- | --- |
| ← plugin  | `{"type":"hello","token":"..."}` (Windows only, first line) |
| → overlay | `{"type":"state","state":"thinking"}` |
| → overlay | `{"type":"say","text":"...","emotion":"happy"}` |
| → overlay | `{"type":"action","verb":"click","target":{...}}` |
| → overlay | `{"type":"point","target":{"app":"Kiro"}}` |
| → overlay | `{"type":"config","mischief":true}` |
| ← plugin  | `{"type":"chat","text":"chan, what's this file?"}` |
| ← plugin  | `{"type":"intent","name":"deactivate"}` (Ctrl+Shift+click) |

## MVP vs stretch (hackathon scope)

**MVP (ships for the demo)**
- Plugin: all four surfaces wired (done, compiles against hermes-agent).
- Overlay: transparent always-on-top sprite; states from hooks; speech bubble
  with text input → inject_message; faces active window.
- Backend: public `ComputerUseBackend` + Rust native macOS input, synchronized
  to manifest contact events.
- One flagship demo chore end-to-end (type into the active app, or switch a
  video) performed visibly by the character.

**Stretch (if time)**
- Real window physics: AX move/drag of the frontmost window (grab/shake/throw).
- `bend`/`glitch`: warped-mesh overlay of a window snapshot (needs Screen
  Recording permission).
- `tidy_desktop` with real icon moves + undo log.
- Windows/Linux body (Win32 / X11): done. Native Wayland stays out of scope; she runs
  through XWayland there.

## 5-day plan

1. Overlay shell (Tauri transparent/always-on-top/click-through) + socket client; render a sprite and switch states from live hooks.
2. Speech bubble input → inject_message round-trip; TTS out. Faces active window via AX frontmost-window frame.
3. Backend delegate verified: agent `computer_use` → body animates → real action. Flagship chore scripted.
4. Attested CI build (mac arm64) per catalog rule 17; `hermes plugins validate --install-deps` green; polish sprites.
5. Record < 90 s demo; open catalog PR with a full 40-char SHA pin + disclosure; submit form.

## Catalog admission checklist (rules that bite this plugin)

- [ ] Rule 5 — `capabilities` block matches what `register(ctx)` actually registers.
- [ ] Rule 6 — `hermes plugins validate` security scan: no `dangerous` findings.
- [ ] Rule 12 — credentials only via Hermes; no `.env` reads (we read none).
- [ ] Rule 11 — approvals respected: body never bypasses `computer_use` gating.
- [ ] Rule 13 — disclose: local socket server, spawns overlay, drives screen.
- [ ] Rule 14 — `requires_hermes` floor set truthfully in plugin.yaml + entry.
- [ ] Rule 17 — overlay binary built by our public GitHub Actions with
      `actions/attest-build-provenance`; nothing compiled committed to the repo.
