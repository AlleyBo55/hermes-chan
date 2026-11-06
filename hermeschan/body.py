"""Switching Hermes-chan's body on and off.

"On" means this process owns the bridge and supervises the overlay; "off" means
neither runs. The choice persists in settings, so Ctrl+Shift+click or
``/hermeschan off`` keeps her away until ``/hermeschan on``. Disabling the
plugin in Hermes unloads it, which shuts everything down too.
"""
from __future__ import annotations

import logging
import threading
from typing import Optional

log = logging.getLogger("hermes-chan.body")

# Short-lived Hermes commands (`hermes plugins install --enable`, validation) load
# plugins too; waiting a moment keeps them from flashing a body on screen.
_START_DELAY_SECONDS = 2.0


class Body:
    def __init__(self, ctx, bridge, runner, settings, autonomy) -> None:
        self._ctx = ctx
        self._bridge = bridge
        self._runner = runner
        self._settings = settings
        self._autonomy = autonomy
        self._cancel = threading.Event()
        self._starter: Optional[threading.Thread] = None
        self._lock = threading.Lock()

    @property
    def runner(self):
        return self._runner

    def is_on(self) -> bool:
        return bool(self._settings.get("active", True))

    def start(self, *, delay: float = 0.0) -> None:
        """Bring her up (if switched on) without blocking the caller."""
        with self._lock:
            self._cancel.clear()
            if self._starter is not None and self._starter.is_alive():
                return
            self._starter = threading.Thread(target=self._start_after, args=(delay,),
                                             name="hermeschan-start", daemon=True)
            self._starter.start()

    def start_soon(self) -> None:
        self.start(delay=_START_DELAY_SECONDS)

    def _start_after(self, delay: float) -> None:
        if delay and self._cancel.wait(delay):
            return
        if self._cancel.is_set() or not self.is_on():
            return
        self._bridge.start_inbound(self._ctx)
        self._runner.maybe_start(self._ctx)

    def stop(self) -> None:
        self._cancel.set()
        self._runner.stop()
        self._bridge.stop()

    def switch_on(self) -> None:
        self._settings.update(active=True)
        self.start()

    def switch_off(self) -> None:
        self._settings.update(active=False)
        self.stop()

    def switched_off_from_overlay(self) -> None:
        """Ctrl+Shift+click: the overlay already left; remember it and stop supervising."""
        log.info("hermes-chan: switched off from her body; /hermeschan on brings her back")
        self.switch_off()

    def shutdown(self) -> None:
        """Plugin unload or interpreter exit: stop everything, keep the on/off choice."""
        self._autonomy.stop()
        self.stop()
