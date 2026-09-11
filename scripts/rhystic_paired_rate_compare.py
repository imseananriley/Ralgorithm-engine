#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import random
import shutil
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from rhystic_quick_compare import (
    ROOT,
    SIM,
    Variant,
    parse_swap,
    read_moxfield,
    write_variant_deck,
)
from source_digest import engine_source_digest


LABEL_ENGINE = {
    "Rhystic Study": "rhystic",
    "Underworld Breach combo": "rhystic",
    "Heartwood Storyteller": "heartwood",
    "Mystic Remora": "remora",
    "Smothering Tithe": "tithe",
}


def default_chunks_per_worker(workers: int) -> int:
    return 8 if workers <= 12 else 16


@dataclass(frozen=True)
class PairedStats:
    variant: str
    cut: str
    add: str
    games: int
    baseline_successes: int
    candidate_successes: int
    baseline_rate: float
    candidate_rate: float
    rate_delta: float
    rate_delta_ci_low: float | None
    rate_delta_ci_high: float | None
    score_delta_mean: float
    score_delta_ci_low: float | None
    score_delta_ci_high: float | None
    bootstrap_score_ci_low: float | None
    bootstrap_score_ci_high: float | None
    candidate_only_successes: int
    baseline_only_successes: int
    both_successes: int
    both_misses: int
    mcnemar_p: float | None
    score_positive: int
    score_negative: int
    score_tied: int
    candidate_cap_misses: int
    baseline_cap_misses: int
    n_for_rate_half_width: int | None
    n_for_score_half_width: int | None
    n_for_observed_rate_power: int | None
    n_for_observed_score_power: int | None


def run_sim(args: argparse.Namespace, deck_path: Path, json_out: Path, thresholds_json: Path | None = None) -> None:
    cmd = [
        sys.executable,
        str(SIM),
        "--target",
        args.target,
        "--deck-json",
        str(deck_path),
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
        "--workers",
        str(args.workers),
        "--chunks-per-worker",
        str(args.chunks_per_worker),
        "--seed",
        str(args.seed),
        "--gemstone-caverns-live-rate",
        str(args.gemstone_caverns_live_rate),
        "--gamble-mode",
        args.gamble_mode,
        "--engine-success-policy",
        args.engine_success_policy,
        "--actual-rerun-state-limit",
        str(args.actual_rerun_state_limit),
        "--json-out",
        str(json_out),
        "--suppress-json-stdout",
        "--include-game-records",
        "--compact-game-records",
        "--paired-stage-orders",
    ]
    if thresholds_json is not None:
        cmd.extend(["--thresholds-json", str(thresholds_json)])
    if args.threshold_cache_dir:
        cmd.extend(["--threshold-cache-dir", args.threshold_cache_dir])
    if args.normalize_no_caverns_gemstone_key:
        cmd.append("--normalize-no-caverns-gemstone-key")
    if args.adaptive_threshold_sampling:
        cmd.append("--adaptive-threshold-sampling")
    if args.reuse_worker_pool and not args.rust_full_sim:
        cmd.append("--reuse-worker-pool")
    if args.disable_action_sort:
        cmd.append("--disable-action-sort")
    if args.weighted_policy_ev:
        cmd.extend(
            [
                "--weighted-policy-ev",
                "--rhystic-t1-weight",
                str(args.rhystic_t1_weight),
                "--rhystic-t2-weight",
                str(args.rhystic_t2_weight),
                "--heartwood-t1-weight",
                str(args.heartwood_t1_weight),
                "--heartwood-t2-weight",
                str(args.heartwood_t2_weight),
            ]
        )
    if args.rust_full_sim:
        cmd.append("--rust-full-sim")
        if args.rust_full_sim_games_per_shard is not None:
            cmd.extend(["--rust-full-sim-games-per-shard", str(args.rust_full_sim_games_per_shard)])
        if args.rust_full_sim_shard_workers is not None:
            cmd.extend(["--rust-full-sim-shard-workers", str(args.rust_full_sim_shard_workers)])
        if args.rust_full_sim_internal_shards:
            cmd.append("--rust-full-sim-internal-shards")
    subprocess.run(cmd, cwd=ROOT, check=True)


def load_payload(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def write_json_atomic(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    tmp_path.write_text(json.dumps(payload, indent=2, sort_keys=True))
    tmp_path.replace(path)


def input_fingerprint(deck_path: Path, thresholds_path: Path | None) -> dict[str, Any]:
    thresholds = None
    if thresholds_path is not None:
        payload = load_payload(thresholds_path)
        thresholds = {
            key: payload.get(key)
            for key in ("thresholds", "thresholds_by_gemstone_caverns_live")
        }
    return {"deck_digest": deck_digest(deck_path), "shared_thresholds": thresholds}


def stamp_result_source(path: Path, source_digest: str, fingerprint: dict[str, Any]) -> None:
    payload = load_payload(path)
    payload["engine_source_digest"] = source_digest
    payload["comparison_inputs"] = fingerprint
    write_json_atomic(path, payload)


def value_matches(actual: Any, expected: Any) -> bool:
    if isinstance(expected, float):
        try:
            return abs(float(actual) - expected) <= 1e-12
        except (TypeError, ValueError):
            return False
    return actual == expected


def result_payload_mismatches(
    payload: dict[str, Any],
    args: argparse.Namespace,
    *,
    require_game_records: bool,
) -> list[str]:
    checks: dict[str, Any] = {
        "engine_source_digest": args.engine_source_digest,
        "target": args.target,
        "threshold_hands_per_stage": args.threshold_hands,
        "eval_games": args.eval_games,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "state_limit": args.state_limit,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "engine_success_policy": args.engine_success_policy,
        "gamble_mode": args.gamble_mode,
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "normalize_no_caverns_gemstone_key": args.normalize_no_caverns_gemstone_key,
        "paired_stage_orders": True,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "action_sort": not args.disable_action_sort,
        "seed": args.seed,
        "weighted_policy_ev": args.weighted_policy_ev,
        "rust_full_sim": args.rust_full_sim,
    }
    if args.weighted_policy_ev:
        checks.update(
            {
                "rhystic_t1_weight": args.rhystic_t1_weight,
                "rhystic_t2_weight": args.rhystic_t2_weight,
                "heartwood_t1_weight": args.heartwood_t1_weight,
                "heartwood_t2_weight": args.heartwood_t2_weight,
            }
        )
    if args.rust_full_sim:
        checks.update(
            {
                "rust_full_sim_games_per_shard": args.rust_full_sim_games_per_shard,
                "rust_full_sim_internal_shards": args.rust_full_sim_internal_shards,
            }
        )
    mismatches: list[str] = []
    for key, expected in checks.items():
        actual = payload.get(key)
        if key in {"adaptive_threshold_sampling", "weighted_policy_ev"} and actual is None:
            actual = False
        if key == "action_sort" and actual is None:
            actual = True
        if not value_matches(actual, expected):
            mismatches.append(f"{key}: cached={actual!r} expected={expected!r}")

    evaluation = payload.get("evaluation")
    if not isinstance(evaluation, dict):
        mismatches.append("evaluation: missing or invalid")
        return mismatches
    if not value_matches(evaluation.get("games"), args.eval_games):
        mismatches.append(f"evaluation.games: cached={evaluation.get('games')!r} expected={args.eval_games!r}")
    if require_game_records:
        records = evaluation.get("game_records")
        if not isinstance(records, list):
            mismatches.append("evaluation.game_records: missing")
        elif len(records) != args.eval_games:
            mismatches.append(f"evaluation.game_records: cached_len={len(records)} expected_len={args.eval_games}")
        else:
            try:
                by_game = records_by_game(payload)
                if set(by_game) != set(range(args.eval_games)):
                    mismatches.append("evaluation.game_records: expected contiguous game indices starting at zero")
            except ValueError as exc:
                mismatches.append(str(exc))
    return mismatches


def reusable_result_json(
    path: Path, args: argparse.Namespace, variant_name: str, fingerprint: dict[str, Any]
) -> bool:
    if not path.exists():
        return False
    try:
        payload = load_payload(path)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"rerunning {variant_name}: existing result is unreadable ({exc})", flush=True)
        return False
    mismatches = result_payload_mismatches(payload, args, require_game_records=True)
    if payload.get("comparison_inputs") != fingerprint:
        mismatches.append("deck or shared threshold policy changed (or fingerprint missing)")
    if mismatches:
        preview = "; ".join(mismatches[:4])
        if len(mismatches) > 4:
            preview += f"; ... {len(mismatches) - 4} more"
        print(f"rerunning {variant_name}: existing result settings mismatch ({preview})", flush=True)
        return False
    return True


def read_swap_files(paths: list[str]) -> list[Variant]:
    variants: list[Variant] = []
    for path_text in paths:
        path = Path(path_text)
        if not path.is_absolute():
            path = ROOT / path
        for raw_line in path.read_text().splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            variants.append(parse_swap(line))
    return variants


def deck_digest(path: Path) -> str:
    return hashlib.blake2b(path.read_bytes(), digest_size=12).hexdigest()


def baseline_cache_key(args: argparse.Namespace, deck_json: Path) -> str:
    payload = {
        "engine_source_digest": args.engine_source_digest,
        "deck_digest": deck_digest(deck_json),
        "target": args.target,
        "threshold_hands": args.threshold_hands,
        "eval_games": args.eval_games,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "state_limit": args.state_limit,
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "seed": args.seed,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "engine_success_policy": args.engine_success_policy,
        "gamble_mode": args.gamble_mode,
        "normalize_no_caverns_gemstone_key": args.normalize_no_caverns_gemstone_key,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "weighted_policy_ev": args.weighted_policy_ev,
        "rhystic_t1_weight": args.rhystic_t1_weight,
        "rhystic_t2_weight": args.rhystic_t2_weight,
        "heartwood_t1_weight": args.heartwood_t1_weight,
        "heartwood_t2_weight": args.heartwood_t2_weight,
        "rust_full_sim": args.rust_full_sim,
        "rust_full_sim_games_per_shard": args.rust_full_sim_games_per_shard,
        "rust_full_sim_shard_workers": args.rust_full_sim_shard_workers,
        "rust_full_sim_internal_shards": args.rust_full_sim_internal_shards,
    }
    if args.disable_action_sort:
        payload["action_sort"] = False
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.blake2b(encoded, digest_size=12).hexdigest()


def validate_baseline_result_cache(payload: dict[str, Any], args: argparse.Namespace, source: Path) -> None:
    mismatches = result_payload_mismatches(payload, args, require_game_records=True)
    if payload.get("comparison_inputs") != input_fingerprint((ROOT / args.deck_json).resolve(), None):
        mismatches.append("baseline deck fingerprint differs or is missing")
    if mismatches:
        raise ValueError(f"Baseline result cache {source} does not match this run: " + "; ".join(mismatches))


def records_by_game(payload: dict[str, Any]) -> dict[int, dict[str, Any]]:
    records = payload.get("evaluation", {}).get("game_records") or []
    out: dict[int, dict[str, Any]] = {}
    for row in records:
        index = row.get("game_index") if isinstance(row, dict) else None
        if type(index) is not int or index < 0:
            raise ValueError("game_index must be a nonnegative integer")
        if index in out:
            raise ValueError(f"duplicate game_index: {index}")
        out[index] = row
    return out


def score_record(row: dict[str, Any], weights: dict[tuple[str, int], float]) -> float:
    if not row.get("hit"):
        return 0.0
    turn = row.get("turn")
    if turn is None:
        return 0.0
    try:
        turn_int = int(turn)
    except (TypeError, ValueError):
        return 0.0
    engine = LABEL_ENGINE.get(str(row.get("engine_label") or ""))
    if engine is None:
        engine = "unknown"
    return weights.get((engine, turn_int), 0.0)


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def sample_sd(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    mu = mean(values)
    return math.sqrt(sum((value - mu) ** 2 for value in values) / (len(values) - 1))


def normal_mean_ci(values: list[float], z: float = 1.959963984540054) -> tuple[float, float | None, float | None]:
    if not values:
        return 0.0, None, None
    mu = mean(values)
    if len(values) < 2:
        return mu, None, None
    half = z * sample_sd(values) / math.sqrt(len(values))
    return mu, mu - half, mu + half


def bootstrap_mean_ci(
    values: list[float],
    *,
    samples: int,
    seed: int,
    alpha: float = 0.05,
) -> tuple[float | None, float | None]:
    if samples <= 0 or len(values) < 2:
        return None, None
    rng = random.Random(seed)
    n = len(values)
    means: list[float] = []
    counts = sorted(Counter(values).items())
    binomial = getattr(rng, "binomialvariate", None)
    if binomial is not None and len(counts) < n:
        # Empirical bootstrap counts are multinomial. Conditional binomial draws
        # sample that same law in O(distinct scores), rather than O(games).
        for _ in range(samples):
            remaining_draws = n
            remaining_count = n
            terms = []
            for value, count in counts[:-1]:
                drawn = binomial(remaining_draws, count / remaining_count)
                terms.append(value * drawn)
                remaining_draws -= drawn
                remaining_count -= count
            terms.append(counts[-1][0] * remaining_draws)
            means.append(math.fsum(terms) / n)
    else:
        for _ in range(samples):
            means.append(math.fsum(values[rng.randrange(n)] for _ in range(n)) / n)
    means.sort()
    low_index = max(0, min(samples - 1, int((alpha / 2) * samples)))
    high_index = max(0, min(samples - 1, int((1 - alpha / 2) * samples)))
    return means[low_index], means[high_index]


def log_choose(n: int, k: int) -> float:
    return math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)


def binomial_cdf_leq(k: int, n: int, p: float = 0.5) -> float:
    if k < 0:
        return 0.0
    if k >= n:
        return 1.0
    logs = [log_choose(n, i) + i * math.log(p) + (n - i) * math.log1p(-p) for i in range(k + 1)]
    max_log = max(logs)
    return math.exp(max_log) * sum(math.exp(value - max_log) for value in logs)


def mcnemar_exact_p(candidate_only: int, baseline_only: int) -> float | None:
    n = candidate_only + baseline_only
    if n == 0:
        return 1.0
    p = 2.0 * binomial_cdf_leq(min(candidate_only, baseline_only), n, 0.5)
    return min(1.0, p)


def n_for_half_width(values: list[float], half_width: float) -> int | None:
    if half_width <= 0 or len(values) < 2:
        return None
    sd = sample_sd(values)
    if sd <= 0:
        return 0
    return math.ceil((1.959963984540054 * sd / half_width) ** 2)


def n_for_observed_power(values: list[float], *, alpha_z: float = 1.959963984540054, power_z: float = 0.8416212335729143) -> int | None:
    if len(values) < 2:
        return None
    mu = mean(values)
    sd = sample_sd(values)
    if sd <= 0:
        return 0 if abs(mu) > 0 else None
    if abs(mu) <= 0:
        return None
    return math.ceil(((alpha_z + power_z) * sd / abs(mu)) ** 2)


def paired_stats(
    baseline_payload: dict[str, Any],
    candidate_payload: dict[str, Any],
    *,
    variant: Variant,
    weights: dict[tuple[str, int], float],
    bootstrap_samples: int,
    bootstrap_seed: int,
    rate_half_width: float,
    score_half_width: float,
) -> tuple[PairedStats, list[dict[str, Any]]]:
    baseline_records = records_by_game(baseline_payload)
    candidate_records = records_by_game(candidate_payload)
    if not baseline_records or set(baseline_records) != set(candidate_records):
        raise ValueError("paired comparison requires identical nonempty game-index sets")
    if baseline_payload.get("seed") != candidate_payload.get("seed"):
        raise ValueError("paired comparison requires matching root seeds")
    shared_games = sorted(baseline_records)
    rate_deltas: list[float] = []
    score_deltas: list[float] = []
    rows: list[dict[str, Any]] = []
    candidate_only = 0
    baseline_only = 0
    both_success = 0
    both_miss = 0
    score_positive = 0
    score_negative = 0
    score_tied = 0
    baseline_successes = 0
    candidate_successes = 0
    baseline_caps = 0
    candidate_caps = 0

    for game_index in shared_games:
        base = baseline_records[game_index]
        cand = candidate_records[game_index]
        base_hit = bool(base.get("hit"))
        cand_hit = bool(cand.get("hit"))
        baseline_successes += int(base_hit)
        candidate_successes += int(cand_hit)
        baseline_caps += int(bool(base.get("capped")) and not base_hit)
        candidate_caps += int(bool(cand.get("capped")) and not cand_hit)
        if cand_hit and not base_hit:
            candidate_only += 1
        elif base_hit and not cand_hit:
            baseline_only += 1
        elif cand_hit and base_hit:
            both_success += 1
        else:
            both_miss += 1
        rate_delta = float(cand_hit) - float(base_hit)
        base_score = score_record(base, weights)
        cand_score = score_record(cand, weights)
        score_delta = cand_score - base_score
        rate_deltas.append(rate_delta)
        score_deltas.append(score_delta)
        score_positive += int(score_delta > 0)
        score_negative += int(score_delta < 0)
        score_tied += int(score_delta == 0)
        rows.append(
            {
                "game_index": game_index,
                "variant": variant.name,
                "cut": variant.cut or "",
                "add": variant.add or "",
                "baseline_hit": base_hit,
                "candidate_hit": cand_hit,
                "rate_delta": rate_delta,
                "baseline_turn": base.get("turn") or "miss",
                "candidate_turn": cand.get("turn") or "miss",
                "baseline_engine_label": base.get("engine_label") or "",
                "candidate_engine_label": cand.get("engine_label") or "",
                "baseline_score": base_score,
                "candidate_score": cand_score,
                "score_delta": score_delta,
                "baseline_capped": bool(base.get("capped")),
                "candidate_capped": bool(cand.get("capped")),
            }
        )

    _rate_mean, rate_low, rate_high = normal_mean_ci(rate_deltas)
    score_mean, score_low, score_high = normal_mean_ci(score_deltas)
    boot_low, boot_high = bootstrap_mean_ci(
        score_deltas,
        samples=bootstrap_samples,
        seed=bootstrap_seed,
    )
    games = len(shared_games)
    stats = PairedStats(
        variant=variant.name,
        cut=variant.cut or "",
        add=variant.add or "",
        games=games,
        baseline_successes=baseline_successes,
        candidate_successes=candidate_successes,
        baseline_rate=baseline_successes / games if games else 0.0,
        candidate_rate=candidate_successes / games if games else 0.0,
        rate_delta=mean(rate_deltas),
        rate_delta_ci_low=rate_low,
        rate_delta_ci_high=rate_high,
        score_delta_mean=score_mean,
        score_delta_ci_low=score_low,
        score_delta_ci_high=score_high,
        bootstrap_score_ci_low=boot_low,
        bootstrap_score_ci_high=boot_high,
        candidate_only_successes=candidate_only,
        baseline_only_successes=baseline_only,
        both_successes=both_success,
        both_misses=both_miss,
        mcnemar_p=mcnemar_exact_p(candidate_only, baseline_only),
        score_positive=score_positive,
        score_negative=score_negative,
        score_tied=score_tied,
        candidate_cap_misses=candidate_caps,
        baseline_cap_misses=baseline_caps,
        n_for_rate_half_width=n_for_half_width(rate_deltas, rate_half_width),
        n_for_score_half_width=n_for_half_width(score_deltas, score_half_width),
        n_for_observed_rate_power=n_for_observed_power(rate_deltas),
        n_for_observed_score_power=n_for_observed_power(score_deltas),
    )
    return stats, rows


def as_csv_value(value: Any) -> Any:
    if isinstance(value, float):
        return f"{value:.8f}"
    if value is None:
        return ""
    return value


def write_summary_csv(path: Path, stats_rows: list[PairedStats]) -> None:
    fields = list(PairedStats.__dataclass_fields__)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in stats_rows:
            payload = {field: as_csv_value(getattr(row, field)) for field in fields}
            writer.writerow(payload)


def write_detail_csv(path: Path, detail_rows: list[dict[str, Any]]) -> None:
    fields = [
        "game_index",
        "variant",
        "cut",
        "add",
        "baseline_hit",
        "candidate_hit",
        "rate_delta",
        "baseline_turn",
        "candidate_turn",
        "baseline_engine_label",
        "candidate_engine_label",
        "baseline_score",
        "candidate_score",
        "score_delta",
        "baseline_capped",
        "candidate_capped",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in detail_rows:
            writer.writerow({field: as_csv_value(row.get(field)) for field in fields})


def fmt_pct(value: float | None) -> str:
    if value is None:
        return ""
    return f"{100 * value:.3f}%"


def fmt_float(value: float | None) -> str:
    if value is None:
        return ""
    return f"{value:.4f}"


def write_markdown(path: Path, stats_rows: list[PairedStats], args: argparse.Namespace) -> None:
    lines = [
        "# Paired Rhystic/Heartwood Rate Comparison",
        "",
        f"Deck: `{args.deck_json}`",
        f"Games per variant: `{args.eval_games}`. Paired stage orders: enabled. Records: compact, no trace.",
        f"Target: `{args.target}`. Gamble mode: `{args.gamble_mode}`. Engine policy: `{args.engine_success_policy}`.",
        f"Weights: Rhystic T1={args.rhystic_t1_weight}, Rhystic T2={args.rhystic_t2_weight}, Heartwood T1={args.heartwood_t1_weight}, Heartwood T2={args.heartwood_t2_weight}.",
        "",
        "Primary inference uses paired per-game deltas. Rate CIs and score CIs are normal approximations over paired deltas; optional bootstrap CI is reported for score when enabled. McNemar p-values use exact discordant-pair tests for binary success.",
        "",
        "| Variant | Swap | Rate Delta | Rate CI95 | Score Delta | Score CI95 | Bootstrap Score CI95 | Discordant W-L | McNemar p | n rate half-width | n score half-width |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in stats_rows:
        swap = f"{row.cut} -> {row.add}" if row.cut else "baseline"
        p_text = "" if row.mcnemar_p is None else f"{row.mcnemar_p:.4g}"
        lines.append(
            "| {variant} | {swap} | {rate_delta} | [{rate_low}, {rate_high}] | {score_delta} | [{score_low}, {score_high}] | [{boot_low}, {boot_high}] | {win}-{loss} | {p} | {n_rate} | {n_score} |".format(
                variant=row.variant,
                swap=swap,
                rate_delta=fmt_pct(row.rate_delta),
                rate_low=fmt_pct(row.rate_delta_ci_low),
                rate_high=fmt_pct(row.rate_delta_ci_high),
                score_delta=fmt_float(row.score_delta_mean),
                score_low=fmt_float(row.score_delta_ci_low),
                score_high=fmt_float(row.score_delta_ci_high),
                boot_low=fmt_float(row.bootstrap_score_ci_low),
                boot_high=fmt_float(row.bootstrap_score_ci_high),
                win=row.candidate_only_successes,
                loss=row.baseline_only_successes,
                p=p_text,
                n_rate=row.n_for_rate_half_width or "",
                n_score=row.n_for_score_half_width or "",
            )
        )
    path.write_text("\n".join(lines) + "\n")


def serializable_config(args: argparse.Namespace) -> dict[str, Any]:
    payload = vars(args).copy()
    payload["swap"] = [
        {"name": variant.name, "cut": variant.cut, "add": variant.add, "swaps": list(variant.swaps)}
        for variant in args.swap
    ]
    return payload


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run paired card-swap comparisons with common random numbers."
    )
    parser.add_argument(
        "--deck-json",
        default="fixtures/decks/nick_fury_generation32_balanced_optimized.json",
    )
    parser.add_argument("--out-dir", default="benchmarks/results/paired_rate_compare")
    parser.add_argument("--target", default="rhystic_heartwood", choices=("rhystic", "heartwood", "rhystic_heartwood"))
    parser.add_argument("--swap", action="append", type=parse_swap, default=[])
    parser.add_argument("--swap-file", action="append", default=[])
    parser.add_argument("--threshold-hands", type=int, default=60)
    parser.add_argument("--eval-games", type=int, default=1000)
    parser.add_argument("--samples-per-bottom", type=int, default=2)
    parser.add_argument("--validation-samples", type=int, default=2)
    parser.add_argument(
        "--allow-single-sample-mulligan-policy",
        action="store_true",
        help="Allow paired runs with samples-per-bottom or validation-samples below 2. Intended only for tiny smoke tests.",
    )
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument(
        "--workers",
        type=int,
        default=max(1, min(8, (os.cpu_count() or 2) - 1)),
    )
    parser.add_argument("--chunks-per-worker", type=int, default=None)
    parser.add_argument("--seed", type=int, default=2026062905)
    parser.add_argument("--threshold-cache-dir", default=None)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--normalize-no-caverns-gemstone-key", action="store_true", default=True)
    parser.add_argument("--no-normalize-no-caverns-gemstone-key", dest="normalize_no_caverns_gemstone_key", action="store_false")
    parser.add_argument("--adaptive-threshold-sampling", action="store_true")
    parser.add_argument("--reuse-worker-pool", action="store_true")
    parser.add_argument("--disable-action-sort", action="store_true")
    parser.add_argument("--weighted-policy-ev", action="store_true")
    parser.add_argument("--rust-full-sim", action="store_true")
    parser.add_argument("--rust-full-sim-games-per-shard", type=int, default=None)
    parser.add_argument("--rust-full-sim-shard-workers", type=int, default=None)
    parser.add_argument("--rust-full-sim-internal-shards", action="store_true")
    parser.add_argument("--shared-thresholds", action="store_true", default=True)
    parser.add_argument("--independent-thresholds", dest="shared_thresholds", action="store_false")
    parser.add_argument("--bootstrap-samples", type=int, default=0)
    parser.add_argument("--rate-half-width", type=float, default=0.0025)
    parser.add_argument("--score-half-width", type=float, default=0.25)
    parser.add_argument("--rhystic-t1-weight", type=float, default=1.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=0.75)
    parser.add_argument("--heartwood-t1-weight", type=float, default=0.70)
    parser.add_argument("--heartwood-t2-weight", type=float, default=0.55)
    parser.add_argument("--force", action="store_true")
    parser.add_argument(
        "--baseline-result-json",
        default=None,
        help="Existing baseline simulator result to reuse. It must match this comparison's simulator settings.",
    )
    parser.add_argument(
        "--baseline-cache-dir",
        default=None,
        help="Directory for automatic validated baseline result reuse. Defaults to <out-dir>/baseline_cache.",
    )
    parser.add_argument("--no-baseline-cache", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    args.engine_source_digest = engine_source_digest(ROOT)
    if (
        args.eval_games >= 100
        and not args.allow_single_sample_mulligan_policy
        and (args.samples_per_bottom < 2 or args.validation_samples < 2)
    ):
        raise ValueError(
            "Paired comparison runs with eval-games >= 100 require "
            "--samples-per-bottom >= 2 and --validation-samples >= 2. "
            "The prior 1/1 setting produced unstable mulligan keeps; use "
            "--allow-single-sample-mulligan-policy only for smoke tests."
        )
    args.swap = [*read_swap_files(args.swap_file), *args.swap]
    if args.chunks_per_worker is None:
        args.chunks_per_worker = default_chunks_per_worker(args.workers)
    if args.chunks_per_worker <= 0:
        raise ValueError("--chunks-per-worker must be positive")
    deck_json = (ROOT / args.deck_json).resolve()
    out_dir = (ROOT / args.out_dir).resolve()
    deck_dir = out_dir / "decks"
    result_dir = out_dir / "results"
    deck_dir.mkdir(parents=True, exist_ok=True)
    result_dir.mkdir(parents=True, exist_ok=True)
    baseline_cache_dir = (
        (ROOT / args.baseline_cache_dir).resolve()
        if args.baseline_cache_dir
        else out_dir / "baseline_cache"
    )
    if not args.no_baseline_cache:
        baseline_cache_dir.mkdir(parents=True, exist_ok=True)

    _payload = read_moxfield(deck_json)
    variants = [Variant("baseline", None, None), *args.swap]
    names = [variant.name for variant in variants]
    if len(names) != len(set(names)):
        raise ValueError("Variant names must be unique; 'baseline' is reserved")
    if any(not name or name in {".", ".."} or "/" in name or "\\" in name for name in names):
        raise ValueError("Variant names must be plain filenames without path separators")
    if len(variants) <= 1:
        raise ValueError("Provide at least one --swap for paired comparison.")

    weights = {
        ("rhystic", 1): args.rhystic_t1_weight,
        ("rhystic", 2): args.rhystic_t2_weight,
        ("heartwood", 1): args.heartwood_t1_weight,
        ("heartwood", 2): args.heartwood_t2_weight,
    }

    result_paths: dict[str, Path] = {}
    baseline_thresholds_json: Path | None = None
    variants_to_run = variants
    baseline_source: Path | None = None
    explicit_baseline_source = False
    if args.baseline_result_json:
        baseline_source = (ROOT / args.baseline_result_json).resolve()
        explicit_baseline_source = True
    elif not args.no_baseline_cache and not args.force:
        candidate_cache = baseline_cache_dir / f"baseline_{baseline_cache_key(args, deck_json)}.json"
        if candidate_cache.exists():
            baseline_source = candidate_cache
    if baseline_source is not None:
        if not baseline_source.exists():
            raise FileNotFoundError(f"--baseline-result-json not found: {baseline_source}")
        try:
            baseline_payload = load_payload(baseline_source)
            validate_baseline_result_cache(baseline_payload, args, baseline_source)
        except (OSError, json.JSONDecodeError, ValueError) as exc:
            if explicit_baseline_source:
                raise
            print(f"ignoring unusable baseline cache {baseline_source}: {exc}", flush=True)
            baseline_source = None
        if baseline_source is not None:
            baseline_deck = deck_dir / "baseline.json"
            baseline_result = result_dir / "baseline.json"
            write_variant_deck(deck_json, baseline_deck, None, None)
            if baseline_source != baseline_result:
                shutil.copyfile(baseline_source, baseline_result)
            result_paths["baseline"] = baseline_result
            baseline_thresholds_json = baseline_result
            variants_to_run = variants[1:]

    for variant in variants_to_run:
        variant_deck = deck_dir / f"{variant.name}.json"
        result_json = result_dir / f"{variant.name}.json"
        write_variant_deck(deck_json, variant_deck, variant.cut, variant.add, swaps=variant.swaps)
        thresholds_json = baseline_thresholds_json if args.shared_thresholds and variant.name != "baseline" else None
        fingerprint = input_fingerprint(deck_json if variant.name == "baseline" else variant_deck, thresholds_json)
        if args.force or not reusable_result_json(result_json, args, variant.name, fingerprint):
            print(f"running {variant.name}", flush=True)
            run_sim(args, variant_deck, result_json, thresholds_json=thresholds_json)
            stamp_result_source(result_json, args.engine_source_digest, fingerprint)
        else:
            print(f"reusing {variant.name}", flush=True)
        if variant.name == "baseline":
            baseline_thresholds_json = result_json
            if not args.no_baseline_cache:
                cache_path = baseline_cache_dir / f"baseline_{baseline_cache_key(args, deck_json)}.json"
                if args.force or not cache_path.exists():
                    shutil.copyfile(result_json, cache_path)
        result_paths[variant.name] = result_json

    baseline_payload = load_payload(result_paths["baseline"])
    stats_rows: list[PairedStats] = []
    detail_rows: list[dict[str, Any]] = []
    for index, variant in enumerate(variants[1:], start=1):
        candidate_payload = load_payload(result_paths[variant.name])
        stats, rows = paired_stats(
            baseline_payload,
            candidate_payload,
            variant=variant,
            weights=weights,
            bootstrap_samples=args.bootstrap_samples,
            bootstrap_seed=args.seed + index * 10_000,
            rate_half_width=args.rate_half_width,
            score_half_width=args.score_half_width,
        )
        stats_rows.append(stats)
        detail_rows.extend(rows)

    summary_csv = out_dir / "paired_summary.csv"
    detail_csv = out_dir / "paired_game_deltas.csv"
    report_md = out_dir / "paired_report.md"
    config_json = out_dir / "run_config.json"
    write_summary_csv(summary_csv, stats_rows)
    write_detail_csv(detail_csv, detail_rows)
    write_markdown(report_md, stats_rows, args)
    write_json_atomic(config_json, serializable_config(args))
    print(summary_csv)
    print(detail_csv)
    print(report_md)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
