"""Launch and supervise the Rust overlay body.

Design choice: we **build the overlay from source on the user's machine** and
spawn it, rather than shipping a compiled binary. Everything in the repo stays
reviewable source, which sidesteps the catalog's binary-attestation rule
entirely. The cost is a Rust-toolchain dependency for users (disclosed in
``overlay/BUILD.md``); if cargo is absent we log an actionable message and the
plugin still runs headless (tools/hooks work, there is just no visible body).

Safety:
- Never blocks ``register(ctx)``: building and spawning happen on a daemon thread.
- Starts a body only when this process owns the bridge, so two Hermes processes
  never put two of her on screen.
- Skips unattended runs (CI, kanban workers) and processes with no desktop to draw on.
- Bounded restarts, so a crash-looping body cannot spin the CPU. A body that
  closed itself (Ctrl+Shift+click) stays closed.
- Build and body output go to log files in the plugin's state directory, never
  into the Hermes terminal.
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
from typing import IO, Any, Callable, Optional

from .paths import hermes_home, state_dir

log = logging.getLogger("hermes-chan.overlay")

_PLUGIN_ROOT = Path(__file__).resolve().parent.parent
_OVERLAY_DIR = _PLUGIN_ROOT / "overlay"
_SRC_TAURI = _OVERLAY_DIR / "src-tauri"
_EXE = ".exe" if sys.platform == "win32" else ""
_BINARY = "hermes-chan-overlay" + _EXE

_MAX_RESTARTS = 5
_RESTART_WINDOW_S = 60.0
_MAX_LOG_BYTES = 5 * 1024 * 1024

# Hermes' environment holds every provider API key. The build and her body never get a copy
# of it: each gets only these names (matched case-insensitively, as Windows does) plus the
# HERMESCHAN_* / HERMES_HOME values the plugin sets itself.
_ENV_SHARED = frozenset({
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LANGUAGE", "TZ", "TMPDIR", "TEMP", "TMP",
    "RUST_LOG", "RUST_BACKTRACE",
    # Windows
    "SYSTEMROOT", "SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT", "OS", "USERNAME", "USERPROFILE",
    "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "ALLUSERSPROFILE",
    "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)", "COMMONPROGRAMW6432", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE",
})
_ENV_SHARED_PREFIXES = ("LC_",)
# `cargo build`: the Rust toolchain, the C toolchain and pkg-config its build scripts use,
# and the proxy / CA settings it needs to download the crates pinned in Cargo.lock.
_ENV_BUILD = frozenset({
    "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "CARGO_HTTP_CAINFO",
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "SSL_CERT_FILE", "SSL_CERT_DIR",
    "CC", "CXX", "AR", "PKG_CONFIG", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR", "PKG_CONFIG_SYSROOT_DIR",
    "SDKROOT", "DEVELOPER_DIR", "MACOSX_DEPLOYMENT_TARGET",
    # A Visual Studio developer prompt
    "INCLUDE", "LIB", "LIBPATH", "VSINSTALLDIR", "VCINSTALLDIR", "VCTOOLSINSTALLDIR",
    "WINDOWSSDKDIR", "WINDOWSSDKVERSION", "UNIVERSALCRTSDKDIR", "UCRTVERSION",
})
# Her body: the desktop session she draws on (X11/XWayland, GTK and WebKitGTK on Linux;
# WebView2 on Windows), including the input method her chat box types through.
_ENV_BODY = frozenset({
    "DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS", "XMODIFIERS",
    "LD_LIBRARY_PATH",
})
_ENV_BODY_PREFIXES = ("XDG_", "GDK_", "GTK_", "WEBKIT_", "WEBVIEW2_")


def _minimal_env(names: frozenset[str], prefixes: tuple[str, ...] = ()) -> dict[str, str]:
    """The allowlisted part of this process's environment, nothing else."""
    allowed = _ENV_SHARED | names
    allowed_prefixes = _ENV_SHARED_PREFIXES + prefixes
    return {key: value for key, value in os.environ.items()
            if key.upper() in allowed or key.upper().startswith(allowed_prefixes)}


def _truthy(name: str, default: str = "1") -> bool:
    return os.environ.get(name, default).strip().lower() not in ("0", "false", "no", "")


def _windows_session_id() -> Optional[int]:
    try:
        import ctypes
        from ctypes import wintypes

        session = wintypes.DWORD()
        if ctypes.windll.kernel32.ProcessIdToSessionId(os.getpid(), ctypes.byref(session)):
            return int(session.value)
    except Exception:  # noqa: BLE001 - only used to explain an invisible body
        return None
    return None


def desktop_problem() -> Optional[str]:
    """Why this process cannot put a window on a desktop, or None when it can."""
    if sys.platform.startswith("linux"):
        if not os.environ.get("DISPLAY"):
            if os.environ.get("WAYLAND_DISPLAY"):
                return "this Wayland session has no XWayland (DISPLAY is unset); she needs it to place her window"
            return ("no graphical display in this Hermes process (DISPLAY is unset); if the gateway runs as "
                    "a systemd service, run `systemctl --user import-environment DISPLAY WAYLAND_DISPLAY "
                    "XAUTHORITY` and `hermes gateway restart`")
    elif sys.platform == "win32" and _windows_session_id() == 0:
        return "Hermes runs as a Windows service (session 0), where desktop windows are invisible"
    return None


def _open_log(path: Path, *, truncate: bool = False) -> IO[bytes]:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        too_big = path.stat().st_size > _MAX_LOG_BYTES
    except OSError:
        too_big = False
    return open(path, "wb" if (truncate or too_big) else "ab")  # noqa: SIM115 - closed by the caller


def _child_options() -> dict[str, Any]:
    if sys.platform == "win32":
        # Debug builds are console programs; never flash a console window.
        return {"creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0)}
    # Her own session: Ctrl+C in the Hermes terminal is for Hermes, not for her.
    return {"start_new_session": True}


class OverlayRunner:
    def __init__(self, bridge) -> None:
        self._bridge = bridge
        self._proc: Optional[subprocess.Popen] = None
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._phase = "stopped"
        self.problem: Optional[str] = None

    # -- public ----------------------------------------------------------
    def blocked_reason(self) -> Optional[str]:
        """Why this process will not show a body at all, or None."""
        if not _truthy("HERMESCHAN_AUTOSTART"):
            return "autostart is off (HERMESCHAN_AUTOSTART=0)"
        if self._is_unattended():
            return "unattended run (CI or a kanban worker); no desktop body"
        return desktop_problem()

    def maybe_start(self, ctx=None) -> None:
        """Start the body unless disabled, unattended, headless, or already running."""
        del ctx
        blocked = self.blocked_reason()
        if blocked:
            self.problem = blocked
            log.info("hermes-chan: not showing her body: %s", blocked)
            return
        with self._lock:
            if self._thread and self._thread.is_alive():
                return
            self._stop.clear()
            self.problem = None
            self._phase = "starting"
            self._thread = threading.Thread(target=self._run, name="hermeschan-overlay", daemon=True)
            self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        proc = self._proc
        self._proc = None
        if proc is not None and proc.poll() is None:
            try:
                proc.terminate()
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
            except Exception as exc:  # noqa: BLE001
                log.debug("overlay terminate failed: %s", exc)
        self._phase = "stopped"

    def restart(self, before_start: Optional[Callable[[], None]] = None) -> None:
        """End her body, wait for its supervisor to finish, then start a fresh one.
        ``before_start`` runs in between, while no body is alive to rewrite her files."""
        self.stop()
        thread = self._thread
        if thread is not None and thread is not threading.current_thread():
            thread.join(timeout=5.0)
        if before_start is not None:
            before_start()
        self.problem = None
        self.maybe_start()

    def is_running(self) -> bool:
        proc = self._proc
        return proc is not None and proc.poll() is None

    def describe(self) -> str:
        proc = self._proc
        if proc is not None and proc.poll() is None:
            return f"running (pid {proc.pid})"
        if self._phase == "building":
            return f"building from source (log: {state_dir() / 'build.log'})"
        if self._phase == "starting":
            return "starting"
        if self.problem:
            return f"not running: {self.problem}"
        return "not running"

    # -- internal --------------------------------------------------------
    def _is_unattended(self) -> bool:
        # HERMES_UNATTENDED is an explicit opt-out; the others are set by CI and kanban workers.
        if any(_truthy(flag, default="0") for flag in ("HERMES_UNATTENDED", "CI")):
            return True
        return bool(os.environ.get("HERMES_KANBAN_TASK", "").strip()) or \
            os.environ.get("HERMES_CRON_SESSION", "").strip() == "1"

    def _run(self) -> None:
        try:
            self._run_once()
        finally:
            if self._phase != "running":
                self._phase = "stopped"

    def _run_once(self) -> None:
        # Another Hermes process may own the bridge for a while; she appears once it lets go.
        while not self._bridge.wait_listening(timeout=5.0):
            if self._stop.is_set():
                return
            if self._bridge.failed:
                self.problem = self._bridge.problem or "the bridge to her body is not listening"
                log.warning("hermes-chan: not showing her body: %s", self.problem)
                return
        binary = self._locate_binary()
        if binary is None and _truthy("HERMESCHAN_BUILD"):
            binary = self._try_build()
        if self._stop.is_set():
            return
        if binary is None:
            if not self.problem:
                self.problem = (f"no overlay binary; run `cargo build --release` in {_SRC_TAURI}, "
                                "or set HERMESCHAN_AUTOSTART=0 to silence this")
            log.warning("hermes-chan: %s", self.problem)
            return
        self._spawn_supervised(binary)

    def _locate_binary(self) -> Optional[Path]:
        override = os.environ.get("HERMESCHAN_OVERLAY_BIN")
        if override and Path(override).is_file():
            return Path(override)
        target_dirs = [_SRC_TAURI / "target"]
        cargo_target = os.environ.get("CARGO_TARGET_DIR", "").strip()
        if cargo_target:
            target_dirs.append(Path(cargo_target))
        candidates: list[Path] = []
        for target in target_dirs:
            if sys.platform == "darwin":
                # A proper `tauri build` .app bundle (best fidelity: transparency, Info.plist).
                candidates.append(target / "release/bundle/macos/hermes-chan.app/Contents/MacOS/hermes-chan")
            candidates += [target / "release" / _BINARY, target / "debug" / _BINARY]
        for candidate in candidates:
            if candidate.is_file() and self._binary_is_fresh(candidate):
                return candidate
        return None

    @staticmethod
    def _binary_is_fresh(binary: Path) -> bool:
        """Reject embedded frontends older than any canonical runtime source."""
        try:
            built_at = binary.stat().st_mtime_ns
        except OSError:
            return False
        roots = [
            _OVERLAY_DIR / "src",
            _SRC_TAURI / "src",
            _PLUGIN_ROOT / "sprites" / "hermeschan-v1" / "atlases",
        ]
        files = [
            _SRC_TAURI / "Cargo.toml",
            _SRC_TAURI / "build.rs",
            _SRC_TAURI / "tauri.conf.json",
            _PLUGIN_ROOT / "sprites" / "hermeschan-v1" / "manifest.json",
        ]
        for root in roots:
            if root.exists():
                files.extend(path for path in root.rglob("*") if path.is_file())
        try:
            return all(path.stat().st_mtime_ns <= built_at for path in files if path.exists())
        except OSError:
            return False

    @staticmethod
    def _find_cargo() -> Optional[str]:
        found = shutil.which("cargo")
        if found:
            return found
        # Hermes started by a service manager often lacks rustup's PATH entry.
        default = Path.home() / ".cargo" / "bin" / ("cargo" + _EXE)
        return str(default) if default.is_file() else None

    def _try_build(self) -> Optional[Path]:
        cargo = self._find_cargo()
        if cargo is None:
            self.problem = "Rust is not installed (https://rustup.rs), so her body cannot be built"
            return None
        build_log = state_dir() / "build.log"
        self._phase = "building"
        log.info("hermes-chan: building her body from source; the first build takes a few minutes (log: %s)",
                 build_log)
        command = [cargo, "build", "--release", "--locked", "--target-dir", str(_SRC_TAURI / "target")]
        try:
            with _open_log(build_log, truncate=True) as output:
                result = subprocess.run(
                    command,
                    cwd=str(_SRC_TAURI),
                    env=_minimal_env(_ENV_BUILD),
                    stdin=subprocess.DEVNULL,
                    stdout=output,
                    stderr=subprocess.STDOUT,
                    timeout=float(os.environ.get("HERMESCHAN_BUILD_TIMEOUT", "1800")),
                    check=False,
                    **({"creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0)}
                       if sys.platform == "win32" else {}),
                )
        except (OSError, subprocess.SubprocessError, ValueError) as exc:
            self.problem = f"building her body failed ({exc}); see {build_log}"
            return None
        finally:
            self._phase = "starting"
        if result.returncode != 0:
            self.problem = f"building her body failed; see {build_log}"
            return None
        return self._locate_binary()

    def _spawn_supervised(self, binary: Path) -> None:
        env = _minimal_env(_ENV_BODY, _ENV_BODY_PREFIXES)
        env.update(self._bridge.overlay_env())
        env["HERMESCHAN_PARENT_PID"] = str(os.getpid())
        env["HERMESCHAN_STATE_DIR"] = str(state_dir())
        env["HERMES_HOME"] = str(hermes_home())
        body_log = state_dir() / "overlay.log"
        restarts: list[float] = []
        while not self._stop.is_set():
            try:
                with _open_log(body_log) as output:
                    proc = subprocess.Popen(
                        [str(binary)],
                        env=env,
                        cwd=str(binary.parent),
                        stdin=subprocess.DEVNULL,
                        stdout=output,
                        stderr=subprocess.STDOUT,
                        **_child_options(),
                    )
            except OSError as exc:
                self.problem = f"could not start her body ({exc})"
                log.warning("hermes-chan: %s", self.problem)
                return
            self._proc = proc
            self._phase = "running"
            if self._stop.is_set():
                # stop() raced the spawn: it saw no process to end.
                self.stop()
                return
            log.info("hermes-chan: body started (pid %s)", proc.pid)
            code = proc.wait()
            self._phase = "stopped"
            if self._stop.is_set():
                return
            if code == 0:
                log.info("hermes-chan: her body closed itself; not restarting")
                return
            now = time.monotonic()
            restarts = [t for t in restarts if now - t < _RESTART_WINDOW_S]
            restarts.append(now)
            if len(restarts) > _MAX_RESTARTS:
                self.problem = f"her body crashed {len(restarts)} times in a minute; see {body_log}"
                log.warning("hermes-chan: %s", self.problem)
                return
            log.warning("hermes-chan: body exited with %s; restarting", code)
            if self._stop.wait(1.0):
                return
