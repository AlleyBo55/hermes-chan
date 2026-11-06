"""The ``/hermeschan`` slash command (alias ``/chan``)."""
from __future__ import annotations

import logging

from . import states

log = logging.getLogger("hermes-chan.command")

_USAGE = (
    "/hermeschan — desktop companion controls\n"
    "  status           show whether the body is connected\n"
    "  come             call her to the active window\n"
    "  sleep            send her to a corner and idle\n"
    "  say <text>       make her speak a line\n"
    "  wake             start the visible body (overlay)\n"
    "  hide             stop the visible body\n"
    "  mischief on|off  allow/stop autonomous idle antics"
)


def register(ctx, bridge) -> None:
    def _handler(raw: str) -> str:
        args = (raw or "").strip().split(maxsplit=1)
        sub = args[0].lower() if args else "status"
        rest = args[1] if len(args) > 1 else ""

        if sub in ("help", "-h", "--help"):
            return _USAGE
        if sub == "status":
            connected = bridge._client is not None  # noqa: SLF001 - simple status read
            return f"Hermechan body: {'connected' if connected else 'not connected'} (socket {bridge._path})"
        if sub == "come":
            bridge.set_state(states.PEEK, follow="active-window")
            return "Hermechan is coming over."
        if sub == "sleep":
            bridge.set_state(states.IDLE, corner=True)
            return "Hermechan curls up in the corner."
        if sub == "say":
            if not rest:
                return "Usage: /hermeschan say <text>"
            bridge.say(rest, emotion="happy")
            return f"Hermechan: {rest}"
        if sub == "wake":
            runner = getattr(bridge, "runner", None)
            if runner is None:
                return "No overlay runner available in this context."
            runner.maybe_start()
            return "Waking Hermechan's body…" if not runner.is_running() else "She is already here."
        if sub == "hide":
            runner = getattr(bridge, "runner", None)
            if runner is not None:
                runner.stop()
            bridge.set_state(states.IDLE)
            return "Hermechan's body is dismissed."
        if sub == "mischief":
            on = rest.strip().lower() in ("on", "true", "1", "yes")
            bridge.send({"type": "config", "mischief": on})
            return f"Autonomous mischief {'enabled' if on else 'disabled'}."
        return _USAGE

    ctx.register_command(
        name="hermeschan",
        handler=_handler,
        description="Control the Hermechan desktop companion",
        args_hint="status | come | sleep | say <text> | mischief on|off",
    )
    log.info("hermes-chan: registered /hermeschan command")
