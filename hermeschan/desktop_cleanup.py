"""Plan-first, journaled desktop cleanup with explicit undo."""
from __future__ import annotations

import json
import os
import secrets
import stat
import threading
import time
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

_PLAN_TTL_SECONDS = 15 * 60
_MAX_PLANS = 16
_TIDY_ROOT = "Hermes-chan Tidy"

_TYPE_CATEGORIES = {
    "Images": {".avif", ".bmp", ".gif", ".heic", ".jpeg", ".jpg", ".png", ".svg", ".tif", ".tiff", ".webp"},
    "Documents": {".csv", ".doc", ".docx", ".epub", ".key", ".md", ".numbers", ".odt", ".pages", ".pdf", ".ppt", ".pptx", ".rtf", ".txt", ".xls", ".xlsx"},
    "Archives": {".7z", ".bz2", ".gz", ".rar", ".tar", ".tgz", ".xz", ".zip"},
    "Audio": {".aac", ".aiff", ".flac", ".m4a", ".mp3", ".ogg", ".wav"},
    "Video": {".avi", ".m4v", ".mkv", ".mov", ".mp4", ".webm"},
    "Code": {".c", ".cpp", ".css", ".go", ".html", ".java", ".js", ".json", ".jsx", ".py", ".rs", ".sh", ".ts", ".tsx", ".yaml", ".yml"},
}


class CleanupError(ValueError):
    pass


@dataclass(frozen=True)
class _PlannedMove:
    name: str
    category: str
    device: int
    inode: int
    size: int
    mtime_ns: int


@dataclass(frozen=True)
class _Plan:
    token: str
    rule: str
    created_at: float
    moves: tuple[_PlannedMove, ...]


class DesktopTidier:
    """Moves only direct, regular-file children of ``~/Desktop``."""

    def __init__(self, desktop: Optional[Path] = None, journal: Optional[Path] = None) -> None:
        self.desktop = desktop or (Path.home() / "Desktop")
        self.journal = journal or (Path.home() / ".hermes" / "hermes-chan" / "cleanup-journal.jsonl")
        self._plans: "OrderedDict[str, _Plan]" = OrderedDict()
        self._lock = threading.RLock()

    def plan(self, rule: str = "by-type") -> dict[str, Any]:
        rule = self._validate_rule(rule)
        desktop = self._desktop_dir()
        token = secrets.token_urlsafe(24)
        moves: list[_PlannedMove] = []
        preview: list[dict[str, str]] = []
        for source in sorted(desktop.iterdir(), key=lambda item: item.name.casefold()):
            if source.name.startswith(".") or source.name == _TIDY_ROOT:
                continue
            try:
                info = source.lstat()
            except OSError:
                continue
            # Finder aliases are regular files and are included. Symlinks and directories are not.
            if not stat.S_ISREG(info.st_mode):
                continue
            category = self._category(source, info, rule)
            destination_dir = desktop / _TIDY_ROOT / category
            destination_name = self._available_name(destination_dir, source.name, token)
            moves.append(_PlannedMove(source.name, category, info.st_dev, info.st_ino,
                                      info.st_size, info.st_mtime_ns))
            preview.append({"source": source.name,
                            "destination": str(Path(_TIDY_ROOT) / category / destination_name)})
        plan = _Plan(token, rule, time.time(), tuple(moves))
        with self._lock:
            self._expire_plans_locked()
            self._plans[token] = plan
            while len(self._plans) > _MAX_PLANS:
                self._plans.popitem(last=False)
        return {
            "ok": True,
            "mode": "plan",
            "rule": rule,
            "token": token,
            "expires_in_seconds": _PLAN_TTL_SECONDS,
            "moves": preview,
            "execute_requires": {"execute": True, "confirm": True, "token": token},
        }

    def execute(self, token: str, *, confirm: bool) -> dict[str, Any]:
        if confirm is not True:
            raise CleanupError("tidy execution requires confirm=true")
        with self._lock:
            self._expire_plans_locked()
            plan = self._plans.pop((token or "").strip(), None)
            if plan is None:
                raise CleanupError("unknown or expired tidy token; request a new plan")
            desktop = self._desktop_dir()
            moved: list[dict[str, str]] = []
            skipped: list[dict[str, str]] = []
            for item in plan.moves:
                source = desktop / item.name
                info = self._matching_regular_file(source, item)
                if info is None:
                    skipped.append({"source": item.name, "reason": "missing, changed, or no longer a regular file"})
                    continue
                destination_dir = self._safe_category_dir(item.category)
                destination = self._move_with_journal(source, destination_dir, plan.token, info)
                moved.append({"source": item.name,
                              "destination": str(destination.relative_to(desktop))})
            return {"ok": True, "mode": "execute", "token": plan.token,
                    "moved": moved, "skipped": skipped,
                    "undo_requires": {"token": plan.token, "confirm": True}}

    def undo(self, token: str, *, confirm: bool) -> dict[str, Any]:
        if confirm is not True:
            raise CleanupError("undo requires confirm=true")
        token = (token or "").strip()
        if not token:
            raise CleanupError("undo requires the tidy token")
        with self._lock:
            records = self._move_records(token)
            if not records:
                raise CleanupError("no journaled tidy moves found for that token")
            desktop = self._desktop_dir()
            restored: list[dict[str, str]] = []
            skipped: list[dict[str, str]] = []
            for record in reversed(records):
                source = Path(record.get("source", ""))
                destination = Path(record.get("destination", ""))
                if source.parent != desktop or not self._inside_tidy_root(destination):
                    skipped.append({"destination": str(destination), "reason": "journal path is outside the allowed desktop scope"})
                    continue
                try:
                    self._safe_existing_category(destination.parent)
                except CleanupError as exc:
                    skipped.append({"destination": str(destination), "reason": str(exc)})
                    continue
                if os.path.lexists(source):
                    skipped.append({"destination": str(destination), "reason": "original desktop name is occupied"})
                    continue
                try:
                    info = destination.lstat()
                except OSError:
                    skipped.append({"destination": str(destination), "reason": "moved file is missing"})
                    continue
                if not stat.S_ISREG(info.st_mode) or not self._identity_matches(info, record.get("identity", {})):
                    skipped.append({"destination": str(destination), "reason": "moved file changed; refusing to overwrite or relocate it"})
                    continue
                self._append_journal({"version": 1, "operation": "undo", "batch_token": token,
                                      "source": str(destination), "destination": str(source),
                                      "identity": self._identity(info), "planned_at": time.time()})
                self._link_then_unlink(destination, source)
                restored.append({"source": str(destination.relative_to(desktop)), "destination": source.name})
            return {"ok": True, "mode": "undo", "token": token,
                    "restored": restored, "skipped": skipped}

    @staticmethod
    def _validate_rule(rule: str) -> str:
        value = (rule or "by-type").strip().lower()
        if value not in {"by-type", "by-date", "into-one-folder"}:
            raise CleanupError("rule must be by-type, by-date, or into-one-folder")
        return value

    def _desktop_dir(self) -> Path:
        try:
            info = self.desktop.lstat()
        except OSError as exc:
            raise CleanupError(f"desktop is unavailable: {exc}") from exc
        if not stat.S_ISDIR(info.st_mode) or self.desktop.is_symlink():
            raise CleanupError("~/Desktop must be a real directory, not a symlink")
        return self.desktop

    def _category(self, source: Path, info: os.stat_result, rule: str) -> str:
        if rule == "into-one-folder":
            return "All Files"
        if rule == "by-date":
            age_days = max(0.0, (time.time() - info.st_mtime) / 86400.0)
            if age_days < 1.0:
                return "Today"
            if age_days < 7.0:
                return "This Week"
            return "Older"
        suffix = source.suffix.lower()
        if suffix == ".alias":
            return "Aliases"
        for category, suffixes in _TYPE_CATEGORIES.items():
            if suffix in suffixes:
                return category
        return "Other"

    def _safe_category_dir(self, category: str) -> Path:
        desktop = self._desktop_dir()
        root = desktop / _TIDY_ROOT
        self._ensure_real_directory(root)
        destination = root / category
        self._ensure_real_directory(destination)
        return destination

    def _safe_existing_category(self, path: Path) -> None:
        root = self.desktop / _TIDY_ROOT
        for directory in (root, path):
            try:
                info = directory.lstat()
            except OSError as exc:
                raise CleanupError("tidy destination directory is missing") from exc
            if not stat.S_ISDIR(info.st_mode) or directory.is_symlink():
                raise CleanupError("tidy destination directory is no longer safe")

    @staticmethod
    def _ensure_real_directory(path: Path) -> None:
        try:
            info = path.lstat()
        except FileNotFoundError:
            path.mkdir(mode=0o700)
            info = path.lstat()
        if not stat.S_ISDIR(info.st_mode) or path.is_symlink():
            raise CleanupError(f"refusing unsafe destination directory: {path}")

    def _move_with_journal(self, source: Path, destination_dir: Path,
                           token: str, info: os.stat_result) -> Path:
        for _ in range(1000):
            destination_name = self._available_name(destination_dir, source.name, token)
            destination = destination_dir / destination_name
            record = {"version": 1, "operation": "move", "batch_token": token,
                      "source": str(source), "destination": str(destination),
                      "identity": self._identity(info), "planned_at": time.time()}
            self._append_journal(record)
            try:
                self._link_then_unlink(source, destination)
                return destination
            except FileExistsError:
                continue
        raise CleanupError(f"could not find a collision-free destination for {source.name}")

    @staticmethod
    def _link_then_unlink(source: Path, destination: Path) -> None:
        os.link(source, destination, follow_symlinks=False)
        try:
            source.unlink()
        except Exception:
            try:
                if destination.lstat().st_ino == source.lstat().st_ino:
                    destination.unlink()
            except OSError:
                pass
            raise

    def _append_journal(self, record: dict[str, Any]) -> None:
        self.journal.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
        fd = os.open(self.journal, flags, 0o600)
        try:
            os.fchmod(fd, 0o600)
            payload = (json.dumps(record, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")
            os.write(fd, payload)
            os.fsync(fd)
        finally:
            os.close(fd)

    def _move_records(self, token: str) -> list[dict[str, Any]]:
        try:
            lines = self.journal.read_text(encoding="utf-8").splitlines()
        except FileNotFoundError:
            return []
        except OSError as exc:
            raise CleanupError(f"could not read cleanup journal: {exc}") from exc
        records: list[dict[str, Any]] = []
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record, dict) and record.get("operation") == "move" and record.get("batch_token") == token:
                records.append(record)
        return records

    def _inside_tidy_root(self, path: Path) -> bool:
        root = self.desktop / _TIDY_ROOT
        try:
            path.relative_to(root)
            return path.parent.parent == root and path.parent.name in {
                "Aliases", "Images", "Documents", "Archives", "Audio", "Video", "Code", "Other",
                "Today", "This Week", "Older", "All Files",
            }
        except ValueError:
            return False

    @staticmethod
    def _matching_regular_file(path: Path, planned: _PlannedMove) -> Optional[os.stat_result]:
        try:
            info = path.lstat()
        except OSError:
            return None
        if not stat.S_ISREG(info.st_mode):
            return None
        expected = (planned.device, planned.inode, planned.size, planned.mtime_ns)
        actual = (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
        return info if actual == expected else None

    @staticmethod
    def _identity(info: os.stat_result) -> dict[str, int]:
        return {"device": info.st_dev, "inode": info.st_ino,
                "size": info.st_size, "mtime_ns": info.st_mtime_ns}

    @staticmethod
    def _identity_matches(info: os.stat_result, identity: Any) -> bool:
        if not isinstance(identity, dict):
            return False
        return identity == DesktopTidier._identity(info)

    @staticmethod
    def _available_name(parent: Path, name: str, token: str) -> str:
        if not os.path.lexists(parent / name):
            return name
        suffix = "".join(Path(name).suffixes)
        stem = name[:-len(suffix)] if suffix else name
        short = token[:8]
        index = 1
        while os.path.lexists(parent / f"{stem} ({short}-{index}){suffix}"):
            index += 1
        return f"{stem} ({short}-{index}){suffix}"

    def _expire_plans_locked(self) -> None:
        cutoff = time.time() - _PLAN_TTL_SECONDS
        for token in [key for key, plan in self._plans.items() if plan.created_at < cutoff]:
            self._plans.pop(token, None)
