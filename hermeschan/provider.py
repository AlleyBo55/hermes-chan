"""Computer-use provider factory for Hermechan's body."""
from __future__ import annotations

import sys

from tools.computer_use.backend import ComputerUseBackend, ComputerUseProvider

from .backend import HermechanBackend


class HermechanProvider(ComputerUseProvider):
    name = "hermes-chan"
    display_name = "Hermechan (animated body)"

    def __init__(self, bridge) -> None:
        self._bridge = bridge

    def create_backend(self, *, permission_mode: str) -> ComputerUseBackend:
        return HermechanBackend(self._bridge, permission_mode=permission_mode)

    def is_available(self) -> bool:
        # The character runs anywhere; real control is best on macOS/Windows/Linux.
        return sys.platform in ("darwin", "win32", "linux")

    def doctor(self):
        connected = self._bridge._client is not None  # noqa: SLF001
        print(f"hermes-chan: overlay {'connected' if connected else 'not connected'} "
              f"(socket {self._bridge._path})")
        try:
            from tools.computer_use.cua_backend import CuaDriverBackend  # noqa: F401
            print("hermes-chan: cua-driver present — real screen control available")
        except Exception:  # noqa: BLE001
            print("hermes-chan: cua-driver missing — body animates only (install it to act)")
        return 0
