"""Lifecycle hooks -> animation states.

Every hook is an *observer*: it nudges the overlay to a state and returns
nothing, so it can never block or alter a turn. ``post_tool_call`` also inspects
the payload to show a failure face and to sync the body when the acting tool is
``computer_use`` (the character performed that action herself).
"""
from __future__ import annotations

import logging

from . import states

log = logging.getLogger("hermes-chan.hooks")


def register(ctx, bridge) -> None:
    for hook_name, state in states.HOOK_STATE.items():
        ctx.register_hook(hook_name, _state_setter(bridge, hook_name, state))
    log.info("hermes-chan: registered %d lifecycle hooks", len(states.HOOK_STATE))


def _state_setter(bridge, hook_name, state):
    def _cb(**payload):
        try:
            if hook_name == "post_tool_call":
                _on_post_tool(bridge, payload)
                return
            if hook_name == "post_llm_call" and _plan_finished(payload):
                bridge.set_state(states.CELEBRATE)
                return
            bridge.set_state(state)
        except Exception as exc:  # noqa: BLE001 - observers never raise into the loop
            log.debug("hook %s failed: %s", hook_name, exc)
    return _cb


def _on_post_tool(bridge, payload) -> None:
    ok = payload.get("ok", payload.get("success", True))
    tool = payload.get("tool") or payload.get("tool_name") or ""
    if not ok:
        bridge.set_state(states.FAILED)
        return
    # When the agent drives the screen, the body already animated the action in
    # the backend; here we simply return to a watchful "peek" at the target app.
    if tool == "computer_use":
        bridge.set_state(states.PEEK)
    else:
        bridge.set_state(states.WORKING)


def _plan_finished(payload) -> bool:
    # Best-effort: some builds pass todo/plan completion in the payload.
    plan = payload.get("plan") or {}
    try:
        return bool(plan) and plan.get("remaining", 1) == 0
    except AttributeError:
        return False
