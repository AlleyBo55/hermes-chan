"""Expressive tools the agent can call to drive Hermechan's personality.

These are the *character* actions (speak, emote, point, grab a window, a playful
glitch overlay, tidy the desktop). Real screen control — typing into an app,
clicking, switching a video — goes through the built-in ``computer_use`` tool
backed by :mod:`hermeschan.backend`, so it inherits Hermes' approval gating.
These tools never move or delete a file by themselves; ``tidy_desktop`` returns
a plan and animates, and the actual moves run through the approved file/computer
tools.
"""
from __future__ import annotations

import logging

log = logging.getLogger("hermes-chan.tools")

TOOLSET = "hermeschan"

_EMOTIONS = ["neutral", "happy", "shy", "surprised", "sulk", "sleepy", "proud", "panic", "love"]


def register(ctx, bridge) -> None:
    def _say(args):
        text = (args.get("text") or "").strip()
        if not text:
            return "nothing to say"
        bridge.say(text, emotion=args.get("emotion", "neutral"))
        return f"Hermechan says: {text}"

    def _emote(args):
        emotion = (args.get("emotion") or "neutral").lower()
        if emotion not in _EMOTIONS:
            emotion = "neutral"
        bridge.emote(emotion)
        return f"Hermechan looks {emotion}."

    def _point(args):
        target = {k: args[k] for k in ("app", "window_title", "x", "y") if args.get(k) is not None}
        bridge.point_at(target or {"app": "active"})
        return f"Hermechan points at {target or 'the active window'}."

    def _grab(args):
        bridge.animate_action("grab_window", {"app": args.get("app", "active")},
                              gesture=args.get("gesture", "drag"))
        return f"Hermechan grabs the {args.get('app', 'active')} window ({args.get('gesture', 'drag')})."

    def _glitch(args):
        # Purely a visual overlay effect drawn by the body on top of the window;
        # it does not touch the real app's memory or files.
        bridge.animate_action("glitch", {"app": args.get("app", "active")},
                              intensity=float(args.get("intensity", 0.5)))
        return f"Hermechan scribbles a playful glitch over {args.get('app', 'active')} (visual only)."

    def _tidy(args):
        bridge.animate_action("tidy_desktop", {"rule": args.get("rule", "by-type")})
        return ("Hermechan is ready to tidy the desktop. Real icon moves run through the "
                "approved file tools with an undo log; call computer_use / fs tools to execute.")

    _tools = [
        ("hermeschan_say", "Speak a short line in Hermechan's speech bubble.",
         {"type": "object",
          "properties": {"text": {"type": "string", "description": "What she says"},
                         "emotion": {"type": "string", "enum": _EMOTIONS}},
          "required": ["text"]}, _say),
        ("hermeschan_emote", "Set Hermechan's facial expression.",
         {"type": "object",
          "properties": {"emotion": {"type": "string", "enum": _EMOTIONS}},
          "required": ["emotion"]}, _emote),
        ("hermeschan_point_at", "Make Hermechan walk over and point at an app/window/point.",
         {"type": "object",
          "properties": {"app": {"type": "string"}, "window_title": {"type": "string"},
                         "x": {"type": "integer"}, "y": {"type": "integer"}}}, _point),
        ("hermeschan_grab_window", "Grab and physically tug the active or named window.",
         {"type": "object",
          "properties": {"app": {"type": "string"},
                         "gesture": {"type": "string", "enum": ["drag", "shake", "throw", "bend"]}}}, _grab),
        ("hermeschan_glitch_window", "Draw a playful, harmless glitch effect over a window (visual overlay only).",
         {"type": "object",
          "properties": {"app": {"type": "string"},
                         "intensity": {"type": "number", "minimum": 0, "maximum": 1}}}, _glitch),
        ("hermeschan_tidy_desktop", "Plan + animate tidying desktop icons (real moves go through approved tools).",
         {"type": "object",
          "properties": {"rule": {"type": "string", "enum": ["by-type", "by-date", "into-one-folder"]}}}, _tidy),
    ]

    for name, desc, schema, handler in _tools:
        schema = dict(schema)
        schema.setdefault("description", desc)
        ctx.register_tool(name=name, toolset=TOOLSET, schema=schema, handler=handler)
    log.info("hermes-chan: registered %d expressive tools", len(_tools))
