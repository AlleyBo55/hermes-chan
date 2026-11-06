# hermes-chan — a living desktop companion for Hermes

Hermechan is an **original chibi-maid** character who lives on your screen. She
stands on your windows, faces whatever you are doing, reacts to the agent's
activity, performs the agent's real computer-use actions with her own body, and
talks back through a speech bubble you can type into. Between turns she comes
alive on her own, driven by the session heartbeat.

> Status: **scaffold**. The Hermes-side plugin (tools, hooks, computer-use
> driver, chat bridge, autonomy) is in place and byte-compiles against
> `hermes-agent`. The Rust/Tauri overlay body is specced under `overlay/` and is
> the next build step. Sprite art is an original placeholder; see
> `sprites/SPRITE_SPEC.md`.

## What makes her more than a pet

Hermes already ships a cosmetic *pet* (a sprite that reacts to agent state).
Hermechan is the opposite of cosmetic:

- **She is the body behind `computer_use`.** Select
  `computer_use.backend: hermes-chan` and every click / drag / type the agent
  performs is animated by her — she walks to the window, reaches out, and the
  real OS action fires through the robust cua-driver underneath. Approval gating
  stays in Hermes, above her.
- **She reacts live.** Lifecycle hooks map thinking / tool-running / blocked /
  done / celebrate to animation states, and she turns to *face the active
  window* (her back to you) while watching you work.
- **You can talk to her.** Type into her speech bubble and it becomes a normal
  user turn via `inject_message`; her reply shows in the bubble (and optionally
  speaks through Hermes TTS).
- **She has a life of her own.** A heartbeat tick asks the model for one tiny
  in-character reaction through `ctx.llm` — cheap, no full turn — so she idles,
  peeks, or gets playfully mischievous on her own.

## Install (development)

```bash
# from the Hermes desktop app: Cmd/Ctrl+K -> Capabilities -> Plugins -> Install from Git
# or by hand into your profile:
cp -r hermes-chan ~/.hermes/plugins/hermes-chan
```

Make her the acting body (optional but recommended):

```yaml
# ~/.hermes/config.yaml
computer_use:
  backend: hermes-chan
```

Then run the overlay (once built):

```bash
cd overlay && cargo tauri dev   # connects to ~/.hermes/hermes-chan.sock
```

## Config

| Env | Default | Meaning |
| --- | --- | --- |
| `HERMESCHAN_SOCKET` | `~/.hermes/hermes-chan.sock` | duplex socket to the overlay |
| `HERMESCHAN_TTS` | `false` | speak replies through Hermes TTS |

## Slash command

```
/hermeschan status | come | sleep | say <text> | mischief on|off
```

## Safety / disclosure

- Runs a local `AF_UNIX` socket server and spawns/‑talks to the overlay process.
- As the `computer_use` backend she drives the screen — **only** through the
  built-in cua-driver, and **only** under Hermes' approval system. She adds no
  bypass.
- `glitch_window` is a harmless overlay effect drawn on top of a window; it
  never touches the target app's memory or files.
- `tidy_desktop` plans and animates; real icon/file moves run through approved
  file tools with an undo log.
- No network calls of her own; no telemetry; credentials are never read.

## License

MIT. Hermechan's design and art are original. She is **not** the Nous Research
logo mascot and **not** any existing anime character; see the sprite spec.
