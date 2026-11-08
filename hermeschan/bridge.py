"""Duplex bridge between the Python plugin and the desktop overlay.

The plugin is the server. On macOS and Linux it owns an owner-only ``AF_UNIX``
stream socket. Python has no ``AF_UNIX`` on Windows, so there it listens on
``127.0.0.1`` and accepts only a client whose first line carries the random
token handed to the overlay it starts (also kept in an owner-only
``bridge.json`` for an overlay started by hand). Newline-delimited JSON carries
animation commands to the overlay and chat/action acknowledgements back.
Missing overlays always degrade to immediate no-ops.
"""
from __future__ import annotations

import base64
import binascii
import errno
import hmac
import json
import logging
import os
import secrets
import socket
import stat
import sys
import tempfile
import threading
import types
import uuid
from collections import OrderedDict
from pathlib import Path
from typing import Any, Callable, Optional

from .paths import hermes_home, private_file, state_dir

log = logging.getLogger("hermes-chan.bridge")

_MAX_PENDING_ACTIONS = 64
_MAX_ACTION_EVENTS = 16
_MAX_WINDOW_REQUESTS = 4
_MAX_INBOUND_BYTES = 10 * 1024 * 1024
_MAX_SCREENSHOT_BYTES = 6 * 1024 * 1024
_USE_TCP = sys.platform == "win32" or not hasattr(socket, "AF_UNIX")
_ACCEPT_POLL_SECONDS = 0.5
_HELLO_TIMEOUT_SECONDS = 5.0
_CHAT_SETTING = "plugins.entries.hermes-chan.allow_gateway_injection"
_CHAT_COMMAND = f"hermes config set {_CHAT_SETTING} true"
# Hermes refuses plugin messages into your chats until you allow it for this plugin.
_INJECT_BLOCKED = f"Hermes blocks her chat box until you allow it. In a terminal run: {_CHAT_COMMAND}"
_INJECT_REFUSED = ("Hermes did not take the message. Send any message to Hermes in the chat "
                   "I should talk to, then try again here.")


def _default_socket_path() -> str:
    env = os.environ.get("HERMESCHAN_SOCKET")
    if env:
        return env
    return str(hermes_home() / "hermes-chan.sock")


# Hermes imports this plugin twice in one process when it is also the computer-use backend (the
# general loader and the computer-use loader, under different module names), so each import has
# its own Bridge class. The one that talks to her body is published here for the other to find.
_SHARED_MODULE = "hermes_chan_shared"


def _publish(bridge: "Bridge") -> None:
    holder = sys.modules.get(_SHARED_MODULE)
    if holder is None:
        holder = types.ModuleType(_SHARED_MODULE)
        sys.modules[_SHARED_MODULE] = holder
    holder.bridge = bridge


def live_bridge() -> Optional["Bridge"]:
    """The bridge the general plugin load started, from whichever import of the plugin asks."""
    return getattr(sys.modules.get(_SHARED_MODULE), "bridge", None)


def _close_socket(sock: socket.socket) -> None:
    """Shut down first: on Linux a bare close() does not wake a thread blocked in recv/accept."""
    try:
        sock.shutdown(socket.SHUT_RDWR)
    except OSError:
        pass
    try:
        sock.close()
    except OSError:
        pass


def _pid_alive(pid: int) -> bool:
    if pid <= 0:
        return False
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.restype = wintypes.HANDLE
        kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        kernel32.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
        kernel32.WaitForSingleObject.restype = wintypes.DWORD
        kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
        handle = kernel32.OpenProcess(0x00100000, False, pid)  # SYNCHRONIZE
        if not handle:
            return ctypes.get_last_error() == 5  # ERROR_ACCESS_DENIED: exists, not ours
        try:
            return kernel32.WaitForSingleObject(handle, 0) == 0x102  # WAIT_TIMEOUT: still running
        finally:
            kernel32.CloseHandle(handle)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False
    return True


class _PendingAction:
    def __init__(self) -> None:
        self.complete = threading.Event()
        self.native_result = threading.Event()
        self.result: Optional[dict[str, Any]] = None
        self.events: "OrderedDict[str, threading.Event]" = OrderedDict()
        self.cancelled = False
        self.lock = threading.Lock()

    def mark_event(self, name: str) -> None:
        with self.lock:
            event = self.events.get(name)
            if event is None:
                if len(self.events) >= _MAX_ACTION_EVENTS:
                    return
                event = threading.Event()
                self.events[name] = event
            event.set()

    def wait_event(self, name: str, timeout: float) -> bool:
        with self.lock:
            event = self.events.get(name)
            if event is None:
                if len(self.events) >= _MAX_ACTION_EVENTS:
                    return False
                event = threading.Event()
                self.events[name] = event
        return event.wait(max(0.0, timeout)) and not self.cancelled

    def cancel(self) -> None:
        with self.lock:
            self.cancelled = True
            for event in self.events.values():
                event.set()
            self.native_result.set()
            self.complete.set()


class Bridge:
    """Singleton duplex link to the overlay with thread-safe writes."""

    _instance: Optional["Bridge"] = None
    _instance_lock = threading.Lock()

    def __init__(self, ctx=None) -> None:
        self._ctx = ctx
        self._transport = "tcp" if _USE_TCP else "unix"
        self._path = _default_socket_path()
        self._endpoint_file = state_dir() / "bridge.json"
        self._port: Optional[int] = None
        self._token = secrets.token_urlsafe(32)
        self._server: Optional[socket.socket] = None
        self._client: Optional[socket.socket] = None
        self._write_lock = threading.RLock()
        self._start_lock = threading.Lock()
        self._started = False
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._listening = False
        self.problem: Optional[str] = None
        # Called (on its own thread) when the user switches her off from the overlay.
        self.deactivate_handler: Optional[Callable[[], None]] = None
        # Called once this process owns the bridge (it may have waited for another one).
        self.on_acquired: Optional[Callable[[], None]] = None
        self.chat_setting = _CHAT_SETTING
        # Called each time her window says it is ready (its page listens for messages).
        self.on_connected: Optional[Callable[[], None]] = None
        self._socket_identity: Optional[tuple[int, int]] = None
        self._actions: "OrderedDict[str, _PendingAction]" = OrderedDict()
        self._actions_lock = threading.Lock()
        self._window_requests: "OrderedDict[str, dict[str, Any]]" = OrderedDict()
        self._window_lock = threading.Lock()
        self._active_window: dict[str, Any] = {"app": "", "title": ""}
        self._config: dict[str, Any] = {}
        self._config_lock = threading.Lock()
        self._session_key: Optional[str] = None
        self._agent_busy = threading.Event()

    @classmethod
    def shared(cls, ctx=None) -> "Bridge":
        with cls._instance_lock:
            if cls._instance is None:
                cls._instance = cls(ctx)
                _publish(cls._instance)
            elif ctx is not None:
                cls._instance._ctx = ctx
            return cls._instance

    @property
    def path(self) -> str:
        """Where the bridge listens, for status lines."""
        if self._transport == "tcp":
            return f"127.0.0.1:{self._port}" if self._port else "127.0.0.1 (not listening)"
        return self._path

    @property
    def is_connected(self) -> bool:
        return self._client is not None

    @property
    def is_listening(self) -> bool:
        return self._listening

    @property
    def failed(self) -> bool:
        """The bridge gave up (see ``problem``) rather than still waiting to bind."""
        return self._ready.is_set() and not self._listening

    def wait_listening(self, timeout: float = 5.0) -> bool:
        """True once this process owns the bridge; False on timeout or failure."""
        self._ready.wait(max(0.0, timeout))
        return self._listening

    def overlay_env(self) -> dict[str, str]:
        """How the overlay this process starts finds and proves itself to the bridge."""
        if self._transport == "tcp":
            return {"HERMESCHAN_TCP": f"127.0.0.1:{self._port}", "HERMESCHAN_TOKEN": self._token}
        return {"HERMESCHAN_SOCKET": self._path}

    @property
    def agent_busy(self) -> bool:
        return self._agent_busy.is_set()

    def set_agent_busy(self, busy: bool) -> None:
        if busy:
            self._agent_busy.set()
        else:
            self._agent_busy.clear()

    def remember_session(self, session_key: Optional[str]) -> None:
        value = session_key.strip() if isinstance(session_key, str) else ""
        if value and value != self._session_key:
            self._session_key = value
            log.info("hermes-chan: her chat box now talks to the chat you used last (%s…)", value[:20])

    def chat_steps(self) -> list[str]:
        """What is still missing before her chat box can post into your chat, in order."""
        steps = []
        if self.chat_allowed() is False:
            steps.append(f"Allow it once, in a terminal: {_CHAT_COMMAND}")
        if not self._session_key:
            steps.append("Send any message to Hermes in the chat I should talk to")
        return steps

    # -- outbound -------------------------------------------------------
    def send(self, obj: dict[str, Any]) -> bool:
        failed_client: Optional[socket.socket] = None
        with self._write_lock:
            client = self._client
            if client is None:
                return False
            try:
                line = (json.dumps(obj, separators=(",", ":")) + "\n").encode("utf-8")
                client.sendall(line)
                return True
            except (OSError, TypeError, ValueError) as exc:
                log.debug("overlay write failed (%s); dropping client", exc)
                if self._client is client:
                    self._client = None
                failed_client = client
        if failed_client is not None:
            try:
                failed_client.close()
            except OSError:
                pass
            self._release_pending()
        return False

    def set_state(self, state: str, **extra: Any) -> None:
        self.send({"type": "state", "state": state, **extra})

    def say(self, text: str, emotion: str = "neutral", **extra: Any) -> None:
        self.send({"type": "say", "text": text, "emotion": emotion, **extra})

    def react(self, category: str) -> None:
        """Her own quick reaction to the tool the agent just used: the overlay picks a line
        for this category and mumbles it. Local, no model call."""
        self.send({"type": "react", "category": category})

    def reaction(self, reaction_id: str, text: str = "", emotion: str = "") -> None:
        """A full desktop-event reaction (pose + click-through effect + spoken line).
        The overlay owns the choreography (overlay/REACTIONS.md A-D); this names which one.
        Known ids: email, message, reminder, affection."""
        payload = {"type": "reaction", "id": str(reaction_id)}
        if text:
            payload["text"] = str(text)[:160]
        if emotion:
            payload["emotion"] = str(emotion)
        self.send(payload)

    def emote(self, emotion: str) -> None:
        self.send({"type": "emote", "emotion": emotion})

    def point_at(self, target: dict[str, Any]) -> None:
        self.send({"type": "point", "target": target})

    def update_config(self, **changes: Any) -> None:
        with self._config_lock:
            self._config.update(changes)
            payload = dict(self._config)
        self.send({"type": "config", **payload})

    def animate_action(self, verb: str, target: Optional[dict[str, Any]] = None,
                       **extra: Any) -> str:
        """Send an action with a unique ID and retain bounded synchronization state."""
        action_id = uuid.uuid4().hex
        pending = _PendingAction()
        with self._actions_lock:
            self._actions[action_id] = pending
            while len(self._actions) > _MAX_PENDING_ACTIONS:
                _, evicted = self._actions.popitem(last=False)
                evicted.cancel()
        if not self.send({"type": "action", "action_id": action_id, "verb": verb,
                          "target": target or {}, **extra}):
            self.finish_action(action_id, cancel=True)
        return action_id

    def wait_for_action_event(self, action_id: str, event: str, timeout: float = 0.7) -> bool:
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is None or not self.is_connected:
            return False
        return pending.wait_event(event, min(max(0.0, timeout), 0.7))

    def request_native_input(self, action_id: str, verb: str,
                             payload: dict[str, Any], timeout: float = 5.0) -> dict[str, Any]:
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is None or not self.is_connected:
            return {"ok": False, "action": verb, "code": "body_disconnected",
                    "message": "Hermes-chan's native body is not connected."}
        if not self.send({"type": "native_input", "action_id": action_id,
                          "verb": verb, "input": payload}):
            return {"ok": False, "action": verb, "code": "body_disconnected",
                    "message": "Hermes-chan's native body disconnected."}
        if not pending.native_result.wait(min(max(timeout, 0.0), 10.0)):
            return {"ok": False, "action": verb, "code": "input_timeout",
                    "message": "Hermes-chan's native input timed out."}
        with pending.lock:
            return dict(pending.result or {
                "ok": False, "action": verb, "code": "input_failed",
                "message": "Hermes-chan returned no native input result.",
            })

    def notify_action_result(self, action_id: str, result: Any) -> None:
        if isinstance(result, dict):
            ok = bool(result.get("ok"))
            action = result.get("action")
            code = result.get("code")
            message = result.get("message")
        else:
            ok = bool(getattr(result, "ok", False))
            action = getattr(result, "action", None)
            code = getattr(result, "code", None)
            message = getattr(result, "message", None)
        self.send({"type": "action_result", "action_id": action_id, "ok": ok,
                   "action": action, "code": code, "message": message})

    def wait_for_action_complete(self, action_id: str, timeout: float = 0.9) -> bool:
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is None or not self.is_connected:
            return False
        return pending.complete.wait(min(max(0.0, timeout), 0.9)) and not pending.cancelled

    def finish_action(self, action_id: str, *, cancel: bool = False) -> None:
        with self._actions_lock:
            pending = self._actions.pop(action_id, None)
        if cancel and pending is not None:
            pending.cancel()

    def request_active_window(self, *, include_screenshot: bool, timeout: float = 0.75) -> dict[str, Any]:
        """Request one active-window sample; screenshots are accepted only for opted-in requests."""
        request_id = uuid.uuid4().hex
        request = {"event": threading.Event(), "include_screenshot": bool(include_screenshot), "result": None}
        with self._window_lock:
            self._window_requests[request_id] = request
            while len(self._window_requests) > _MAX_WINDOW_REQUESTS:
                _, evicted = self._window_requests.popitem(last=False)
                evicted["event"].set()
            latest = dict(self._active_window)
        if not self.send({"type": "active_window_request", "request_id": request_id,
                          "include_screenshot": bool(include_screenshot)}):
            with self._window_lock:
                self._window_requests.pop(request_id, None)
            return latest
        request["event"].wait(min(max(0.0, timeout), 5.0))
        with self._window_lock:
            current = self._window_requests.pop(request_id, None)
            result = current.get("result") if current else request.get("result")
            return dict(result) if isinstance(result, dict) else dict(self._active_window)

    def latest_active_window(self) -> dict[str, Any]:
        with self._window_lock:
            return dict(self._active_window)

    # -- inbound server -------------------------------------------------
    def start_inbound(self, ctx) -> None:
        self._ctx = ctx
        with self._start_lock:
            if self._started:
                return
            self._started = True
            self._stop.clear()
            self._ready.clear()
            self._listening = False
            self.problem = None
            thread = threading.Thread(target=self._serve, name="hermeschan-bridge", daemon=True)
            self._thread = thread
        thread.start()

    def stop(self) -> None:
        self._stop.set()
        with self._write_lock:
            client, server = self._client, self._server
            self._client = None
            self._server = None
        for sock in (client, server):
            if sock is not None:
                _close_socket(sock)
        self._release_pending()
        thread = self._thread
        if thread is not None and thread is not threading.current_thread():
            thread.join(timeout=2.0)
        with self._start_lock:
            if thread is None or not thread.is_alive():
                self._started = False
                self._thread = None
                self._listening = False
                self._unlink_owned_socket()
                self._remove_endpoint_file()

    def _bind_when_free(self) -> Optional[socket.socket]:
        """Bind, waiting while another live Hermes process owns the bridge (it may be a
        short-lived `hermes plugins` command that is about to exit)."""
        while not self._stop.is_set():
            try:
                srv = self._bind()
            except OSError as exc:
                if exc.errno != errno.EADDRINUSE:
                    raise
                self.problem = "another Hermes process is showing her; waiting for it to let go"
                log.info("hermes-chan bridge: %s", self.problem)
                if self._stop.wait(5.0):
                    return None
                continue
            self.problem = None
            return srv
        return None

    def _serve(self) -> None:
        me = threading.current_thread()
        srv: Optional[socket.socket] = None
        try:
            srv = self._bind_when_free()
            if srv is None:
                return
            with self._write_lock:
                self._server = srv
            self._listening = True
            self._ready.set()
            log.info("hermes-chan bridge listening on %s", self.path)
            if self.on_acquired is not None:
                try:
                    self.on_acquired()
                except Exception:  # noqa: BLE001 - a handler failure must not end the server loop
                    log.exception("hermes-chan: taking over her body failed")
            while not self._stop.is_set():
                try:
                    conn, _ = srv.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                if self._stop.is_set():
                    _close_socket(conn)
                    break
                leftover = b""
                if self._transport == "tcp":
                    accepted, leftover = self._authenticate(conn)
                    if not accepted:
                        log.warning("hermes-chan bridge refused a local client without the overlay token")
                        _close_socket(conn)
                        continue
                with self._write_lock:
                    self._client = conn
                self._replay_state()
                self._read_loop(conn, leftover)
                with self._write_lock:
                    if self._client is conn:
                        self._client = None
                _close_socket(conn)
                self._release_pending()
        except OSError as exc:
            self.problem = f"could not listen on {self.path}: {exc}"
            log.warning("hermes-chan bridge %s; body disabled", self.problem)
        finally:
            if srv is not None:
                _close_socket(srv)
            with self._start_lock:
                if self._thread is me:
                    with self._write_lock:
                        if self._server is srv:
                            self._server = None
                    self._listening = False
                    self._unlink_owned_socket()
                    self._remove_endpoint_file()
                    self._started = False
                    self._thread = None
            self._ready.set()

    def _bind(self) -> socket.socket:
        if self._transport == "tcp":
            if self._live_tcp_owner():
                raise OSError(errno.EADDRINUSE, "another live Hermes-chan bridge owns this profile")
            srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            try:
                exclusive = getattr(socket, "SO_EXCLUSIVEADDRUSE", None)
                if exclusive is not None:
                    srv.setsockopt(socket.SOL_SOCKET, exclusive, 1)
                srv.bind(("127.0.0.1", 0))
                self._port = srv.getsockname()[1]
                self._token = secrets.token_urlsafe(32)
                self._write_endpoint_file()
                srv.listen(1)
            except OSError:
                srv.close()
                raise
        else:
            Path(self._path).parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            self._remove_stale_socket()
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                srv.bind(self._path)
                os.chmod(self._path, 0o600)
                info = os.lstat(self._path)
                self._socket_identity = (info.st_dev, info.st_ino)
                srv.listen(1)
            except OSError:
                srv.close()
                raise
        # Lets stop() and a closed server end the accept loop on every OS.
        srv.settimeout(_ACCEPT_POLL_SECONDS)
        return srv

    # -- Windows: loopback TCP with a token ----------------------------------
    def _read_endpoint_file(self) -> Optional[dict[str, Any]]:
        try:
            data = json.loads(self._endpoint_file.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
        return data if isinstance(data, dict) else None

    def _live_tcp_owner(self) -> bool:
        """Another running Hermes in this profile already shows her (one body per profile)."""
        data = self._read_endpoint_file()
        if not data:
            return False
        port, pid = data.get("port"), data.get("pid")
        if not isinstance(port, int) or not isinstance(pid, int) or pid == os.getpid() or not _pid_alive(pid):
            return False
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.3):
                return True
        except OSError:
            return False

    def _write_endpoint_file(self) -> None:
        parent = self._endpoint_file.parent
        parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        payload = {"transport": "tcp", "host": "127.0.0.1", "port": self._port,
                   "token": self._token, "pid": os.getpid()}
        fd, temporary = tempfile.mkstemp(prefix=".bridge-", suffix=".tmp", dir=str(parent))
        try:
            private_file(fd)
            with os.fdopen(fd, "w", encoding="utf-8") as handle:
                fd = -1
                json.dump(payload, handle)
            os.replace(temporary, self._endpoint_file)
        finally:
            if fd >= 0:
                os.close(fd)
            try:
                os.unlink(temporary)
            except FileNotFoundError:
                pass

    def _remove_endpoint_file(self) -> None:
        if self._transport != "tcp":
            return
        data = self._read_endpoint_file()
        if data and data.get("token") == self._token:
            try:
                self._endpoint_file.unlink()
            except OSError as exc:
                log.debug("could not remove %s: %s", self._endpoint_file, exc)

    def _authenticate(self, conn: socket.socket) -> tuple[bool, bytes]:
        """The overlay's first line must be {"type": "hello", "token": <ours>}."""
        buffer = b""
        try:
            conn.settimeout(_HELLO_TIMEOUT_SECONDS)
            while b"\n" not in buffer and len(buffer) < 4096:
                chunk = conn.recv(4096)
                if not chunk:
                    return False, b""
                buffer += chunk
            conn.settimeout(None)
        except OSError:
            return False, b""
        line, _, rest = buffer.partition(b"\n")
        try:
            hello = json.loads(line.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            return False, b""
        token = hello.get("token") if isinstance(hello, dict) else None
        accepted = (isinstance(hello, dict) and hello.get("type") == "hello" and isinstance(token, str)
                    and hmac.compare_digest(token.encode("utf-8"), self._token.encode("utf-8")))
        return accepted, rest

    def _remove_stale_socket(self) -> None:
        try:
            info = os.lstat(self._path)
        except FileNotFoundError:
            return
        if not stat.S_ISSOCK(info.st_mode):
            raise OSError(errno.EEXIST, "refusing to unlink a non-socket path", self._path)
        probe = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        probe.settimeout(0.2)
        try:
            probe.connect(self._path)
        except OSError as exc:
            if exc.errno not in (errno.ECONNREFUSED, errno.ENOENT):
                raise OSError(errno.EADDRINUSE, "existing socket could not be proven stale", self._path) from exc
        else:
            raise OSError(errno.EADDRINUSE, "another live Hermes-chan bridge owns the socket", self._path)
        finally:
            probe.close()
        try:
            current = os.lstat(self._path)
        except FileNotFoundError:
            return
        if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
            raise OSError(errno.EBUSY, "socket path changed during stale check", self._path)
        os.unlink(self._path)

    def _unlink_owned_socket(self) -> None:
        identity = self._socket_identity
        self._socket_identity = None
        if identity is None:
            return
        try:
            info = os.lstat(self._path)
            if stat.S_ISSOCK(info.st_mode) and (info.st_dev, info.st_ino) == identity:
                os.unlink(self._path)
        except FileNotFoundError:
            pass
        except OSError as exc:
            log.debug("could not remove owned socket %s: %s", self._path, exc)

    def _replay_state(self) -> None:
        with self._config_lock:
            config = dict(self._config)
        if config:
            self.send({"type": "config", **config})
        self.set_state("idle", **({"corner": True} if config.get("sleeping") else {}))

    def _page_ready(self) -> None:
        # Her body connects before her page listens, so what it was sent on connect went
        # unseen (she pranked with mischief off, the welcome never showed). Send it again.
        log.info("hermes-chan: her window is ready")
        self._replay_state()
        if self.on_connected is not None:
            try:
                self.on_connected()
            except Exception:  # noqa: BLE001 - a greeting must never drop the connection
                log.exception("hermes-chan: on_connected failed")

    def _read_loop(self, conn: socket.socket, initial: bytes = b"") -> None:
        # A bytearray searched only past what was already searched: a multi-MB screenshot
        # line costs one pass instead of one copy and one scan per 4 KB chunk.
        pending = bytearray(initial)
        searched = 0
        try:
            while not self._stop.is_set():
                newline = pending.find(b"\n", searched)
                if newline < 0:
                    if len(pending) > _MAX_INBOUND_BYTES:
                        log.warning("overlay message exceeded %d bytes; dropping connection", _MAX_INBOUND_BYTES)
                        return
                    searched = len(pending)
                    chunk = conn.recv(65536)
                    if not chunk:
                        return
                    pending += chunk
                    continue
                line = bytes(pending[:newline])
                del pending[:newline + 1]
                searched = 0
                if len(line) > _MAX_INBOUND_BYTES:
                    log.warning("overlay message exceeded %d bytes; ignoring it", _MAX_INBOUND_BYTES)
                elif line.strip():
                    self._handle_inbound(line)
        except OSError:
            return

    def _handle_inbound(self, raw: bytes) -> None:
        try:
            msg = json.loads(raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            log.debug("overlay sent malformed line")
            return
        if not isinstance(msg, dict):
            log.debug("overlay sent a non-object message")
            return
        kind = msg.get("type")
        if kind == "ready":
            self._page_ready()
        elif kind == "chat":
            self._inject(msg.get("text", ""), msg.get("session_key"), msg.get("chat_id"))
        elif kind == "chat_check":
            # Her chat box just opened: tell her what (if anything) you still have to do.
            blocked = self.chat_allowed() is False
            self.send({"type": "chat_status", "steps": self.chat_steps(),
                       "copy": _CHAT_COMMAND if blocked else ""})
        elif kind == "action_event":
            self._mark_action_event(msg.get("action_id"), msg.get("event"))
        elif kind == "action_complete":
            self._mark_action_complete(msg.get("action_id"))
        elif kind == "native_result":
            self._mark_native_result(msg.get("action_id"), msg)
        elif kind == "active_window":
            self._handle_active_window(msg)
        elif kind == "intent":
            if msg.get("name") == "deactivate":
                self._deactivated()
            else:
                log.debug("overlay intent: %s", msg.get("name"))

    def _deactivated(self) -> None:
        """Ctrl+Shift+click: the overlay is already leaving; make the rest of the plugin agree."""
        handler = self.deactivate_handler
        if callable(handler):
            threading.Thread(target=handler, name="hermeschan-deactivate", daemon=True).start()

    def _mark_action_event(self, action_id: Any, event: Any) -> None:
        if not isinstance(action_id, str) or not isinstance(event, str) or not event or len(event) > 64:
            return
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is not None:
            pending.mark_event(event)

    def _mark_action_complete(self, action_id: Any) -> None:
        if not isinstance(action_id, str):
            return
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is not None:
            pending.complete.set()

    def _mark_native_result(self, action_id: Any, result: Any) -> None:
        if not isinstance(action_id, str) or not isinstance(result, dict):
            return
        with self._actions_lock:
            pending = self._actions.get(action_id)
        if pending is not None:
            with pending.lock:
                pending.result = dict(result)
                pending.native_result.set()

    def _handle_active_window(self, msg: dict[str, Any]) -> None:
        app = msg.get("app")
        title = msg.get("title", msg.get("window_title"))
        context: dict[str, Any] = {
            "app": app[:256] if isinstance(app, str) else "",
            "title": title[:512] if isinstance(title, str) else "",
        }
        for key in ("x", "y", "w", "h"):
            value = msg.get(key)
            if isinstance(value, int) and not isinstance(value, bool):
                context[key] = value
        request_id = msg.get("request_id")
        with self._window_lock:
            self._active_window = context
            request = self._window_requests.get(request_id) if isinstance(request_id, str) else None
        if request is None:
            return
        result: dict[str, Any] = dict(context)
        if request.get("include_screenshot"):
            screenshot = msg.get("screenshot")
            if isinstance(screenshot, dict):
                encoded = screenshot.get("data")
                mime_type = screenshot.get("mime_type", "image/png")
                if (isinstance(encoded, str) and len(encoded) <= (_MAX_SCREENSHOT_BYTES * 4 // 3 + 8)
                        and mime_type in ("image/png", "image/jpeg")):
                    try:
                        image = base64.b64decode(encoded, validate=True)
                    except (ValueError, binascii.Error):
                        image = b""
                    if 0 < len(image) <= _MAX_SCREENSHOT_BYTES:
                        result.update({"screenshot": image, "mime_type": mime_type})
        with self._window_lock:
            current = self._window_requests.get(request_id)
            if current is request:
                request["result"] = result
                request["event"].set()

    def _inject(self, text: str, session_key: Optional[str], chat_id: Any = None) -> None:
        text = text.strip() if isinstance(text, str) else ""
        if isinstance(session_key, str) and session_key.strip():
            self.remember_session(session_key)
        effective_session = (session_key or self._session_key) if isinstance(session_key, (str, type(None))) else self._session_key
        ctx = self._ctx
        if not text:
            self._chat_result(chat_id, False, "Message was empty.")
            return
        if ctx is None:
            self._chat_result(chat_id, False, "Hermes is not ready to receive messages.")
            return
        try:
            if effective_session:
                accepted = ctx.inject_message(text, role="user", session_key=effective_session)
            else:
                accepted = ctx.inject_message(text, role="user")
            if accepted is False:
                if self.chat_allowed() is False:
                    self._chat_result(chat_id, False, _INJECT_BLOCKED, copy=_CHAT_COMMAND)
                else:
                    self._chat_result(chat_id, False, _INJECT_REFUSED)
                return
            self.set_state("listening")
            self._chat_result(chat_id, True)
        except Exception as exc:  # noqa: BLE001 - never crash the socket reader
            log.warning("inject_message failed (%s)", exc)
            self._chat_result(chat_id, False, "Hermes could not deliver that message.")

    def _chat_result(self, chat_id: Any, ok: bool, error: Optional[str] = None,
                     copy: Optional[str] = None) -> None:
        payload: dict[str, Any] = {"type": "chat_result", "ok": ok}
        if isinstance(chat_id, str) and chat_id:
            payload["chat_id"] = chat_id
        if error:
            payload["error"] = error
        if copy:
            payload["copy"] = copy  # her bubble copies this command when you click it
        self.send(payload)

    def _release_pending(self) -> None:
        with self._actions_lock:
            actions = list(self._actions.values())
            self._actions.clear()
        for pending in actions:
            pending.cancel()
        with self._window_lock:
            requests = list(self._window_requests.values())
            self._window_requests.clear()
        for request in requests:
            request["event"].set()

    def reset_runtime(self) -> None:
        """For a resummon: drop in-flight actions and stale flags a fresh body would inherit."""
        self._release_pending()
        self._agent_busy.clear()
        with self._config_lock:
            self._config["sleeping"] = False

    def chat_allowed(self) -> Optional[bool]:
        """Whether Hermes lets her chat box post into your chats; None when Hermes can't say."""
        check = getattr(self._ctx, "_gateway_injection_allowed", None)
        if not callable(check):
            return None
        try:
            return bool(check())
        except Exception:  # noqa: BLE001 - a status helper, never fatal
            return None
