#!/usr/bin/env bash
# Dev bring-up: link the plugin into your Hermes profile and build the body.
set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "$0")" && pwd)"
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
DEST="$HERMES_HOME/plugins/hermes-chan"

echo "==> Linking plugin into $DEST"
mkdir -p "$HERMES_HOME/plugins"
ln -sfn "$PLUGIN_DIR" "$DEST"

echo "==> Building overlay body (first build pulls crates; be patient)"
if command -v cargo >/dev/null 2>&1; then
  ( cd "$PLUGIN_DIR/overlay/src-tauri" && cargo build --release )
  echo "    built: overlay/src-tauri/target/release/hermes-chan-overlay"
else
  echo "    cargo not found — install Rust from https://rustup.rs to get the visible body."
  echo "    (the plugin still runs headless: tools, hooks and chat work without it.)"
fi

cat <<EOF

==> Done. Next steps:
  1. Make her the acting body (optional) — add to $HERMES_HOME/config.yaml:
         computer_use:
           backend: hermes-chan
  2. Start a Hermes session (CLI, TUI, or desktop).
  3. In the session:  /hermeschan wake      # she appears and perches
                       /hermeschan say hi    # speech bubble
     or just type into her bubble to talk to the agent.
  4. First run prompts for macOS Accessibility (to move/track windows). Grant it
     in System Settings > Privacy & Security > Accessibility.

  Validate before submitting:  hermes plugins validate "$PLUGIN_DIR" --install-deps
EOF
