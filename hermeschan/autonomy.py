"""Heartbeat-driven idle brain.

Hermechan "comes alive" on her own between turns. Rather than burning a full
agent turn on every tick, the cheap path asks the user's active model for one
small decision through ``ctx.llm.complete_structured`` and returns a tiny plan
the overlay can act on (an idle animation, a one-liner, a glance at the active
window). The expensive path — when she decides to actually *do* something — is a
normal ``ctx.inject_message`` user turn so it runs under full approval.

Wire the cheap tick with a session heartbeat::

    /heartbeat every 2m (idle) glance at what I'm doing and react in one short line

or call :func:`decide_idle_action` from a scheduled/no_agent cron job.
"""
from __future__ import annotations

import json
import logging
from typing import Any, Optional

log = logging.getLogger("hermes-chan.autonomy")

_IDLE_SCHEMA = {
    "type": "object",
    "properties": {
        "action": {"type": "string", "enum": ["idle", "peek", "say", "emote", "mischief"]},
        "line": {"type": "string", "description": "<= 12 words, in character, optional"},
        "emotion": {"type": "string"},
    },
    "required": ["action"],
}

_SYSTEM = (
    "You are Hermechan, a small, warm, slightly mischievous desktop maid companion. "
    "Given a one-line hint of what the user is doing, choose ONE tiny idle reaction. "
    "Keep any spoken line under 12 words and in character. Prefer 'idle' or 'peek' most "
    "of the time; 'mischief' rarely."
)


def decide_idle_action(ctx, context_hint: str = "") -> dict[str, Any]:
    """Return a small idle plan: {"action": ..., "line": ..., "emotion": ...}."""
    try:
        result = ctx.llm.complete_structured(
            instructions=_SYSTEM,
            input=[{"type": "text", "text": context_hint or "The user is idle."}],
            json_schema=_IDLE_SCHEMA,
            schema_name="hermeschan.idle",
            purpose="hermes-chan.idle",
            temperature=0.8,
            max_tokens=64,
        )
    except Exception as exc:  # noqa: BLE001 - autonomy must never crash the host
        log.debug("idle decision failed: %s", exc)
        return {"action": "idle"}

    plan = getattr(result, "parsed", None)
    if isinstance(plan, dict) and plan.get("action"):
        return plan
    raw = getattr(result, "text", "") or ""
    try:
        parsed = json.loads(raw)
        if isinstance(parsed, dict) and parsed.get("action"):
            return parsed
    except ValueError:
        pass
    return {"action": "idle"}


def apply_idle_action(bridge, plan: dict[str, Any]) -> None:
    action = plan.get("action", "idle")
    line = (plan.get("line") or "").strip()
    emotion = plan.get("emotion") or "neutral"
    if action == "say" and line:
        bridge.say(line, emotion=emotion)
    elif action == "emote":
        bridge.emote(emotion)
    elif action == "peek":
        bridge.set_state("peek")
    elif action == "mischief":
        bridge.set_state("mischief")
        if line:
            bridge.say(line, emotion="happy")
    else:
        bridge.set_state("idle")
