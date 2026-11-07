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
missing body. Disabling the plugin in Hermes unloads it and takes her body down;
``/hermeschan off`` or Ctrl+Shift+click on her switches her off and keeps her off.
"""
from __future__ import annotations

import atexit
import logging

log = logging.getLogger("hermes-chan")

_EXIT_HOOK_REGISTERED = False


def _shutdown_at_exit() -> None:
    """Take her body down with Hermes (the overlay also watches for that itself)."""
    from .hermeschan.bridge import Bridge

    body = getattr(Bridge._instance, "body", None)  # noqa: SLF001 - our own singleton
    if body is not None:
        body.shutdown()


def _register_provider_only(ctx) -> None:
    """With ``computer_use.backend: hermes-chan``, Hermes' computer-use loader imports this plugin a
    second time with a context that only collects the provider. Hand it a provider that drives the
    body the general load started; start nothing here."""
    from .hermeschan.provider import HermechanProvider

    ctx.register_computer_use_provider(HermechanProvider())


_WELCOME = ("Hi, I'm Hermes-chan! Ctrl+click me to chat, drag me anywhere, "
            "Ctrl+Shift+click to send me away.")


def _greet_once(bridge, settings) -> None:
    """Her first appearance after install says how to use her; never again after that."""
    if settings.refresh().get("welcomed", False):
        return
    settings.update(welcomed=True)
    bridge.say(_WELCOME, emotion="happy")


def register(ctx):
    if not callable(getattr(ctx, "register_command", None)):
        _register_provider_only(ctx)
        return
    # Local imports keep module import cheap for `hermes plugins validate`,
    # and let the tool/hook surfaces register even when the computer-use
    # infrastructure is unavailable in a given context.
    global _EXIT_HOOK_REGISTERED
    from .hermeschan import tools as _tools
    from .hermeschan import hooks as _hooks
    from .hermeschan import command as _command
    from .hermeschan.autonomy import AutonomyController
    from .hermeschan.body import Body
    from .hermeschan.bridge import Bridge
    from .hermeschan.overlay_runner import OverlayRunner
    from .hermeschan.settings import SettingsStore, mischief_setting

    bridge = Bridge.shared(ctx)
    settings = SettingsStore()
    mischief = mischief_setting(ctx)
    if settings.get("mischief") != mischief:
        settings.update(mischief=mischief)
    current = settings.snapshot()
    bridge.update_config(
        mischief=current["mischief"],
        watch=current["watch"],
        sleeping=current["sleeping"],
        heartbeat_enabled=current["heartbeat_enabled"],
        heartbeat_interval=current["heartbeat_interval"],
        rest_reminder=current["rest_reminder"],
    )
    # A reload in the same process: retire the previous body before building a new one.
    previous = getattr(bridge, "body", None)
    if previous is not None:
        previous.shutdown()
    autonomy = AutonomyController(ctx, bridge, settings)
    bridge.autonomy_controller = autonomy
    runner = OverlayRunner(bridge)
    bridge.runner = runner
    body = Body(ctx, bridge, runner, settings, autonomy)
    bridge.body = body
    bridge.deactivate_handler = body.switched_off_from_overlay
    bridge.on_acquired = body.took_over
    bridge.on_connected = lambda: _greet_once(bridge, settings)

    # 1. Computer-use driver: the character's body performs real actions. Current Hermes loads it
    #    through _register_provider_only; older releases offered the registrar here.
    if callable(getattr(ctx, "register_computer_use_provider", None)):
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

    # 4. Slash command (including /hermeschan on|off).
    _command.register(ctx, bridge, settings, autonomy, body)

    # 5. The bridge (overlay -> inject_message) and the visible body. Loading the plugin is
    #    a fresh start: whoever takes the bridge shows her (see body.py).
    body.start_soon()

    # PluginContext currently has no public heartbeat callback, so use the
    # persisted, opt-in daemon controller and stop it on plugin unload.
    autonomy.start_from_settings()
    on_unload = getattr(ctx, "on_unload", None)
    if callable(on_unload):
        on_unload(body.shutdown)
    if not _EXIT_HOOK_REGISTERED:
        atexit.register(_shutdown_at_exit)
        _EXIT_HOOK_REGISTERED = True
