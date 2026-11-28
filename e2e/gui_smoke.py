#!/usr/bin/env python3
"""Real-OS GUI window smoke E2E for safe-your-eyes (Python 3 stdlib only).

Verifies against the actual desktop window server (X11/Xvfb on Linux, Quartz on
macOS, Win32 on Windows) that:
1. `--internal-settings` opens a visible desktop window with non-zero bounds
   and exits cleanly when closed.
2. `--internal-overlay` emits `{"kind":"shown"}`, opens visible overlay
   window(s) covering every active display, and exits with a completed outcome.
3. On Linux and Windows, closing the daemon's startup Settings window keeps
   the daemon running and preserves its active countdown.
"""

from __future__ import annotations

import argparse
import ctypes
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import NamedTuple

REPO_ROOT = Path(__file__).resolve().parent.parent
TMP_BASE = REPO_ROOT / ".tmp" / f"gui-smoke-{os.getpid()}"
SM_CMONITORS = 80
SETTINGS_TITLE = "Safe Your Eyes — Settings"


class VisibleWindow(NamedTuple):
    handle: int
    title: str
    width: int
    height: int


def _linux_active_display_count() -> int:
    out = subprocess.check_output(
        ["xrandr", "--listmonitors"],
        text=True,
        stderr=subprocess.DEVNULL,
        timeout=5,
    )
    count = sum(1 for line in out.splitlines() if re.match(r"^\s*\d+:", line))
    if count < 1:
        raise RuntimeError(f"xrandr reported no active monitors: {out!r}")
    return count


def _linux_visible_windows(pid: int) -> list[VisibleWindow]:
    return _linux_search_windows(["--pid", str(pid)])


def _linux_search_windows(search_args: list[str]) -> list[VisibleWindow]:
    proc = subprocess.run(
        ["xdotool", "search", "--onlyvisible", *search_args],
        text=True,
        capture_output=True,
        timeout=5,
        check=False,
    )
    if proc.returncode != 0:
        return []
    windows: list[VisibleWindow] = []
    for raw_id in proc.stdout.split():
        try:
            wid = int(raw_id)
        except ValueError:
            continue
        info = subprocess.run(
            ["xwininfo", "-id", str(wid)],
            text=True,
            capture_output=True,
            timeout=5,
            check=False,
        )
        if info.returncode != 0 or "Map State: IsViewable" not in info.stdout:
            continue
        w_match = re.search(r"Width:\s*(\d+)", info.stdout)
        h_match = re.search(r"Height:\s*(\d+)", info.stdout)
        title_match = re.search(r'xwininfo: Window id: \S+ "(.*)"', info.stdout)
        width = int(w_match.group(1)) if w_match else 0
        height = int(h_match.group(1)) if h_match else 0
        title = title_match.group(1) if title_match else ""
        if width > 0 and height > 0:
            windows.append(VisibleWindow(wid, title, width, height))
    return windows


def _linux_close_window(handle: int) -> None:
    # Under bare Xvfb (no EWMH window manager), `xdotool windowclose` falls back
    # to XDestroyWindow; Ctrl+W triggers GTK's normal close-request path.
    subprocess.run(
        ["xdotool", "key", "--window", str(handle), "ctrl+w"],
        capture_output=True,
        timeout=5,
        check=False,
    )


def _macos_active_display_count() -> int:
    cg = ctypes.CDLL(
        "/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics"
    )
    count = ctypes.c_uint32(0)
    cg.CGGetActiveDisplayList.argtypes = [
        ctypes.c_uint32,
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_uint32),
    ]
    cg.CGGetActiveDisplayList.restype = ctypes.c_int32
    err = cg.CGGetActiveDisplayList(0, None, ctypes.byref(count))
    if err != 0 or count.value < 1:
        raise RuntimeError(
            f"CGGetActiveDisplayList failed (err={err}, count={count.value})"
        )
    return int(count.value)


MACOS_PROBE = TMP_BASE / "macos_windows"


def _macos_build_probe() -> None:
    # Query onscreen Quartz windows via Swift/CoreGraphics; avoids third-party
    # PyObjC while decoding CFDictionary bounds cleanly. Compiled once because
    # `swift <file>` recompiles on every run, which outlasts a short break.
    swift_src = r"""
import CoreGraphics
import Foundation

let targetPid = Int32(CommandLine.arguments[1]) ?? -1
let opts: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
guard let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
    print("[]")
    exit(0)
}
var out: [[String: Any]] = []
for entry in list {
    guard let ownerPid = entry[kCGWindowOwnerPID as String] as? Int32, ownerPid == targetPid else {
        continue
    }
    let alpha = (entry[kCGWindowAlpha as String] as? Double) ?? 1.0
    let bounds = entry[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = Int((bounds["Width"] as? Double) ?? 0)
    let height = Int((bounds["Height"] as? Double) ?? 0)
    let number = Int((entry[kCGWindowNumber as String] as? Int) ?? 0)
    let name = (entry[kCGWindowName as String] as? String) ?? ""
    if alpha > 0 && width > 0 && height > 0 {
        out.append(["handle": number, "title": name, "width": width, "height": height])
    }
}
if let data = try? JSONSerialization.data(withJSONObject: out),
   let text = String(data: data, encoding: .utf8) {
    print(text)
}
"""
    script_path = TMP_BASE / "macos_windows.swift"
    script_path.write_text(swift_src, encoding="utf-8")
    subprocess.run(
        ["swiftc", "-O", "-o", str(MACOS_PROBE), str(script_path)],
        capture_output=True,
        timeout=300,
        check=True,
    )


def _macos_visible_windows(pid: int) -> list[VisibleWindow]:
    proc = subprocess.run(
        [str(MACOS_PROBE), str(pid)],
        text=True,
        capture_output=True,
        timeout=15,
        check=False,
    )
    if proc.returncode != 0 or not proc.stdout.strip():
        return []
    try:
        items = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return []
    return [
        VisibleWindow(
            int(item["handle"]),
            str(item["title"]),
            int(item["width"]),
            int(item["height"]),
        )
        for item in items
    ]


def _windows_active_display_count() -> int:
    user32 = ctypes.windll.user32  # type: ignore[attr-defined]
    return max(int(user32.GetSystemMetrics(SM_CMONITORS)), 1)


def _windows_visible_windows(
    pid: int | None, title: str | None = None
) -> list[VisibleWindow]:
    user32 = ctypes.windll.user32  # type: ignore[attr-defined]

    class RECT(ctypes.Structure):
        _fields_ = [
            ("left", ctypes.c_long),
            ("top", ctypes.c_long),
            ("right", ctypes.c_long),
            ("bottom", ctypes.c_long),
        ]

    user32.IsWindowVisible.argtypes = [ctypes.c_void_p]
    user32.IsWindowVisible.restype = ctypes.c_bool
    user32.GetWindowThreadProcessId.argtypes = [
        ctypes.c_void_p,
        ctypes.POINTER(ctypes.c_ulong),
    ]
    user32.GetWindowThreadProcessId.restype = ctypes.c_ulong
    user32.GetWindowRect.argtypes = [ctypes.c_void_p, ctypes.POINTER(RECT)]
    user32.GetWindowRect.restype = ctypes.c_bool
    user32.GetWindowTextLengthW.argtypes = [ctypes.c_void_p]
    user32.GetWindowTextLengthW.restype = ctypes.c_int
    user32.GetWindowTextW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]
    user32.GetWindowTextW.restype = ctypes.c_int

    windows: list[VisibleWindow] = []
    enum_proc_t = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    user32.EnumWindows.argtypes = [enum_proc_t, ctypes.c_void_p]
    user32.EnumWindows.restype = ctypes.c_bool

    def callback(hwnd: int, _lparam: int) -> bool:
        if not user32.IsWindowVisible(hwnd):
            return True
        owner_pid = ctypes.c_ulong(0)
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner_pid))
        if pid is not None and owner_pid.value != pid:
            return True
        rect = RECT()
        if not user32.GetWindowRect(hwnd, ctypes.byref(rect)):
            return True
        width = rect.right - rect.left
        height = rect.bottom - rect.top
        if width <= 0 or height <= 0:
            return True
        length = user32.GetWindowTextLengthW(hwnd)
        buf = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, buf, length + 1)
        if title is not None and buf.value != title:
            return True
        windows.append(VisibleWindow(int(hwnd), buf.value, int(width), int(height)))
        return True

    user32.EnumWindows(enum_proc_t(callback), 0)
    return windows


def _windows_close_window(handle: int) -> None:
    user32 = ctypes.windll.user32  # type: ignore[attr-defined]
    user32.PostMessageW.argtypes = [
        ctypes.c_void_p,
        ctypes.c_uint,
        ctypes.c_size_t,
        ctypes.c_ssize_t,
    ]
    user32.PostMessageW.restype = ctypes.c_bool
    wm_close = 0x0010
    user32.PostMessageW(handle, wm_close, 0, 0)


def active_display_count() -> int:
    sys_name = platform.system()
    if sys_name == "Linux":
        return _linux_active_display_count()
    if sys_name == "Darwin":
        return _macos_active_display_count()
    if sys_name == "Windows":
        return _windows_active_display_count()
    return 1


def visible_windows(pid: int) -> list[VisibleWindow]:
    sys_name = platform.system()
    if sys_name == "Linux":
        return _linux_visible_windows(pid)
    if sys_name == "Darwin":
        return _macos_visible_windows(pid)
    if sys_name == "Windows":
        return _windows_visible_windows(pid)
    return []


def wait_for_windows(
    pid: int,
    *,
    min_count: int,
    min_width: int,
    min_height: int,
    timeout_s: float,
) -> list[VisibleWindow]:
    deadline = time.monotonic() + timeout_s
    last: list[VisibleWindow] = []
    while time.monotonic() < deadline:
        found = [
            w
            for w in visible_windows(pid)
            if w.width >= min_width and w.height >= min_height
        ]
        last = found
        if len(found) >= min_count:
            return found
        time.sleep(0.2)
    raise AssertionError(
        f"Expected >= {min_count} visible window(s) (>= {min_width}x{min_height}) "
        f"for pid {pid} within {timeout_s:.1f}s; saw {last!r}"
    )


def windows_titled(title: str) -> list[VisibleWindow]:
    sys_name = platform.system()
    if sys_name == "Linux":
        return [
            w
            for w in _linux_search_windows(["--name", re.escape(title)])
            if w.title == title
        ]
    if sys_name == "Windows":
        return _windows_visible_windows(None, title)
    return []


def close_window(handle: int) -> None:
    if platform.system() == "Linux":
        _linux_close_window(handle)
    else:
        _windows_close_window(handle)


def _read_log(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="replace")


def verify_settings_window(binary: Path) -> None:
    cfg = TMP_BASE / "config.toml"
    sock = TMP_BASE / "sye.sock"
    log_path = TMP_BASE / "settings.stderr.log"
    log = log_path.open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [
            str(binary),
            "--internal-settings",
            "--config",
            str(cfg),
            "--socket",
            str(sock),
        ],
        stdout=subprocess.DEVNULL,
        stderr=log,
        text=True,
    )
    try:
        wins = wait_for_windows(
            proc.pid, min_count=1, min_width=400, min_height=300, timeout_s=20.0
        )
        print(f"PASS settings window visible: {wins}")
        sys_name = platform.system()
        if sys_name in ("Linux", "Windows"):
            close_window(wins[0].handle)
            rc = proc.wait(timeout=10.0)
            if rc != 0:
                raise AssertionError(f"Settings exited with {rc}: {_read_log(log_path)}")
        else:
            proc.terminate()
            proc.wait(timeout=10.0)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5.0)
        log.close()


def daemon_status(binary: Path, cfg: Path, sock: Path) -> dict[str, object]:
    out = subprocess.run(
        [str(binary), "--status", "--json", "--config", str(cfg), "--socket", str(sock)],
        text=True,
        capture_output=True,
        timeout=10,
        check=True,
    ).stdout
    status = json.loads(out)
    assert isinstance(status, dict), out
    return status


def verify_daemon_survives_settings_close(binary: Path) -> None:
    base = TMP_BASE / "daemon"
    base.mkdir(parents=True, exist_ok=True)
    cfg = base / "config.toml"
    # Idle CI runners report system idle > 300s, which puts SmartPause into
    # `resting` and freezes the countdown. Disable SmartPause for this check.
    # A first non-headless run registers a login item unless the config turns autostart off.
    cfg.write_text("autostart = false\n\n[smart_pause]\nenabled = false\n", encoding="utf-8")
    sock = base / "sye.sock"
    log_path = base / "daemon.stderr.log"
    log = log_path.open("w", encoding="utf-8")
    daemon = subprocess.Popen(
        [str(binary), "--config", str(cfg), "--socket", str(sock)],
        stdout=subprocess.DEVNULL,
        stderr=log,
        text=True,
    )
    try:
        deadline = time.monotonic() + 20.0
        wins: list[VisibleWindow] = []
        while time.monotonic() < deadline and not wins:
            if daemon.poll() is not None:
                raise AssertionError(
                    f"Daemon exited early ({daemon.returncode}): {_read_log(log_path)}"
                )
            wins = windows_titled(SETTINGS_TITLE)
            time.sleep(0.2)
        if not wins:
            raise AssertionError("Daemon never opened the startup Settings window")
        # Wait until the countdown drops at least 5s below its 900s start so a
        # restart on close cannot be masked by the 1s post-close wait.
        before: dict[str, object] = {}
        tick_deadline = time.monotonic() + 12.0
        while time.monotonic() < tick_deadline:
            before = daemon_status(binary, cfg, sock)
            if (
                before.get("state") == "waiting"
                and int(before["seconds_until_next_break"]) <= 895  # type: ignore[arg-type]
            ):
                break
            time.sleep(0.3)
        if (
            before.get("state") != "waiting"
            or int(before["seconds_until_next_break"]) > 895  # type: ignore[arg-type]
        ):
            raise AssertionError(
                f"Expected waiting countdown <= 895s before closing Settings, got {before}"
            )

        close_window(wins[0].handle)
        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline and windows_titled(SETTINGS_TITLE):
            time.sleep(0.2)
        if windows_titled(SETTINGS_TITLE):
            raise AssertionError("Settings window did not close")
        time.sleep(1.0)

        if daemon.poll() is not None:
            raise AssertionError(
                f"Closing Settings stopped the daemon ({daemon.returncode}): "
                f"{_read_log(log_path)}"
            )
        after = daemon_status(binary, cfg, sock)
        if after.get("state") != "waiting" or int(
            after["seconds_until_next_break"]  # type: ignore[arg-type]
        ) >= int(before["seconds_until_next_break"]):  # type: ignore[arg-type]
            raise AssertionError(
                f"Closing Settings changed the countdown: before={before} after={after}"
            )
        print(f"PASS daemon survives Settings close, countdown kept: {before} -> {after}")
    finally:
        subprocess.run(
            [str(binary), "--quit", "--config", str(cfg), "--socket", str(sock)],
            capture_output=True,
            timeout=10,
            check=False,
        )
        try:
            daemon.wait(timeout=10.0)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=5.0)
        log.close()


def verify_overlay_windows(binary: Path) -> None:
    expected_displays = active_display_count()
    payload = json.dumps(
        {
            "break_type": "short",
            "title": "GUI Smoke Overlay Check",
            "duration_secs": 8,
            "strict_break": False,
            "allow_postpone": True,
            "postpone_duration_secs": 60,
            "shortcut_disable_secs": 0,
            "fade_in_ms": 0,
            "status_note": None,
        }
    )
    log_path = TMP_BASE / "overlay.stderr.log"
    log = log_path.open("w", encoding="utf-8")
    proc = subprocess.Popen(
        [str(binary), "--internal-overlay", payload],
        stdout=subprocess.PIPE,
        stderr=log,
        text=True,
    )
    events: list[dict[str, object]] = []
    raw_lines: list[str] = []

    def reader() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            text = line.strip()
            if not text:
                continue
            raw_lines.append(text)
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict):
                events.append(parsed)

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()

    try:
        deadline = time.monotonic() + 15.0
        while time.monotonic() < deadline and not any(
            e.get("kind") == "shown" for e in events
        ):
            if proc.poll() is not None:
                thread.join(timeout=1.0)
                raise AssertionError(
                    f"Overlay exited early ({proc.returncode}) before shown event; "
                    f"stdout={raw_lines!r} stderr={_read_log(log_path)}"
                )
            time.sleep(0.05)
        if not any(e.get("kind") == "shown" for e in events):
            raise AssertionError(
                f"Overlay never emitted shown event; stdout={raw_lines!r}"
            )

        wins = wait_for_windows(
            proc.pid,
            min_count=expected_displays,
            min_width=400,
            min_height=300,
            timeout_s=10.0,
        )
        print(
            f"PASS overlay windows visible ({len(wins)}/{expected_displays} display(s)): {wins}"
        )

        rc = proc.wait(timeout=15.0)
        thread.join(timeout=2.0)
        if rc != 0:
            raise AssertionError(f"Overlay exited with {rc}: {_read_log(log_path)}")
        if not any(e.get("kind") == "completed" for e in events):
            raise AssertionError(
                f"Expected completed overlay outcome, got stdout={raw_lines!r}"
            )
        print(f"PASS overlay completed cleanly: {events}")
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5.0)
        log.close()


def main() -> int:
    parser = argparse.ArgumentParser(description="Real-OS GUI window smoke E2E")
    parser.add_argument("--bin", required=True, help="Path to safe-your-eyes binary")
    args = parser.parse_args()

    binary = Path(args.bin).resolve()
    if not binary.is_file():
        print(f"Binary not found: {binary}", file=sys.stderr)
        return 2

    TMP_BASE.mkdir(parents=True, exist_ok=True)
    try:
        if platform.system() == "Darwin":
            _macos_build_probe()
        verify_settings_window(binary)
        verify_overlay_windows(binary)
        if platform.system() in ("Linux", "Windows"):
            verify_daemon_survives_settings_close(binary)
    finally:
        shutil.rmtree(TMP_BASE, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
