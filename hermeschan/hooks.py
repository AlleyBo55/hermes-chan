"""Hermes lifecycle observers that drive the overlay without altering turns."""
from __future__ import annotations

import logging

from . import reactions, states

log = logging.getLogger("hermes-chan.hooks")


def register(ctx, bridge, settings) -> None:
    gate = reactions.ReactionGate()
    for hook_name, state in states.HOOK_STATE.items():
        ctx.register_hook(hook_name, _state_setter(bridge, settings, gate, hook_name, state))
    log.info("hermes-chan: registered %d lifecycle hooks", len(states.HOOK_STATE))


# Where her bubble lives: the classic CLI, the TUI and the Desktop app. Messaging chats
# (Telegram, Discord, ...) are left alone.
_LOCAL_PLATFORMS = {"cli", "tui", "desktop", "local"}

# A warm hello fires at most this often, so reopening sessions does not spam hearts.
_GREET_GATE = reactions.ReactionGate(gap=1800.0)


def _remember_chat(bridge, payload) -> None:
    """Each turn in a local chat names its chat, so her chat box talks to the one you used last.

    Hooks carry the session id, not the route key inject_message needs, so read the turn's
    HERMES_SESSION_KEY through Hermes' public session accessor. Only the key is kept, never
    the message text.
    """
    if str(payload.get("platform") or "") not in _LOCAL_PLATFORMS:
        return
    try:
        from gateway.session_context import get_session_env
    except ImportError:  # an older Hermes: /hermeschan in a chat still teaches her
        return
    bridge.remember_session(get_session_env("HERMES_SESSION_KEY"))


def _state_setter(bridge, settings, gate, hook_name, state):
    def _cb(**payload):
        try:
            if hook_name == "pre_llm_call":
                _remember_chat(bridge, payload)
            if hook_name in ("pre_llm_call", "pre_tool_call", "pre_approval_request",
                             "on_human_input_request"):
                bridge.set_agent_busy(True)
            elif hook_name in ("on_session_start", "on_session_end"):
                bridge.set_agent_busy(False)
                if hook_name == "on_session_start":
                    _greet(bridge, settings, payload)

            if hook_name == "post_tool_call":
                _on_post_tool(bridge, settings, gate, payload)
                return
            if hook_name == "post_llm_call":
                _on_post_llm(bridge, settings, payload)
                return
            if settings.get("sleeping", False):
                bridge.set_state(states.IDLE, corner=True)
                return
            bridge.set_state(state)
        except Exception as exc:  # noqa: BLE001 - observers never raise into the loop
            log.debug("hook %s failed: %s", hook_name, exc)
        finally:
            if hook_name == "post_llm_call":
                bridge.set_agent_busy(False)
    return _cb


def _on_post_llm(bridge, settings, payload) -> None:
    # This is the current observer payload from turn_finalizer; no output transform is registered.
    response = payload.get("assistant_response")
    if isinstance(response, str) and response.strip():
        metadata = {key: payload[key] for key in ("session_id", "turn_id")
                    if isinstance(payload.get(key), str) and payload[key]}
        bridge.say(response, **metadata)
    if settings.get("sleeping", False):
        bridge.set_state(states.IDLE, corner=True)
    elif _plan_finished(payload):
        bridge.set_state(states.CELEBRATE)
    else:
        bridge.set_state(states.SPEAKING)


def _on_post_tool(bridge, settings, gate, payload) -> None:
    status = payload.get("status")
    if isinstance(status, str):
        ok = status == "ok"
    else:
        ok = payload.get("ok", payload.get("success", True)) is not False
    tool = payload.get("tool_name") or payload.get("tool") or ""
    if not ok:
        bridge.set_state(states.FAILED)
        return
    if tool == "computer_use":
        # The backend waits for action_complete before this observer can replace the clip.
        bridge.set_state(states.PEEK)
    else:
        bridge.set_state(states.WORKING)
    _react_to_tool(bridge, settings, gate, tool)


def _react_to_tool(bridge, settings, gate, tool) -> None:
    """A themed one-liner about the tool the agent just used, like Maido-chan reacting to
    her current task. Her own antics, so it rides the mischief switch; quiet while asleep,
    and rate-limited so a burst of tool calls earns one line."""
    if not settings.get("mischief", True) or settings.get("sleeping", False):
        return
    category = reactions.category_for(tool)
    if not category or not gate.allow():
        return
    # Mail and messaging tools earn the full reaction (pose + effect + line); the rest
    # stay a themed mumble. See overlay/REACTIONS.md A (email) and B (message).
    if category == "mail":
        bridge.reaction("email", emotion="happy")
    elif category == "social":
        bridge.reaction("message", emotion="surprised")
    else:
        bridge.react(category)


def _greet(bridge, settings, payload) -> None:
    """A warm hello (hearts + a line) when a session begins on a local surface. Her own
    antics, so it rides the mischief switch, stays quiet while asleep, and is rate-limited.
    See overlay/REACTIONS.md D (affection)."""
    if not settings.get("mischief", True) or settings.get("sleeping", False):
        return
    if str(payload.get("platform") or "local") not in _LOCAL_PLATFORMS:
        return
    if _GREET_GATE.allow():
        bridge.reaction("affection", text="welcome back ♥", emotion="love")


def _plan_finished(payload) -> bool:
    plan = payload.get("plan") or {}
    try:
        return bool(plan) and plan.get("remaining", 1) == 0
    except AttributeError:
        return False
