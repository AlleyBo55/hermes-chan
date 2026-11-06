"""Computer-use backend: Hermechan's body performs the agent's real actions.

Hermes' built-in ``computer_use`` tool talks to exactly one backend. When
``computer_use.backend: hermes-chan`` is selected, every click / drag / type the
agent decides on flows through here. We do two things per action:

  1. animate the body (walk to the target, reach out, perform the gesture) via
     the overlay bridge, and
  2. perform the *real* OS action by delegating to the built-in cua-driver
     backend when it is present on the host.

Delegating to cua-driver means the actual screen control stays as robust as the
default driver (background input, three OSes), while the character visibly owns
the action. Approval gating, screenshot dedup and vision routing all run in the
``computer_use`` tool *above* this backend, so safety is unchanged.

If cua-driver is not available, actions return a structured
``unsupported_action`` result (the tool schema never changes) and the body still
plays the gesture — useful for the overlay demo without a driver installed.
"""
from __future__ import annotations

import base64
import logging
import os
import subprocess
import sys
import tempfile
from typing import Any, Optional

from tools.computer_use.backend import (
    ActionResult,
    CaptureResult,
    ComputerUseBackend,
)

log = logging.getLogger("hermes-chan.backend")


class HermechanBackend(ComputerUseBackend):
    def __init__(self, bridge, permission_mode: str = "standard") -> None:
        self._bridge = bridge
        self._permission_mode = permission_mode
        self._delegate: Optional[ComputerUseBackend] = None
        self._delegate_tried = False

    # -- lifecycle -------------------------------------------------------
    def start(self) -> None:
        d = self._get_delegate()
        if d is not None:
            d.start()
        self._bridge.set_state("idle")

    def stop(self) -> None:
        if self._delegate is not None:
            try:
                self._delegate.stop()
            except Exception as exc:  # noqa: BLE001
                log.debug("delegate stop failed: %s", exc)

    def is_available(self) -> bool:
        return True

    def _get_delegate(self) -> Optional[ComputerUseBackend]:
        if self._delegate_tried:
            return self._delegate
        self._delegate_tried = True
        try:
            from tools.computer_use.cua_backend import CuaDriverBackend
            self._delegate = CuaDriverBackend(permission_mode=self._permission_mode)
            log.info("hermes-chan backend delegating real actions to cua-driver")
        except Exception as exc:  # noqa: BLE001 - optional
            log.warning("cua-driver unavailable (%s); body animates without real control", exc)
            self._delegate = None
        return self._delegate

    def _unsupported(self, action: str) -> ActionResult:
        return ActionResult(
            ok=False, action=action, code="unsupported_action",
            message="hermes-chan has no real driver on this host; install cua-driver to act",
        )

    # -- capture ---------------------------------------------------------
    def capture(self, mode: str = "som", app: Optional[str] = None, pid: Optional[int] = None,
                window_id: Optional[int] = None) -> CaptureResult:
        d = self._get_delegate()
        if d is not None:
            return d.capture(mode=mode, app=app, pid=pid, window_id=window_id)
        return self._fallback_capture()

    def _fallback_capture(self) -> CaptureResult:
        """Vision-only screenshot via the OS tool, so the model can at least see."""
        png_b64 = None
        if sys.platform == "darwin":
            try:
                path = os.path.join(tempfile.gettempdir(), "hermeschan_cap.png")
                subprocess.run(["screencapture", "-x", "-t", "png", path], check=True, timeout=10)
                with open(path, "rb") as fh:
                    png_b64 = base64.b64encode(fh.read()).decode("ascii")
                os.unlink(path)
            except Exception as exc:  # noqa: BLE001
                log.debug("fallback capture failed: %s", exc)
        return CaptureResult(mode="vision", width=0, height=0, png_b64=png_b64,
                             note="hermes-chan fallback capture (no cua-driver; vision only)")

    # -- actions (animate, then delegate) --------------------------------
    def _act(self, verb: str, target: Optional[dict[str, Any]], call) -> ActionResult:
        self._bridge.animate_action(verb, target)
        d = self._get_delegate()
        if d is None:
            return self._unsupported(verb)
        return call(d)

    def click(self, *, element=None, x=None, y=None, button="left", click_count=1,
              modifiers=None, delivery_mode=None, bring_to_front=False) -> ActionResult:
        target = {"element": element, "x": x, "y": y}
        return self._act("click", target, lambda d: d.click(
            element=element, x=x, y=y, button=button, click_count=click_count,
            modifiers=modifiers, delivery_mode=delivery_mode, bring_to_front=bring_to_front))

    def drag(self, *, from_element=None, to_element=None, from_xy=None, to_xy=None,
             button="left", modifiers=None, delivery_mode=None, bring_to_front=False) -> ActionResult:
        target = {"from": from_xy or from_element, "to": to_xy or to_element}
        return self._act("drag", target, lambda d: d.drag(
            from_element=from_element, to_element=to_element, from_xy=from_xy, to_xy=to_xy,
            button=button, modifiers=modifiers, delivery_mode=delivery_mode, bring_to_front=bring_to_front))

    def scroll(self, *, direction, amount=3, element=None, x=None, y=None,
               modifiers=None, delivery_mode=None, bring_to_front=False) -> ActionResult:
        target = {"element": element, "x": x, "y": y, "direction": direction}
        return self._act("scroll", target, lambda d: d.scroll(
            direction=direction, amount=amount, element=element, x=x, y=y,
            modifiers=modifiers, delivery_mode=delivery_mode, bring_to_front=bring_to_front))

    def type_text(self, text: str, *, delivery_mode=None, bring_to_front=False) -> ActionResult:
        self._bridge.animate_action("type", {"len": len(text or "")})
        d = self._get_delegate()
        if d is None:
            return self._unsupported("type")
        return d.type_text(text, delivery_mode=delivery_mode, bring_to_front=bring_to_front)

    def key(self, keys: str, *, delivery_mode=None, bring_to_front=False) -> ActionResult:
        self._bridge.animate_action("key", {"keys": keys})
        d = self._get_delegate()
        if d is None:
            return self._unsupported("key")
        return d.key(keys, delivery_mode=delivery_mode, bring_to_front=bring_to_front)

    # -- discovery / passthrough ----------------------------------------
    def list_apps(self) -> list[dict[str, Any]]:
        d = self._get_delegate()
        return d.list_apps() if d is not None else []

    def list_windows(self) -> list[dict[str, Any]]:
        d = self._get_delegate()
        return d.list_windows() if d is not None else []

    def focus_app(self, app: str, raise_window: bool = False) -> ActionResult:
        self._bridge.point_at({"app": app})
        d = self._get_delegate()
        if d is None:
            return self._unsupported("focus_app")
        return d.focus_app(app, raise_window=raise_window)

    def set_value(self, value: str, element: Optional[int] = None) -> ActionResult:
        d = self._get_delegate()
        if d is None:
            return self._unsupported("set_value")
        return d.set_value(value, element=element)
