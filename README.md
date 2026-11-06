# hermes-chan — a living desktop companion for Hermes

Hermes-chan is a **colored chibi Nous Girl-inspired** character who lives on your screen. She
stands on your windows, faces whatever you are doing, reacts to the agent's
activity, performs the agent's real computer-use actions with her own body, and
talks back through a speech bubble you can type into. Between turns she comes
alive on her own, driven by the session heartbeat.

> Status: **working implementation, runtime verification in progress**. The plugin,
> Rust/Tauri overlay, manifest player, and reviewed 250-frame/44-clip sprite pack
> are wired. Catalog validation passes against Hermes 0.23. A live session still
> needs macOS Accessibility and Screen Recording permissions.

## What makes her more than a pet

Hermes already ships a cosmetic *pet* (a sprite that reacts to agent state).
Hermes-chan is the opposite of cosmetic:

- **She is the body behind `computer_use`.** Select
  `computer_use.backend: hermes-chan` and every click / drag / type the agent
  performs is animated by her — she walks to the window, reaches out, and her
  Rust body sends the native macOS input at the sprite's contact frame. Approval
  gating stays in Hermes, above her.
- **She reacts live.** Lifecycle hooks map thinking / tool-running / blocked /
  done / celebrate to animation states, and she turns to *face the active
  window* (her back to you) while watching you work.
- **You can talk to her.** Type into her speech bubble and it becomes a normal
  user turn via `inject_message`; her normal assistant reply shows in the bubble.
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
./run.sh                         # link plugin + build the Rust body
# then start Hermes and run /hermeschan wake
```

## Config

| Env | Default | Meaning |
| --- | --- | --- |
| `HERMESCHAN_SOCKET` | `~/.hermes/hermes-chan.sock` | duplex socket to the overlay |
| `HERMESCHAN_AUTOSTART` | `true` | build/start the visible body with the plugin |

## Slash command

```
/hermeschan status | come | sleep | wake | hide | say <text>
/hermeschan mischief on|off | watch on|off | react
/hermeschan heartbeat on|off|status [interval]
```

## Safety / disclosure

- Runs a local `AF_UNIX` socket server and spawns/‑talks to the overlay process.
- As the `computer_use` backend she drives the screen through her source-built
  Rust macOS input driver, and **only** under Hermes' approval system. She adds
  no bypass.
- `glitch_window` is a harmless overlay effect drawn on top of a window; it
  never touches the target app's memory or files.
- `tidy_desktop` is plan-first; execution needs the short-lived plan token plus
  explicit `confirm=true`, journals each move before it happens, and has undo.
- No network calls of her own; no telemetry; credentials are never read.

## License

Code is MIT. The user-directed sprite artwork draws on the supplied Nous Girl
portrait; the repository's code license is not a claim of independent rights
to the Nous Research character or source illustration. See the sprite spec.
