"""Tier 1: Core feature coverage opaque-box E2E tests (5 areas, 28 tests)."""

from __future__ import annotations

import os
import re
import signal
import time

from e2e.harness import SyeTestEnv, normalize_state, parse_toml, wait_until

# ---------------------------------------------------------------------------
# Area 1A: CLI Info & Config Flags (6 tests)
# ---------------------------------------------------------------------------


def test_t1_cli_01_version_flag(env: SyeTestEnv) -> None:
    """-V and --version print `safe-your-eyes <semver>` and exit 0."""
    pattern = re.compile(r"^safe-your-eyes\s+\d+\.\d+\.\d+")
    for flag in ("--version", "-V"):
        res = env.run_cli([flag])
        assert res.returncode == 0, f"{flag} failed: rc={res.returncode}, stderr={res.stderr!r}"
        assert pattern.search(res.stdout.strip()), (
            f"{flag} stdout did not match version contract: {res.stdout!r}"
        )
        assert res.stderr.strip() == "", f"{flag} emitted unexpected stderr: {res.stderr!r}"


def test_t1_cli_02_print_default_config(env: SyeTestEnv) -> None:
    """--print-default-config emits valid TOML matching PROJECT.md defaults."""
    res = env.run_cli(["--print-default-config"])
    assert res.returncode == 0, f"rc={res.returncode}, stderr={res.stderr!r}"
    cfg = parse_toml(res.stdout)

    expected_scalars = {
        "short_break_interval_secs": 900,
        "short_break_duration_secs": 15,
        "long_break_interval_secs": 4500,
        "long_break_duration_secs": 60,
        "pre_break_warning_secs": 10,
        "strict_break": False,
        "allow_postpone": False,
        "postpone_duration_secs": 300,
        "random_order": True,
        "shortcut_disable_secs": 2,
        "fade_in_ms": 1500,
        "persist_state": False,
        "autostart": True,
    }
    for key, expected in expected_scalars.items():
        assert key in cfg, f"Missing default config key: {key}"
        assert cfg[key] == expected, f"Key {key}: expected {expected!r}, got {cfg[key]!r}"

    for table in ("smart_pause", "do_not_disturb", "audible_alert", "limit_consecutive_skips"):
        assert isinstance(cfg.get(table), dict), f"Missing or invalid table [{table}]"

    assert cfg["do_not_disturb"].get("while_gaming") is True, (
        f"Expected do_not_disturb.while_gaming=True, got {cfg['do_not_disturb'].get('while_gaming')!r}"
    )

    short_breaks = cfg.get("short_breaks")
    long_breaks = cfg.get("long_breaks")
    assert isinstance(short_breaks, list) and len(short_breaks) == 13, (
        f"Expected 13 default short_breaks, got {len(short_breaks) if isinstance(short_breaks, list) else short_breaks!r}"
    )
    assert isinstance(long_breaks, list) and len(long_breaks) == 6, (
        f"Expected 6 default long_breaks, got {len(long_breaks) if isinstance(long_breaks, list) else long_breaks!r}"
    )


def test_t1_cli_03_validate_default_config(env: SyeTestEnv) -> None:
    """--validate-config accepts the output of --print-default-config."""
    printed = env.run_cli(["--print-default-config"])
    assert printed.returncode == 0
    cfg_path = env.write_config(printed.stdout, name="default.toml")
    res = env.run_cli(["--validate-config"], config=cfg_path)
    assert res.returncode == 0, f"Default config rejected: stderr={res.stderr!r}"


def test_t1_cli_04_validate_custom_valid_config(env: SyeTestEnv) -> None:
    """--validate-config accepts a customized valid TOML config."""
    cfg_path = env.write_profile_config(
        {
            "short_break_interval_secs": 5,
            "short_break_duration_secs": 2,
            "long_break_interval_secs": 20,
            "long_break_duration_secs": 4,
            "pre_break_warning_secs": 1,
            "strict_break": True,
            "allow_postpone": True,
            "random_order": False,
            "short_breaks": [{"name": "Custom Short"}],
            "long_breaks": [{"name": "Custom Long"}],
        },
        name="custom.toml",
    )
    res = env.run_cli(["--validate-config"], config=cfg_path)
    assert res.returncode == 0, f"Custom valid config rejected: stderr={res.stderr!r}"


def test_t1_cli_05_help_output(env: SyeTestEnv) -> None:
    """--help exits 0 and documents core CLI options."""
    res = env.run_cli(["--help"])
    assert res.returncode == 0, f"--help failed: rc={res.returncode}, stderr={res.stderr!r}"
    for flag in (
        "--config",
        "--socket",
        "--headless",
        "--status",
        "--enable",
        "--disable",
        "--take-break",
        "--quit",
    ):
        assert flag in res.stdout, f"Expected {flag} in --help output"


def test_t1_cli_06_config_override_respected_by_daemon(env: SyeTestEnv) -> None:
    """Daemon loads custom --config values and exposes them via --status --json."""
    cfg_path = env.write_profile_config(
        {
            "short_break_interval_secs": 42,
            "short_break_duration_secs": 2,
            "long_break_interval_secs": 200,
            "long_break_duration_secs": 4,
            "pre_break_warning_secs": 2,
            "random_order": False,
            "short_breaks": [{"name": "Custom Eye Exercise"}],
            "long_breaks": [{"name": "Custom Long Exercise"}],
        }
    )
    env.start_daemon(config=cfg_path)
    st = env.status_json()
    env.assert_status_schema(st, expect_enabled=True)
    assert st["next_break_name"] == "Custom Eye Exercise", f"Unexpected break name: {st}"
    assert st["next_break_type"] == "short", f"Unexpected break type: {st}"
    assert 0 <= st["seconds_until_next_break"] <= 42, f"Unexpected countdown: {st}"


# ---------------------------------------------------------------------------
# Area 1B: Daemon Lifecycle & Single-Instance Lock (5 tests)
# ---------------------------------------------------------------------------


def test_t1_dmn_01_headless_daemon_startup_and_readiness(env: SyeTestEnv) -> None:
    """Headless daemon starts, stays alive, and reports enabled status."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    daemon = env.start_daemon(config=cfg)
    assert daemon.poll() is None, "Daemon exited unexpectedly"
    st = env.status_json()
    norm = env.assert_status_schema(st, expect_enabled=True)
    assert norm == "waiting", f"Expected waiting state on startup, got {st}"


def test_t1_dmn_02_single_instance_duplicate_launch(env: SyeTestEnv) -> None:
    """Second daemon on same --socket prints already-running message and exits 0."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    daemon = env.start_daemon(config=cfg)

    dup = env.run_cli(["--headless"], config=cfg, socket=env.default_socket, timeout=5.0)
    assert dup.returncode == 0, f"Duplicate launch rc={dup.returncode}, stderr={dup.stderr!r}"
    assert "Safe Your Eyes is already running" in dup.stdout, (
        f"Expected already-running message on stdout, got: {dup.stdout!r}"
    )
    assert daemon.poll() is None, "First daemon died after duplicate launch"
    st = env.status_json()
    env.assert_status_schema(st, expect_enabled=True)


def test_t1_dmn_03_graceful_stop_via_quit_and_q(env: SyeTestEnv) -> None:
    """Both --quit and -q stop the daemon cleanly with exit 0."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    for flag in ("--quit", "-q"):
        sock = env.alloc_socket()
        daemon = env.start_daemon(config=cfg, socket=sock)
        res = env.run_cli([flag], socket=sock)
        assert res.returncode == 0, f"{flag} failed: rc={res.returncode}, stderr={res.stderr!r}"
        rc = daemon.proc.wait(timeout=5.0)
        assert rc == 0, f"Daemon exited with non-zero code {rc} after {flag}"

        after = env.run_cli(["--status"], socket=sock)
        assert after.returncode == 1, f"Expected rc=1 after {flag}, got {after.returncode}"
        assert "Safe Your Eyes is not running" in after.stderr, (
            f"Expected not-running error, got stderr={after.stderr!r}"
        )


def test_t1_dmn_04_graceful_shutdown_via_sigterm(env: SyeTestEnv) -> None:
    """Daemon exits cleanly on SIGTERM and allows immediate restart on same socket."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    daemon = env.start_daemon(config=cfg)

    if os.name != "nt":
        daemon.proc.send_signal(signal.SIGTERM)
    else:
        daemon.proc.terminate()
    daemon.proc.wait(timeout=5.0)

    restarted = env.start_daemon(config=cfg, socket=env.default_socket)
    assert restarted.poll() is None
    st = env.status_json()
    env.assert_status_schema(st, expect_enabled=True)


def test_t1_dmn_05_multi_instance_isolation_via_socket(env: SyeTestEnv) -> None:
    """Two daemons on distinct --socket paths operate completely independently."""
    cfg_a = env.write_profile_config(
        {"short_break_interval_secs": 30, "short_breaks": [{"name": "Instance A"}]},
        name="a.toml",
    )
    cfg_b = env.write_profile_config(
        {"short_break_interval_secs": 30, "short_breaks": [{"name": "Instance B"}]},
        name="b.toml",
    )
    sock_a = env.alloc_socket()
    sock_b = env.alloc_socket()
    env.start_daemon(config=cfg_a, socket=sock_a)
    env.start_daemon(config=cfg_b, socket=sock_b)

    res = env.run_cli(["--disable"], socket=sock_a)
    assert res.returncode == 0

    st_a = env.status_json(socket=sock_a)
    st_b = env.status_json(socket=sock_b)
    assert st_a["enabled"] is False, f"Daemon A should be disabled: {st_a}"
    assert st_b["enabled"] is True, f"Daemon B should remain enabled: {st_b}"
    assert st_b["next_break_name"] == "Instance B"


# ---------------------------------------------------------------------------
# Area 1C: Status & IPC Control Commands (6 tests)
# ---------------------------------------------------------------------------


def test_t1_ipc_01_human_readable_status(env: SyeTestEnv) -> None:
    """--status prints non-empty human-readable text (not raw JSON)."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)
    res = env.run_cli(["--status"], socket=env.default_socket)
    assert res.returncode == 0, f"rc={res.returncode}, stderr={res.stderr!r}"
    out = res.stdout.strip()
    assert len(out) > 0, "--status produced empty stdout"
    assert not out.startswith("{"), f"Human-readable --status looks like raw JSON: {out!r}"


def test_t1_ipc_02_status_json_schema(env: SyeTestEnv) -> None:
    """--status --json returns exact contract fields with valid types."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)
    st = env.status_json()
    norm = env.assert_status_schema(st, expect_enabled=True)
    assert norm == "waiting"
    assert st["next_break_type"] in ("short", "long")
    assert isinstance(st["next_break_name"], str) and len(st["next_break_name"]) > 0
    assert isinstance(st["seconds_until_next_break"], int)
    assert st["consecutive_skips"] == 0


def test_t1_ipc_03_countdown_progression(env: SyeTestEnv) -> None:
    """seconds_until_next_break actively decreases while Waiting."""
    cfg = env.write_profile_config(
        {"short_break_interval_secs": 30, "pre_break_warning_secs": 2}
    )
    env.start_daemon(config=cfg)
    t0 = env.status_json()["seconds_until_next_break"]
    assert isinstance(t0, int) and t0 > 2

    def _decreased() -> bool:
        t1 = env.status_json()["seconds_until_next_break"]
        return isinstance(t1, int) and t1 < t0

    wait_until(_decreased, timeout=4.0, interval=0.2, desc="countdown to decrease")


def test_t1_ipc_04_disable_daemon(env: SyeTestEnv) -> None:
    """--disable transitions running daemon to Stopped with enabled=false."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)
    res = env.run_cli(["--disable"], socket=env.default_socket)
    assert res.returncode == 0, f"--disable failed: stderr={res.stderr!r}"
    st = env.status_json()
    norm = env.assert_status_schema(st, expect_enabled=False)
    assert norm == "stopped", f"Expected stopped state, got {st}"


def test_t1_ipc_05_reenable_daemon(env: SyeTestEnv) -> None:
    """--enable resumes a disabled daemon back to Waiting with enabled=true."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)
    assert env.run_cli(["--disable"], socket=env.default_socket).returncode == 0
    assert env.status_json()["enabled"] is False

    res = env.run_cli(["--enable"], socket=env.default_socket)
    assert res.returncode == 0, f"--enable failed: stderr={res.stderr!r}"
    st = env.status_json()
    norm = env.assert_status_schema(st, expect_enabled=True)
    assert norm == "waiting", f"Expected waiting state after --enable, got {st}"


def test_t1_ipc_06_short_flag_aliases(env: SyeTestEnv) -> None:
    """Short aliases -d, -e, -q work identically to long flags."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    daemon = env.start_daemon(config=cfg)

    assert env.run_cli(["-d"], socket=env.default_socket).returncode == 0
    st = env.status_json()
    assert st["enabled"] is False and normalize_state(st["state"]) == "stopped"

    assert env.run_cli(["-e"], socket=env.default_socket).returncode == 0
    st = env.status_json()
    assert st["enabled"] is True and normalize_state(st["state"]) == "waiting"

    assert env.run_cli(["-q"], socket=env.default_socket).returncode == 0
    assert daemon.proc.wait(timeout=5.0) == 0


# ---------------------------------------------------------------------------
# Area 1D: Break Scheduling & --headless Execution (6 tests)
# ---------------------------------------------------------------------------


def test_t1_sch_01_full_headless_short_break_cycle(env: SyeTestEnv) -> None:
    """Headless timer drives Waiting -> PreBreak -> Break -> Waiting."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 3,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 60,
            "long_break_duration_secs": 2,
            "pre_break_warning_secs": 1,
            "random_order": False,
            "short_breaks": [{"name": "Cycle Ex 1"}, {"name": "Cycle Ex 2"}],
            "long_breaks": [{"name": "Cycle Long 1"}],
        }
    )
    env.start_daemon(config=cfg)

    observed: list[str] = []
    deadline = time.monotonic() + 10.0
    while time.monotonic() < deadline:
        norm = normalize_state(env.status_json()["state"])
        if not observed or observed[-1] != norm:
            observed.append(norm)
        if "break" in observed and observed[-1] == "waiting":
            break
        time.sleep(0.1)

    assert "waiting" in observed, f"Never saw waiting: {observed}"
    assert "break" in observed, f"Never saw break: {observed}"
    assert observed[-1] == "waiting", f"Did not return to waiting after break: {observed}"


def test_t1_sch_02_sequential_break_order(env: SyeTestEnv) -> None:
    """random_order = false advances short_breaks sequentially and wraps around."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 30,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 300,
            "long_break_duration_secs": 2,
            "pre_break_warning_secs": 0,
            "random_order": False,
            "short_breaks": [{"name": "Ex1"}, {"name": "Ex2"}, {"name": "Ex3"}],
            "long_breaks": [{"name": "LongEx"}],
        }
    )
    env.start_daemon(config=cfg)
    assert env.status_json()["next_break_name"] == "Ex1"

    for expected_next in ("Ex2", "Ex3", "Ex1"):
        res = env.run_cli(["-b"], socket=env.default_socket)
        assert res.returncode == 0
        env.wait_for_state("break", timeout=3.0)
        st = env.wait_for_state("waiting", timeout=5.0)
        assert st["next_break_name"] == expected_next, (
            f"Expected {expected_next!r}, got {st['next_break_name']!r}"
        )


def test_t1_sch_03_dual_tier_short_long_ratio(env: SyeTestEnv) -> None:
    """Scheduler alternates short -> long -> short per configured interval ratio."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 2,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 4,
            "long_break_duration_secs": 1,
            "pre_break_warning_secs": 0,
            "random_order": False,
            "short_breaks": [{"name": "ShortA"}, {"name": "ShortB"}],
            "long_breaks": [{"name": "LongA"}],
        }
    )
    env.start_daemon(config=cfg)

    seen_types: list[str] = []
    deadline = time.monotonic() + 14.0
    while time.monotonic() < deadline:
        st = env.status_json()
        btype = st["next_break_type"]
        if btype in ("short", "long") and (not seen_types or seen_types[-1] != btype):
            seen_types.append(btype)
        if len(seen_types) >= 3 and seen_types[:3] == ["short", "long", "short"]:
            break
        time.sleep(0.1)

    assert seen_types[:3] == ["short", "long", "short"], (
        f"Expected ['short', 'long', 'short'] progression, observed {seen_types}"
    )


def test_t1_sch_04_custom_exercise_names_surface(env: SyeTestEnv) -> None:
    """Distinct short and long exercise names surface in Waiting/PreBreak states."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 2,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 4,
            "long_break_duration_secs": 1,
            "pre_break_warning_secs": 0,
            "random_order": False,
            "short_breaks": [{"name": "ShortAlpha"}],
            "long_breaks": [{"name": "LongOmega"}],
        }
    )
    env.start_daemon(config=cfg)

    seen_pairs: set[tuple[str, str]] = set()
    deadline = time.monotonic() + 12.0
    while time.monotonic() < deadline:
        st = env.status_json()
        if normalize_state(st["state"]) in ("waiting", "prebreak"):
            btype = st["next_break_type"]
            bname = st["next_break_name"]
            if isinstance(btype, str) and isinstance(bname, str):
                seen_pairs.add((btype, bname))
            if {("short", "ShortAlpha"), ("long", "LongOmega")} <= seen_pairs:
                break
        time.sleep(0.1)

    assert {("short", "ShortAlpha"), ("long", "LongOmega")} <= seen_pairs, (
        f"Expected both ('short', 'ShortAlpha') and ('long', 'LongOmega') in Waiting/PreBreak, "
        f"observed {sorted(seen_pairs)}"
    )


def test_t1_sch_05_pre_break_warning_window(env: SyeTestEnv) -> None:
    """PreBreak state is observed prior to Break when pre_break_warning_secs > 0."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 3,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 60,
            "long_break_duration_secs": 2,
            "pre_break_warning_secs": 1,
            "random_order": False,
        }
    )
    env.start_daemon(config=cfg)
    env.wait_for_state("prebreak", timeout=5.0, interval=0.08)
    env.wait_for_state("break", timeout=4.0, interval=0.08)


def test_t1_sch_06_persist_state_clean_restart(env: SyeTestEnv) -> None:
    """Daemon with persist_state=true cleanly quits and restarts."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 30,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 120,
            "persist_state": True,
            "random_order": False,
            "short_breaks": [{"name": "P1"}, {"name": "P2"}],
            "long_breaks": [{"name": "PL1"}],
        }
    )
    d1 = env.start_daemon(config=cfg)
    assert env.run_cli(["-b"], socket=env.default_socket).returncode == 0
    env.wait_for_state("break", timeout=3.0)
    env.wait_for_state("waiting", timeout=5.0)
    assert env.run_cli(["--quit"], socket=env.default_socket).returncode == 0
    assert d1.proc.wait(timeout=5.0) == 0

    d2 = env.start_daemon(config=cfg)
    assert d2.poll() is None
    st = env.status_json()
    env.assert_status_schema(st, expect_enabled=True)


# ---------------------------------------------------------------------------
# Area 1E: Manual Break Triggers & Pause/Disable/Enable (5 tests)
# ---------------------------------------------------------------------------


def test_t1_man_01_manual_take_break_bypasses_prebreak(env: SyeTestEnv) -> None:
    """-t/--take-break enters Break immediately, bypassing long pre_break_warning_secs."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 600,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 3000,
            "pre_break_warning_secs": 30,
        }
    )
    env.start_daemon(config=cfg)
    t0 = time.monotonic()
    res = env.run_cli(["--take-break"], socket=env.default_socket)
    assert res.returncode == 0, f"--take-break failed: stderr={res.stderr!r}"
    env.wait_for_state("break", timeout=2.0, interval=0.08)
    assert time.monotonic() - t0 < 2.5, "Manual break did not bypass pre-break wait"
    env.wait_for_state("waiting", timeout=5.0, interval=0.08)


def test_t1_man_02_manual_short_break(env: SyeTestEnv) -> None:
    """-b/--short-break triggers an immediate 1s short break (not 10s long) and returns to Waiting."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 600,
            "short_break_duration_secs": 1,
            "long_break_interval_secs": 3000,
            "long_break_duration_secs": 10,
            "short_breaks": [{"name": "ShortManual"}],
            "long_breaks": [{"name": "LongManual"}],
        }
    )
    env.start_daemon(config=cfg)
    assert env.run_cli(["-b"], socket=env.default_socket).returncode == 0
    env.wait_for_state("break", timeout=2.5, interval=0.08)
    t_break = time.monotonic()
    env.wait_for_state("waiting", timeout=5.0, interval=0.08)
    elapsed = time.monotonic() - t_break
    assert elapsed < 5.0, f"Short break took {elapsed:.2f}s (expected < 5.0s)"


def test_t1_man_03_manual_long_break(env: SyeTestEnv) -> None:
    """-l/--long-break triggers an immediate 1s long break (not 10s short) and returns to Waiting."""
    cfg = env.write_profile_config(
        {
            "short_break_interval_secs": 600,
            "short_break_duration_secs": 10,
            "long_break_interval_secs": 3000,
            "long_break_duration_secs": 1,
            "short_breaks": [{"name": "ShortManual"}],
            "long_breaks": [{"name": "LongManual"}],
        }
    )
    env.start_daemon(config=cfg)
    assert env.status_json()["next_break_type"] == "short"
    assert env.run_cli(["-l"], socket=env.default_socket).returncode == 0
    env.wait_for_state("break", timeout=2.5, interval=0.08)
    t_break = time.monotonic()
    env.wait_for_state("waiting", timeout=5.0, interval=0.08)
    elapsed = time.monotonic() - t_break
    assert elapsed < 5.0, f"Long break took {elapsed:.2f}s (expected < 5.0s)"


def test_t1_man_04_timed_disable_and_early_resume(env: SyeTestEnv) -> None:
    """--disable 5 pauses daemon and --enable resumes it early."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)

    res = env.run_cli(["--disable", "5"], socket=env.default_socket)
    assert res.returncode == 0, f"--disable 5 failed: stderr={res.stderr!r}"
    st = env.status_json()
    assert st["enabled"] is False and normalize_state(st["state"]) == "stopped"

    res = env.run_cli(["--enable"], socket=env.default_socket)
    assert res.returncode == 0
    st = env.status_json()
    assert st["enabled"] is True and normalize_state(st["state"]) == "waiting"


def test_t1_man_05_indefinite_disable_zero_and_omitted(env: SyeTestEnv) -> None:
    """Both --disable 0 and --disable pause daemon until re-enabled."""
    cfg = env.write_profile_config({"short_break_interval_secs": 30})
    env.start_daemon(config=cfg)

    for cmd in (["--disable", "0"], ["--disable"]):
        res = env.run_cli(cmd, socket=env.default_socket)
        assert res.returncode == 0, f"{cmd} failed: stderr={res.stderr!r}"
        st = env.status_json()
        assert st["enabled"] is False and normalize_state(st["state"]) == "stopped"
        assert env.run_cli(["--enable"], socket=env.default_socket).returncode == 0
        assert env.status_json()["enabled"] is True


TESTS = [
    test_t1_cli_01_version_flag,
    test_t1_cli_02_print_default_config,
    test_t1_cli_03_validate_default_config,
    test_t1_cli_04_validate_custom_valid_config,
    test_t1_cli_05_help_output,
    test_t1_cli_06_config_override_respected_by_daemon,
    test_t1_dmn_01_headless_daemon_startup_and_readiness,
    test_t1_dmn_02_single_instance_duplicate_launch,
    test_t1_dmn_03_graceful_stop_via_quit_and_q,
    test_t1_dmn_04_graceful_shutdown_via_sigterm,
    test_t1_dmn_05_multi_instance_isolation_via_socket,
    test_t1_ipc_01_human_readable_status,
    test_t1_ipc_02_status_json_schema,
    test_t1_ipc_03_countdown_progression,
    test_t1_ipc_04_disable_daemon,
    test_t1_ipc_05_reenable_daemon,
    test_t1_ipc_06_short_flag_aliases,
    test_t1_sch_01_full_headless_short_break_cycle,
    test_t1_sch_02_sequential_break_order,
    test_t1_sch_03_dual_tier_short_long_ratio,
    test_t1_sch_04_custom_exercise_names_surface,
    test_t1_sch_05_pre_break_warning_window,
    test_t1_sch_06_persist_state_clean_restart,
    test_t1_man_01_manual_take_break_bypasses_prebreak,
    test_t1_man_02_manual_short_break,
    test_t1_man_03_manual_long_break,
    test_t1_man_04_timed_disable_and_early_resume,
    test_t1_man_05_indefinite_disable_zero_and_omitted,
]
