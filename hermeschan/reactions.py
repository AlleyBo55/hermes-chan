"""Her reaction to the tool the agent just used.

The overlay owns her voice (lines.js) and poses; this module only decides *which*
reaction category a tool belongs to, and rate-limits so a burst of tool calls earns
one line, not ten. Pure and dependency-free, so it is easy to test.
"""
from __future__ import annotations

import threading
import time
from typing import Optional

# First keyword found in the tool name wins, so a specific category beats a generic one.
# Matched against the lowercased tool name as plain substrings (any tool, any plugin).
_CATEGORIES: list[tuple[str, tuple[str, ...]]] = [
    ("hands", ("computer_use",)),
    ("mail", ("mail", "gmail", "email", "imap", "smtp", "inbox", "mbox", "mailbox")),
    ("social", ("telegram", "discord", "slack", "whatsapp", "wechat", "tweet", "twitter",
                "bluesky", "reddit", "mastodon", "replaid", "redreplier", "social",
                "zalo", "channel", "post_", "dm_")),
    ("memory", ("memory", "mem0", "honcho", "recall", "remember", "mnemo", "knowledge",
                "note_", "notes")),
    ("art", ("image", "video", "paint", "draw", "diffus", "flux", "sdxl", "render",
             "imagegen", "image_gen", "video_gen")),
    ("browser", ("browser", "playwright", "puppeteer", "navigate", "screenshot", "click_")),
    ("web", ("search", "web_", "web ", "fetch", "browse", "crawl", "scrape", "http",
             "url", "wiki", "serp")),
    ("data", ("sql", "query", "database", "sqlite", "sheet", "csv", "dataframe")),
    ("code", ("read_file", "write_file", "str_replace", "edit", "fs_", "shell", "terminal",
              "bash", "git", "patch", "apply", "compile", "lint", "code")),
]


def category_for(tool_name: str) -> Optional[str]:
    """A reaction category for a tool, or None when nothing fits (she stays quiet)."""
    name = (tool_name or "").lower()
    if not name:
        return None
    for category, keywords in _CATEGORIES:
        if any(keyword in name for keyword in keywords):
            return category
    return None


class ReactionGate:
    """At most one spoken reaction per `gap` seconds, so a burst of tool calls is one line."""

    def __init__(self, gap: float = 18.0) -> None:
        self._gap = gap
        self._lock = threading.Lock()
        self._last = -1e9

    def allow(self, now: Optional[float] = None) -> bool:
        current = time.monotonic() if now is None else now
        with self._lock:
            if current - self._last < self._gap:
                return False
            self._last = current
            return True
