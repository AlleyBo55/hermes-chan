"""Expressive tools plus confirmation-gated, reversible desktop tidying."""
from __future__ import annotations

import json
import logging
import math

from .desktop_cleanup import _TIDY_ROOT, CleanupError, DesktopTidier

log = logging.getLogger("hermes-chan.tools")

TOOLSET = "hermeschan"
_TIDY_TOOL = "hermeschan_tidy_desktop"
_UNDO_TOOL = "hermeschan_undo_tidy"
_EMOTIONS = ["neutral", "happy", "shy", "surprised", "sulk", "sleepy", "proud", "panic", "love"]
_VISUAL_EFFECTS = ["glitch", "crack", "portal", "bend", "text_shatter",
                   "ripple", "pixel_dissolve", "lightning", "tear", "hijack", "collapse"]


def _files(count: int) -> str:
    return f"{count} Desktop file" + ("" if count == 1 else "s")


def _intensity(value, default: float) -> float:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return default
    return max(0.0, min(1.0, number)) if math.isfinite(number) else default


def register(ctx, bridge, tidier=None) -> None:
    tidier = tidier or DesktopTidier()

    def _say(args):
        text = (args.get("text") or "").strip()
        if not text:
            return "nothing to say"
        bridge.say(text, emotion=args.get("emotion", "neutral"))
        return f"Hermes-chan says: {text}"

    def _emote(args):
        emotion = (args.get("emotion") or "neutral").lower()
        if emotion not in _EMOTIONS:
            emotion = "neutral"
        bridge.emote(emotion)
        return f"Hermes-chan looks {emotion}."

    def _point(args):
        target = {key: args[key] for key in ("app", "window_title", "x", "y")
                  if args.get(key) is not None}
        bridge.point_at(target or {"app": "active"})
        return f"Hermes-chan points at {target or 'the active window'}."

    def _animate_native(verb, target, contact_event, **extra):
        action_id = bridge.animate_action(
            verb, target, defer_native=True, **extra)
        bridge.wait_for_action_event(action_id, contact_event, timeout=0.7)
        bridge.send({"type": "native_action", "action_id": action_id,
                     "verb": verb, "target": target, **extra})
        bridge.wait_for_action_complete(action_id, timeout=0.9)
        bridge.finish_action(action_id)

    def _animate_visual(verb, target, **extra):
        action_id = bridge.animate_action(verb, target, **extra)
        bridge.wait_for_action_complete(action_id, timeout=0.9)
        bridge.finish_action(action_id)

    def _grab(args):
        target = {"app": args.get("app", "active")}
        gesture = args.get("gesture", "drag")
        _animate_native("grab_window", target, "grip", gesture=gesture)
        return f"Hermes-chan grabs the {args.get('app', 'active')} window ({gesture})."

    def _glitch(args):
        target = {"app": args.get("app", "active")}
        intensity = _intensity(args.get("intensity"), 0.5)
        _animate_native("glitch", target, "effect_start", intensity=intensity)
        return f"Hermes-chan draws a playful glitch over {args.get('app', 'active')} (visual only)."

    def _visual_effect(args):
        effect = str(args.get("effect") or "glitch").lower()
        if effect not in _VISUAL_EFFECTS:
            return ("Choose glitch, crack, portal, bend, text_shatter, ripple, pixel_dissolve, lightning, "
                    "tear, hijack, or collapse.")
        if not bridge.is_connected:
            return "Hermes-chan's overlay is not connected."
        target = {"app": args.get("app", "active")}
        intensity = _intensity(args.get("intensity"), 0.75)
        _animate_native("glitch", target, "effect_start", effect=effect, intensity=intensity)
        return f"Hermes-chan plays a {effect} effect over {target['app']} (visual only)."

    def _tidy(args):
        try:
            if args.get("execute") is True:
                result = tidier.execute(str(args.get("token") or ""),
                                        confirm=args.get("confirm") is True)
            else:
                result = tidier.plan(str(args.get("rule") or "by-type"))
        except (CleanupError, OSError) as exc:
            result = {"ok": False, "error": str(exc)}
        if result.get("ok"):
            _animate_visual("tidy_desktop", {"rule": args.get("rule", "by-type"),
                                              "mode": result.get("mode")})
        return json.dumps(result, ensure_ascii=False)

    def _undo_tidy(args):
        try:
            result = tidier.undo(str(args.get("token") or ""),
                                  confirm=args.get("confirm") is True)
        except (CleanupError, OSError) as exc:
            result = {"ok": False, "error": str(exc)}
        return json.dumps(result, ensure_ascii=False)

    def _approve_file_moves(tool_name="", args=None, **_):
        """pre_tool_call: every call that moves Desktop files goes through Hermes' own approval
        prompt. The model can set confirm=true itself, so that flag is not a human yes. Never
        raises: Hermes skips a hook that raises, which would let the call run unapproved."""
        if tool_name not in (_TIDY_TOOL, _UNDO_TOOL):
            return None
        args = args if isinstance(args, dict) else {}
        # Hermes may coerce a "true" string to True after this hook, so match both.
        if tool_name == _TIDY_TOOL and str(args.get("execute")).strip().lower() != "true":
            return None  # planning moves nothing
        token = str(args.get("token") or "")
        try:
            if tool_name == _TIDY_TOOL:
                count = tidier.planned_moves(token)
                if count is None:
                    return {"action": "block", "message": "unknown or expired tidy token; request a new plan"}
                return {"action": "approve", "message": f"moves {_files(count)} into '{_TIDY_ROOT}'"}
            count = tidier.journaled_moves(token)
            if not count:
                return {"action": "block", "message": "no journaled tidy moves found for that token"}
            return {"action": "approve", "message": f"moves {_files(count)} back to where they were"}
        except Exception as exc:  # noqa: BLE001 - fail closed
            return {"action": "block", "message": f"could not check this tidy request ({exc})"}

    tools = [
        ("hermeschan_say", "Speak a short line in Hermes-chan's speech bubble.",
         {"type": "object",
          "properties": {"text": {"type": "string", "description": "What she says"},
                         "emotion": {"type": "string", "enum": _EMOTIONS}},
          "required": ["text"]}, _say),
        ("hermeschan_emote", "Set Hermes-chan's facial expression.",
         {"type": "object",
          "properties": {"emotion": {"type": "string", "enum": _EMOTIONS}},
          "required": ["emotion"]}, _emote),
        ("hermeschan_point_at", "Make Hermes-chan walk over and point at an app/window/point.",
         {"type": "object",
          "properties": {"app": {"type": "string"}, "window_title": {"type": "string"},
                         "x": {"type": "integer"}, "y": {"type": "integer"}}}, _point),
        ("hermeschan_grab_window", "Grab and physically tug the active or named window.",
         {"type": "object",
          "properties": {"app": {"type": "string"},
                         "gesture": {"type": "string", "enum": ["drag", "shake", "throw", "bend"]}}}, _grab),
        ("hermeschan_glitch_window", "Draw a harmless visual glitch effect over a window.",
         {"type": "object",
          "properties": {"app": {"type": "string"},
                         "intensity": {"type": "number", "minimum": 0, "maximum": 1}}}, _glitch),
        ("hermeschan_visual_effect", "Make Hermes-chan play a temporary visual effect over the active app window, including text shatter, portal, and hijack (the app near her glitches out); visual only.",
         {"type": "object",
          "properties": {"effect": {"type": "string", "enum": _VISUAL_EFFECTS},
                         "app": {"type": "string"},
                         "intensity": {"type": "number", "minimum": 0, "maximum": 1}},
          "required": ["effect"]}, _visual_effect),
        (_TIDY_TOOL,
         "Plan desktop tidying by default. Execution needs the returned token and confirm=true, "
         "then the user approves the moves in Hermes' approval prompt.",
         {"type": "object",
          "properties": {
              "rule": {"type": "string", "enum": ["by-type", "by-date", "into-one-folder"]},
              "execute": {"type": "boolean", "default": False},
              "token": {"type": "string", "description": "Token from the current in-process plan"},
              "confirm": {"type": "boolean", "default": False},
          }}, _tidy),
        (_UNDO_TOOL, "Undo one journaled tidy batch; requires its token and confirm=true, "
         "then the user approves it in Hermes' approval prompt.",
         {"type": "object",
          "properties": {
              "token": {"type": "string", "description": "Token returned by tidy execution"},
              "confirm": {"type": "boolean"},
          },
          "required": ["token", "confirm"]}, _undo_tidy),
    ]

    for name, description, schema, handler in tools:
        schema = dict(schema)
        schema.setdefault("description", description)
        ctx.register_tool(name=name, toolset=TOOLSET, schema=schema, handler=handler)
    ctx.register_hook("pre_tool_call", _approve_file_moves)
    log.info("hermes-chan: registered %d tools", len(tools))
