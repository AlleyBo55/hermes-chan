#!/usr/bin/env bash
# Dev bring-up (macOS and Linux): link this checkout into your Hermes profile,
# enable it, and build her body. On Windows use `hermes plugins install` (README).
set -euo pipefail
PLUGIN_DIR="$(cd "$(dirname "$0")" && pwd)"
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
DEST="$HERMES_HOME/plugins/hermes-chan"
echo "==> Linking plugin into $DEST"
mkdir -p "$HERMES_HOME/plugins"
ln -sfn "$PLUGIN_DIR" "$DEST"
if command -v hermes >/dev/null 2>&1; then
  echo "==> Enabling it (Hermes plugins are opt-in)"
  hermes plugins enable hermes-chan || echo "    could not enable it; run: hermes plugins enable hermes-chan"
else
  echo "    hermes is not on PATH; once it is, run: hermes plugins enable hermes-chan"
fi
echo "==> Building her body (the first build compiles the crates; be patient)"
if command -v cargo >/dev/null 2>&1; then
  ( cd "$PLUGIN_DIR/overlay/src-tauri" && cargo build --release --locked )
  echo "    built: overlay/src-tauri/target/release/hermes-chan-overlay"
else
  echo "    cargo not found — install Rust from https://rustup.rs to get the visible body."
  echo "    (the plugin still runs headless: tools, hooks and chat work without it.)"
fi
cat <<EOF
==> Done. Next steps:
  1. Optional, make her the acting body:  hermes config set computer_use.backend hermes-chan
  2. Start a Hermes session (CLI, TUI or Desktop app); she appears a couple of seconds later.
       /hermeschan status     what her body is doing, and why if she is not there
     Ctrl+click her to chat. Ctrl+Shift+click her to switch her off (/hermeschan on brings her back).
EOF
if [ "$(uname -s)" = "Darwin" ]; then
  echo "  3. macOS asks for Accessibility on first use: System Settings > Privacy & Security > Accessibility."
fi
echo "  Validate before submitting:  hermes plugins validate \"$PLUGIN_DIR\" --install-deps"
