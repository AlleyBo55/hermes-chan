"""Persistent, non-secret Hermes-chan settings."""
from __future__ import annotations

import json
import os
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Optional

from .paths import private_file, state_dir

_MIN_HEARTBEAT_SECONDS = 60.0
_DEFAULTS: dict[str, Any] = {
    # Sent away (Ctrl+Shift+click, /hermeschan off) while the Hermes process showing her runs.
    # The next process to take her over clears it, so a restart or plugin reload brings her back.
    "away": False,
    "mischief": False,
    "watch": False,
    "sleeping": False,
    "heartbeat_enabled": False,
    "heartbeat_interval": 120.0,
    "decision_times": [],
}


class SettingsStore:
    """Small atomic JSON store. Values are preferences and rate-limit timestamps only."""

    def __init__(self, path: Optional[Path] = None) -> None:
        self.path = path or (state_dir() / "settings.json")
        self._lock = threading.RLock()
        self._data = self._load()

    def snapshot(self) -> dict[str, Any]:
        with self._lock:
            data = dict(self._data)
            data["decision_times"] = list(data.get("decision_times", []))
            return data

    def get(self, key: str, default: Any = None) -> Any:
        with self._lock:
            value = self._data.get(key, default)
            return list(value) if key == "decision_times" and isinstance(value, list) else value

    def refresh(self) -> "SettingsStore":
        """Re-read the file: every Hermes process of this profile writes it."""
        with self._lock:
            self._data = self._load()
        return self

    def update(self, **changes: Any) -> dict[str, Any]:
        with self._lock:
            # Read-modify-write, so a stale copy here never undoes another process's change.
            merged = self._load()
            for key, value in changes.items():
                if key not in _DEFAULTS:
                    raise KeyError(f"unknown Hermes-chan setting: {key}")
                merged[key] = value
            self._data = self._validated(merged)
            self._write_locked()
            return self.snapshot()

    def claim_decision_slot(self, *, limit: int = 10, window_seconds: float = 3600.0) -> bool:
        """Atomically consume one persisted autonomy decision slot."""
        now = time.time()
        with self._lock:
            self._data = self._load()
            recent = [stamp for stamp in self._data.get("decision_times", [])
                      if isinstance(stamp, (int, float)) and now - window_seconds < stamp <= now + 5.0]
            if len(recent) >= limit:
                if recent != self._data.get("decision_times", []):
                    self._data["decision_times"] = recent
                    self._write_locked()
                return False
            recent.append(now)
            self._data["decision_times"] = recent
            self._write_locked()
            return True

    def _load(self) -> dict[str, Any]:
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            raw = {}
        return self._validated(raw if isinstance(raw, dict) else {})

    @staticmethod
    def _validated(raw: dict[str, Any]) -> dict[str, Any]:
        data = dict(_DEFAULTS)
        for key in ("away", "mischief", "watch", "sleeping", "heartbeat_enabled"):
            if isinstance(raw.get(key), bool):
                data[key] = raw[key]
        interval = raw.get("heartbeat_interval")
        if isinstance(interval, (int, float)) and not isinstance(interval, bool):
            data["heartbeat_interval"] = max(_MIN_HEARTBEAT_SECONDS, float(interval))
        times = raw.get("decision_times")
        if isinstance(times, list):
            now = time.time()
            data["decision_times"] = [float(stamp) for stamp in times[-10:]
                                      if isinstance(stamp, (int, float))
                                      and now - 3600.0 < float(stamp) <= now + 5.0]
        return data

    def _write_locked(self) -> None:
        parent = self.path.parent
        parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, temporary = tempfile.mkstemp(prefix=".settings-", suffix=".tmp", dir=str(parent))
        try:
            private_file(fd)
            with os.fdopen(fd, "w", encoding="utf-8") as handle:
                fd = -1
                json.dump(self._data, handle, indent=2, sort_keys=True)
                handle.write("\n")
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, self.path)
            os.chmod(self.path, 0o600)
        finally:
            if fd >= 0:
                os.close(fd)
            try:
                os.unlink(temporary)
            except FileNotFoundError:
                pass
