"""Launch and supervise the Rust overlay body.

Design choice: we **build the overlay from source on the user's machine** and
spawn it, rather than shipping a compiled binary. Everything in the repo stays
reviewable source, which sidesteps the catalog's binary-attestation rule
entirely. The cost is a Rust-toolchain dependency for users (disclosed in
``overlay/BUILD.md``); if cargo is absent we log an actionable message and the
plugin still runs headless (tools/hooks work, there is just no visible body).

Safety:
- Never blocks ``register(ctx)`` — building/spawning happen on a daemon thread.
- Skips under unattended runs (cron / gateway) and off macOS for now.
- Bounded restarts so a crash-looping body cannot spin the CPU.
"""
from __future__ import annotations

import logging
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Optional

log = logging.getLogger("hermes-chan.overlay")

_PLUGIN_ROOT = Path(__file__).resolve().parent.parent
_OVERLAY_DIR = _PLUGIN_ROOT / "overlay"
_SRC_TAURI = _OVERLAY_DIR / "src-tauri"

_MAX_RESTARTS = 5
_RESTART_WINDOW_S = 60.0


def _truthy(name: str, default: str = "1") -> bool:
    return os.environ.get(name, default).strip().lower() not in ("0", "false", "no", "")


class OverlayRunner:
    def __init__(self, bridge) -> None:
        self._bridge = bridge
        self._proc: Optional[subprocess.Popen] = None
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()

    # -- public ----------------------------------------------------------
    def maybe_start(self, ctx=None) -> None:
        """Start the body unless disabled, headless, or already running."""
        if not _truthy("HERMESCHAN_AUTOSTART"):
            log.info("hermes-chan: autostart disabled (HERMESCHAN_AUTOSTART=0)")
            return
        if self._is_unattended():
            log.info("hermes-chan: unattended context; not spawning a GUI body")
            return
        if sys.platform != "darwin":
            log.info("hermes-chan: overlay autostart is macOS-only for now (%s)", sys.platform)
            return
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="hermeschan-overlay", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        proc = self._proc
        if proc and proc.poll() is None:
            try:
                proc.terminate()
            except Exception as exc:  # noqa: BLE001
                log.debug("overlay terminate failed: %s", exc)
        self._proc = None

    def is_running(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    # -- internal --------------------------------------------------------
    def _is_unattended(self) -> bool:
        for flag in ("HERMES_UNATTENDED", "HERMES_GATEWAY", "CI"):
            if _truthy(flag, default="0"):
                return True
        return False

    def _run(self) -> None:
        binary = self._locate_binary()
        if binary is None and _truthy("HERMESCHAN_BUILD"):
            binary = self._try_build()
        if binary is None:
            log.warning(
                "hermes-chan: no overlay binary and could not build it. "
                "Install Rust (https://rustup.rs) and run `cargo build --release` in %s, "
                "or `HERMESCHAN_AUTOSTART=0` to silence this.", _SRC_TAURI)
            return
        self._spawn_supervised(binary)

    def _locate_binary(self) -> Optional[Path]:
        override = os.environ.get("HERMESCHAN_OVERLAY_BIN")
        if override and Path(override).exists():
            return Path(override)
        candidates = [
            # A proper `tauri build` .app bundle (best fidelity: transparency, Info.plist).
            _SRC_TAURI / "target/release/bundle/macos/hermes-chan.app/Contents/MacOS/hermes-chan",
            # A bare `cargo build --release` binary.
            _SRC_TAURI / "target/release/hermes-chan-overlay",
            _SRC_TAURI / "target/debug/hermes-chan-overlay",
        ]
        for c in candidates:
            if c.exists():
                return c
        return None

    def _try_build(self) -> Optional[Path]:
        if shutil.which("cargo") is None:
            log.warning("hermes-chan: cargo not found; cannot build the body")
            return None
        log.info("hermes-chan: building overlay from source (first run may take a few minutes)…")
        try:
            subprocess.run(
                ["cargo", "build", "--release"],
                cwd=str(_SRC_TAURI), check=True,
                timeout=float(os.environ.get("HERMESCHAN_BUILD_TIMEOUT", "900")),
            )
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as exc:
            log.warning("hermes-chan: overlay build failed (%s)", exc)
            return None
        return self._locate_binary()

    def _spawn_supervised(self, binary: Path) -> None:
        env = dict(os.environ)
        env["HERMESCHAN_SOCKET"] = self._bridge._path  # noqa: SLF001
        restarts: list[float] = []
        while not self._stop.is_set():
            try:
                self._proc = subprocess.Popen([str(binary)], env=env)
                log.info("hermes-chan: body started (pid %s)", self._proc.pid)
            except Exception as exc:  # noqa: BLE001
                log.warning("hermes-chan: could not spawn body (%s)", exc)
                return
            self._proc.wait()
            if self._stop.is_set():
                return
            now = time.monotonic()
            restarts = [t for t in restarts if now - t < _RESTART_WINDOW_S]
            restarts.append(now)
            if len(restarts) > _MAX_RESTARTS:
                log.warning("hermes-chan: body crashed %d× in %.0fs; giving up",
                            len(restarts), _RESTART_WINDOW_S)
                return
            time.sleep(1.0)
