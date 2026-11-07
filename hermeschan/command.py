"""The ``/hermeschan`` slash command."""
from __future__ import annotations

import math
import logging

from . import states

log = logging.getLogger("hermes-chan.command")

_USAGE = (
    "/hermeschan — desktop companion controls\n"
    "  status                         show body, driver, and autonomy state\n"
    "  on | off                       switch her on or off (Ctrl+Shift+click her = off)\n"
    "  resummon                       restart her fresh: clears away, sleep, saved spot, stuck chat\n"
    "  come                           call her to the active window\n"
    "  sleep | wake                   persist sleeping/awake state\n"
    "  say <text>                     make her speak a line\n"
    "  effect <name>                  play an effect on the active app\n"
    "    crack portal text_shatter ripple pixel_dissolve lightning tear glitch bend hijack\n"
    "  hide                           stop the visible body\n"
    "  mischief on|off                allow/stop autonomous antics\n"
    "  watch on|off                   allow/stop one screenshot per decision\n"
    "  react                          run one bounded watch/reaction now\n"
    "  heartbeat on|off|status [60s]  control bounded idle decisions\n"
    "  rest on|off|test [hours]       her take-a-break reminder after 3 h of work"
)


def _parse_interval(raw: str) -> float:
    value = (raw or "").strip().lower()
    if not value:
        return 120.0
    multiplier = 1.0
    if value[-1:] in {"s", "m", "h"}:
        suffix = value[-1]
        value = value[:-1]
        multiplier = {"s": 1.0, "m": 60.0, "h": 3600.0}[suffix]
    try:
        seconds = float(value) * multiplier
    except ValueError as exc:
        raise ValueError("interval must be seconds or use s/m/h, for example 90s or 2m") from exc
    if not math.isfinite(seconds) or seconds <= 0:
        raise ValueError("interval must be a positive finite duration")
    return max(60.0, seconds)


def _toggle(raw: str, name: str) -> bool:
    value = (raw or "").strip().lower()
    if value == "on":
        return True
    if value == "off":
        return False
    raise ValueError(f"Usage: /hermeschan {name} on|off")


def _driver_available() -> bool:
    try:
        from .backend import real_driver_available
        return real_driver_available()
    except Exception:  # noqa: BLE001 - command survives optional computer-use imports
        return False


def _status(bridge, settings, autonomy, body) -> str:
    current = settings.refresh().snapshot()
    heartbeat = autonomy.status()
    heartbeat_state = "on" if heartbeat["enabled"] else "off"
    if heartbeat["enabled"] and (heartbeat["sleeping"] or not heartbeat["opted_in"]):
        heartbeat_state += " (paused)"
    if bridge.is_connected:
        link = f"connected ({bridge.path})"
    elif bridge.is_listening:
        link = f"waiting for her body ({bridge.path})"
    else:
        link = f"not listening{f': {bridge.problem}' if bridge.problem else ''}"
    switched = "away (/hermeschan on brings her back)" if current["away"] else "on"
    steps = bridge.chat_steps()
    chat = "ready" if not steps else "not ready yet:" + "".join(
        f"\n    {i}. {step}" for i, step in enumerate(steps, 1))
    return (
        f"Hermes-chan is {switched}.\n"
        f"  body: {body.runner.describe()}\n"
        f"  bridge: {link}\n"
        f"  chat box: {chat}\n"
        f"  computer use: {'native input available' if _driver_available() else 'native input unavailable here'}\n"
        f"  sleeping: {'yes' if current['sleeping'] else 'no'} · "
        f"mischief: {'on' if current['mischief'] else 'off'} · "
        f"watch: {'on' if current['watch'] else 'off'} · "
        f"heartbeat: {heartbeat_state} every {heartbeat['interval']:g}s · "
        f"rest reminder: {'on' if current['rest_reminder'] else 'off'}"
    )


_REST_ON = ("Rest reminder on: after 3 hours at the computer without a 10-minute break, "
            "she takes over your screen to ask you to rest, and again every 3 hours.")


def _rest(raw: str, bridge, settings) -> str:
    words = (raw or "").split()
    action = words[0].lower() if words else "status"
    if action in ("on", "off"):
        enabled = action == "on"
        settings.update(rest_reminder=enabled)
        bridge.update_config(rest_reminder=enabled)
        return _REST_ON if enabled else "Rest reminder off."
    if action == "test":
        try:
            hours = max(1, min(99, int(words[1]))) if len(words) > 1 else 3
        except ValueError:
            return "Usage: /hermeschan rest test [hours]"
        if not bridge.is_connected:
            return "Hermes-chan's overlay is not connected."
        bridge.send({"type": "rest_test", "hours": hours})
        return f"Hermes-chan shows her {hours}-hour rest reminder now. Click it to close it."
    if action == "status":
        return _REST_ON if settings.refresh().get("rest_reminder", True) else "Rest reminder off."
    return "Usage: /hermeschan rest on|off|test [hours]"


def _with_chat_steps(reply: str, bridge) -> str:
    """Add what you still have to do before her chat box can reach this chat."""
    steps = bridge.chat_steps()
    if not steps:
        return reply
    return reply + "\nTo chat with her from her bubble:" + "".join(
        f"\n  {i}. {step}" for i, step in enumerate(steps, 1))


def register(ctx, bridge, settings, autonomy, body) -> None:
    def _handler(raw: str, session_key: str = "", session_id: str = "", **_context) -> str:
        bridge.remember_session(session_key)
        args = (raw or "").strip().split(maxsplit=1)
        sub = args[0].lower() if args else "status"
        rest = args[1] if len(args) > 1 else ""

        if sub in ("help", "-h", "--help"):
            return _USAGE
        if sub == "status":
            return _status(bridge, settings, autonomy, body)
        if sub == "on":
            blocked = body.runner.blocked_reason()
            body.switch_on()
            if blocked:
                return f"Hermes-chan is switched on, but her body cannot appear here: {blocked}."
            return _with_chat_steps("Hermes-chan is switched on; she will appear in a moment.", bridge)
        if sub == "resummon":
            blocked = body.runner.blocked_reason()
            if blocked:
                return f"Hermes-chan cannot appear here: {blocked}."
            here = body.resummon()
            return _with_chat_steps("Hermes-chan is coming back fresh." if here
                                    else "Hermes-chan will come back fresh in a moment.", bridge)
        if sub == "off":
            body.switch_off()
            return ("Hermes-chan left. /hermeschan on brings her back, and so does a Hermes restart; "
                    "to keep her off, switch the plugin off in Hermes.")
        if settings.refresh().get("away", False) and sub in ("come", "wake", "effect", "react"):
            return "Hermes-chan is away; /hermeschan on brings her back."
        if sub == "come":
            if settings.get("sleeping", False):
                return "Hermes-chan is asleep; use /hermeschan wake first."
            bridge.set_state(states.PEEK, follow="active-window")
            return "Hermes-chan is coming over."
        if sub == "sleep":
            settings.update(sleeping=True)
            bridge.update_config(sleeping=True)
            bridge.set_state(states.IDLE, corner=True)
            return "Hermes-chan curls up in the corner."
        if sub == "say":
            if not rest:
                return "Usage: /hermeschan say <text>"
            bridge.say(rest, emotion="happy", session_id=session_id or None)
            if settings.get("sleeping", False):
                bridge.set_state(states.IDLE, corner=True)
            return f"Hermes-chan: {rest}"
        if sub == "effect":
            effect = rest.strip().lower()
            if effect not in {"glitch", "crack", "portal", "bend", "text_shatter",
                              "ripple", "pixel_dissolve", "lightning", "tear", "hijack"}:
                return ("Usage: /hermeschan effect "
                        "glitch|crack|portal|bend|text_shatter|ripple|pixel_dissolve|lightning|tear|hijack")
            if not bridge.is_connected:
                return "Hermes-chan's overlay is not connected."
            target = {"app": "active"}
            action_id = bridge.animate_action("glitch", target, defer_native=True,
                                              effect=effect, intensity=0.8)
            bridge.wait_for_action_event(action_id, "effect_start", timeout=0.7)
            bridge.send({"type": "native_action", "action_id": action_id,
                         "verb": "glitch", "target": target, "effect": effect,
                         "intensity": 0.8})
            bridge.wait_for_action_complete(action_id, timeout=0.9)
            bridge.finish_action(action_id)
            return f"Hermes-chan plays {effect} over the active app."
        if sub == "wake":
            settings.update(sleeping=False)
            bridge.update_config(sleeping=False)
            body.start()
            bridge.set_state(states.IDLE)
            return "Hermes-chan is awake."
        if sub == "hide":
            body.runner.stop()
            return "Hermes-chan's body is dismissed until /hermeschan wake."
        if sub == "rest":
            return _rest(rest, bridge, settings)
        if sub == "mischief":
            try:
                enabled = _toggle(rest, "mischief")
            except ValueError as exc:
                return str(exc)
            settings.update(mischief=enabled)
            bridge.update_config(mischief=enabled)
            return f"Autonomous mischief {'enabled' if enabled else 'disabled'}."
        if sub == "watch":
            try:
                enabled = _toggle(rest, "watch")
            except ValueError as exc:
                return str(exc)
            settings.update(watch=enabled)
            bridge.update_config(watch=enabled)
            return ("Watch enabled: one active-window screenshot may be requested per bounded decision."
                    if enabled else "Watch disabled: autonomy will not request screenshots.")
        if sub == "react":
            if not bridge.is_connected:
                return "Hermes-chan's overlay is not connected."
            if autonomy.trigger_once():
                return "Hermes-chan is taking one bounded look and will react shortly."
            return "Enable /hermeschan watch on or /hermeschan mischief on first."
        if sub == "heartbeat":
            parts = rest.split()
            operation = parts[0].lower() if parts else "status"
            if operation == "status":
                status = autonomy.status()
                state = "on" if status["enabled"] else "off"
                if status["enabled"] and (status["sleeping"] or not status["opted_in"]):
                    state += " (paused until awake with mischief or watch enabled)"
                return f"Heartbeat {state}; interval {status['interval']:g}s; maximum 10 decisions/hour."
            if operation == "off":
                autonomy.configure(False)
                return "Heartbeat autonomy disabled."
            if operation != "on" or len(parts) > 2:
                return "Usage: /hermeschan heartbeat on|off|status [interval]"
            current = settings.snapshot()
            if not (current["mischief"] or current["watch"]):
                return "Enable /hermeschan mischief on or /hermeschan watch on before heartbeat autonomy."
            try:
                interval = _parse_interval(parts[1] if len(parts) == 2 else "")
            except ValueError as exc:
                return str(exc)
            autonomy.configure(True, interval)
            return f"Heartbeat autonomy enabled every {interval:g}s (maximum 10 decisions/hour)."
        return _USAGE

    ctx.register_command(
        name="hermeschan",
        handler=_handler,
        description="Control the Hermes-chan desktop companion",
        args_hint="status | on | off | come | sleep | wake | hide | say <text> | effect <name> | mischief on|off | watch on|off | react | heartbeat on|off|status [interval]",
    )
    log.info("hermes-chan: registered /hermeschan command")
