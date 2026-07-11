#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import shutil
import subprocess
import sys
from pathlib import Path
from statistics import NormalDist
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PAIR = ROOT / "scripts" / "rhystic_paired_rate_compare.py"
sys.path.insert(0, str(ROOT / "scripts"))
from rhystic_quick_compare import parse_swap  # noqa: E402


def default_chunks_per_worker(workers: int) -> int:
    return 8 if workers <= 12 else 16


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def sample_sd(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    mu = mean(values)
    return math.sqrt(sum((value - mu) ** 2 for value in values) / (len(values) - 1))


def mean_ci(values: list[float], z: float) -> tuple[float, float, float]:
    if not values:
        return 0.0, 0.0, 0.0
    mu = mean(values)
    if len(values) < 2:
        return mu, mu, mu
    half = z * sample_sd(values) / math.sqrt(len(values))
    return mu, mu - half, mu + half


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run paired comparisons in independent shards and stop variants early "
            "when a conservative interim confidence interval resolves the requested objective."
        )
    )
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--out-dir", default="data/rhystic_study_turn12/sequential_paired_compare")
    parser.add_argument("--target", default="rhystic_heartwood", choices=("rhystic", "heartwood", "rhystic_heartwood"))
    parser.add_argument("--swap", action="append", required=True, help="Variant swap spec accepted by rhystic_paired_rate_compare.py.")
    parser.add_argument("--objective", choices=("rate", "score"), default="rate")
    parser.add_argument("--alpha", type=float, default=0.05)
    parser.add_argument("--min-games", type=int, default=2_000)
    parser.add_argument("--max-games", type=int, default=20_000)
    parser.add_argument("--chunk-games", type=int, default=2_000)
    parser.add_argument("--benefit-margin", type=float, default=0.0, help="Stop positive only if CI low is above this delta.")
    parser.add_argument("--harm-margin", type=float, default=0.0, help="Stop negative only if CI high is below negative this delta.")
    parser.add_argument("--no-alpha-spending", action="store_true", help="Use ordinary 95%% CIs at each look instead of Bonferroni interim bounds.")
    parser.add_argument("--threshold-hands", type=int, default=80)
    parser.add_argument("--samples-per-bottom", type=int, default=2)
    parser.add_argument("--validation-samples", type=int, default=2)
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument("--workers", type=int, default=6)
    parser.add_argument("--chunks-per-worker", type=int, default=None)
    parser.add_argument("--seed", type=int, default=2026070202)
    parser.add_argument("--threshold-cache-dir", default=None)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--adaptive-threshold-sampling", action="store_true")
    parser.add_argument("--reuse-worker-pool", action="store_true")
    parser.add_argument("--disable-action-sort", action="store_true")
    parser.add_argument("--bootstrap-samples", type=int, default=0)
    parser.add_argument("--rate-half-width", type=float, default=0.0025)
    parser.add_argument("--score-half-width", type=float, default=0.25)
    parser.add_argument("--rhystic-t1-weight", type=float, default=100.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=60.0)
    parser.add_argument("--heartwood-t1-weight", type=float, default=20.0)
    parser.add_argument("--heartwood-t2-weight", type=float, default=10.0)
    parser.add_argument(
        "--baseline-cache-dir",
        default=None,
        help="Directory for per-shard baseline simulator result reuse. Defaults to <out-dir>/baseline_cache.",
    )
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def baseline_cache_key(args: argparse.Namespace, seed: int) -> str:
    payload = {
        "deck_json": args.deck_json,
        "target": args.target,
        "threshold_hands": args.threshold_hands,
        "eval_games": args.chunk_games,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "state_limit": args.state_limit,
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "workers_sensitive": False,
        "chunks_per_worker_sensitive": False,
        "seed": seed,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "engine_success_policy": args.engine_success_policy,
        "gamble_mode": args.gamble_mode,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "action_sort": not args.disable_action_sort,
        "threshold_cache_dir": args.threshold_cache_dir,
    }
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.blake2b(encoded, digest_size=12).hexdigest()


def pair_cmd(args: argparse.Namespace, out_dir: Path, seed: int, swaps: list[str], baseline_result_json: Path | None = None) -> list[str]:
    cmd = [
        sys.executable,
        str(PAIR),
        "--deck-json",
        args.deck_json,
        "--out-dir",
        str(out_dir),
        "--target",
        args.target,
        "--threshold-hands",
        str(args.threshold_hands),
        "--eval-games",
        str(args.chunk_games),
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
        "--bootstrap-samples",
        str(args.bootstrap_samples),
        "--rate-half-width",
        str(args.rate_half_width),
        "--score-half-width",
        str(args.score_half_width),
        "--rhystic-t1-weight",
        str(args.rhystic_t1_weight),
        "--rhystic-t2-weight",
        str(args.rhystic_t2_weight),
        "--heartwood-t1-weight",
        str(args.heartwood_t1_weight),
        "--heartwood-t2-weight",
        str(args.heartwood_t2_weight),
        "--normalize-no-caverns-gemstone-key",
    ]
    for swap in swaps:
        cmd.extend(["--swap", swap])
    if args.threshold_cache_dir:
        cmd.extend(["--threshold-cache-dir", args.threshold_cache_dir])
    if args.force:
        cmd.append("--force")
    if args.adaptive_threshold_sampling:
        cmd.append("--adaptive-threshold-sampling")
    if args.reuse_worker_pool:
        cmd.append("--reuse-worker-pool")
    if args.disable_action_sort:
        cmd.append("--disable-action-sort")
    if baseline_result_json is not None:
        cmd.extend(["--baseline-result-json", str(baseline_result_json)])
    return cmd


def read_detail_rows(path: Path, look: int) -> list[dict[str, Any]]:
    with path.open(newline="") as handle:
        rows = list(csv.DictReader(handle))
    for row in rows:
        row["look"] = look
        row["rate_delta"] = float(row["rate_delta"])
        row["score_delta"] = float(row["score_delta"])
    return rows


def variant_name_for_swap(swap: str) -> str:
    return parse_swap(swap).name


def summarize_variant(rows: list[dict[str, Any]], variant: str, z: float) -> dict[str, Any]:
    variant_rows = [row for row in rows if row["variant"] == variant]
    rate_values = [float(row["rate_delta"]) for row in variant_rows]
    score_values = [float(row["score_delta"]) for row in variant_rows]
    rate_mean, rate_low, rate_high = mean_ci(rate_values, z)
    score_mean, score_low, score_high = mean_ci(score_values, z)
    return {
        "variant": variant,
        "games": len(variant_rows),
        "rate_delta": rate_mean,
        "rate_delta_ci_low": rate_low,
        "rate_delta_ci_high": rate_high,
        "score_delta_mean": score_mean,
        "score_delta_ci_low": score_low,
        "score_delta_ci_high": score_high,
        "candidate_only_successes": sum(1 for row in variant_rows if row["candidate_hit"] == "True" and row["baseline_hit"] == "False"),
        "baseline_only_successes": sum(1 for row in variant_rows if row["baseline_hit"] == "True" and row["candidate_hit"] == "False"),
        "score_positive": sum(1 for row in variant_rows if float(row["score_delta"]) > 0),
        "score_negative": sum(1 for row in variant_rows if float(row["score_delta"]) < 0),
        "score_tied": sum(1 for row in variant_rows if float(row["score_delta"]) == 0),
    }


def fmt(value: Any) -> Any:
    return f"{value:.8f}" if isinstance(value, float) else value


def write_summary(path: Path, rows: list[dict[str, Any]]) -> None:
    fields = [
        "look",
        "variant",
        "status",
        "reason",
        "games",
        "rate_delta",
        "rate_delta_ci_low",
        "rate_delta_ci_high",
        "score_delta_mean",
        "score_delta_ci_low",
        "score_delta_ci_high",
        "candidate_only_successes",
        "baseline_only_successes",
        "score_positive",
        "score_negative",
        "score_tied",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: fmt(row.get(field, "")) for field in fields})


def stop_decision(summary: dict[str, Any], args: argparse.Namespace) -> tuple[str | None, str | None]:
    if summary["games"] < args.min_games:
        return None, None
    if args.objective == "rate":
        low = summary["rate_delta_ci_low"]
        high = summary["rate_delta_ci_high"]
    else:
        low = summary["score_delta_ci_low"]
        high = summary["score_delta_ci_high"]
    if low > args.benefit_margin:
        return "benefit", f"{args.objective} CI low {low:.6f} > benefit margin {args.benefit_margin:.6f}"
    if high < -args.harm_margin:
        return "harm", f"{args.objective} CI high {high:.6f} < -harm margin {-args.harm_margin:.6f}"
    return None, None


def main() -> int:
    args = parse_args()
    if args.chunks_per_worker is None:
        args.chunks_per_worker = default_chunks_per_worker(args.workers)
    if args.chunk_games <= 0 or args.max_games <= 0:
        raise ValueError("--chunk-games and --max-games must be positive")
    if args.chunks_per_worker <= 0:
        raise ValueError("--chunks-per-worker must be positive")
    if args.min_games > args.max_games:
        raise ValueError("--min-games cannot exceed --max-games")
    max_looks = math.ceil(args.max_games / args.chunk_games)
    if args.no_alpha_spending:
        z = NormalDist().inv_cdf(1 - args.alpha / 2)
        alpha_note = "ordinary per-look interval"
    else:
        z = NormalDist().inv_cdf(1 - args.alpha / (2 * max_looks))
        alpha_note = f"Bonferroni interim interval across {max_looks} looks"

    out_dir = ROOT / args.out_dir
    shard_root = out_dir / "shards"
    out_dir.mkdir(parents=True, exist_ok=True)
    shard_root.mkdir(parents=True, exist_ok=True)
    baseline_cache_dir = (
        (ROOT / args.baseline_cache_dir).resolve()
        if args.baseline_cache_dir
        else out_dir / "baseline_cache"
    )
    baseline_cache_dir.mkdir(parents=True, exist_ok=True)

    active_swaps = list(args.swap)
    stopped: dict[str, tuple[str, str]] = {}
    all_rows: list[dict[str, Any]] = []
    summary_history: list[dict[str, Any]] = []

    for look in range(1, max_looks + 1):
        if not active_swaps:
            break
        shard_dir = shard_root / f"look_{look:03d}"
        shard_seed = args.seed + (look - 1) * 1_000_003
        baseline_cache_json = baseline_cache_dir / f"baseline_seed_{shard_seed}_{baseline_cache_key(args, shard_seed)}.json"
        baseline_result_json = baseline_cache_json if baseline_cache_json.exists() and not args.force else None
        cmd = pair_cmd(args, shard_dir, shard_seed, active_swaps, baseline_result_json=baseline_result_json)
        print(f"look {look}/{max_looks} running {len(active_swaps)} active swap(s) seed={shard_seed}", flush=True)
        if baseline_result_json is not None:
            print(f"look {look} reusing cached baseline {baseline_result_json}", flush=True)
        subprocess.run(cmd, cwd=ROOT, check=True)
        shard_baseline = shard_dir / "results" / "baseline.json"
        if shard_baseline.exists() and (args.force or not baseline_cache_json.exists()):
            shutil.copyfile(shard_baseline, baseline_cache_json)
        shard_rows = read_detail_rows(shard_dir / "paired_game_deltas.csv", look)
        all_rows.extend(shard_rows)

        next_active: list[str] = []
        for swap in active_swaps:
            variant = variant_name_for_swap(swap)
            summary = summarize_variant(all_rows, variant, z)
            status, reason = stop_decision(summary, args)
            if status is None and look == max_looks:
                status = "max_games"
                reason = "Reached max games without a resolved interim CI."
            summary["look"] = look
            summary["status"] = status or "running"
            summary["reason"] = reason or ""
            summary_history.append(summary)
            if status and status != "max_games":
                stopped[variant] = (status, reason or "")
                print(f"stop {variant}: {status} ({reason})", flush=True)
            elif status != "max_games":
                next_active.append(swap)
        active_swaps = next_active

    final_variants = sorted({row["variant"] for row in all_rows})
    final_recorded = {
        row["variant"]
        for row in summary_history
        if row.get("look") == max_looks and row.get("status") in {"benefit", "harm", "max_games"}
    }
    for variant in final_variants:
        if variant in stopped or variant in final_recorded:
            continue
        summary = summarize_variant(all_rows, variant, z)
        summary["look"] = max_looks
        summary["status"] = "max_games"
        summary["reason"] = "Reached max games without a resolved interim CI."
        summary_history.append(summary)

    detail_csv = out_dir / "sequential_game_deltas.csv"
    with detail_csv.open("w", newline="") as handle:
        fields = sorted({key for row in all_rows for key in row})
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(all_rows)
    summary_csv = out_dir / "sequential_summary.csv"
    write_summary(summary_csv, summary_history)
    config_json = out_dir / "run_config.json"
    payload = vars(args).copy()
    payload["z"] = z
    payload["alpha_note"] = alpha_note
    payload["baseline_cache_dir"] = str(baseline_cache_dir)
    config_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    report = out_dir / "sequential_report.md"
    lines = [
        "# Sequential Paired Comparison",
        "",
        f"Objective: `{args.objective}`.",
        f"Interval: `{alpha_note}`, z=`{z:.4f}`.",
        f"Chunk games: `{args.chunk_games}`. Max games: `{args.max_games}`.",
        "",
        f"Summary CSV: `{summary_csv}`",
        f"Detail CSV: `{detail_csv}`",
    ]
    report.write_text("\n".join(lines) + "\n")
    print(summary_csv)
    print(detail_csv)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
