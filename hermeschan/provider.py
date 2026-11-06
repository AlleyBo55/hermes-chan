"""Computer-use provider factory for Hermechan's animated real actions."""
from __future__ import annotations

from typing import Optional

from tools.computer_use.backend import ComputerUseBackend, ComputerUseProvider

from .backend import HermechanBackend, real_driver_available
from .bridge import Bridge, live_bridge


class HermechanProvider(ComputerUseProvider):
    name = "hermes-chan"
    display_name = "Hermes-chan (animated body)"

    def __init__(self, bridge: Optional[Bridge] = None) -> None:
        self._own_bridge = bridge

    @property
    def _bridge(self):
        # Loaded by Hermes' computer-use loader, this module has its own Bridge class: drive the
        # bridge the general plugin load started. With no body at all, a bridge that is never
        # started makes every action fail cleanly as body_disconnected.
        bridge = self._own_bridge or live_bridge()
        return bridge if bridge is not None else Bridge()

    def create_backend(self, *, permission_mode: str) -> ComputerUseBackend:
        return HermechanBackend(self._bridge, permission_mode=permission_mode)

    def is_available(self) -> bool:
        # Expressive tools register independently; this provider promises real input.
        return real_driver_available()

    def doctor(self):
        print(f"hermes-chan: overlay {'connected' if self._bridge.is_connected else 'not connected'} "
              f"(bridge {self._bridge.path})")
        available = real_driver_available()
        if available:
            print("hermes-chan: native input available — real screen control enabled")
        else:
            print("hermes-chan: native input unavailable here (Linux needs an X11 display) — animations still work")
        return 0 if available else 1
