"""Duplex bridge between the Python plugin and the Rust overlay body.

The plugin is the **server** on an ``AF_UNIX`` stream socket; the overlay
connects to it. Messages are newline-delimited JSON, both directions:

  plugin -> overlay   {"type": "state",  "state": "thinking"}
                      {"type": "say",    "text": "...", "emotion": "happy"}
                      {"type": "action", "verb": "click", "target": {...}}
  overlay -> plugin   {"type": "chat",   "text": "hey chan, what's this file?"}
                      {"type": "intent", "name": "come_here"}

Everything is best-effort: if the socket cannot bind, or no overlay is
connected, :meth:`send` is a silent no-op so the agent is never blocked by a
missing body.
"""
from __future__ import annotations

import json
import logging
import os
import socket
import threading
from pathlib import Path
from typing import Any, Optional

log = logging.getLogger("hermes-chan.bridge")


def _default_socket_path() -> str:
    env = os.environ.get("HERMESCHAN_SOCKET")
    if env:
        return env
    home = os.environ.get("HERMES_HOME") or str(Path.home() / ".hermes")
    return str(Path(home) / "hermes-chan.sock")


class Bridge:
    """Singleton duplex link to the overlay. Thread-safe writes."""

    _instance: Optional["Bridge"] = None
    _instance_lock = threading.Lock()

    def __init__(self, ctx=None) -> None:
        self._ctx = ctx
        self._path = _default_socket_path()
        self._server: Optional[socket.socket] = None
        self._client: Optional[socket.socket] = None
        self._write_lock = threading.Lock()
        self._started = False

    # -- singleton -------------------------------------------------------
    @classmethod
    def shared(cls, ctx=None) -> "Bridge":
        with cls._instance_lock:
            if cls._instance is None:
                cls._instance = cls(ctx)
            elif ctx is not None:
                cls._instance._ctx = ctx
            return cls._instance

    # -- outbound (plugin -> overlay) -----------------------------------
    def send(self, obj: dict[str, Any]) -> None:
        client = self._client
        if client is None:
            return
        try:
            line = (json.dumps(obj, separators=(",", ":")) + "\n").encode("utf-8")
            with self._write_lock:
                client.sendall(line)
        except OSError as exc:
            log.debug("overlay write failed (%s); dropping client", exc)
            self._client = None

    def set_state(self, state: str, **extra: Any) -> None:
        self.send({"type": "state", "state": state, **extra})

    def say(self, text: str, emotion: str = "neutral") -> None:
        self.send({"type": "say", "text": text, "emotion": emotion})

    def emote(self, emotion: str) -> None:
        self.send({"type": "emote", "emotion": emotion})

    def point_at(self, target: dict[str, Any]) -> None:
        self.send({"type": "point", "target": target})

    def animate_action(self, verb: str, target: Optional[dict[str, Any]] = None, **extra: Any) -> None:
        """Ask the body to play the approach+perform animation for a desktop action."""
        self.send({"type": "action", "verb": verb, "target": target or {}, **extra})

    # -- inbound (overlay -> plugin) ------------------------------------
    def start_inbound(self, ctx) -> None:
        """Bind the socket and accept the overlay connection on a daemon thread."""
        self._ctx = ctx
        if self._started:
            return
        self._started = True
        t = threading.Thread(target=self._serve, name="hermeschan-bridge", daemon=True)
        t.start()

    def _serve(self) -> None:
        try:
            if os.path.exists(self._path):
                os.unlink(self._path)
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(self._path)
            srv.listen(1)
            self._server = srv
            log.info("hermes-chan bridge listening on %s", self._path)
        except OSError as exc:
            log.warning("hermes-chan bridge could not bind %s (%s); body disabled", self._path, exc)
            self._started = False
            return

        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            self._client = conn
            self.set_state("idle")
            self._read_loop(conn)
            self._client = None

    def _read_loop(self, conn: socket.socket) -> None:
        buf = b""
        try:
            while True:
                chunk = conn.recv(4096)
                if not chunk:
                    return
                buf += chunk
                while b"\n" in buf:
                    line, buf = buf.split(b"\n", 1)
                    if line.strip():
                        self._handle_inbound(line)
        except OSError:
            return

    def _handle_inbound(self, raw: bytes) -> None:
        try:
            msg = json.loads(raw.decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            log.debug("overlay sent malformed line")
            return
        kind = msg.get("type")
        if kind == "chat":
            self._inject(msg.get("text", ""), msg.get("session_key"))
        elif kind == "intent":
            # Reserved: overlay-driven intents (come_here, sleep, ...) handled by hooks/autonomy.
            log.debug("overlay intent: %s", msg.get("name"))

    def _inject(self, text: str, session_key: Optional[str]) -> None:
        text = (text or "").strip()
        ctx = self._ctx
        if not text or ctx is None:
            return
        try:
            # Typed into the bubble -> a normal user turn in the live session.
            if session_key:
                ctx.inject_message(text, role="user", session_key=session_key)
            else:
                ctx.inject_message(text, role="user")
            self.set_state("listening")
        except Exception as exc:  # noqa: BLE001 - never crash the reader thread
            log.warning("inject_message failed (%s)", exc)
