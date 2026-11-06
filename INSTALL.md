# Install & run Hermes-chan (end to end)

A walkthrough from nothing to a living companion on your screen. macOS.

## 0. Prerequisites

- **Hermes Agent** installed (`hermes --version`). Get it from
  https://hermes-agent.nousresearch.com/ if missing.
- **Rust** toolchain for the visible body — https://rustup.rs (`cargo` on PATH).
  Without it the plugin still runs headless (tools, hooks, chat), just no sprite.
- **Xcode Command Line Tools** — `xcode-select --install`.
- Optional, for the best-looking body: Node + `@tauri-apps/cli`
  (`npm i -D @tauri-apps/cli` inside `overlay/`).

## 1. Install the plugin

The fast path (symlink for dev, builds the body):

```bash
cd /Users/gilang/ngoding/hermes-chan
./run.sh
```

Or with `just`:

```bash
just install      # link into ~/.hermes/plugins
just build        # build the overlay binary
```

## 2. Make her the acting body (optional but recommended)

Add to `~/.hermes/config.yaml`:

```yaml
computer_use:
  backend: hermes-chan
```

Now every `computer_use` action the agent takes is performed by Hermes-chan
through her Rust macOS input driver, with Hermes' approval gating intact.

## 3. Start Hermes and wake her

```bash
hermes            # or the desktop app / TUI
```

In the session:

```
/hermeschan wake            # she fades in and perches on the active window
/hermeschan say hello       # speech bubble
/hermeschan mischief on     # allow autonomous idle antics
```

Type directly into her bubble to talk to the agent — it becomes a normal user
turn.

## 4. Grant macOS permissions on first run

- **Accessibility** (required for perch/grab/track): System Settings ▸ Privacy &
  Security ▸ Accessibility ▸ enable the Hermes app / your terminal.
- **Screen Recording** (only for the `glitch`/`bend` snapshot effect).
- **Automation ▸ Finder** (only for real `tidy_desktop` icon moves).

If a permission is missing she fails cleanly and keeps animating; she never
hangs the agent.

## 5. Make her come alive on her own

Give the session a heartbeat so she reacts between turns without you prompting:

```
/heartbeat every 2m (idle) glance at what I'm doing and react in one short line
```

The tick uses a cheap `ctx.llm` call, not a full agent turn, so it is nearly
free. See `hermeschan/autonomy.py`.

## 6. Troubleshooting

| Symptom | Fix |
| --- | --- |
| No sprite appears | `cargo` missing or build failed — run `just build` and read errors; or `HERMESCHAN_AUTOSTART=0` to silence |
| She appears but doesn't move windows | grant Accessibility (step 4) |
| `/hermeschan status` says "not connected" | the overlay isn't running — `/hermeschan wake`, or check the socket path it prints |
| Runs on a server/cron and spawns nothing | by design — unattended contexts don't launch a GUI |

## 7. Uninstall

```bash
just uninstall        # unlink the plugin
just clean            # remove build artifacts
```
