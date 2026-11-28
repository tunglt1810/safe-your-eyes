#!/usr/bin/env python3
"""Opaque-box E2E test runner for safe-your-eyes (Python 3 stdlib only)."""

from __future__ import annotations

import argparse
import os
import signal
import sys
import time
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from e2e.harness import (  # noqa: E402
    E2E_TMP_BASE,
    SyeTestEnv,
    _cleanup_all_global,
    cleanup_run_dir,
    register_run_dir,
    set_keep_tmp_global,
)
from e2e.tests import (  # noqa: E402
    test_tier1_features,
    test_tier2_boundaries,
    test_tier3_combinations,
    test_tier4_scenarios,
)

TIER_REGISTRY: dict[int, tuple[str, list[Callable[[SyeTestEnv], None]]]] = {
    1: ("Tier 1 — Core Feature Coverage", test_tier1_features.TESTS),
    2: ("Tier 2 — Boundary & Corner Cases", test_tier2_boundaries.TESTS),
    3: ("Tier 3 — Cross-Feature Combinations", test_tier3_combinations.TESTS),
    4: ("Tier 4 — Real-World User Scenarios", test_tier4_scenarios.TESTS),
}


@dataclass
class TestOutcome:
    tier: int
    name: str
    status: str  # PASS | FAIL | ERROR
    duration_s: float
    detail: str = ""


def _parse_tiers(raw: str) -> list[int]:
    token = raw.strip().lower()
    if token == "all":
        return [1, 2, 3, 4]
    tiers: list[int] = []
    for part in token.split(","):
        part = part.strip()
        if not part.isdigit() or int(part) not in TIER_REGISTRY:
            raise argparse.ArgumentTypeError(
                f"Invalid --tier value {raw!r}; expected 1, 2, 3, 4, comma list, or 'all'"
            )
        num = int(part)
        if num not in tiers:
            tiers.append(num)
    return tiers


def _build_arg_parser() -> argparse.ArgumentParser:
    default_bin = REPO_ROOT / "target" / "debug" / "safe-your-eyes"
    if os.name == "nt":
        default_bin = default_bin.with_suffix(".exe")
    parser = argparse.ArgumentParser(
        prog="run_e2e.py",
        description="Run opaque-box E2E test suites (Tiers 1-4) against safe-your-eyes.",
    )
    parser.add_argument(
        "--bin",
        dest="binary",
        type=Path,
        default=default_bin,
        help=f"Path to safe-your-eyes executable (default: {default_bin})",
    )
    parser.add_argument(
        "--tier",
        default="all",
        help="Tier(s) to run: 1, 2, 3, 4, comma-separated (e.g. 1,2), or all (default: all)",
    )
    parser.add_argument(
        "-v",
        "--verbose",
        action="store_true",
        help="Show verbose progress and diagnostic details.",
    )
    parser.add_argument(
        "--keep-tmp",
        action="store_true",
        help="Keep <repo>/.tmp/e2e/run_<pid>/ and sockets after test execution.",
    )
    return parser


def _install_signal_handlers() -> None:
    def _handle_sig(signum: int, _frame: object) -> None:
        sys.stderr.write(f"\nReceived signal {signum}; cleaning up daemons and workspace...\n")
        _cleanup_all_global()
        sys.exit(128 + signum)

    signal.signal(signal.SIGINT, _handle_sig)
    if hasattr(signal, "SIGTERM"):
        signal.signal(signal.SIGTERM, _handle_sig)


def main(argv: list[str] | None = None) -> int:
    parser = _build_arg_parser()
    args = parser.parse_args(argv)

    try:
        selected_tiers = _parse_tiers(args.tier)
    except argparse.ArgumentTypeError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    binary = args.binary.resolve()
    if not binary.exists() or not binary.is_file():
        sys.stderr.write(f"error: binary not found: {binary}\n")
        return 2
    if not os.access(binary, os.X_OK):
        sys.stderr.write(f"error: binary is not executable: {binary}\n")
        return 2

    set_keep_tmp_global(args.keep_tmp)
    _install_signal_handlers()

    run_dir = E2E_TMP_BASE / f"run_{os.getpid()}"
    run_dir.mkdir(parents=True, exist_ok=True)
    register_run_dir(run_dir)

    print(f"safe-your-eyes E2E runner | binary: {binary} | tiers: {selected_tiers}")
    suite_start = time.monotonic()
    all_outcomes: list[TestOutcome] = []
    tier_stats: dict[int, tuple[int, int, int, float]] = {}

    try:
        for tier_num in selected_tiers:
            tier_label, tests = TIER_REGISTRY[tier_num]
            print(f"\n=== {tier_label} ({len(tests)} tests) ===")
            t_start = time.monotonic()
            passed = failed = errors = 0

            for test_fn in tests:
                test_name = test_fn.__name__
                t0 = time.monotonic()
                status = "PASS"
                detail = ""
                with SyeTestEnv(
                    binary=binary,
                    run_dir=run_dir,
                    test_id=test_name,
                    verbose=args.verbose,
                    keep_tmp=args.keep_tmp,
                ) as env:
                    try:
                        test_fn(env)
                        passed += 1
                    except AssertionError as exc:
                        status = "FAIL"
                        failed += 1
                        tb = traceback.format_exc()
                        diag = env.format_diagnostics()
                        detail = f"{exc}\n{tb}{diag}"
                    except Exception as exc:  # noqa: BLE001
                        status = "ERROR"
                        errors += 1
                        tb = traceback.format_exc()
                        diag = env.format_diagnostics()
                        detail = f"{type(exc).__name__}: {exc}\n{tb}{diag}"

                dur = time.monotonic() - t0
                all_outcomes.append(TestOutcome(tier_num, test_name, status, dur, detail))
                print(f"  [{status:5s}] {test_name} ({dur:.2f}s)")
                if status != "PASS":
                    for line in detail.splitlines():
                        print(f"          {line}")

            tier_dur = time.monotonic() - t_start
            tier_stats[tier_num] = (passed, failed, errors, tier_dur)
    finally:
        if not args.keep_tmp:
            cleanup_run_dir(run_dir)

    total_dur = time.monotonic() - suite_start
    total_tests = len(all_outcomes)
    total_pass = sum(1 for o in all_outcomes if o.status == "PASS")
    total_fail = sum(1 for o in all_outcomes if o.status == "FAIL")
    total_err = sum(1 for o in all_outcomes if o.status == "ERROR")

    print("\n=== Summary ===")
    for tier_num in selected_tiers:
        p, f, e, d = tier_stats[tier_num]
        t = p + f + e
        print(f"  Tier {tier_num}: {p}/{t} passed, {f} failed, {e} errors ({d:.2f}s)")
    print(
        f"  TOTAL : {total_pass}/{total_tests} passed, "
        f"{total_fail} failed, {total_err} errors ({total_dur:.2f}s)"
    )

    if total_tests > 0 and total_fail == 0 and total_err == 0:
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
