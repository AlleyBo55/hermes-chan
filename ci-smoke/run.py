"""Temporary platform smoke test for hermes-chan (lives only on the ci/platform-smoke branch).

Runs the plugin's real register(ctx) with a stand-in for Hermes, so the plugin itself builds her
body from source (cargo through its minimal environment) and starts it, then checks:
  1. the build and her body got no provider API keys,
  2. the built body embeds all 7 sprite files,
  3. her page loaded and talked back over the bridge, and she stays up,
  4. the tidy approval hook asks before moving files, and tidy + undo work on this OS.
"""
from __future__ import annotations

import importlib.util
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import types
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "smoke-out"
OUT.mkdir(exist_ok=True)
OS_NAME = {"darwin": "macos", "win32": "windows"}.get(sys.platform, "linux")
home = Path(tempfile.mkdtemp(prefix="hc-"))
os.environ.pop("CI", None)  # the plugin skips unattended runs; this run stands in for a desktop session
os.environ["HERMES_HOME"] = str(home)
if sys.platform != "win32":
    os.environ["HERMESCHAN_SOCKET"] = str(home / "bridge.sock")
FAKE_KEYS = {"OPENAI_API_KEY": "sk-fake", "ANTHROPIC_API_KEY": "sk-ant-fake", "OPENROUTER_API_KEY": "or-fake"}
os.environ.update(FAKE_KEYS)

results: list[tuple[bool, str, str]] = []


def check(ok: bool, name: str, detail: str = "") -> bool:
    results.append((bool(ok), name, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {name}{': ' + detail if detail else ''}", flush=True)
    return bool(ok)


def tail(path: Path, lines: int = 60) -> str:
    try:
        return "\n".join(path.read_text(errors="replace").splitlines()[-lines:])
    except OSError as exc:
        return f"(no {path.name}: {exc})"


def finish(code: int) -> None:
    state = home / "hermes-chan"
    for name in ("build.log", "overlay.log"):
        if (state / name).exists():
            shutil.copy2(state / name, OUT / f"{OS_NAME}-{name}")
    body = getattr(bridge_mod.Bridge._instance, "body", None)
    if body is not None:
        body.shutdown()
    passed = sum(ok for ok, _, _ in results)
    print(f"\n{OS_NAME}: {passed}/{len(results)} checks passed", flush=True)
    sys.stdout.flush()
    os._exit(code)


spec = importlib.util.spec_from_file_location("hermes_chan_smoke", ROOT / "__init__.py",
                                              submodule_search_locations=[str(ROOT)])
plugin = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = plugin
spec.loader.exec_module(plugin)
from hermes_chan_smoke.hermeschan import bridge as bridge_mod  # noqa: E402
from hermes_chan_smoke.hermeschan import desktop_cleanup  # noqa: E402
from hermes_chan_smoke.hermeschan import overlay_runner as runner_mod  # noqa: E402
from hermes_chan_smoke.hermeschan import tools as tools_mod  # noqa: E402

# Record the exact environment the plugin hands to cargo and to her body.
seen_env: dict[str, dict[str, str]] = {}
_real_run, _real_popen = subprocess.run, subprocess.Popen


def _recording_run(*args, **kwargs):
    seen_env["build"] = dict(kwargs.get("env") or {})
    return _real_run(*args, **kwargs)


def _recording_popen(*args, **kwargs):
    seen_env["body"] = dict(kwargs.get("env") or {})
    return _real_popen(*args, **kwargs)


runner_mod.subprocess = types.SimpleNamespace(**{**vars(subprocess), "run": _recording_run,
                                                 "Popen": _recording_popen})

page_ready = threading.Event()
_original_page_ready = bridge_mod.Bridge._page_ready


def _page_ready(self):
    _original_page_ready(self)
    page_ready.set()


bridge_mod.Bridge._page_ready = _page_ready


class StandInHermes:
    def __init__(self):
        self.tools, self.hooks, self.commands, self.config = {}, {}, {}, {}

    def register_tool(self, name, toolset, schema, handler):
        self.tools[name] = handler

    def register_hook(self, name, callback):
        self.hooks.setdefault(name, []).append(callback)

    def register_command(self, name, handler, description="", args_hint="", **_):
        self.commands[name] = handler

    def get_config(self, key, default=None):
        return self.config.get(key, default)

    def set_config(self, key, value):
        self.config[key] = value

    def inject_message(self, text, role="user", session_key=None, **_):
        return True


class FakeBridge:
    is_connected = False

    def animate_action(self, *args, **kwargs):
        return "id"

    def wait_for_action_complete(self, *args, **kwargs):
        pass

    def finish_action(self, *args, **kwargs):
        pass


def screenshot(path: Path) -> None:
    if not os.environ.get("GITHUB_ACTIONS"):
        print("screenshot: skipped outside GitHub Actions", flush=True)
        return
    try:
        if sys.platform == "darwin":
            command = ["screencapture", "-x", str(path)]
        elif sys.platform == "win32":
            command = ["powershell", "-NoProfile", "-Command",
                       "Add-Type -AssemblyName System.Windows.Forms,System.Drawing;"
                       "$b=[System.Windows.Forms.SystemInformation]::VirtualScreen;"
                       "$bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height;"
                       "$g=[System.Drawing.Graphics]::FromImage($bmp);"
                       "$g.CopyFromScreen($b.Left,$b.Top,0,0,$bmp.Size);"
                       f"$bmp.Save('{path}')"]
        else:
            command = ["import", "-window", "root", str(path)]
        subprocess.run(command, timeout=60, check=False, capture_output=True)
        print(f"screenshot: {path.name if path.exists() else 'not captured'}", flush=True)
    except Exception as exc:  # noqa: BLE001 - best effort
        print(f"screenshot failed: {exc}", flush=True)


print(f"== {OS_NAME} ({platform.platform()}), Python {platform.python_version()}", flush=True)
ctx = StandInHermes()
plugin.register(ctx)
bridge = bridge_mod.Bridge._instance
runner = bridge.runner
check(len(ctx.tools) == 8 and "pre_tool_call" in ctx.hooks and "hermeschan" in ctx.commands,
      "plugin registers", f"{len(ctx.tools)} tools, {len(ctx.hooks)} hook names, /hermeschan")

# 3. Her body: the plugin builds it (first run) and starts it.
start = time.monotonic()
deadline = start + 30 * 60
bypassed = False
while not page_ready.is_set() and time.monotonic() < deadline:
    if runner.problem:
        if "session 0" in runner.problem and not bypassed:
            print(f"note: {runner.problem}; bypassing that check to test anyway", flush=True)
            runner_mod.desktop_problem = lambda: None
            runner.problem = None
            bypassed = True
            runner.maybe_start()
            continue
        check(False, "plugin builds and starts her body", runner.problem)
        print("---- build.log (tail)\n" + tail(home / "hermes-chan" / "build.log"), flush=True)
        print("---- overlay.log (tail)\n" + tail(home / "hermes-chan" / "overlay.log"), flush=True)
        finish(1)
    time.sleep(1)
proc = runner._proc  # noqa: SLF001
if not check(page_ready.is_set() and proc is not None, "plugin builds and starts her body",
             f"page ready after {time.monotonic() - start:.0f}s ({runner.describe()})"):
    print("---- overlay.log (tail)\n" + tail(home / "hermes-chan" / "overlay.log"), flush=True)
    finish(1)
check(bridge.is_connected, "her page talks to the plugin over the bridge",
      "TCP + token" if sys.platform == "win32" else "owner-only socket")

# 1. Environments the plugin handed out.
build_env, body_env = seen_env.get("build", {}), seen_env.get("body", {})
check(bool(build_env) and not set(FAKE_KEYS) & set(build_env), "cargo build gets no API keys",
      f"{len(build_env)} vars: {', '.join(sorted(build_env))}")
check(bool(body_env) and not set(FAKE_KEYS) & set(body_env), "her body gets no API keys",
      f"{len(body_env)} vars: {', '.join(sorted(body_env))}")
if sys.platform.startswith("linux"):
    actual = Path(f"/proc/{proc.pid}/environ").read_bytes().split(b"\0")
    names = sorted({item.split(b"=", 1)[0].decode() for item in actual if b"=" in item})
    check(not set(FAKE_KEYS) & set(names), "her running process has no API keys (/proc)",
          ", ".join(names))

# 2. Sprites embedded in the binary the plugin built.
binary = Path(proc.args[0])
embedded = {m.decode() for m in re.findall(rb"/hermeschan/((?:manifest\.json|atlases/[a-z-]+\.png))",
                                            binary.read_bytes())}
check(len(embedded) == 7, "her body embeds all 7 sprite files", ", ".join(sorted(embedded)))

time.sleep(6)  # her welcome bubble
bridge.say(f"Hello from {OS_NAME} ♡", emotion="happy")
time.sleep(3)
screenshot(OUT / f"{OS_NAME}-screen.png")
time.sleep(5)
check(proc.poll() is None, "she is still running", f"pid {proc.pid}")

# 4. Tidy approval hook and the tidy/undo moves on this OS.
desk = home / "Desktop"
desk.mkdir()
for name in ("a.png", "b.pdf", "c.txt"):
    (desk / name).write_text(name)
tidier = desktop_cleanup.DesktopTidier(desktop=desk, journal=home / "tidy-journal.jsonl")
ctx2 = StandInHermes()
tools_mod.register(ctx2, FakeBridge(), tidier)
hook = ctx2.hooks["pre_tool_call"][0]
tidy, undo = ctx2.tools["hermeschan_tidy_desktop"], ctx2.tools["hermeschan_undo_tidy"]
token = json.loads(tidy({"rule": "by-type"}))["token"]
plan_directive = hook(tool_name="hermeschan_tidy_desktop", args={"rule": "by-type"})
run_args = {"execute": True, "token": token, "confirm": True}
run_directive = hook(tool_name="hermeschan_tidy_desktop", args=run_args)
moved = json.loads(tidy(run_args))
undo_directive = hook(tool_name="hermeschan_undo_tidy", args={"token": token, "confirm": True})
undone = json.loads(undo({"token": token, "confirm": True}))
back = sorted(p.name for p in desk.iterdir() if p.is_file())
check(plan_directive is None, "planning a tidy needs no approval")
check(run_directive == {"action": "approve", "message": "moves 3 Desktop files into 'Hermes-chan Tidy'"},
      "running a tidy asks for approval", json.dumps(run_directive))
check(moved.get("ok") and len(moved.get("moved", [])) == 3, "tidy moves the files", f"{len(moved.get('moved', []))} moved")
check(undo_directive == {"action": "approve", "message": "moves 3 Desktop files back to where they were"},
      "undo asks for approval", json.dumps(undo_directive))
check(undone.get("ok") and back == ["a.png", "b.pdf", "c.txt"], "undo puts them back", ", ".join(back))

finish(0 if all(ok for ok, _, _ in results) else 1)
