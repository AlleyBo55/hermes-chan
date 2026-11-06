"""Public ``computer_use`` backend powered by Hermes-chan's Rust body.

Hermes keeps approval gating, vision routing, and the model-facing schema above
this backend. Each mutation waits for the matching sprite contact frame, asks
the Rust overlay to perform one native input event (CoreGraphics on macOS,
SendInput on Windows, XTest on Linux/X11), relays the structured result, then
lets the animation finish. No Hermes private driver is imported.
"""
from __future__ import annotations

import base64
import logging
import os
import struct
import subprocess
import sys
import tempfile
from typing import Any, Optional

from tools.computer_use.backend import ActionResult, CaptureResult, ComputerUseBackend

log = logging.getLogger("hermes-chan.backend")


def real_driver_available() -> bool:
    """Native input exists on macOS, Windows, and Linux sessions with an X11 display
    (Wayland sessions qualify through XWayland, for X11 apps)."""
    if sys.platform in ("darwin", "win32"):
        return True
    return sys.platform.startswith("linux") and bool(os.environ.get("DISPLAY"))


def _png_size(data: bytes) -> tuple[int, int]:
    if len(data) >= 24 and data[:8] == b"\x89PNG\r\n\x1a\n":
        return struct.unpack(">II", data[16:24])
    return (0, 0)


class HermechanBackend(ComputerUseBackend):
    def __init__(self, bridge, permission_mode: str = "standard") -> None:
        self._bridge = bridge
        self._permission_mode = permission_mode

    def start(self) -> None:
        self._bridge.set_state("idle")

    def stop(self) -> None:
        return None

    def is_available(self) -> bool:
        return real_driver_available()

    def _result(self, verb: str, payload: dict[str, Any]) -> ActionResult:
        return ActionResult(
            ok=bool(payload.get("ok")),
            action=str(payload.get("action") or verb),
            message=str(payload.get("message") or ""),
            code=str(payload["code"]) if payload.get("code") else None,
            path="hermes-chan-native",
            delivery_mode="foreground",
        )

    # -- capture ---------------------------------------------------------
    def capture(self, mode: str = "som", app: Optional[str] = None,
                pid: Optional[int] = None, window_id: Optional[int] = None) -> CaptureResult:
        del app, pid, window_id
        sample = self._bridge.request_active_window(include_screenshot=True, timeout=3.0)
        image = sample.get("screenshot")
        if isinstance(image, bytes) and image:
            width = int(sample.get("w") or 0)
            height = int(sample.get("h") or 0)
            if width <= 0 or height <= 0:
                width, height = _png_size(image)
            return CaptureResult(
                mode="vision" if mode == "vision" else mode,
                width=width,
                height=height,
                png_b64=base64.b64encode(image).decode("ascii"),
                elements=[],
                app=str(sample.get("app") or ""),
                window_title=str(sample.get("title") or ""),
                png_bytes_len=len(image),
                image_mime_type=str(sample.get("mime_type") or "image/png"),
                note=("Hermes-chan supplies a window image but no accessibility element map; "
                      "use screen coordinates for actions."),
            )
        return self._fallback_capture(mode)

    def _fallback_capture(self, mode: str) -> CaptureResult:
        """Full-screen vision fallback when the overlay is not connected."""
        image = b""
        if sys.platform == "darwin":
            handle = tempfile.NamedTemporaryFile(prefix="hermes-chan-", suffix=".png", delete=False)
            path = handle.name
            handle.close()
            try:
                subprocess.run(
                    ["screencapture", "-x", "-t", "png", path],
                    check=True,
                    timeout=10,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                with open(path, "rb") as stream:
                    image = stream.read(6 * 1024 * 1024 + 1)
                if len(image) > 6 * 1024 * 1024:
                    image = b""
            except (OSError, subprocess.SubprocessError):
                image = b""
            finally:
                try:
                    os.unlink(path)
                except OSError:
                    pass
        width, height = _png_size(image)
        return CaptureResult(
            mode="vision" if mode == "vision" else mode,
            width=width,
            height=height,
            png_b64=base64.b64encode(image).decode("ascii") if image else None,
            elements=[],
            png_bytes_len=len(image),
            image_mime_type="image/png" if image else None,
            note=(("Full-screen fallback; grant Screen Recording and run the Hermes-chan body. "
                   if sys.platform == "darwin" else
                   "Hermes-chan's body is not connected, so no screenshot is available. ")
                  + "No element map is available."),
        )

    # -- synchronized native actions -----------------------------------
    def _act(self, verb: str, animation_target: dict[str, Any], contact_event: str,
             native_input: dict[str, Any]) -> ActionResult:
        action_id = self._bridge.animate_action(verb, animation_target)
        self._bridge.wait_for_action_event(action_id, contact_event, timeout=0.7)
        try:
            payload = self._bridge.request_native_input(
                action_id, verb, native_input, timeout=5.0)
            result = self._result(verb, payload)
            self._bridge.notify_action_result(action_id, result)
            return result
        finally:
            self._bridge.wait_for_action_complete(action_id, timeout=0.9)
            self._bridge.finish_action(action_id)

    def click(self, *, element=None, x=None, y=None, button="left", click_count=1,
              modifiers=None, delivery_mode=None, bring_to_front=False) -> ActionResult:
        del delivery_mode, bring_to_front
        target = {"element": element, "x": x, "y": y}
        native = {"x": x, "y": y, "button": button, "click_count": click_count,
                  "modifiers": modifiers or []}
        return self._act("click", target, "contact", native)

    def drag(self, *, from_element=None, to_element=None, from_xy=None, to_xy=None,
             button="left", modifiers=None, delivery_mode=None,
             bring_to_front=False) -> ActionResult:
        del delivery_mode, bring_to_front
        target = {"from": from_xy or from_element, "to": to_xy or to_element}
        native = {"from_xy": from_xy, "to_xy": to_xy, "button": button,
                  "modifiers": modifiers or []}
        return self._act("drag", target, "grip", native)

    def scroll(self, *, direction, amount=3, element=None, x=None, y=None,
               modifiers=None, delivery_mode=None, bring_to_front=False) -> ActionResult:
        del delivery_mode, bring_to_front
        target = {"element": element, "x": x, "y": y, "direction": direction}
        native = {"x": x, "y": y, "direction": direction, "amount": amount,
                  "modifiers": modifiers or []}
        return self._act("scroll", target, "scroll_contact", native)

    def type_text(self, text: str, *, delivery_mode=None,
                  bring_to_front=False) -> ActionResult:
        del delivery_mode, bring_to_front
        return self._act("type", {"len": len(text or "")}, "left_key_contact",
                         {"text": text})

    def key(self, keys: str, *, delivery_mode=None,
            bring_to_front=False) -> ActionResult:
        del delivery_mode, bring_to_front
        return self._act("key", {"keys": keys}, "contact", {"keys": keys})

    def list_apps(self) -> list[dict[str, Any]]:
        window = self._bridge.latest_active_window()
        if not window.get("app"):
            return []
        return [{"name": window["app"], "window_count": 1}]

    def list_windows(self) -> list[dict[str, Any]]:
        window = self._bridge.latest_active_window()
        if not window.get("app"):
            return []
        return [dict(window)]

    def focus_app(self, app: str, raise_window: bool = False) -> ActionResult:
        return self._act(
            "focus_app",
            {"app": app, "raise_window": raise_window},
            "point_target",
            {"app": app, "raise_window": raise_window},
        )

    def set_value(self, value: str, element: Optional[int] = None) -> ActionResult:
        return self._act(
            "set_value",
            {"element": element, "len": len(value or "")},
            "left_key_contact",
            {"text": value, "element": element},
        )
