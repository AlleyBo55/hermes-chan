"""Computer-use provider factory for Hermechan's animated real actions."""
from __future__ import annotations

from tools.computer_use.backend import ComputerUseBackend, ComputerUseProvider

from .backend import HermechanBackend, real_driver_available


class HermechanProvider(ComputerUseProvider):
    name = "hermes-chan"
    display_name = "Hermes-chan (animated body)"

    def __init__(self, bridge) -> None:
        self._bridge = bridge

    def create_backend(self, *, permission_mode: str) -> ComputerUseBackend:
        return HermechanBackend(self._bridge, permission_mode=permission_mode)

    def is_available(self) -> bool:
        # Expressive tools register independently; this provider promises real input.
        return real_driver_available()

    def doctor(self):
        print(f"hermes-chan: overlay {'connected' if self._bridge.is_connected else 'not connected'} "
              f"(socket {self._bridge.path})")
        available = real_driver_available()
        if available:
            print("hermes-chan: native input available — real screen control enabled")
        else:
            print("hermes-chan: native input unavailable here (Linux needs an X11 display) — animations still work")
        return 0 if available else 1
