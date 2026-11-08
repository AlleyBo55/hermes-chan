# Hermes-chan reactions & effects catalog

Every visible thing she does is built as a self-contained **animation** here, with a
**label (A, B, C…)** so the integration phase can wire each to a real event source
without touching the animation again.

- **Animation** = the sprite pose + the click-through canvas effect + the speech bubble.
  All of this is done and previewable now (`overlay/src/effects-preview.html`).
- **Integration** = the trigger that fires the reaction from a real event. A–D are now
  wired (see "Wired triggers" at the bottom); they go live after a body rebuild and a
  Hermes restart.

Preview locally (no Hermes, no permissions):

```bash
open overlay/src/effects-preview.html      # or drag it into a browser
```

## Reactions

| Label | Reaction | Pose clip | Effect (canvas) | Bubble | Status | Integration target |
| --- | --- | --- | --- | --- | --- | --- |
| **A** | New email | `action/point` | `mail` (envelope opens, letter + "1" badge) | "📬 New email from …" | animation done | an email plugin / the gateway inbox sends `{type:"reaction", id:"email", from, subject}` |
| **B** | New message / DM | `back/peek` | `ping` (notification chip + rings) | "💬 Message from …" | animation done | the messaging gateway (the ~20 connected platforms) sends `{type:"reaction", id:"message", …}` |
| **C** | Reminder / scheduled nudge | `action/point` | `ping` | "⏰ …" | animation done (reuses `ping`) | a cron / reminder fires `{type:"reaction", id:"reminder", text}` |
| **D** | Affection / greeting | `emotion/love` | `hearts` (hearts drift up) | "…" | animation done | session start, or a warmth beat; `{type:"reaction", id:"affection"}` |
| **E** | Agent task update (Maido-chan) | faces active window | none / emote | tool-themed line | done on `feat/task-reactions` | Hermes lifecycle hooks → `react(category)` (already wired) |

## Pranks (her own mischief, already built)

| Label | Prank | Casting clip | Effect | Integration target |
| --- | --- | --- | --- | --- |
| **F** | Touch screen → crack | `action/crack` | `crack` | idle mischief, or `/hermeschan effect crack` |
| **G** | Open a portal | `action/portal` | `portal` | idle mischief |
| **H** | Break / hijack the app | `action/*` | `hijack`, `collapse`, `glitch`, `bend`, `text_shatter`, `ripple`, `pixel_dissolve`, `lightning`, `tear` | idle mischief; real window grabs (shake/nudge/throw/pull-down) gated on user idle |

Pranks fire their effect on the casting clip's **impact frame** (`effect_start`, or
`bend_peak` for bend), so her hand and the effect line up. Reactions A–D have no dedicated
casting clip yet, so they fire just after the pose begins; giving them their own 5-frame
casting clips later is the only art work left for them.

## How a new animation is added

1. Canvas effect: a `make<Name>` + `draw<Name>` in `overlay/src/effect-canvas.js`, then
   register it in `start()` and dispatch it in `tick()`.
2. Name it in `overlay/src/effect.js` (`EFFECT_MAP` + the `canvasEffect` list).
3. Add a button + pose + bubble in `overlay/src/effects-preview.html`.
4. Give it a row here with a label and its integration target.

## Wired triggers

Each reaction is one message, `bridge.reaction(id, text?, emotion?)` → `{type:"reaction", id}`
→ overlay `handleReactionFx` (pose + `play_effect` over the active window + bubble).

| Label | Fires when | Code |
| --- | --- | --- |
| **A** email | the agent uses a mail/IMAP/SMTP tool (`reactions.category_for` == `mail`), rate-limited to one per 18 s | `hooks.py::_react_to_tool` |
| **B** message | the agent uses a messaging tool (category `social`: Telegram, Slack, Discord, …) | `hooks.py::_react_to_tool` |
| **C** reminder | `/hermeschan reaction reminder <text>`, or a cron / reminder calling `bridge.reaction("reminder", text=…)` | `command.py`; no native hook yet |
| **D** affection | a local session starts (`on_session_start`), rate-limited to one per 30 min, mischief on, awake | `hooks.py::_greet` |

All of A–D can also be fired by hand: `/hermeschan reaction email|message|reminder|affection [text]`.

They respect the same switches as her other antics: **off while asleep**, and **off when
`mischief` is off** (`/hermeschan mischief off`). The effect decorates the **active window**;
if no active window has been seen yet, she still poses and speaks.

### To go live

The body is a release build and the plugin is Python loaded by Hermes, so:

```bash
cd overlay/src-tauri && cargo build --release   # re-embeds the overlay frontend
```

then restart Hermes (or `/hermeschan resummon` after a plugin reload) so `hooks.py`,
`bridge.py` and `command.py` reload. Test with `/hermeschan reaction email`.

### Still open

- **A/B are triggered by the agent *using* a mail/messaging tool**, not by a raw inbound
  email/message. Hermes delivers inbound messages as new sessions/turns, not as a
  mid-session event a plugin can observe, so a true "mail just arrived" trigger needs a
  dedicated source (an email-poll cron or a gateway webhook) that calls
  `bridge.reaction("email", …)`. The animation and the plumbing are ready for it.
- **C** has no native event; it is manual or cron-driven.
