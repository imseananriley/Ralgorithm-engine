#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import math
import random
from pathlib import Path
from typing import Any


WEIGHTS = {
    ("Rhystic Study", 1): 1.0,
    ("Rhystic Study", 2): 0.75,
    ("Heartwood Storyteller", 1): 0.65,
    ("Heartwood Storyteller", 2): 0.50,
}


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def sample_sd(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    mu = mean(values)
    return math.sqrt(sum((value - mu) ** 2 for value in values) / (len(values) - 1))


def normal_ci(values: list[float], z: float = 1.959963984540054) -> tuple[float, float, float]:
    mu = mean(values)
    if len(values) < 2:
        return mu, mu, mu
    half = z * sample_sd(values) / math.sqrt(len(values))
    return mu, mu - half, mu + half


def bootstrap_ci(values: list[float], *, seed: int, samples: int) -> tuple[float | None, float | None]:
    if not values or samples <= 0:
        return None, None
    rng = random.Random(seed)
    n = len(values)
    means = []
    for _ in range(samples):
        means.append(sum(values[rng.randrange(n)] for _i in range(n)) / n)
    means.sort()
    return means[int(0.025 * samples)], means[min(samples - 1, int(0.975 * samples))]


def log_choose(n: int, k: int) -> float:
    return math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)


def binomial_cdf(k: int, n: int, p: float = 0.5) -> float:
    if k < 0:
        return 0.0
    if k >= n:
        return 1.0
    logs = [log_choose(n, i) + i * math.log(p) + (n - i) * math.log1p(-p) for i in range(k + 1)]
    max_log = max(logs)
    return math.exp(max_log) * sum(math.exp(value - max_log) for value in logs)


def mcnemar_exact(candidate_only: int, baseline_only: int) -> float | None:
    n = candidate_only + baseline_only
    if n == 0:
        return None
    return min(1.0, 2.0 * binomial_cdf(min(candidate_only, baseline_only), n))


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def records_by_game(payload: dict[str, Any]) -> dict[int, dict[str, Any]]:
    records = payload.get("evaluation", {}).get("game_records")
    if not isinstance(records, list):
        raise ValueError(f"{payload.get('deck_json', '<unknown>')} is missing evaluation.game_records")
    return {int(row["game_index"]): row for row in records}


def weighted_score(row: dict[str, Any]) -> float:
    if not row.get("hit"):
        return 0.0
    try:
        turn = int(row.get("turn"))
    except (TypeError, ValueError):
        return 0.0
    return WEIGHTS.get((row.get("engine_label"), turn), 0.0)


def write_rows(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results-dir", required=True)
    parser.add_argument("--out-prefix", required=True)
    parser.add_argument("--bootstrap-samples", type=int, default=5000)
    parser.add_argument("--bootstrap-seed", type=int, default=2026070404)
    parser.add_argument("--promote-swap-file", default=None)
    parser.add_argument("--source-swap-file", default=None)
    parser.add_argument("--top-n", type=int, default=16)
    parser.add_argument("--min-score-ci-high", type=float, default=0.005)
    parser.add_argument("--min-score-mean", type=float, default=-0.001)
    parser.add_argument("--fill-top-n", action="store_true")
    args = parser.parse_args()

    results_dir = Path(args.results_dir)
    out_prefix = Path(args.out_prefix)
    baseline_path = results_dir / "baseline.json"
    if not baseline_path.exists():
        raise FileNotFoundError(f"Missing baseline result: {baseline_path}")

    baseline = load_json(baseline_path)
    baseline_records = records_by_game(baseline)
    baseline_scores = {index: weighted_score(row) for index, row in baseline_records.items()}
    baseline_hits = {index: bool(row.get("hit")) for index, row in baseline_records.items()}
    baseline_games = int(baseline.get("eval_games") or baseline["evaluation"]["games"])

    summary_rows: list[dict[str, Any]] = []
    detail_rows: list[dict[str, Any]] = []
    for path in sorted(results_dir.glob("*.json")):
        if path.name == "baseline.json":
            continue
        payload = load_json(path)
        candidate_records = records_by_game(payload)
        indices = sorted(set(baseline_records) & set(candidate_records))
        if len(indices) != baseline_games:
            raise ValueError(f"{path} has {len(indices)} paired games, expected {baseline_games}")

        hit_deltas: list[float] = []
        score_deltas: list[float] = []
        candidate_only = baseline_only = both = misses = 0
        score_positive = score_negative = score_tied = 0
        turn_counts: dict[str, int] = {}
        label_counts: dict[str, int] = {}
        for index in indices:
            baseline_row = baseline_records[index]
            candidate_row = candidate_records[index]
            baseline_hit = baseline_hits[index]
            candidate_hit = bool(candidate_row.get("hit"))
            baseline_score = baseline_scores[index]
            candidate_score = weighted_score(candidate_row)
            hit_delta = int(candidate_hit) - int(baseline_hit)
            score_delta = candidate_score - baseline_score
            hit_deltas.append(float(hit_delta))
            score_deltas.append(score_delta)
            if candidate_hit and baseline_hit:
                both += 1
            elif candidate_hit:
                candidate_only += 1
            elif baseline_hit:
                baseline_only += 1
            else:
                misses += 1
            if score_delta > 0:
                score_positive += 1
            elif score_delta < 0:
                score_negative += 1
            else:
                score_tied += 1
            if candidate_hit:
                turn_key = str(candidate_row.get("turn"))
                label_key = str(candidate_row.get("engine_label"))
                turn_counts[turn_key] = turn_counts.get(turn_key, 0) + 1
                label_counts[label_key] = label_counts.get(label_key, 0) + 1
            detail_rows.append(
                {
                    "variant": path.stem,
                    "game_index": index,
                    "baseline_hit": baseline_hit,
                    "candidate_hit": candidate_hit,
                    "hit_delta": hit_delta,
                    "baseline_score": baseline_score,
                    "candidate_score": candidate_score,
                    "score_delta": score_delta,
                    "baseline_turn": baseline_row.get("turn"),
                    "candidate_turn": candidate_row.get("turn"),
                    "baseline_engine": baseline_row.get("engine_label"),
                    "candidate_engine": candidate_row.get("engine_label"),
                }
            )

        raw_mean, raw_low, raw_high = normal_ci(hit_deltas)
        score_mean, score_low, score_high = normal_ci(score_deltas)
        boot_low, boot_high = bootstrap_ci(
            score_deltas,
            seed=args.bootstrap_seed + len(summary_rows),
            samples=args.bootstrap_samples,
        )
        raw_sd = sample_sd(hit_deltas)
        score_sd = sample_sd(score_deltas)
        raw_n = math.ceil((1.959963984540054 * raw_sd / 0.0025) ** 2) if raw_sd else 0
        score_n = math.ceil((1.959963984540054 * score_sd / 0.002) ** 2) if score_sd else 0
        p_value = mcnemar_exact(candidate_only, baseline_only)
        candidate_successes = int(payload["evaluation"]["successes"])
        baseline_successes = int(baseline["evaluation"]["successes"])
        summary_rows.append(
            {
                "variant": path.stem,
                "games": len(indices),
                "baseline_successes": baseline_successes,
                "candidate_successes": candidate_successes,
                "baseline_rate": baseline_successes / len(indices),
                "candidate_rate": candidate_successes / len(indices),
                "raw_rate_delta": raw_mean,
                "raw_delta_ci_low": raw_low,
                "raw_delta_ci_high": raw_high,
                "baseline_weighted_score": mean([baseline_scores[index] for index in indices]),
                "candidate_weighted_score": mean([weighted_score(candidate_records[index]) for index in indices]),
                "weighted_score_delta": score_mean,
                "score_delta_ci_low": score_low,
                "score_delta_ci_high": score_high,
                "bootstrap_score_ci_low": boot_low,
                "bootstrap_score_ci_high": boot_high,
                "candidate_only_successes": candidate_only,
                "baseline_only_successes": baseline_only,
                "both_successes": both,
                "both_misses": misses,
                "mcnemar_exact_p": "" if p_value is None else p_value,
                "score_positive_games": score_positive,
                "score_negative_games": score_negative,
                "score_tied_games": score_tied,
                "turn_counts": json.dumps(turn_counts, sort_keys=True),
                "label_counts": json.dumps(label_counts, sort_keys=True),
                "n_for_raw_half_width_0.25pp": raw_n,
                "n_for_score_half_width_0.20pp": score_n,
                "elapsed_seconds": payload.get("elapsed_seconds"),
                "threshold_seconds": payload.get("threshold_elapsed_seconds"),
                "eval_seconds": payload.get("eval_elapsed_seconds"),
                "cap_misses": payload["evaluation"].get("cap_misses"),
            }
        )

    summary_rows.sort(key=lambda row: (row["weighted_score_delta"], row["raw_rate_delta"]), reverse=True)
    write_rows(out_prefix.with_suffix(".csv"), summary_rows)
    write_rows(Path(str(out_prefix) + "_game_deltas.csv"), detail_rows)

    lines = [
        "# Paired Result Analysis",
        "",
        f"Completed candidate variants: {len(summary_rows)}; games per variant: {baseline_games}.",
        f"Baseline raw rate: {baseline['evaluation']['successes']}/{baseline_games} = {baseline['evaluation']['success_rate']:.3%}.",
        f"Baseline weighted score: {mean(list(baseline_scores.values())):.4f}.",
        "",
        "| variant | raw delta | weighted delta | 95% score CI | cand-only | base-only | McNemar p |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for row in summary_rows:
        p_value = row["mcnemar_exact_p"]
        p_text = "" if p_value == "" else f"{p_value:.4f}"
        lines.append(
            f"| {row['variant']} | {row['raw_rate_delta']:+.2%} | {row['weighted_score_delta']:+.4f} | "
            f"[{row['score_delta_ci_low']:+.4f}, {row['score_delta_ci_high']:+.4f}] | "
            f"{row['candidate_only_successes']} | {row['baseline_only_successes']} | {p_text} |"
        )
    out_prefix.with_suffix(".md").write_text("\n".join(lines) + "\n")

    if args.promote_swap_file:
        if not args.source_swap_file:
            raise ValueError("--promote-swap-file requires --source-swap-file")
        swap_by_name: dict[str, str] = {}
        for raw_line in Path(args.source_swap_file).read_text().splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#") or ":" not in line:
                continue
            name = line.split(":", 1)[0].strip()
            swap_by_name[name] = raw_line
        promoted: list[dict[str, Any]] = []
        for row in summary_rows:
            plausible = (
                (row["weighted_score_delta"] > 0 and row["raw_delta_ci_high"] >= 0)
                or row["score_delta_ci_low"] > 0
                or (row["score_delta_ci_high"] >= args.min_score_ci_high and row["weighted_score_delta"] >= args.min_score_mean)
            )
            if plausible and row["variant"] in swap_by_name:
                promoted.append(row)
        if args.fill_top_n:
            for row in summary_rows:
                if len(promoted) >= args.top_n:
                    break
                if row["variant"] in swap_by_name and row not in promoted:
                    promoted.append(row)
        promoted = promoted[: args.top_n]
        promote_path = Path(args.promote_swap_file)
        promote_path.parent.mkdir(parents=True, exist_ok=True)
        promote_path.write_text(
            "# Auto-selected follow-up swaps from paired analysis.\n"
            f"# Source results: {results_dir}\n"
            "# Selection: positive weighted mean with non-negative raw upper CI, positive score lower CI,\n"
            f"# or uncertain upside score_ci_high >= {args.min_score_ci_high} with score_mean >= {args.min_score_mean}.\n"
            f"# fill_top_n={args.fill_top_n}; top_n={args.top_n}.\n\n"
            + "\n".join(swap_by_name[row["variant"]] for row in promoted)
            + ("\n" if promoted else "")
        )
    print(out_prefix.with_suffix(".csv"))
    print(out_prefix.with_suffix(".md"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
