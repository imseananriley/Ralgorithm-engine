#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import multiprocessing
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SIM = ROOT / "scripts" / "rhystic_belief_mulligan_sim.py"


def default_chunks_per_worker(workers: int, reuse_worker_pool: bool = False) -> int:
    if reuse_worker_pool:
        return 16
    return 8 if workers <= 12 else 16


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run reproducible timing benchmarks for the Rhystic/Heartwood simulator."
    )
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--out-dir", default="data/rhystic_study_turn12/benchmarks")
    parser.add_argument("--label", default="")
    parser.add_argument("--target", default="rhystic_heartwood")
    parser.add_argument("--threshold-hands", type=int, default=20)
    parser.add_argument("--eval-games", type=int, default=200)
    parser.add_argument("--samples-per-bottom", type=int, default=2)
    parser.add_argument("--validation-samples", type=int, default=2)
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument("--workers", type=int, default=max(1, os.cpu_count() or 1))
    parser.add_argument("--chunks-per-worker", type=int, default=None)
    parser.add_argument("--seed", type=int, default=2026070201)
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--thresholds-json", default=None)
    parser.add_argument("--threshold-cache-dir", default=None)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--rust-action-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_ACTIONS", "off"))
    parser.add_argument("--rust-close-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_CLOSE", "off"))
    parser.add_argument("--rust-solver-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_SOLVER", "off"))
    parser.add_argument("--rust-action-bin", default=os.environ.get("RHYSTIC_RUST_ACTION_BIN"))
    parser.add_argument("--rust-full-sim", action="store_true", default=os.environ.get("RHYSTIC_RUST_FULL_SIM", "0").lower() in {"1", "true", "yes", "on"})
    parser.add_argument("--rust-full-sim-shards", type=int, default=int(os.environ.get("RHYSTIC_RUST_FULL_SIM_SHARDS", "1")))
    parser.add_argument(
        "--rust-full-sim-games-per-shard",
        type=int,
        default=int(os.environ["RHYSTIC_RUST_FULL_SIM_GAMES_PER_SHARD"]) if os.environ.get("RHYSTIC_RUST_FULL_SIM_GAMES_PER_SHARD") else None,
    )
    parser.add_argument(
        "--rust-full-sim-shard-workers",
        type=int,
        default=int(os.environ["RHYSTIC_RUST_FULL_SIM_SHARD_WORKERS"]) if os.environ.get("RHYSTIC_RUST_FULL_SIM_SHARD_WORKERS") else None,
    )
    parser.add_argument(
        "--rust-full-sim-internal-shards",
        action="store_true",
        default=os.environ.get("RHYSTIC_RUST_FULL_SIM_INTERNAL_SHARDS", "0").lower() in {"1", "true", "yes", "on"},
    )
    parser.add_argument("--adaptive-threshold-sampling", action="store_true")
    parser.add_argument("--reuse-worker-pool", action="store_true")
    parser.add_argument("--disable-action-sort", action="store_true")
    parser.add_argument("--heuristic-bottom-order", action="store_true")
    parser.add_argument("--omit-game-records", action="store_true")
    parser.add_argument(
        "--include-cap-replay-records",
        action="store_true",
        default=os.environ.get("RHYSTIC_INCLUDE_CAP_REPLAY_RECORDS", "0").lower() in {"1", "true", "yes", "on"},
    )
    parser.add_argument(
        "--include-validation-records",
        action="store_true",
        default=os.environ.get("RHYSTIC_INCLUDE_VALIDATION_RECORDS", "0").lower() in {"1", "true", "yes", "on"},
    )
    parser.add_argument("--profile", choices=("none", "cprofile", "worker", "all"), default="none")
    parser.add_argument("--worker-profile-chunks-per-process", type=int, default=1)
    parser.add_argument(
        "--worker-start-method",
        choices=tuple(multiprocessing.get_all_start_methods()),
        default=None,
        help="Optional worker multiprocessing start method. Defaults to spawn for --profile all, otherwise simulator default.",
    )
    parser.add_argument(
        "--warm-threshold-cache",
        action="store_true",
        help="Run an untimed warmup first for each seed so threshold-cache construction does not contaminate timed results.",
    )
    parser.add_argument(
        "--warmup-eval-games",
        type=int,
        default=1,
        help="Evaluation games for --warm-threshold-cache. Must be positive because the simulator reports rates.",
    )
    parser.add_argument("--keep-run-outputs", action="store_true")
    return parser.parse_args()


def command_for(
    args: argparse.Namespace,
    json_out: Path,
    seed: int,
    worker_profile_dir: Path | None = None,
    *,
    eval_games: int | None = None,
) -> list[str]:
    cmd = [
        str(SIM),
        "--target",
        args.target,
        "--deck-json",
        args.deck_json,
        "--threshold-hands",
        str(args.threshold_hands),
        "--eval-games",
        str(args.eval_games if eval_games is None else eval_games),
        "--samples-per-bottom",
        str(args.samples_per_bottom),
        "--validation-samples",
        str(args.validation_samples),
        "--state-limit",
        str(args.state_limit),
        "--actual-rerun-state-limit",
        str(args.actual_rerun_state_limit),
        "--workers",
        str(args.workers),
        "--chunks-per-worker",
        str(args.chunks_per_worker),
        "--seed",
        str(seed),
        "--gemstone-caverns-live-rate",
        str(args.gemstone_caverns_live_rate),
        "--engine-success-policy",
        args.engine_success_policy,
        "--gamble-mode",
        args.gamble_mode,
        "--rust-action-mode",
        args.rust_action_mode,
        "--rust-close-mode",
        args.rust_close_mode,
        "--rust-solver-mode",
        args.rust_solver_mode,
        "--json-out",
        str(json_out),
        "--suppress-json-stdout",
        "--paired-stage-orders",
        "--normalize-no-caverns-gemstone-key",
    ]
    if not args.omit_game_records:
        cmd.extend(["--include-game-records", "--compact-game-records"])
    if args.threshold_cache_dir:
        cmd.extend(["--threshold-cache-dir", args.threshold_cache_dir])
    if args.thresholds_json:
        cmd.extend(["--thresholds-json", args.thresholds_json])
    if args.rust_action_bin:
        cmd.extend(["--rust-action-bin", args.rust_action_bin])
    if args.rust_full_sim:
        cmd.append("--rust-full-sim")
    if args.rust_full_sim_shards != 1:
        cmd.extend(["--rust-full-sim-shards", str(args.rust_full_sim_shards)])
    if args.rust_full_sim_games_per_shard is not None:
        cmd.extend(["--rust-full-sim-games-per-shard", str(args.rust_full_sim_games_per_shard)])
    if args.rust_full_sim_shard_workers is not None:
        cmd.extend(["--rust-full-sim-shard-workers", str(args.rust_full_sim_shard_workers)])
    if args.rust_full_sim_internal_shards:
        cmd.append("--rust-full-sim-internal-shards")
    if args.adaptive_threshold_sampling:
        cmd.append("--adaptive-threshold-sampling")
    if args.reuse_worker_pool:
        cmd.append("--reuse-worker-pool")
    if args.disable_action_sort:
        cmd.append("--disable-action-sort")
    if args.heuristic_bottom_order:
        cmd.append("--heuristic-bottom-order")
    if args.include_cap_replay_records:
        cmd.append("--include-cap-replay-records")
    if args.include_validation_records:
        cmd.append("--include-validation-records")
    if args.worker_start_method:
        cmd.extend(["--worker-start-method", args.worker_start_method])
    if worker_profile_dir is not None:
        cmd.extend(
            [
                "--worker-profile-dir",
                str(worker_profile_dir),
                "--worker-profile-chunks-per-process",
                str(args.worker_profile_chunks_per_process),
            ]
        )
    return cmd


def load_json(path: Path) -> dict[str, Any]:
    with path.open() as handle:
        return json.load(handle)


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    fields = sorted({key for row in rows for key in row})
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow(row)


def main() -> int:
    args = parse_args()
    if args.repeat <= 0:
        raise ValueError("--repeat must be positive")
    if args.workers <= 0:
        raise ValueError("--workers must be positive")
    if args.chunks_per_worker is None:
        args.chunks_per_worker = default_chunks_per_worker(args.workers, args.reuse_worker_pool)
    if args.warm_threshold_cache and not args.threshold_cache_dir:
        raise ValueError("--warm-threshold-cache requires --threshold-cache-dir")
    if args.warmup_eval_games <= 0:
        raise ValueError("--warmup-eval-games must be positive")

    out_dir = ROOT / args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    run_stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    label = args.label or f"benchmark_{run_stamp}"
    run_dir = out_dir / label
    run_dir.mkdir(parents=True, exist_ok=True)

    rows: list[dict[str, Any]] = []
    for repeat_index in range(args.repeat):
        seed = args.seed + repeat_index
        json_out = run_dir / f"run_{repeat_index + 1:03d}.json"
        stdout_path = run_dir / f"run_{repeat_index + 1:03d}.stdout"
        profile_path = run_dir / f"run_{repeat_index + 1:03d}.prof"
        worker_profile_dir = (
            run_dir / f"run_{repeat_index + 1:03d}_worker_profiles"
            if args.profile in {"worker", "all"}
            else None
        )
        warmup_elapsed = ""
        warmup_returncode = ""
        parent_profile_elapsed = ""
        parent_profile_returncode = ""
        parent_profile_stdout = ""
        if args.warm_threshold_cache:
            warmup_json = run_dir / f"warmup_{repeat_index + 1:03d}.json"
            warmup_stdout = run_dir / f"warmup_{repeat_index + 1:03d}.stdout"
            warmup_cmd = [
                sys.executable,
                *command_for(args, warmup_json, seed, eval_games=args.warmup_eval_games),
            ]
            warmup_started = time.perf_counter()
            with warmup_stdout.open("w") as stdout_handle:
                warmup_completed = subprocess.run(
                    warmup_cmd,
                    cwd=ROOT,
                    text=True,
                    stdout=stdout_handle,
                    stderr=subprocess.STDOUT,
                    check=False,
                )
            warmup_elapsed = f"{time.perf_counter() - warmup_started:.6f}"
            warmup_returncode = warmup_completed.returncode
            if warmup_completed.returncode != 0:
                rows.append(
                    {
                        "label": label,
                        "repeat": repeat_index + 1,
                        "returncode": warmup_completed.returncode,
                        "elapsed_seconds": "",
                        "warmup_elapsed_seconds": warmup_elapsed,
                        "warmup_returncode": warmup_returncode,
                        "json_out": str(warmup_json),
                        "stdout_path": str(warmup_stdout),
                    }
                )
                break
            if not args.keep_run_outputs:
                warmup_json.unlink(missing_ok=True)

        if args.profile == "all":
            parent_profile_json = run_dir / f"run_{repeat_index + 1:03d}_parent_profile.json"
            parent_profile_stdout_path = run_dir / f"run_{repeat_index + 1:03d}_parent_profile.stdout"
            parent_profile_stdout = str(parent_profile_stdout_path)
            parent_profile_cmd = command_for(args, parent_profile_json, seed)
            parent_profile_run_cmd = [
                sys.executable,
                "-m",
                "cProfile",
                "-o",
                str(profile_path),
                *parent_profile_cmd,
            ]
            parent_profile_started = time.perf_counter()
            with parent_profile_stdout_path.open("w") as stdout_handle:
                parent_profile_completed = subprocess.run(
                    parent_profile_run_cmd,
                    cwd=ROOT,
                    text=True,
                    stdout=stdout_handle,
                    stderr=subprocess.STDOUT,
                    check=False,
                )
            parent_profile_elapsed = f"{time.perf_counter() - parent_profile_started:.6f}"
            parent_profile_returncode = parent_profile_completed.returncode
            if not args.keep_run_outputs:
                parent_profile_json.unlink(missing_ok=True)
            if parent_profile_completed.returncode != 0:
                rows.append(
                    {
                        "label": label,
                        "repeat": repeat_index + 1,
                        "returncode": parent_profile_completed.returncode,
                        "elapsed_seconds": "",
                        "parent_profile_elapsed_seconds": parent_profile_elapsed,
                        "parent_profile_returncode": parent_profile_returncode,
                        "profile_path": str(profile_path),
                        "stdout_path": str(parent_profile_stdout_path),
                    }
                )
                break

        cmd = command_for(args, json_out, seed, worker_profile_dir=worker_profile_dir)
        run_cmd = [sys.executable, *cmd]
        if args.profile == "cprofile":
            run_cmd = [sys.executable, "-m", "cProfile", "-o", str(profile_path), *cmd]

        started = time.perf_counter()
        with stdout_path.open("w") as stdout_handle:
            completed = subprocess.run(
                run_cmd,
                cwd=ROOT,
                text=True,
                stdout=stdout_handle,
                stderr=subprocess.STDOUT,
                check=False,
            )
        elapsed = time.perf_counter() - started
        payload = load_json(json_out) if json_out.exists() else {}
        evaluation = payload.get("evaluation") or {}
        row = {
            "label": label,
            "repeat": repeat_index + 1,
            "returncode": completed.returncode,
            "elapsed_seconds": f"{elapsed:.6f}",
            "games": args.eval_games,
            "games_per_second": f"{args.eval_games / elapsed:.6f}" if elapsed > 0 else "",
            "workers": args.workers,
            "chunks_per_worker": args.chunks_per_worker,
            "threshold_hands": args.threshold_hands,
            "threshold_cache_dir": args.threshold_cache_dir or "",
            "thresholds_json": args.thresholds_json or "",
            "threshold_cache_hit": payload.get("threshold_cache_hit"),
            "samples_per_bottom": args.samples_per_bottom,
            "validation_samples": args.validation_samples,
            "state_limit": args.state_limit,
            "actual_rerun_state_limit": args.actual_rerun_state_limit,
            "seed": seed,
            "success_rate": evaluation.get("success_rate"),
            "successes": evaluation.get("successes"),
            "cap_misses": evaluation.get("cap_misses"),
            "actual_cap_rerun_attempts": evaluation.get("actual_cap_rerun_attempts"),
            "actual_cap_rerun_successes": evaluation.get("actual_cap_rerun_successes"),
            "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
            "reuse_worker_pool": args.reuse_worker_pool,
            "heuristic_bottom_order": args.heuristic_bottom_order,
            "include_game_records": not args.omit_game_records,
            "include_cap_replay_records": args.include_cap_replay_records,
            "cap_replay_record_count": len(evaluation.get("cap_replay_records") or []),
            "include_validation_records": args.include_validation_records,
            "validation_record_count": len(evaluation.get("validation_records") or []),
            "compact_game_records": not args.omit_game_records,
            "warm_threshold_cache": args.warm_threshold_cache,
            "warmup_eval_games": args.warmup_eval_games if args.warm_threshold_cache else "",
            "warmup_elapsed_seconds": warmup_elapsed,
            "warmup_returncode": warmup_returncode,
            "parent_profile_elapsed_seconds": parent_profile_elapsed,
            "parent_profile_returncode": parent_profile_returncode,
            "parent_profile_stdout": parent_profile_stdout,
            "action_sort": not args.disable_action_sort,
            "rust_action_mode": args.rust_action_mode,
            "rust_close_mode": args.rust_close_mode,
            "rust_solver_mode": args.rust_solver_mode,
            "rust_action_bin": args.rust_action_bin or "",
            "rust_full_sim": args.rust_full_sim,
            "rust_full_sim_shards": args.rust_full_sim_shards,
            "rust_full_sim_games_per_shard": args.rust_full_sim_games_per_shard or "",
            "rust_full_sim_internal_shards": args.rust_full_sim_internal_shards,
            "rust_full_sim_shard_workers": args.rust_full_sim_shard_workers or "",
            "adaptive_validation_samples_saved": sum(
                int(row.get("adaptive_validation_samples_saved") or 0)
                for row in evaluation.get("policy_stage_timing", [])
            ),
            "validation_samples_used": sum(
                int(row.get("validation_samples_used") or 0)
                for row in evaluation.get("policy_stage_timing", [])
            ),
            "validation_sample_budget": sum(
                int(row.get("validation_sample_budget") or 0)
                for row in evaluation.get("policy_stage_timing", [])
            ),
            "python": sys.version.split()[0],
            "platform": platform.platform(),
            "machine": platform.machine(),
            "processor": platform.processor(),
            "logical_cpus": os.cpu_count(),
            "nproc": shutil.which("nproc"),
            "profile_path": str(profile_path) if args.profile in {"cprofile", "all"} else "",
            "worker_profile_dir": str(worker_profile_dir) if worker_profile_dir is not None else "",
            "worker_profile_chunks_per_process": args.worker_profile_chunks_per_process,
            "worker_start_method": args.worker_start_method or "",
            "json_out": str(json_out),
            "stdout_path": str(stdout_path),
        }
        rows.append(row)
        if completed.returncode != 0:
            break
        if not args.keep_run_outputs:
            json_out.unlink(missing_ok=True)

    summary_json = run_dir / "benchmark_summary.json"
    summary_csv = run_dir / "benchmark_summary.csv"
    config_json = run_dir / "benchmark_config.json"
    summary_json.write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")
    config_json.write_text(json.dumps(vars(args), indent=2, sort_keys=True) + "\n")
    write_csv(summary_csv, rows)
    print(summary_csv)
    return 0 if rows and int(rows[-1]["returncode"]) == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
