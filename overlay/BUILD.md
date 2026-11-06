# Building Hermes-chan's body

The overlay is built **from source on your machine** — no binary ships in the
repo. That keeps every file reviewable source for the catalog and avoids
shipping anything compiled. The tradeoff: you need a Rust toolchain installed
(disclosed here and in the plugin README).

## Requirements

- **Rust** 1.90+ — https://rustup.rs (`cargo` on your `PATH`)
- **macOS**: Xcode Command Line Tools (`xcode-select --install`) for the system WebKit
- **Windows**: Visual Studio C++ Build Tools; the WebView2 runtime (already on Windows 11)
- **Linux**: the WebKitGTK 4.1 build packages listed in the [README](../README.md#install-by-hand)

## Quick build (bare binary)

```bash
cd overlay/src-tauri
cargo build --release --locked
# -> target/release/hermes-chan-overlay   (hermes-chan-overlay.exe on Windows)
```

The plugin's `overlay_runner.py` finds this automatically and launches it, or
builds it for you on first run when `cargo` is present (`HERMESCHAN_BUILD=0`
to opt out). Its output goes to `<Hermes home>/hermes-chan/build.log`.

## Best-fidelity build on macOS (.app bundle)

Transparency and the macOS private-API window behave best inside a proper app
bundle with an `Info.plist`. Build the bundle with the Tauri CLI:

```bash
cd overlay
npm install -D @tauri-apps/cli   # one-time
npx tauri build                  # -> src-tauri/target/release/bundle/macos/hermes-chan.app
# or, for live iteration:
npx tauri dev
```

The runner prefers the `.app` bundle over the bare binary when both exist.

## How the plugin finds it

`overlay_runner.py` checks, in order, and only uses a build newer than every source file:

1. `HERMESCHAN_OVERLAY_BIN` (explicit path override)
2. macOS: `src-tauri/target/release/bundle/macos/hermes-chan.app/Contents/MacOS/hermes-chan`
3. `src-tauri/target/release/hermes-chan-overlay[.exe]`
4. `src-tauri/target/debug/hermes-chan-overlay[.exe]`
5. the same two under `$CARGO_TARGET_DIR`, if set

## One module per OS

| | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Window list and active window (`src/winctl/`) | CGWindowList + System Events | EnumWindows + DWM | EWMH over X11 (x11rb) |
| Window pranks | System Events | SetWindowPos | `_NET_MOVERESIZE_WINDOW` |
| Native input (`src/input/`) | CoreGraphics + System Events | SendInput | XTest |
| Snapshots | `screencapture` | GDI / PrintWindow | GetImage / Composite |
| Idle time | CGEventSource | GetLastInputInfo | MIT-SCREEN-SAVER |
| Click-through | toggled under the cursor | toggled under the cursor | GTK input shape |
| Bridge (`src/bridge.rs`) | AF_UNIX socket | 127.0.0.1 + token | AF_UNIX socket |

On Wayland sessions the body runs through XWayland (`GDK_BACKEND=x11`), because Wayland does not
let an app place its own window, list other windows or send input.

## Permissions on first run

- **macOS**: Accessibility (move/track windows, native input), Screen Recording (snapshots),
  Automation for System Events / Finder (window titles, real `tidy_desktop` moves)
- **Windows** and **Linux**: none

The body must prompt for these without hanging; under CI or a kanban worker it does not
spawn at all.
