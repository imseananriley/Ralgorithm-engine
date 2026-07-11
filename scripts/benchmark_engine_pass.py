#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import statistics
import subprocess
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target" / "release" / "rhystic-core-smoke"
FIXTURE = ROOT / "fixtures" / "parity" / "action_fixtures_pass48_20260703.json"


def run_json(command: list[str]) -> dict[str, Any]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return json.loads(completed.stdout)


def median_field(rows: list[dict[str, Any]], field: str) -> float:
    return statistics.median(float(row[field]) for row in rows)


def source_manifest() -> dict[str, Any]:
    out = ROOT / "benchmarks" / "source_manifest.json"
    subprocess.run(
        ["python3", str(ROOT / "scripts" / "source_manifest.py"), "--out", str(out)],
        cwd=ROOT,
        text=True,
        stdout=subprocess.DEVNULL,
        check=True,
    )
    return json.loads(out.read_text())


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark one next-generation engine pass.")
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--nextgen-iterations", type=int, default=20_000)
    parser.add_argument("--fast-state-iterations", type=int, default=200)
    parser.add_argument("--fast-action-iterations", type=int, default=10)
    parser.add_argument("--out", default="benchmarks/results/latest.json")
    args = parser.parse_args()
    if args.repeats < 1:
        raise ValueError("--repeats must be positive")

    subprocess.run(["cargo", "build", "--release"], cwd=ROOT, check=True)
    manifest = source_manifest()
    nextgen: list[dict[str, Any]] = []
    fast_state: list[dict[str, Any]] = []
    fast_actions: list[dict[str, Any]] = []
    started = time.time()
    for _ in range(args.repeats):
        nextgen.append(run_json([str(BINARY), "bench-nextgen", str(args.nextgen_iterations)]))
        fast_state.append(
            run_json(
                [
                    str(BINARY),
                    "bench-fast-state",
                    str(FIXTURE),
                    str(args.fast_state_iterations),
                ]
            )
        )
        fast_actions.append(
            run_json(
                [
                    str(BINARY),
                    "bench-fast-actions",
                    str(FIXTURE),
                    str(args.fast_action_iterations),
                    "20",
                ]
            )
        )

    payload = {
        "schema": 1,
        "source_digest": manifest["source_digest"],
        "git_commit": manifest.get("git_commit"),
        "git_dirty": bool(manifest.get("git_status_porcelain")),
        "machine": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "logical_cpus": os.cpu_count(),
        },
        "settings": vars(args),
        "elapsed_seconds": time.time() - started,
        "median": {
            "packed_state_bytes": int(nextgen[0]["packed_state_bytes"]),
            "state_operations_per_second": median_field(nextgen, "state_operations_per_second"),
            "mana_closures_per_second": median_field(nextgen, "mana_closures_per_second"),
            "payment_plans_per_second": median_field(nextgen, "payment_plans_per_second"),
            "payment_plan_frontier_size": int(nextgen[0]["payment_plan_frontier_size"]),
            "legacy_fast_clone_hash_speedup": median_field(fast_state, "clone_hash_speedup"),
            "legacy_fast_action_generation_speedup": median_field(fast_actions, "action_generation_speedup"),
        },
        "runs": {
            "nextgen": nextgen,
            "fast_state": fast_state,
            "fast_actions": fast_actions,
        },
    }
    out = Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    print(out)
    print(json.dumps(payload["median"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
