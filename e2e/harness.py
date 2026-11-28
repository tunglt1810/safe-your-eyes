"""Opaque-box E2E test harness for safe-your-eyes (Python 3 stdlib only)."""

from __future__ import annotations

import atexit
import copy
import json
import os
import secrets
import shutil
import signal
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, TypeVar

try:
    import tomllib
except ModuleNotFoundError as exc:  # pragma: no cover
    raise RuntimeError("Python >= 3.11 is required for stdlib tomllib") from exc

T = TypeVar("T")

REPO_ROOT = Path(__file__).resolve().parent.parent
E2E_TMP_BASE = REPO_ROOT / ".tmp" / "e2e"
MAX_UDS_PATH_BYTES = 103

VALID_STATES = frozenset(
    {"start", "waiting", "prebreak", "break", "stopped", "quit", "resting"}
)
REQUIRED_STATUS_KEYS = frozenset(
    {
        "state",
        "enabled",
        "next_break_type",
        "next_break_name",
        "seconds_until_next_break",
        "consecutive_skips",
    }
)

_LOCK = threading.Lock()
_SOCK_SEQ = 0
_ACTIVE_DAEMONS: list[DaemonHandle] = []
_ACTIVE_SOCKETS: list[Path] = []
_ACTIVE_RUN_DIRS: list[Path] = []
_KEEP_TMP_GLOBAL = False
_DEFAULT_CONFIG_CACHE: dict[Path, dict[str, Any]] = {}


class DaemonStartError(RuntimeError):
    """Raised when the daemon exits prematurely or fails readiness polling."""


class TomlValidationError(ValueError):
    """Raised when TOML parsing fails."""


@dataclass(frozen=True)
class CommandResult:
    args: list[str]
    returncode: int
    stdout: str
    stderr: str
    duration_s: float


def normalize_state(state: str) -> str:
    """Normalize SchedulerState strings across PascalCase / snake_case / kebab-case."""
    return state.strip().lower().replace("_", "").replace("-", "")


def parse_toml(text: str) -> dict[str, Any]:
    """Parse TOML text using Python stdlib tomllib."""
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise TomlValidationError(f"Invalid TOML: {exc}") from exc
    if not isinstance(data, dict):
        raise TomlValidationError("TOML root must be a table")
    return data


def _toml_scalar(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ", ".join(_toml_scalar(item) for item in value) + "]"
    raise TypeError(f"Unsupported TOML scalar type: {type(value).__name__}")


def toml_dumps(data: dict[str, Any]) -> str:
    """Serialize a Config dict (scalars, nested tables, array-of-tables) to valid TOML."""
    lines: list[str] = []
    tables: list[tuple[str, dict[str, Any]]] = []
    array_tables: list[tuple[str, list[dict[str, Any]]]] = []

    for key, val in data.items():
        if val is None:
            continue
        if isinstance(val, dict):
            tables.append((key, val))
        elif isinstance(val, list) and val and all(isinstance(x, dict) for x in val):
            array_tables.append((key, val))
        else:
            lines.append(f"{key} = {_toml_scalar(val)}")

    for name, tbl in tables:
        lines.append("")
        lines.append(f"[{name}]")
        for k, v in tbl.items():
            if v is None:
                continue
            lines.append(f"{k} = {_toml_scalar(v)}")

    for name, items in array_tables:
        for item in items:
            lines.append("")
            lines.append(f"[[{name}]]")
            for k, v in item.items():
                if v is None:
                    continue
                lines.append(f"{k} = {_toml_scalar(v)}")

    return "\n".join(lines) + "\n"


def deep_merge(base: dict[str, Any], overrides: dict[str, Any]) -> dict[str, Any]:
    """Return a deep-merged copy of base with overrides applied."""
    result = copy.deepcopy(base)
    for key, val in overrides.items():
        if isinstance(val, dict) and isinstance(result.get(key), dict):
            result[key] = deep_merge(result[key], val)
        else:
            result[key] = copy.deepcopy(val)
    return result


def wait_until(
    predicate: Callable[[], T],
    *,
    timeout: float = 5.0,
    interval: float = 0.08,
    desc: str = "condition",
) -> T:
    """Poll predicate until truthy or timeout expires."""
    deadline = time.monotonic() + timeout
    last_val: Any = None
    last_exc: Exception | None = None
    start = time.monotonic()
    while True:
        try:
            val = predicate()
            last_val = val
            last_exc = None
            if val:
                return val
        except Exception as exc:  # noqa: BLE001
            last_exc = exc
        if time.monotonic() >= deadline:
            elapsed = time.monotonic() - start
            detail = f"last_exc={last_exc!r}" if last_exc else f"last_val={last_val!r}"
            raise TimeoutError(
                f"Timed out after {elapsed:.2f}s waiting for {desc} ({detail})"
            )
        time.sleep(interval)


def _check_uds_path_len(candidate: Path) -> Path:
    encoded_len = len(str(candidate).encode("utf-8"))
    if encoded_len > MAX_UDS_PATH_BYTES:
        raise RuntimeError(
            f"UDS socket path exceeds {MAX_UDS_PATH_BYTES} bytes "
            f"({encoded_len}): {candidate}"
        )
    return candidate


def _unlink_socket(sock: Path) -> None:
    try:
        if sock.exists() or sock.is_symlink():
            sock.unlink()
    except OSError:
        pass


def cleanup_run_dir(run_dir: Path) -> None:
    shutil.rmtree(run_dir, ignore_errors=True)
    try:
        if E2E_TMP_BASE.exists() and not any(E2E_TMP_BASE.iterdir()):
            E2E_TMP_BASE.rmdir()
    except OSError:
        pass


def _alloc_flat_socket() -> Path:
    """Allocate a short flat UDS socket path directly under <repo>/.tmp/e2e/."""
    global _SOCK_SEQ
    E2E_TMP_BASE.mkdir(parents=True, exist_ok=True)
    if os.name == "nt":
        with _LOCK:
            _SOCK_SEQ += 1
            seq = _SOCK_SEQ
        return Path(f"sye_e2e_{os.getpid() % 65536:04x}{seq % 4096:03x}")

    pid_hex = f"{os.getpid() % 65536:04x}"
    for _ in range(64):
        with _LOCK:
            _SOCK_SEQ += 1
            seq = _SOCK_SEQ
        candidate = E2E_TMP_BASE / f"s_{pid_hex}{seq % 4096:03x}.sock"
        if not candidate.exists():
            return _check_uds_path_len(candidate)

    return _check_uds_path_len(E2E_TMP_BASE / f"s_{secrets.token_hex(4)}.sock")


def _cleanup_all_global() -> None:
    """Atexit/signal safety net: stop leaked daemons and clean workspace."""
    with _LOCK:
        daemons = list(reversed(_ACTIVE_DAEMONS))
    for handle in daemons:
        try:
            handle.stop(graceful_timeout=1.5)
        except Exception:  # noqa: BLE001
            pass

    if not _KEEP_TMP_GLOBAL:
        with _LOCK:
            sockets = list(_ACTIVE_SOCKETS)
            run_dirs = list(_ACTIVE_RUN_DIRS)
            _ACTIVE_SOCKETS.clear()
            _ACTIVE_RUN_DIRS.clear()
        for sock in sockets:
            _unlink_socket(sock)
        for rdir in run_dirs:
            cleanup_run_dir(rdir)


atexit.register(_cleanup_all_global)


def set_keep_tmp_global(keep: bool) -> None:
    global _KEEP_TMP_GLOBAL
    _KEEP_TMP_GLOBAL = keep


def register_run_dir(run_dir: Path) -> None:
    with _LOCK:
        if run_dir not in _ACTIVE_RUN_DIRS:
            _ACTIVE_RUN_DIRS.append(run_dir)


class DaemonHandle:
    """Manages a spawned safe-your-eyes daemon subprocess with file-backed logs."""

    def __init__(
        self,
        *,
        env: SyeTestEnv,
        proc: subprocess.Popen[str],
        socket_path: Path,
        stdout_path: Path,
        stderr_path: Path,
        stdout_fh: Any,
        stderr_fh: Any,
    ) -> None:
        self._env = env
        self.proc = proc
        self.socket_path = socket_path
        self.stdout_path = stdout_path
        self.stderr_path = stderr_path
        self._stdout_fh = stdout_fh
        self._stderr_fh = stderr_fh
        self._closed = False
        with _LOCK:
            _ACTIVE_DAEMONS.append(self)

    @property
    def pid(self) -> int:
        return self.proc.pid

    def poll(self) -> int | None:
        return self.proc.poll()

    @staticmethod
    def _read_log_file(fh: Any, path: Path) -> str:
        try:
            if not fh.closed:
                fh.flush()
            return path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return ""

    def read_stdout(self) -> str:
        return self._read_log_file(self._stdout_fh, self.stdout_path)

    def read_stderr(self) -> str:
        return self._read_log_file(self._stderr_fh, self.stderr_path)

    def wait_until_ready(self, timeout: float = 5.0) -> dict[str, Any]:
        """Poll --status --json until daemon is responsive or exits prematurely."""
        deadline = time.monotonic() + timeout
        last_err = ""
        while True:
            rc = self.proc.poll()
            if rc is not None:
                raise DaemonStartError(
                    f"Daemon exited prematurely with code {rc} on {self.socket_path}\n"
                    f"--- daemon stdout ---\n{self.read_stdout()}\n"
                    f"--- daemon stderr ---\n{self.read_stderr()}"
                )
            try:
                res = self._env.run_cli(
                    ["--status", "--json"],
                    socket=self.socket_path,
                    config=None,
                    timeout=2.0,
                )
            except subprocess.TimeoutExpired as exc:
                last_err = f"status poll timed out: {exc}"
            else:
                if res.returncode == 0:
                    try:
                        data = json.loads(res.stdout)
                        if isinstance(data, dict):
                            return data
                    except json.JSONDecodeError as exc:
                        last_err = f"Invalid JSON: {exc}; stdout={res.stdout!r}"
                else:
                    last_err = f"rc={res.returncode}; stderr={res.stderr.strip()!r}"

            if time.monotonic() >= deadline:
                stdout = self.read_stdout()
                stderr = self.read_stderr()
                self.stop(graceful_timeout=1.5)
                raise DaemonStartError(
                    f"Daemon did not become ready within {timeout:.1f}s "
                    f"on {self.socket_path} ({last_err})\n"
                    f"--- daemon stdout ---\n{stdout}\n"
                    f"--- daemon stderr ---\n{stderr}"
                )
            time.sleep(0.06)

    def _close_logs(self) -> None:
        if not self._closed:
            self._closed = True
            for fh in (self._stdout_fh, self._stderr_fh):
                try:
                    if not fh.closed:
                        fh.close()
                except OSError:
                    pass
            with _LOCK:
                if self in _ACTIVE_DAEMONS:
                    _ACTIVE_DAEMONS.remove(self)

    def stop(self, graceful_timeout: float = 3.0) -> int:
        """3-stage guaranteed shutdown: --quit -> SIGTERM -> SIGKILL."""
        try:
            if self.proc.poll() is not None:
                return self.proc.returncode

            # Stage 1: graceful IPC --quit
            try:
                self._env.run_cli(
                    ["--quit"],
                    socket=self.socket_path,
                    config=None,
                    timeout=2.0,
                )
                return self.proc.wait(timeout=graceful_timeout)
            except (subprocess.TimeoutExpired, OSError):
                pass

            if self.proc.poll() is not None:
                return self.proc.returncode

            # Stage 2: SIGTERM process group
            try:
                if os.name != "nt":
                    os.killpg(self.proc.pid, signal.SIGTERM)
                else:
                    self.proc.terminate()
                return self.proc.wait(timeout=2.0)
            except (ProcessLookupError, PermissionError, subprocess.TimeoutExpired, OSError):
                pass

            if self.proc.poll() is not None:
                return self.proc.returncode

            # Stage 3: SIGKILL process group
            try:
                if os.name != "nt":
                    os.killpg(self.proc.pid, signal.SIGKILL)
                else:
                    self.proc.kill()
            except (ProcessLookupError, PermissionError, OSError):
                try:
                    self.proc.kill()
                except OSError:
                    pass
            try:
                return self.proc.wait(timeout=1.5)
            except subprocess.TimeoutExpired:
                return -9
        finally:
            self._close_logs()


class SyeTestEnv:
    """Isolated per-test environment strictly under <repo>/.tmp/e2e/."""

    def __init__(
        self,
        *,
        binary: Path,
        run_dir: Path,
        test_id: str,
        verbose: bool = False,
        keep_tmp: bool = False,
    ) -> None:
        self.binary = binary
        self.run_dir = run_dir
        self.test_id = test_id
        self.verbose = verbose
        self.keep_tmp = keep_tmp

        self.root = run_dir / test_id
        self.home_dir = self.root / "home"
        self.tmp_dir = self.root / "tmp"
        self.xdg_config = self.root / "xdg_config"
        self.xdg_data = self.root / "xdg_data"
        self.xdg_state = self.root / "xdg_state"
        self.xdg_cache = self.root / "xdg_cache"
        self.xdg_runtime = self.root / "xdg_runtime"

        for d in (
            self.root,
            self.home_dir,
            self.tmp_dir,
            self.xdg_config,
            self.xdg_data,
            self.xdg_state,
            self.xdg_cache,
            self.xdg_runtime,
        ):
            d.mkdir(parents=True, exist_ok=True)
        try:
            os.chmod(self.xdg_runtime, 0o700)
        except OSError:
            pass

        self._lock = threading.Lock()
        self._daemons: list[DaemonHandle] = []
        self._sockets: list[Path] = []
        self._daemon_counter = 0
        self.default_socket = self.alloc_socket()
        self.last_commands: list[CommandResult] = []

    def __enter__(self) -> SyeTestEnv:
        return self

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.cleanup()

    def alloc_socket(self) -> Path:
        sock = _alloc_flat_socket()
        self._sockets.append(sock)
        with _LOCK:
            _ACTIVE_SOCKETS.append(sock)
        return sock

    def build_env(self, overrides: dict[str, str] | None = None) -> dict[str, str]:
        env: dict[str, str] = {}
        for key in (
            "PATH",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "TZ",
            "RUST_BACKTRACE",
        ):
            if key in os.environ:
                env[key] = os.environ[key]
        env.update(
            {
                "HOME": str(self.home_dir),
                "TMPDIR": str(self.tmp_dir),
                "TEMP": str(self.tmp_dir),
                "TMP": str(self.tmp_dir),
                "XDG_CONFIG_HOME": str(self.xdg_config),
                "XDG_DATA_HOME": str(self.xdg_data),
                "XDG_STATE_HOME": str(self.xdg_state),
                "XDG_CACHE_HOME": str(self.xdg_cache),
                "XDG_RUNTIME_DIR": str(self.xdg_runtime),
            }
        )
        if overrides:
            env.update(overrides)
        return env

    def config_path(self, name: str = "config.toml") -> Path:
        return self.root / name

    def write_config(self, content: str, name: str = "config.toml") -> Path:
        path = self.config_path(name)
        path.write_text(content, encoding="utf-8")
        return path

    def default_config_dict(self) -> dict[str, Any]:
        """Fetch and cache the baseline Config from --print-default-config."""
        if self.binary not in _DEFAULT_CONFIG_CACHE:
            res = self.run_cli(["--print-default-config"], socket=None, config=None)
            if res.returncode != 0:
                raise RuntimeError(
                    f"--print-default-config failed (rc={res.returncode}): {res.stderr}"
                )
            _DEFAULT_CONFIG_CACHE[self.binary] = parse_toml(res.stdout)
        return copy.deepcopy(_DEFAULT_CONFIG_CACHE[self.binary])

    def write_profile_config(
        self,
        overrides: dict[str, Any],
        *,
        name: str = "config.toml",
        disable_policies: bool = True,
    ) -> Path:
        """Build a valid TOML config from --print-default-config plus overrides."""
        base = self.default_config_dict()
        if disable_policies:
            policy_off = {
                "smart_pause": {"enabled": False},
                "do_not_disturb": {"enabled": False},
                "audible_alert": {"pre_break": False, "post_break": False, "volume": 0},
                "limit_consecutive_skips": {"enabled": False},
            }
            base = deep_merge(base, policy_off)
        merged = deep_merge(base, overrides)
        return self.write_config(toml_dumps(merged), name=name)

    def run_cli(
        self,
        args: list[str],
        *,
        socket: Path | None = None,
        config: Path | None = None,
        timeout: float = 10.0,
        env_overrides: dict[str, str] | None = None,
    ) -> CommandResult:
        cmd: list[str] = [str(self.binary)]
        if config is not None:
            cmd.extend(["--config", str(config)])
        if socket is not None:
            cmd.extend(["--socket", str(socket)])
        cmd.extend(args)

        start = time.monotonic()
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            env=self.build_env(env_overrides),
        )
        duration = time.monotonic() - start
        result = CommandResult(
            args=cmd,
            returncode=proc.returncode,
            stdout=proc.stdout,
            stderr=proc.stderr,
            duration_s=duration,
        )
        with self._lock:
            self.last_commands.append(result)
            if len(self.last_commands) > 12:
                self.last_commands = self.last_commands[-12:]
        return result

    def start_daemon(
        self,
        *,
        config: Path | None = None,
        socket: Path | None = None,
        headless: bool = True,
        extra_args: list[str] | None = None,
        ready_timeout: float = 6.0,
        wait_ready: bool = True,
        env_overrides: dict[str, str] | None = None,
    ) -> DaemonHandle:
        sock = socket if socket is not None else self.default_socket
        cmd: list[str] = [str(self.binary)]
        if config is not None:
            cmd.extend(["--config", str(config)])
        cmd.extend(["--socket", str(sock)])
        if headless:
            cmd.append("--headless")
        if extra_args:
            cmd.extend(extra_args)

        self._daemon_counter += 1
        stdout_path = self.root / f"daemon_{self._daemon_counter}.stdout.log"
        stderr_path = self.root / f"daemon_{self._daemon_counter}.stderr.log"
        stdout_fh = stdout_path.open("w", encoding="utf-8")
        stderr_fh = stderr_path.open("w", encoding="utf-8")

        popen_kwargs: dict[str, Any] = {
            "stdout": stdout_fh,
            "stderr": stderr_fh,
            "env": self.build_env(env_overrides),
        }
        if os.name != "nt":
            popen_kwargs["start_new_session"] = True

        proc = subprocess.Popen(cmd, **popen_kwargs)
        handle = DaemonHandle(
            env=self,
            proc=proc,
            socket_path=sock,
            stdout_path=stdout_path,
            stderr_path=stderr_path,
            stdout_fh=stdout_fh,
            stderr_fh=stderr_fh,
        )
        self._daemons.append(handle)
        if wait_ready:
            handle.wait_until_ready(timeout=ready_timeout)
        return handle

    def status_json(
        self, socket: Path | None = None, *, timeout: float = 3.0
    ) -> dict[str, Any]:
        sock = socket if socket is not None else self.default_socket
        res = self.run_cli(["--status", "--json"], socket=sock, timeout=timeout)
        assert res.returncode == 0, (
            f"--status --json failed (rc={res.returncode}): "
            f"stdout={res.stdout!r}, stderr={res.stderr!r}"
        )
        data = json.loads(res.stdout)
        assert isinstance(data, dict), f"Expected JSON object, got: {data!r}"
        return data

    def assert_status_schema(
        self, data: dict[str, Any], *, expect_enabled: bool | None = None
    ) -> str:
        missing = REQUIRED_STATUS_KEYS - set(data.keys())
        assert not missing, f"Missing keys in --status --json: {missing}; got {data!r}"
        assert isinstance(data["state"], str), f"state must be str: {data!r}"
        norm = normalize_state(data["state"])
        assert norm in VALID_STATES, f"Unknown state {data['state']!r} (norm={norm!r})"
        assert isinstance(data["enabled"], bool), f"enabled must be bool: {data!r}"
        if expect_enabled is not None:
            assert data["enabled"] is expect_enabled, (
                f"Expected enabled={expect_enabled}, got {data!r}"
            )
        assert data["next_break_type"] in ("short", "long", None), (
            f"Invalid next_break_type: {data['next_break_type']!r}"
        )
        assert data["next_break_name"] is None or isinstance(
            data["next_break_name"], str
        ), f"Invalid next_break_name: {data['next_break_name']!r}"
        secs = data["seconds_until_next_break"]
        assert secs is None or (
            isinstance(secs, int) and not isinstance(secs, bool) and secs >= 0
        ), f"Invalid seconds_until_next_break: {secs!r}"
        skips = data["consecutive_skips"]
        assert isinstance(skips, int) and not isinstance(skips, bool) and skips >= 0, (
            f"Invalid consecutive_skips: {skips!r}"
        )
        return norm

    def wait_for_state(
        self,
        target_states: str | set[str] | tuple[str, ...],
        *,
        socket: Path | None = None,
        timeout: float = 8.0,
        interval: float = 0.1,
    ) -> dict[str, Any]:
        targets = (
            {normalize_state(target_states)}
            if isinstance(target_states, str)
            else {normalize_state(s) for s in target_states}
        )
        sock = socket if socket is not None else self.default_socket

        def _check() -> dict[str, Any] | None:
            st = self.status_json(socket=sock)
            if normalize_state(st["state"]) in targets:
                return st
            return None

        return wait_until(
            _check,
            timeout=timeout,
            interval=interval,
            desc=f"state in {sorted(targets)}",
        )

    def format_diagnostics(self) -> str:
        parts: list[str] = []
        with self._lock:
            recent = list(self.last_commands[-5:])
        if recent:
            parts.append("Recent CLI commands:")
            for r in recent:
                parts.append(
                    f"  cmd={r.args} rc={r.returncode} "
                    f"stdout={r.stdout.strip()!r} stderr={r.stderr.strip()!r}"
                )
        for idx, d in enumerate(self._daemons, start=1):
            parts.append(f"Daemon #{idx} (pid={d.pid}, poll={d.poll()}):")
            out = d.read_stdout().strip()
            err = d.read_stderr().strip()
            if out:
                parts.append(f"  stdout: {out[-800:]}")
            if err:
                parts.append(f"  stderr: {err[-800:]}")
        return "\n".join(parts)

    def cleanup(self) -> None:
        for handle in reversed(self._daemons):
            try:
                handle.stop(graceful_timeout=2.0)
            except Exception:  # noqa: BLE001
                pass
        self._daemons.clear()

        if not self.keep_tmp:
            for sock in self._sockets:
                _unlink_socket(sock)
                with _LOCK:
                    if sock in _ACTIVE_SOCKETS:
                        _ACTIVE_SOCKETS.remove(sock)
            self._sockets.clear()
            shutil.rmtree(self.root, ignore_errors=True)
