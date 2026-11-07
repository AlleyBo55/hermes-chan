"""Switching Hermes-chan's body on and off.

The lasting switch is the plugin's own switch in Hermes: disabling the plugin
unloads it and shuts everything down. Inside a running Hermes she can also be
sent away (Ctrl+Shift+click, closing her window, ``/hermeschan off``): her
process ends, but this process keeps the bridge, so no other Hermes process
brings her straight back. She returns with ``/hermeschan on`` from any Hermes
process, or when the bridge changes hands: Hermes restarts, or the plugin is
switched off and on.
"""
from __future__ import annotations

import logging
import threading
from typing import Optional

log = logging.getLogger("hermes-chan.body")


def _forget_overlay_layout() -> None:
    """Her saved spot and follow mode (written by the overlay); a fresh body starts roaming."""
    from .paths import state_dir

    try:
        (state_dir() / "overlay.json").unlink()
    except FileNotFoundError:
        pass
    except OSError as exc:
        log.warning("hermes-chan: could not reset her saved position (%s)", exc)

# Short-lived Hermes commands (`hermes plugins install --enable`, validation) load
# plugins too; waiting a moment keeps them from flashing a body on screen.
_START_DELAY_SECONDS = 2.0
# How often the process showing her checks for `/hermeschan on|off` typed in another process.
_WATCH_SECONDS = 2.0


class Body:
    def __init__(self, ctx, bridge, runner, settings, autonomy) -> None:
        self._ctx = ctx
        self._bridge = bridge
        self._runner = runner
        self._settings = settings
        self._autonomy = autonomy
        self._cancel = threading.Event()
        self._starter: Optional[threading.Thread] = None
        self._watcher: Optional[threading.Thread] = None
        self._seen_away = False
        self._seen_resummons = int(settings.get("resummons", 0))
        self._lock = threading.Lock()

    @property
    def runner(self):
        return self._runner

    def is_on(self) -> bool:
        return not self._settings.get("away", False)

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
        if self._cancel.is_set():
            return
        if self._bridge.is_listening:
            # This process already shows her: `/hermeschan on` after she was sent away.
            self._runner.maybe_start(self._ctx)
        else:
            self._bridge.start_inbound(self._ctx)  # took_over() runs once the bridge is ours

    def took_over(self) -> None:
        """This process now owns the bridge. A new owner is a fresh start, so an earlier
        send-away (from a process that has since exited or reloaded) is over."""
        self._settings.update(away=False)
        self._seen_away = False
        if self._cancel.is_set():
            return
        self._runner.maybe_start(self._ctx)
        with self._lock:
            if self._watcher is None or not self._watcher.is_alive():
                self._watcher = threading.Thread(target=self._watch, name="hermeschan-watch", daemon=True)
                self._watcher.start()

    def _watch(self) -> None:
        """While this process shows her, follow `/hermeschan on|off` typed in other processes.
        Only a change of the flag counts, so her own exit after Ctrl+Shift+click never
        races the flag into restarting her."""
        while not self._cancel.wait(_WATCH_SECONDS):
            self._follow_mischief_setting()
            current = self._settings.refresh().snapshot()
            if current["resummons"] != self._seen_resummons:
                self._resummon_here()  # also re-syncs away
                continue
            away = bool(current["away"])
            if away == self._seen_away:
                continue
            self._seen_away = away
            if away:
                self._runner.stop()
            else:
                self._runner.maybe_start(self._ctx)

    def _follow_mischief_setting(self) -> None:
        """The mischief switch in Hermes' plugin settings (or `hermes config set …`) changes
        without telling the plugin; pass a change on to her within a couple of seconds."""
        from .settings import mischief_setting

        try:
            enabled = mischief_setting(self._ctx)
        except Exception as exc:  # noqa: BLE001 - an unreadable config must not stop the watch
            log.debug("hermes-chan: could not read the mischief setting (%s)", exc)
            return
        if self._settings.get("mischief") != enabled:
            self._settings.update(mischief=enabled)
            self._bridge.update_config(mischief=enabled)

    def resummon(self) -> bool:
        """Bring her back fresh. True when this process restarted her itself; False when
        another Hermes process shows her and will do it within a couple of seconds."""
        if self._bridge.is_listening:
            self._resummon_here()
            return True
        count = int(self._settings.refresh().get("resummons", 0))
        self._settings.update(resummons=count + 1, away=False, sleeping=False)
        return False

    def _resummon_here(self) -> None:
        """Clear every state that can keep her stuck or hidden, then restart her body.
        Opted-in preferences (mischief, watch, heartbeat) stay as they are."""
        current = self._settings.update(away=False, sleeping=False)
        self._seen_resummons = current["resummons"]
        self._seen_away = False
        self._bridge.reset_runtime()
        self._cancel.clear()
        log.info("hermes-chan: resummoned; restarting her body fresh")
        self._runner.restart(before_start=_forget_overlay_layout)

    def stop(self) -> None:
        self._cancel.set()
        self._runner.stop()
        self._bridge.stop()

    def switch_on(self) -> None:
        self._settings.update(away=False)
        self._seen_away = False
        self.start()

    def switch_off(self) -> None:
        """Send her away; this process keeps the bridge so nobody else brings her back."""
        self._settings.update(away=True)
        self._seen_away = True
        self._runner.stop()

    def switched_off_from_overlay(self) -> None:
        """Ctrl+Shift+click: the overlay already left; remember it and stop supervising."""
        log.info("hermes-chan: sent away from her body; /hermeschan on, a plugin off/on or "
                 "a Hermes restart brings her back")
        self.switch_off()

    def shutdown(self) -> None:
        """Plugin unload or interpreter exit: stop everything and let the bridge go."""
        self._autonomy.stop()
        self.stop()
