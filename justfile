# hermes-chan tasks. Run `just` with no args to list.
set shell := ["bash", "-uc"]

plugin_dir := justfile_directory()
hermes_home := env_var_or_default("HERMES_HOME", env_var("HOME") + "/.hermes")

default:
    @just --list

# Compile-check the overlay (no GUI).
check:
    cd overlay/src-tauri && cargo check

# Build the bare overlay binary.
build:
    cd overlay/src-tauri && cargo build --release

# Live-iterate the overlay window (needs @tauri-apps/cli).
dev:
    cd overlay && npx tauri dev

# Build the macOS .app bundle (best fidelity: transparency).
bundle:
    cd overlay && npx tauri build

# Symlink the plugin into your Hermes profile (dev install).
install:
    mkdir -p "{{hermes_home}}/plugins"
    ln -sfn "{{plugin_dir}}" "{{hermes_home}}/plugins/hermes-chan"
    @echo "linked -> {{hermes_home}}/plugins/hermes-chan"

uninstall:
    rm -f "{{hermes_home}}/plugins/hermes-chan"

# Run the catalog validation CI check locally.
validate:
    hermes plugins validate "{{plugin_dir}}" --install-deps

# Byte-compile the Python plugin.
pycheck:
    python3 -m compileall -q .

clean:
    cd overlay/src-tauri && cargo clean
    find . -name __pycache__ -type d -prune -exec rm -rf {} + 2>/dev/null || true
