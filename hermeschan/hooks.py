"""Hermes lifecycle observers that drive the overlay without altering turns."""
from __future__ import annotations

import logging

from . import states

log = logging.getLogger("hermes-chan.hooks")


def register(ctx, bridge, settings) -> None:
    for hook_name, state in states.HOOK_STATE.items():
        ctx.register_hook(hook_name, _state_setter(bridge, settings, hook_name, state))
    log.info("hermes-chan: registered %d lifecycle hooks", len(states.HOOK_STATE))


def _state_setter(bridge, settings, hook_name, state):
    def _cb(**payload):
        try:
            if hook_name in ("pre_llm_call", "pre_tool_call", "pre_approval_request",
                             "on_human_input_request"):
                bridge.set_agent_busy(True)
            elif hook_name in ("on_session_start", "on_session_end"):
                bridge.set_agent_busy(False)

            if hook_name == "post_tool_call":
                _on_post_tool(bridge, payload)
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


def _on_post_tool(bridge, payload) -> None:
    status = payload.get("status")
    if isinstance(status, str):
        ok = status == "ok"
    else:
        ok = payload.get("ok", payload.get("success", True)) is not False
    tool = payload.get("tool_name") or payload.get("tool") or ""
    if not ok:
        bridge.set_state(states.FAILED)
    elif tool == "computer_use":
        # The backend waits for action_complete before this observer can replace the clip.
        bridge.set_state(states.PEEK)
    else:
        bridge.set_state(states.WORKING)


def _plan_finished(payload) -> bool:
    plan = payload.get("plan") or {}
    try:
        return bool(plan) and plan.get("remaining", 1) == 0
    except AttributeError:
        return False
