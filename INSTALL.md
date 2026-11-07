# Install & run Hermes-chan (end to end)

A walkthrough from nothing to a living companion on your screen, on macOS, Windows or Linux.
The [README](README.md#install-in-one-message) has a one-message prompt that makes Hermes do all
of this for you.

## 0. Prerequisites

- **Hermes Agent** 0.23 or newer (`hermes --version`), from https://hermes-agent.nousresearch.com/.
- **Rust** 1.90 or newer for her body, from https://rustup.rs (`cargo --version`; `rustup update`
  if older). Without it the plugin still runs headless: tools, hooks and chat work, there is just
  no sprite.
- Your system's build tools:

| System | Install |
| --- | --- |
| macOS | `xcode-select --install` |
| Windows | Visual Studio C++ Build Tools ("Desktop development with C++"); WebView2 is already on Windows 11 and updated Windows 10 |
| Debian / Ubuntu | `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev` |
| Fedora | `sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file libappindicator-gtk3-devel librsvg2-devel libxdo-devel` and `sudo dnf group install c-development` |
| Arch | `sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl appmenu-gtk-module libappindicator-gtk3 librsvg xdotool` |

## 1. Install the plugin

```bash
hermes plugins install https://github.com/AlleyBo55/hermes-chan --enable
```

`--enable` switches it on right away; Hermes plugins are off until enabled. Without it, run
`hermes plugins enable hermes-chan` later, or use the Plugins switch in Hermes Desktop.

Working from a clone instead? `./run.sh` (macOS, Linux) links the checkout into your Hermes
profile, enables it and builds her body.

## 2. Build her body

```bash
# macOS / Linux
cd ~/.hermes/plugins/hermes-chan/overlay/src-tauri
# Windows (PowerShell)
cd $env:LOCALAPPDATA\hermes\plugins\hermes-chan\overlay\src-tauri

cargo build --release --locked
```

If you skip this, she builds herself in the background the first time the plugin loads and
`cargo` is on the PATH; progress goes to `<Hermes home>/hermes-chan/build.log`.

## 3. Make her the acting body (optional, recommended)

```bash
hermes config set computer_use.backend hermes-chan
```

Every `computer_use` action the agent takes is then performed by Hermes-chan through her native
input driver (CoreGraphics on macOS, SendInput on Windows, XTest on Linux), with Hermes' approval
gating intact.

To talk to the agent through her chat bubble, also allow plugin chat (plugins run inside Hermes'
gateway, which serves the CLI, the TUI and the Desktop app) and run any `/hermeschan` command
once in the chat you want her to talk to:

```bash
hermes config set plugins.entries.hermes-chan.allow_gateway_injection true
```

## 4. Start Hermes and say hi

```bash
hermes            # or the TUI / Desktop app
```

She appears a couple of seconds after the session starts. In the session:

```
/hermeschan status          # body, bridge and driver state, and why she is missing if she is
/hermeschan say hello       # speech bubble
/hermeschan mischief on     # allow her own pranks
```

Ctrl+click her to open the chat box, Ctrl+Shift+click her to send her away, `/hermeschan on` to
bring her back. To keep her off, switch the plugin off in Hermes.

## 5. Grant permissions (macOS only)

- **Accessibility** (perch, follow, window pranks, native input): System Settings ▸ Privacy &
  Security ▸ Accessibility.
- **Screen Recording** (the collapse snapshot and the heartbeat's optional glance).
- **Automation ▸ System Events / Finder** (window titles; real `tidy_desktop` moves).

macOS grants these to whatever launched the Hermes gateway, because she runs inside it. For the
background gateway Hermes starts by itself that is **`osascript`**: Hermes launches its gateway
through `/usr/bin/osascript`, so the entry to turn on is named `osascript`, not your terminal.
That grant also covers the rest of the gateway (and anything else launched the same way), so only
turn it on if you want her window features. If you run `hermes gateway run` in a terminal, grant
that terminal instead.

`osascript` does not show in these lists until someone adds it, and the **+** button may refuse a
command-line tool. Drag it in instead. Her bubble does the first part for you: when a prank needs a
switch, click the bubble and she opens the page and selects the right program in Finder. By hand:

```bash
open "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"   # or ?Privacy_Accessibility
open -R /usr/bin/osascript
```

Then drag `osascript` from the Finder window into the list and switch it on. A new grant works on
her next try; Hermes does not need a restart.

Windows and Linux ask for nothing. On Windows she cannot touch apps running as administrator; on
Linux she needs an X11 display, and on Wayland she only sees X11/XWayland apps.

If a permission is missing she fails cleanly and keeps animating; she never hangs the agent.

## 6. Make her come alive on her own

```
/hermeschan heartbeat on 2m
```

Each tick is one cheap structured `ctx.llm` call, not a full agent turn, capped at 10 an hour.
It needs `mischief` or `watch` on. See `hermeschan/autonomy.py`.

## 7. Troubleshooting

| Symptom | Fix |
| --- | --- |
| No sprite appears | `/hermeschan status`; then `build.log` / `overlay.log` in `<Hermes home>/hermes-chan/` |
| Status says she is away | `/hermeschan on`, or switch the plugin off and on |
| She appears but doesn't move windows (macOS) | grant Accessibility (step 5) |
| Linux: black box around her | run a compositor; on some GPU drivers set `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| Linux: "no graphical display" | the gateway runs without your session's display (a systemd service): `systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XAUTHORITY`, then `hermes gateway restart` |
| Runs on a server, CI or kanban worker and spawns nothing | by design: unattended runs don't start a GUI |

## 8. Uninstall

```bash
hermes plugins remove hermes-chan
```

Her settings and logs stay in `<Hermes home>/hermes-chan/` until you delete that folder.
