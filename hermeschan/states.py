"""Map Hermes agent activity to Hermes-chan animation states.

The overlay owns the sprite sheet; this module only decides which named state to
request. States intentionally mirror the vocabulary a Hermes *pet* already uses
(idle / review / run / wave / failed / waiting) plus a few expressive extras
Hermes-chan adds (listening / speaking / peek / mischief).
"""
from __future__ import annotations

# Canonical animation states the overlay must provide a row for.
IDLE = "idle"
THINKING = "thinking"      # model is reading / reasoning  (pet: review)
WORKING = "working"        # a tool is executing           (pet: run)
SPEAKING = "speaking"      # delivering a reply
LISTENING = "listening"    # user is typing to her
DONE = "done"              # turn finished cleanly         (pet: wave)
CELEBRATE = "celebrate"    # a plan finished               (pet: jump)
FAILED = "failed"          # a tool/turn failed
BLOCKED = "blocked"        # waiting on the user           (pet: waiting)
PEEK = "peek"              # glancing at the active window (faces away from user)
MISCHIEF = "mischief"      # autonomous idle antics

ALL = [
    IDLE, THINKING, WORKING, SPEAKING, LISTENING, DONE,
    CELEBRATE, FAILED, BLOCKED, PEEK, MISCHIEF,
]

# Which lifecycle hook drives which state. Used by hooks.register().
HOOK_STATE = {
    "on_session_start": IDLE,
    "pre_llm_call": THINKING,
    "post_llm_call": SPEAKING,
    "pre_tool_call": WORKING,
    "post_tool_call": WORKING,
    # Waiting on you: an approval prompt or a clarifying question.
    "pre_approval_request": BLOCKED,
    "on_human_input_request": BLOCKED,
    "on_session_end": IDLE,
}
