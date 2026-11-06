# Building Hermechan's body

The overlay is built **from source on your machine** — no binary ships in the
repo. That keeps every file reviewable source for the catalog and avoids
shipping anything compiled. The tradeoff: you need a Rust toolchain installed
(disclosed here and in the plugin README).

## Requirements

- **Rust** — https://rustup.rs (`cargo` on your `PATH`)
- macOS (Windows/Linux bodies are a later step)
- Xcode Command Line Tools (`xcode-select --install`) for the system WebKit

## Quick build (bare binary)

```bash
cd overlay/src-tauri
cargo build --release
# -> target/release/hermes-chan-overlay
```

The plugin's `overlay_runner.py` finds this automatically and launches it, or
builds it for you on first run when `cargo` is present
(`HERMESCHAN_BUILD=0` to opt out).

## Best-fidelity build (.app bundle)

Transparency and the macOS private-API window rely on a proper app bundle with
an `Info.plist`. For the demo, build the bundle with the Tauri CLI:

```bash
cd overlay
npm install -D @tauri-apps/cli   # one-time
npx tauri build                  # -> src-tauri/target/release/bundle/macos/hermes-chan.app
# or, for live iteration:
npx tauri dev
```

The runner prefers the `.app` bundle over the bare binary when both exist.

## How the plugin finds it

`overlay_runner.py` checks, in order:
1. `HERMESCHAN_OVERLAY_BIN` (explicit path override)
2. `src-tauri/target/release/bundle/macos/hermes-chan.app/Contents/MacOS/hermes-chan`
3. `src-tauri/target/release/hermes-chan-overlay`
4. `src-tauri/target/debug/hermes-chan-overlay`

## Permissions on first run

- **Accessibility** — move/track windows (System Settings ▸ Privacy ▸ Accessibility)
- **Screen Recording** — only for the `bend`/`glitch` window-snapshot effect
- **Automation (Finder)** — only for real `tidy_desktop` icon moves

The body must prompt for these without hanging; under cron/gateway it does not
spawn at all.
