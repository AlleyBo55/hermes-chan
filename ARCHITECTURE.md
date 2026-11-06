# Architecture

Two processes, one duplex socket.

```
┌────────────────────────── Hermes agent process ──────────────────────────┐
│  plugin: hermes-chan  (Python, register(ctx))                             │
│                                                                           │
│   register_computer_use_provider ─► HermechanProvider ─► HermechanBackend │
│       (selected by computer_use.backend: hermes-chan)        │            │
│   register_hook  × pre/post_llm, pre/post_tool, blocked, session          │
│   register_tool  × say / emote / point / grab / glitch / tidy             │
│   register_command  /hermeschan                                           │
│   ctx.llm.complete_structured  ◄── autonomy (heartbeat idle brain)        │
│   ctx.inject_message          ◄── chat bubble + autonomous "do it" turns  │
│                                                                           │
│                         Bridge (AF_UNIX server) ──────────┐               │
└───────────────────────────────────────────────────────────┼─────────────┘
                                                             │ newline JSON
                                                             │ (duplex)
┌────────────────────────── Rust / Tauri overlay ───────────┼─────────────┐
│  transparent, always-on-top, click-through-except-sprite   ▼            │
│   bridge client  ─ state / say / action / point / config in             │
│                  ─ chat / intent out                                    │
│   world    : windows (AX + CGWindowList), desktop icons, monitors       │
│   body     : sprite state machine + 2D kinematics + hand anchors        │
│   render   : canvas; faces the active window (back to user) while peeking │
│   input    : speech-bubble text box -> {"type":"chat"} -> inject_message │
└─────────────────────────────────────────────────────────────────────────┘
```

## Why a computer-use backend (not a parallel controller)

Hermes' `computer_use` tool talks to exactly one backend through the
`ComputerUseBackend` ABC. By registering ours, **the agent's own decisions flow
through Hermechan's body**: `click/drag/type/scroll/key/focus_app` each (1)
animate the character approaching and performing the gesture, then (2) delegate
the real OS action to the built-in **cua-driver** backend. We get the robust,
cross-OS, background-input driver for free, the character visibly owns every
action, and approval gating / screenshot dedup / vision routing stay in the tool
*above* us. If cua-driver is absent, actions return `unsupported_action` and the
body still plays the gesture (demo-friendly).

## The socket protocol

Newline-delimited JSON over `AF_UNIX`. Plugin is the server.

| Dir | Message |
| --- | --- |
| → overlay | `{"type":"state","state":"thinking"}` |
| → overlay | `{"type":"say","text":"...","emotion":"happy"}` |
| → overlay | `{"type":"action","verb":"click","target":{...}}` |
| → overlay | `{"type":"point","target":{"app":"Kiro"}}` |
| → overlay | `{"type":"config","mischief":true}` |
| ← plugin  | `{"type":"chat","text":"chan, what's this file?"}` |
| ← plugin  | `{"type":"intent","name":"come_here"}` |

## MVP vs stretch (hackathon scope)

**MVP (ships for the demo)**
- Plugin: all four surfaces wired (done, compiles against hermes-agent).
- Overlay: transparent always-on-top sprite; states from hooks; speech bubble
  with text input → inject_message; faces active window.
- Backend: delegates to cua-driver for real click/type; body animates.
- One flagship demo chore end-to-end (type into the active app, or switch a
  video) performed visibly by the character.

**Stretch (if time)**
- Real window physics: AX move/drag of the frontmost window (grab/shake/throw).
- `bend`/`glitch`: warped-mesh overlay of a window snapshot (needs Screen
  Recording permission).
- `tidy_desktop` with real icon moves + undo log.
- Windows/Linux body (Win32 / X11). Wayland: out of scope.

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
