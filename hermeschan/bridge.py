"""Duplex bridge between the Python plugin and the desktop overlay.

The plugin owns an ``AF_UNIX`` stream socket. Newline-delimited JSON carries
animation commands to the overlay and chat/action acknowledgements back.
Missing overlays always degrade to immediate no-ops.
"""
from __future__ import annotations

import base64
import binascii
import errno
import json
import logging
import os
import socket
import stat
import threading
import uuid
from collections import OrderedDict
from pathlib import Path
from typing import Any, Optional

log = logging.getLogger("hermes-chan.bridge")

_MAX_PENDING_ACTIONS = 64
_MAX_ACTION_EVENTS = 16
_MAX_WINDOW_REQUESTS = 4
_MAX_INBOUND_BYTES = 10 * 1024 * 1024
_MAX_SCREENSHOT_BYTES = 6 * 1024 * 1024


def _default_socket_path() -> str:
    env = os.environ.get("HERMESCHAN_SOCKET")
    if env:
        return env
    home = os.environ.get("HERMES_HOME") or str(Path.home() / ".hermes")
    return str(Path(home) / "hermes-chan.sock")


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
        self._path = _default_socket_path()
        self._server: Optional[socket.socket] = None
        self._client: Optional[socket.socket] = None
        self._write_lock = threading.RLock()
        self._start_lock = threading.Lock()
        self._started = False
        self._stop = threading.Event()
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
            elif ctx is not None:
                cls._instance._ctx = ctx
            return cls._instance

    @property
    def path(self) -> str:
        return self._path

    @property
    def is_connected(self) -> bool:
        return self._client is not None

    @property
    def agent_busy(self) -> bool:
        return self._agent_busy.is_set()

    def set_agent_busy(self, busy: bool) -> None:
        if busy:
            self._agent_busy.set()
        else:
            self._agent_busy.clear()

    def remember_session(self, session_key: Optional[str]) -> None:
        value = (session_key or "").strip()
        if value:
            self._session_key = value

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
        thread = threading.Thread(target=self._serve, name="hermeschan-bridge", daemon=True)
        thread.start()

    def stop(self) -> None:
        self._stop.set()
        with self._write_lock:
            client, server = self._client, self._server
            self._client = None
            self._server = None
        for sock in (client, server):
            if sock is not None:
                try:
                    sock.close()
                except OSError:
                    pass
        self._release_pending()
        self._unlink_owned_socket()
        with self._start_lock:
            self._started = False

    def _serve(self) -> None:
        srv: Optional[socket.socket] = None
        try:
            Path(self._path).parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            self._remove_stale_socket()
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(self._path)
            os.chmod(self._path, 0o600)
            info = os.lstat(self._path)
            self._socket_identity = (info.st_dev, info.st_ino)
            srv.listen(1)
            self._server = srv
            log.info("hermes-chan bridge listening on %s", self._path)
            while not self._stop.is_set():
                try:
                    conn, _ = srv.accept()
                except OSError:
                    break
                with self._write_lock:
                    self._client = conn
                self._replay_state()
                self._read_loop(conn)
                with self._write_lock:
                    if self._client is conn:
                        self._client = None
                try:
                    conn.close()
                except OSError:
                    pass
                self._release_pending()
        except OSError as exc:
            log.warning("hermes-chan bridge could not bind %s (%s); body disabled", self._path, exc)
        finally:
            if srv is not None:
                try:
                    srv.close()
                except OSError:
                    pass
            self._server = None
            self._unlink_owned_socket()
            with self._start_lock:
                self._started = False

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

    def _read_loop(self, conn: socket.socket) -> None:
        buf = b""
        try:
            while not self._stop.is_set():
                chunk = conn.recv(4096)
                if not chunk:
                    return
                buf += chunk
                if len(buf) > _MAX_INBOUND_BYTES and b"\n" not in buf:
                    log.warning("overlay message exceeded %d bytes; dropping connection", _MAX_INBOUND_BYTES)
                    return
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
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
        if kind == "chat":
            self._inject(msg.get("text", ""), msg.get("session_key"), msg.get("chat_id"))
        elif kind == "action_event":
            self._mark_action_event(msg.get("action_id"), msg.get("event"))
        elif kind == "action_complete":
            self._mark_action_complete(msg.get("action_id"))
        elif kind == "native_result":
            self._mark_native_result(msg.get("action_id"), msg)
        elif kind == "active_window":
            self._handle_active_window(msg)
        elif kind == "intent":
            log.debug("overlay intent: %s", msg.get("name"))

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
                self._chat_result(chat_id, False, "Hermes did not accept the message for this session.")
                return
            self.set_state("listening")
            self._chat_result(chat_id, True)
        except Exception as exc:  # noqa: BLE001 - never crash the socket reader
            log.warning("inject_message failed (%s)", exc)
            self._chat_result(chat_id, False, "Hermes could not deliver that message.")

    def _chat_result(self, chat_id: Any, ok: bool, error: Optional[str] = None) -> None:
        payload: dict[str, Any] = {"type": "chat_result", "ok": ok}
        if isinstance(chat_id, str) and chat_id:
            payload["chat_id"] = chat_id
        if error:
            payload["error"] = error
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
