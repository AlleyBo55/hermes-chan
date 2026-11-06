"""Expressive tools plus confirmation-gated, reversible desktop tidying."""
from __future__ import annotations

import json
import logging

from .desktop_cleanup import CleanupError, DesktopTidier

log = logging.getLogger("hermes-chan.tools")

TOOLSET = "hermeschan"
_EMOTIONS = ["neutral", "happy", "shy", "surprised", "sulk", "sleepy", "proud", "panic", "love"]


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
        intensity = float(args.get("intensity", 0.5))
        _animate_native("glitch", target, "effect_start", intensity=intensity)
        return f"Hermes-chan draws a playful glitch over {args.get('app', 'active')} (visual only)."

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
        ("hermeschan_tidy_desktop",
         "Plan desktop tidying by default. Execution needs the returned token and confirm=true.",
         {"type": "object",
          "properties": {
              "rule": {"type": "string", "enum": ["by-type", "by-date", "into-one-folder"]},
              "execute": {"type": "boolean", "default": False},
              "token": {"type": "string", "description": "Token from the current in-process plan"},
              "confirm": {"type": "boolean", "default": False},
          }}, _tidy),
        ("hermeschan_undo_tidy", "Undo one journaled tidy batch; requires its token and confirm=true.",
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
    log.info("hermes-chan: registered %d tools", len(tools))
