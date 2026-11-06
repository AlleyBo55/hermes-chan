"""hermes-chan — a living desktop companion plugin for Hermes.

One ``register(ctx)`` wires four surfaces:

  1. A ``computer_use`` driver (the character is the body behind the built-in
     ``computer_use`` tool). Selected by ``computer_use.backend: hermes-chan``.
  2. Expressive tools the agent can call directly (say / emote / point / grab /
     glitch / tidy).
  3. Lifecycle hooks that translate agent activity into animation states.
  4. A ``/hermeschan`` slash command and a chat-bubble bridge: text typed into
     the overlay bubble is injected into the session with ``ctx.inject_message``.

Everything degrades safely: if the Rust overlay is not running, tool/hook calls
become no-ops (the bridge logs and returns), so the agent is never blocked by a
missing body.
"""
from __future__ import annotations

import logging

log = logging.getLogger("hermes-chan")


def register(ctx):
    # Local imports keep module import cheap for `hermes plugins validate`,
    # and let the tool/hook surfaces register even when the computer-use
    # infrastructure is unavailable in a given context.
    from .hermeschan import tools as _tools
    from .hermeschan import hooks as _hooks
    from .hermeschan import command as _command
    from .hermeschan.autonomy import AutonomyController
    from .hermeschan.bridge import Bridge
    from .hermeschan.settings import SettingsStore

    bridge = Bridge.shared(ctx)
    settings = SettingsStore()
    current = settings.snapshot()
    bridge.update_config(
        mischief=current["mischief"],
        watch=current["watch"],
        sleeping=current["sleeping"],
        heartbeat_enabled=current["heartbeat_enabled"],
        heartbeat_interval=current["heartbeat_interval"],
    )
    previous_controller = getattr(bridge, "autonomy_controller", None)
    if previous_controller is not None:
        previous_controller.stop()
    autonomy = AutonomyController(ctx, bridge, settings)
    bridge.autonomy_controller = autonomy

    # 1. Computer-use driver: the character's body performs real actions.
    try:
        from .hermeschan.provider import HermechanProvider
        ctx.register_computer_use_provider(HermechanProvider(bridge))
        log.info("hermes-chan: registered computer_use provider")
    except Exception as exc:  # noqa: BLE001 - optional surface, never fatal
        log.warning("hermes-chan: computer_use provider unavailable (%s)", exc)

    # 2. Expressive and confirmation-gated cleanup tools.
    _tools.register(ctx, bridge)

    # 3. Lifecycle hooks -> animation states and final-response bubble.
    _hooks.register(ctx, bridge, settings)

    # 4. Slash command + chat-bubble bridge (overlay -> inject_message).
    _command.register(ctx, bridge, settings, autonomy)
    bridge.start_inbound(ctx)

    # 5. Launch and supervise the visible body (safe no-op if unavailable).
    try:
        from .hermeschan.overlay_runner import OverlayRunner
        runner = OverlayRunner(bridge)
        bridge.runner = runner
        runner.maybe_start(ctx)
    except Exception as exc:  # noqa: BLE001 - the body is optional, never fatal
        log.warning("hermes-chan: overlay runner unavailable (%s)", exc)

    # PluginContext currently has no public heartbeat callback, so use the
    # persisted, opt-in daemon controller and stop it on plugin unload.
    autonomy.start_from_settings()
    on_unload = getattr(ctx, "on_unload", None)
    if callable(on_unload):
        on_unload(autonomy.stop)
