#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SIM = ROOT / "scripts" / "rhystic_belief_mulligan_sim.py"


def default_chunks_per_worker(workers: int) -> int:
    return 8 if workers <= 12 else 16


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify deterministic Rhystic simulator consistency across worker counts."
    )
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--out-dir", default="data/rhystic_study_turn12/consistency")
    parser.add_argument("--label", default="consistency")
    parser.add_argument("--target", default="rhystic_heartwood")
    parser.add_argument("--threshold-hands", type=int, default=8)
    parser.add_argument("--eval-games", type=int, default=64)
    parser.add_argument("--samples-per-bottom", type=int, default=2)
    parser.add_argument("--validation-samples", type=int, default=2)
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument("--workers-a", type=int, default=1)
    parser.add_argument("--workers-b", type=int, default=4)
    parser.add_argument("--chunks-per-worker", type=int, default=None)
    parser.add_argument("--seed", type=int, default=2026070203)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--adaptive-threshold-sampling-a", action="store_true")
    parser.add_argument("--adaptive-threshold-sampling-b", action="store_true")
    parser.add_argument("--reuse-worker-pool-a", action="store_true")
    parser.add_argument("--reuse-worker-pool-b", action="store_true")
    parser.add_argument("--disable-action-sort", action="store_true")
    return parser.parse_args()


def sim_cmd(
    args: argparse.Namespace,
    json_out: Path,
    workers: int,
    adaptive_threshold_sampling: bool,
    reuse_worker_pool: bool,
) -> list[str]:
    cmd = [
        sys.executable,
        str(SIM),
        "--target",
        args.target,
        "--deck-json",
        args.deck_json,
        "--threshold-hands",
        str(args.threshold_hands),
        "--eval-games",
        str(args.eval_games),
        "--samples-per-bottom",
        str(args.samples_per_bottom),
        "--validation-samples",
        str(args.validation_samples),
        "--state-limit",
        str(args.state_limit),
        "--actual-rerun-state-limit",
        str(args.actual_rerun_state_limit),
        "--workers",
        str(workers),
        "--chunks-per-worker",
        str(args.chunks_per_worker),
        "--seed",
        str(args.seed),
        "--gemstone-caverns-live-rate",
        str(args.gemstone_caverns_live_rate),
        "--engine-success-policy",
        args.engine_success_policy,
        "--gamble-mode",
        args.gamble_mode,
        "--json-out",
        str(json_out),
        "--suppress-json-stdout",
        "--include-game-records",
        "--compact-game-records",
        "--paired-stage-orders",
        "--normalize-no-caverns-gemstone-key",
    ]
    if adaptive_threshold_sampling:
        cmd.append("--adaptive-threshold-sampling")
    if reuse_worker_pool:
        cmd.append("--reuse-worker-pool")
    if args.disable_action_sort:
        cmd.append("--disable-action-sort")
    return cmd


def load(path: Path) -> dict[str, Any]:
    with path.open() as handle:
        return json.load(handle)


def comparable_payload(payload: dict[str, Any]) -> dict[str, Any]:
    evaluation = payload.get("evaluation") or {}
    return {
        "success_rate": evaluation.get("success_rate"),
        "successes": evaluation.get("successes"),
        "turn_counts": evaluation.get("turn_counts"),
        "cap_misses": evaluation.get("cap_misses"),
        "actual_cap_rerun_attempts": evaluation.get("actual_cap_rerun_attempts"),
        "actual_cap_rerun_successes": evaluation.get("actual_cap_rerun_successes"),
        "game_records": sorted(evaluation.get("game_records") or [], key=lambda row: row["game_index"]),
    }


def first_diff(left: Any, right: Any, path: str = "$") -> str | None:
    if type(left) is not type(right):
        return f"{path}: type {type(left).__name__} != {type(right).__name__}"
    if isinstance(left, dict):
        left_keys = set(left)
        right_keys = set(right)
        if left_keys != right_keys:
            return f"{path}: keys {sorted(left_keys ^ right_keys)} differ"
        for key in sorted(left):
            diff = first_diff(left[key], right[key], f"{path}.{key}")
            if diff:
                return diff
        return None
    if isinstance(left, list):
        if len(left) != len(right):
            return f"{path}: len {len(left)} != {len(right)}"
        for index, (left_item, right_item) in enumerate(zip(left, right, strict=True)):
            diff = first_diff(left_item, right_item, f"{path}[{index}]")
            if diff:
                return diff
        return None
    if left != right:
        return f"{path}: {left!r} != {right!r}"
    return None


def main() -> int:
    args = parse_args()
    if args.chunks_per_worker is None:
        args.chunks_per_worker = default_chunks_per_worker(max(args.workers_a, args.workers_b))
    if args.chunks_per_worker <= 0:
        raise ValueError("--chunks-per-worker must be positive")
    out_dir = ROOT / args.out_dir / args.label
    out_dir.mkdir(parents=True, exist_ok=True)
    json_a = out_dir / f"side_a_workers_{args.workers_a}.json"
    json_b = out_dir / f"side_b_workers_{args.workers_b}.json"
    stdout_a = out_dir / f"side_a_workers_{args.workers_a}.stdout"
    stdout_b = out_dir / f"side_b_workers_{args.workers_b}.stdout"

    for workers, json_out, stdout_path, adaptive, reuse_pool in (
        (args.workers_a, json_a, stdout_a, args.adaptive_threshold_sampling_a, args.reuse_worker_pool_a),
        (args.workers_b, json_b, stdout_b, args.adaptive_threshold_sampling_b, args.reuse_worker_pool_b),
    ):
        with stdout_path.open("w") as stdout_handle:
            subprocess.run(
                sim_cmd(args, json_out, workers, adaptive, reuse_pool),
                cwd=ROOT,
                stdout=stdout_handle,
                stderr=subprocess.STDOUT,
                text=True,
                check=True,
            )

    left = comparable_payload(load(json_a))
    right = comparable_payload(load(json_b))
    diff = first_diff(left, right)
    report = {
        "consistent": diff is None,
        "first_diff": diff,
        "workers_a": args.workers_a,
        "workers_b": args.workers_b,
        "adaptive_threshold_sampling_a": args.adaptive_threshold_sampling_a,
        "adaptive_threshold_sampling_b": args.adaptive_threshold_sampling_b,
        "reuse_worker_pool_a": args.reuse_worker_pool_a,
        "reuse_worker_pool_b": args.reuse_worker_pool_b,
        "json_a": str(json_a),
        "json_b": str(json_b),
        "success_rate_a": left["success_rate"],
        "success_rate_b": right["success_rate"],
        "successes_a": left["successes"],
        "successes_b": right["successes"],
    }
    report_path = out_dir / "consistency_report.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(report_path)
    if diff:
        print(diff)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
