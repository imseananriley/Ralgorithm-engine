#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import itertools
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path
from statistics import NormalDist


ROOT = Path(__file__).resolve().parents[1]
NORMAL = NormalDist()


def required_precision(variance: float, half_width: float, alpha: float = 0.05, family: int = 1) -> int:
    z = NORMAL.inv_cdf(1.0 - alpha / (2.0 * family))
    return math.ceil(z * z * variance / (half_width * half_width))


def required_power(
    variance: float,
    effect: float,
    power: float = 0.80,
    alpha: float = 0.05,
    family: int = 1,
) -> int:
    z_alpha = NORMAL.inv_cdf(1.0 - alpha / (2.0 * family))
    z_power = NORMAL.inv_cdf(power)
    return math.ceil((z_alpha + z_power) ** 2 * variance / (effect * effect))


def read_paired(path: Path) -> dict[str, list[dict[str, str]]]:
    rows: dict[str, list[dict[str, str]]] = defaultdict(list)
    with path.open(newline="") as handle:
        for row in csv.DictReader(handle):
            rows[row["variant"]].append(row)
    return dict(rows)


def empirical_rows() -> list[dict[str, object]]:
    sources = [
        (
            "balanced_package",
            ROOT
            / "benchmarks/results/generation32_balanced_flooded_blue_independent_4000/paired_game_deltas.csv",
        ),
        (
            "counterspell_finalists",
            ROOT / "benchmarks/results/generation34_counterspell_finalists_2000/paired_game_deltas.csv",
        ),
        (
            "breach_finalists",
            ROOT
            / "benchmarks/results/generation35_breach_package_finalists_shared_2000/paired_game_deltas.csv",
        ),
        (
            "serum_powder_finalists",
            ROOT / "benchmarks/results/generation36_serum_powder_finalists_2000/paired_game_deltas.csv",
        ),
    ]
    out: list[dict[str, object]] = []
    for experiment, path in sources:
        if not path.exists():
            continue
        for variant, rows in read_paired(path).items():
            raw = [float(row["rate_delta"]) for row in rows]
            score = [float(row["score_delta"]) for row in rows]
            raw_mean = statistics.mean(raw)
            score_mean = statistics.mean(score)
            raw_variance = statistics.variance(raw)
            score_variance = statistics.variance(score)
            discordance = sum(value != 0.0 for value in raw) / len(raw)
            out.append(
                {
                    "experiment": experiment,
                    "variant": variant,
                    "pilot_games": len(rows),
                    "raw_delta": raw_mean,
                    "raw_discordance": discordance,
                    "raw_sd": math.sqrt(raw_variance),
                    "weighted_delta": score_mean,
                    "weighted_sd": math.sqrt(score_variance),
                    "raw_n_80pct_power_observed": (
                        required_power(raw_variance, abs(raw_mean)) if raw_mean else None
                    ),
                    "raw_n_90pct_power_observed": (
                        required_power(raw_variance, abs(raw_mean), power=0.90)
                        if raw_mean
                        else None
                    ),
                    "weighted_n_80pct_power_observed": (
                        required_power(score_variance, abs(score_mean)) if score_mean else None
                    ),
                    "weighted_n_90pct_power_observed": (
                        required_power(score_variance, abs(score_mean), power=0.90)
                        if score_mean
                        else None
                    ),
                }
            )
    return out


def absolute_precision_rows() -> list[dict[str, object]]:
    p = 0.70
    variance = p * (1.0 - p)
    return [
        {
            "estimand": "single_deck_success_rate",
            "assumed_rate": p,
            "half_width_pp": half_width_pp,
            "games": required_precision(variance, half_width_pp / 100.0),
        }
        for half_width_pp in (1.0, 0.5, 0.25, 0.10)
    ]


def binary_design_rows() -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    p0 = 0.70
    for effect_pp in (0.25, 0.5, 1.0, 2.0):
        effect = effect_pp / 100.0
        p1 = p0 + effect
        independent_variance = p0 * (1.0 - p0) + p1 * (1.0 - p1)
        for power in (0.80, 0.90):
            rows.append(
                {
                    "design": "independent_two_deck",
                    "family": 1,
                    "discordance": None,
                    "effect_pp": effect_pp,
                    "power": power,
                    "games_per_deck": required_power(independent_variance, effect, power=power),
                }
            )
        for discordance in (0.03, 0.05, 0.10):
            paired_variance = max(1e-12, discordance - effect * effect)
            for family in (1, 8, 50, 72):
                for power in (0.80, 0.90):
                    rows.append(
                        {
                            "design": "paired_common_random_numbers",
                            "family": family,
                            "discordance": discordance,
                            "effect_pp": effect_pp,
                            "power": power,
                            "games_per_deck": required_power(
                                paired_variance,
                                effect,
                                power=power,
                                family=family,
                            ),
                        }
                    )
    return rows


def weighted_design_rows() -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for sigma in (0.13, 0.16, 0.226):
        variance = sigma * sigma
        for effect_pp in (0.10, 0.25, 0.50, 1.00):
            effect = effect_pp / 100.0
            for family in (1, 8, 50, 72):
                for power in (0.80, 0.90):
                    rows.append(
                        {
                            "weighted_delta_sd": sigma,
                            "family": family,
                            "effect_weighted_pp": effect_pp,
                            "power": power,
                            "games": required_power(
                                variance,
                                effect,
                                power=power,
                                family=family,
                            ),
                        }
                    )
    return rows


def minimum_detectable_effect_rows() -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for games in (500, 2_000, 5_000, 10_000, 50_000, 100_000):
        for family in (1, 8, 72):
            z_alpha = NORMAL.inv_cdf(1.0 - 0.05 / (2.0 * family))
            z_power = NORMAL.inv_cdf(0.80)
            factor = (z_alpha + z_power) / math.sqrt(games)
            for discordance in (0.03, 0.10):
                rows.append(
                    {
                        "games": games,
                        "family": family,
                        "metric": "binary_rate",
                        "pilot_scale": f"discordance={discordance}",
                        "mde_pp_80pct_power": 100.0 * factor * math.sqrt(discordance),
                    }
                )
            for sigma in (0.16, 0.226):
                rows.append(
                    {
                        "games": games,
                        "family": family,
                        "metric": "weighted_score",
                        "pilot_scale": f"sd={sigma}",
                        "mde_pp_80pct_power": 100.0 * factor * sigma,
                    }
                )
    return rows


def slot_control_rows() -> tuple[list[dict[str, object]], dict[str, object]]:
    path = ROOT / "benchmarks/results/generation36_serum_powder_slot_control_500/paired_game_deltas.csv"
    if not path.exists():
        return [], {}
    by_game: dict[int, dict[str, tuple[float, float]]] = defaultdict(dict)
    variants: list[str] = []
    with path.open(newline="") as handle:
        for row in csv.DictReader(handle):
            variant = row["variant"]
            if variant not in variants:
                variants.append(variant)
            by_game[int(row["game_index"])][variant] = (
                float(row["rate_delta"]),
                float(row["score_delta"]),
            )
    rows: list[dict[str, object]] = []
    for coupling_count in range(1, len(variants) + 1):
        raw_variances = []
        score_variances = []
        for subset in itertools.combinations(variants, coupling_count):
            raw = [
                statistics.mean(by_game[game][variant][0] for variant in subset)
                for game in sorted(by_game)
            ]
            score = [
                statistics.mean(by_game[game][variant][1] for variant in subset)
                for game in sorted(by_game)
            ]
            raw_variances.append(statistics.variance(raw))
            score_variances.append(statistics.variance(score))
        raw_variance = statistics.mean(raw_variances)
        score_variance = statistics.mean(score_variances)
        raw_precision_n = required_precision(raw_variance, 0.005)
        raw_power_n = required_power(raw_variance, 0.01)
        rows.append(
            {
                "couplings_per_game": coupling_count,
                "raw_delta_sd": math.sqrt(raw_variance),
                "weighted_delta_sd": math.sqrt(score_variance),
                "games_for_raw_half_width_0_5pp": raw_precision_n,
                "candidate_evaluations_for_raw_half_width_0_5pp": raw_precision_n
                * coupling_count,
                "games_for_80pct_power_raw_1pp": raw_power_n,
                "candidate_evaluations_for_80pct_power_raw_1pp": raw_power_n
                * coupling_count,
            }
        )

    raw_cluster = [
        statistics.mean(values[0] for values in by_game[game].values()) for game in sorted(by_game)
    ]
    score_cluster = [
        statistics.mean(values[1] for values in by_game[game].values()) for game in sorted(by_game)
    ]
    raw_grand = statistics.mean(
        value[0] for game in by_game.values() for value in game.values()
    )
    score_grand = statistics.mean(
        value[1] for game in by_game.values() for value in game.values()
    )

    def repeated_measure_summary(metric_index: int, grand_mean: float) -> dict[str, float]:
        cluster_count = len(by_game)
        coupling_count = len(variants)
        cluster_means = [
            statistics.mean(value[metric_index] for value in by_game[game].values())
            for game in sorted(by_game)
        ]
        between_sum = coupling_count * sum((mean - grand_mean) ** 2 for mean in cluster_means)
        within_sum = sum(
            sum(
                (value[metric_index] - cluster_means[index]) ** 2
                for value in by_game[game].values()
            )
            for index, game in enumerate(sorted(by_game))
        )
        between_mean_square = between_sum / (cluster_count - 1)
        within_mean_square = within_sum / (cluster_count * (coupling_count - 1))
        between_variance = max(
            0.0,
            (between_mean_square - within_mean_square) / coupling_count,
        )
        total_variance = between_variance + within_mean_square
        icc = between_variance / total_variance if total_variance else 0.0
        design_effect = 1.0 + (coupling_count - 1) * icc
        return {
            "intraclass_correlation": icc,
            "design_effect": design_effect,
            "effective_independent_observations": cluster_count * coupling_count / design_effect,
        }

    summary = {
        "unique_game_clusters": len(by_game),
        "couplings_per_game": len(variants),
        "raw_mean": statistics.mean(raw_cluster),
        "raw_sd": statistics.stdev(raw_cluster),
        "raw_ci95": [
            statistics.mean(raw_cluster)
            - NORMAL.inv_cdf(0.975) * statistics.stdev(raw_cluster) / math.sqrt(len(raw_cluster)),
            statistics.mean(raw_cluster)
            + NORMAL.inv_cdf(0.975) * statistics.stdev(raw_cluster) / math.sqrt(len(raw_cluster)),
        ],
        "weighted_mean": statistics.mean(score_cluster),
        "weighted_sd": statistics.stdev(score_cluster),
        "weighted_ci95": [
            statistics.mean(score_cluster)
            - NORMAL.inv_cdf(0.975) * statistics.stdev(score_cluster) / math.sqrt(len(score_cluster)),
            statistics.mean(score_cluster)
            + NORMAL.inv_cdf(0.975) * statistics.stdev(score_cluster) / math.sqrt(len(score_cluster)),
        ],
        "raw_repeated_measure": repeated_measure_summary(0, raw_grand),
        "weighted_repeated_measure": repeated_measure_summary(1, score_grand),
    }
    return rows, summary


def write_csv(path: Path, rows: list[dict[str, object]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        path.write_text("")
        return
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    parser = argparse.ArgumentParser(description="Calculate simulator experiment sample sizes.")
    parser.add_argument(
        "--out-dir",
        default="benchmarks/results/statistical_design",
        help="output directory relative to the repository",
    )
    args = parser.parse_args()
    out_dir = Path(args.out_dir)
    if not out_dir.is_absolute():
        out_dir = ROOT / out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    absolute = absolute_precision_rows()
    binary = binary_design_rows()
    weighted = weighted_design_rows()
    minimum_detectable = minimum_detectable_effect_rows()
    empirical = empirical_rows()
    slot_rows, slot_summary = slot_control_rows()
    write_csv(out_dir / "absolute_rate_precision.csv", absolute)
    write_csv(out_dir / "binary_comparison_power.csv", binary)
    write_csv(out_dir / "weighted_comparison_power.csv", weighted)
    write_csv(out_dir / "minimum_detectable_effect.csv", minimum_detectable)
    write_csv(out_dir / "empirical_pilot_power.csv", empirical)
    write_csv(out_dir / "slot_coupling_efficiency.csv", slot_rows)
    (out_dir / "summary.json").write_text(
        json.dumps(
            {
                "assumptions": {
                    "alpha": 0.05,
                    "power_levels": [0.80, 0.90],
                    "family_adjustment": "Bonferroni planning bound; Holm may be used for analysis",
                    "normal_approximation": True,
                    "independent_unit": "unique game seed/index; repeated couplings are clustered within game",
                },
                "slot_control": slot_summary,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    print(out_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
