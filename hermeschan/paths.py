"""Where Hermes-chan keeps her files, inside the active Hermes profile."""
from __future__ import annotations

import os
import sys
from pathlib import Path


def hermes_home() -> Path:
    """Hermes' own answer (profile-aware), resolved at call time."""
    try:
        from hermes_constants import get_hermes_home
        return Path(get_hermes_home())
    except Exception:  # noqa: BLE001 - outside Hermes (scripts, tests): mirror its defaults
        configured = os.environ.get("HERMES_HOME", "").strip()
        if configured:
            return Path(configured).expanduser()
        if sys.platform == "win32":
            local = os.environ.get("LOCALAPPDATA", "").strip()
            return (Path(local) if local else Path.home() / "AppData" / "Local") / "hermes"
        return Path.home() / ".hermes"


def state_dir() -> Path:
    """Settings, journals, logs and the Windows bridge file."""
    return hermes_home() / "hermes-chan"


def private_file(fd: int) -> None:
    """Owner-only permissions where the OS has them (os.fchmod is POSIX before 3.13)."""
    fchmod = getattr(os, "fchmod", None)
    if fchmod is not None and sys.platform != "win32":
        fchmod(fd, 0o600)
