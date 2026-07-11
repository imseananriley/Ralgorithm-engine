#!/usr/bin/env python3
"""Build a unit-consistent land-count curve from raw and strict outputs."""

from __future__ import annotations

import argparse
import csv
import json
import math
import re
import statistics
from pathlib import Path
from xml.sax.saxutils import escape


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_EXPERIMENT_DIR = ROOT / "data/rhystic_study_turn12/land_count_curve_champion_20260707"

MILD_SCORE_POINTS = {
    ("Rhystic Study", 1): 100.0,
    ("Rhystic Study", 2): 75.0,
    ("Heartwood Storyteller", 1): 65.0,
    ("Heartwood Storyteller", 2): 50.0,
}

BRANCH_LABELS = {
    "add_rainbow_over_interaction": "raw +lands over interaction",
    "lands_to_birds": "raw -lands to Birds",
    "add_rainbow_over_gas": "raw +lands over gas",
}

BRANCH_COLORS = {
    "add_rainbow_over_interaction": "#2166ac",
    "lands_to_birds": "#b2182b",
    "add_rainbow_over_gas": "#1b7837",
}


def fnum(value: object, default: float = 0.0) -> float:
    if value in ("", None):
        return default
    return float(value)


def normal_ci(values: list[float]) -> tuple[float, float, float]:
    if not values:
        return 0.0, 0.0, 0.0
    mean = statistics.fmean(values)
    if len(values) < 2:
        return mean, mean, mean
    se = statistics.stdev(values) / math.sqrt(len(values))
    return mean, mean - 1.96 * se, mean + 1.96 * se


def independent_rate_delta_ci_pp(n: int, baseline_rate: float, candidate_rate: float) -> tuple[float, float]:
    if n <= 0:
        delta_pp = (candidate_rate - baseline_rate) * 100.0
        return delta_pp, delta_pp
    se = math.sqrt(
        baseline_rate * (1.0 - baseline_rate) / n
        + candidate_rate * (1.0 - candidate_rate) / n
    )
    delta_pp = (candidate_rate - baseline_rate) * 100.0
    return delta_pp - 1.96 * se * 100.0, delta_pp + 1.96 * se * 100.0


def land_count_from_name(name: str) -> int:
    match = re.search(r"land_count_(\d+)_", name)
    if not match:
        raise ValueError(f"could not parse land count from {name!r}")
    return int(match.group(1))


def branch_from_name(name: str) -> str:
    if "rainbow_over_interaction" in name:
        return "add_rainbow_over_interaction"
    if "lands_to_birds" in name:
        return "lands_to_birds"
    if "rainbow_over_gas" in name:
        return "add_rainbow_over_gas"
    raise ValueError(f"could not parse branch from {name!r}")


def score_record(record: dict[str, object]) -> float:
    label = record.get("engine_label")
    turn = record.get("turn")
    if label is None or turn is None:
        return 0.0
    return MILD_SCORE_POINTS.get((str(label), int(turn)), 0.0)


def strict_mild_row(
    *,
    baseline_path: Path,
    candidate_path: Path,
    summary_by_variant: dict[str, dict[str, str]],
) -> dict[str, object]:
    baseline = json.loads(baseline_path.read_text())["evaluation"]["game_records"]
    candidate_payload = json.loads(candidate_path.read_text())
    candidate = candidate_payload["evaluation"]["game_records"]
    baseline_by_game = {int(row["game_index"]): row for row in baseline}
    candidate_by_game = {int(row["game_index"]): row for row in candidate}
    shared = sorted(set(baseline_by_game) & set(candidate_by_game))
    score_deltas: list[float] = []
    rate_deltas: list[float] = []
    for game in shared:
        base = baseline_by_game[game]
        cand = candidate_by_game[game]
        score_deltas.append(score_record(cand) - score_record(base))
        rate_deltas.append(float(bool(cand.get("hit"))) - float(bool(base.get("hit"))))

    score_mean, score_low, score_high = normal_ci(score_deltas)
    rate_mean, rate_low, rate_high = normal_ci(rate_deltas)
    name = candidate_path.stem
    summary = summary_by_variant.get(name, {})
    land_count = land_count_from_name(name)
    branch = branch_from_name(name)
    return {
        "estimator": "strict_policy_1k_mild_recomputed",
        "branch": branch,
        "land_count": land_count,
        "delta_lands": land_count - 32,
        "n": len(shared),
        "score_delta_points": score_mean,
        "score_ci95_low_points": score_low,
        "score_ci95_high_points": score_high,
        "hit_rate_delta_pp": rate_mean * 100.0,
        "hit_rate_ci95_low_pp": rate_low * 100.0,
        "hit_rate_ci95_high_pp": rate_high * 100.0,
        "baseline_hit_rate": fnum(summary.get("baseline_rate"), 0.0),
        "candidate_hit_rate": fnum(summary.get("candidate_rate"), 0.0),
        "relevance_rate": "",
        "name": name,
        "cut": summary.get("cut", ""),
        "add": summary.get("add", ""),
        "source_file": str(candidate_path.relative_to(ROOT)),
        "notes": "strict policy records rescored with mild weights 100/75/65/50",
    }


def read_raw_rows(path: Path, estimator: str) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    with path.open(newline="") as handle:
        for row in csv.DictReader(handle):
            name = row["name"]
            land_count = land_count_from_name(name)
            branch = branch_from_name(name)
            samples = int(fnum(row.get("samples")))
            baseline_hit_rate = fnum(row.get("baseline_hit_rate"))
            candidate_hit_rate = fnum(row.get("candidate_hit_rate_delta_method"))
            hit_ci_low, hit_ci_high = independent_rate_delta_ci_pp(
                samples, baseline_hit_rate, candidate_hit_rate
            )
            rows.append(
                {
                    "estimator": estimator,
                    "branch": branch,
                    "land_count": land_count,
                    "delta_lands": land_count - 32,
                    "n": samples,
                    "score_delta_points": fnum(row.get("mean_delta_delta_method")) * 100.0,
                    "score_ci95_low_points": fnum(row.get("ci95_low")) * 100.0,
                    "score_ci95_high_points": fnum(row.get("ci95_high")) * 100.0,
                    "hit_rate_delta_pp": fnum(row.get("hit_rate_delta")) * 100.0,
                    "hit_rate_ci95_low_pp": hit_ci_low,
                    "hit_rate_ci95_high_pp": hit_ci_high,
                    "baseline_hit_rate": baseline_hit_rate,
                    "candidate_hit_rate": candidate_hit_rate,
                    "relevance_rate": fnum(row.get("relevance_rate")),
                    "name": name,
                    "cut": row.get("cut", ""),
                    "add": row.get("add", ""),
                    "source_file": str(path.relative_to(ROOT)),
                    "notes": "raw score scaled from mild probability units to score points; raw hit-rate CI is conservative independent-binomial approximation",
                }
            )
    return rows


def load_rows(experiment_dir: Path) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    rows.extend(read_raw_rows(experiment_dir / "raw_grouped_600/land_curve_600_ranked.csv", "raw_delta_600"))
    rows.extend(read_raw_rows(experiment_dir / "raw_gas_fixed_600/gas_fixed_600_ranked.csv", "raw_delta_600"))

    strict_dir = experiment_dir / "strict_policy_focus_1k"
    with (strict_dir / "paired_summary.csv").open(newline="") as handle:
        summary_by_variant = {row["variant"]: row for row in csv.DictReader(handle)}
    baseline_path = strict_dir / "results/baseline.json"
    for candidate_path in sorted((strict_dir / "results").glob("land_count_*.json")):
        rows.append(
            strict_mild_row(
                baseline_path=baseline_path,
                candidate_path=candidate_path,
                summary_by_variant=summary_by_variant,
            )
        )

    rows.sort(
        key=lambda row: (
            str(row["estimator"]),
            str(row["branch"]),
            int(row["land_count"]),
            str(row["name"]),
        )
    )
    return rows


def write_csv(rows: list[dict[str, object]], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = [
        "estimator",
        "branch",
        "land_count",
        "delta_lands",
        "n",
        "score_delta_points",
        "score_ci95_low_points",
        "score_ci95_high_points",
        "hit_rate_delta_pp",
        "hit_rate_ci95_low_pp",
        "hit_rate_ci95_high_pp",
        "baseline_hit_rate",
        "candidate_hit_rate",
        "relevance_rate",
        "name",
        "cut",
        "add",
        "source_file",
        "notes",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def svg_line(points: list[tuple[float, float]]) -> str:
    if not points:
        return ""
    head, *tail = points
    return "M {:.1f} {:.1f}{}".format(
        head[0],
        head[1],
        "".join(" L {:.1f} {:.1f}".format(x, y) for x, y in tail),
    )


def metric_config(metric: str) -> dict[str, str]:
    if metric == "score_delta":
        return {
            "value": "score_delta_points",
            "low": "score_ci95_low_points",
            "high": "score_ci95_high_points",
            "ylabel": "Mild weighted score delta vs champion, points",
            "title": "Champion land-count curves, unit-corrected mild objective",
            "note": "Raw values use mild weights scaled to points; strict markers are the same saved games rescored as 100/75/65/50.",
            "strict_label": "strict policy 1k, rescored mild",
        }
    if metric == "hit_delta":
        return {
            "value": "hit_rate_delta_pp",
            "low": "hit_rate_ci95_low_pp",
            "high": "hit_rate_ci95_high_pp",
            "ylabel": "Hit-rate delta vs champion, percentage points",
            "title": "Champion land-count curves, raw hit-rate delta",
            "note": "Raw curves show hit-rate delta in percentage points; raw CI bars are conservative approximations, strict markers are paired 1k deltas.",
            "strict_label": "strict policy 1k hit-rate delta",
        }
    raise ValueError(f"unsupported metric {metric!r}")


def write_svg(rows: list[dict[str, object]], path: Path, *, metric: str = "score_delta") -> None:
    config = metric_config(metric)
    width = 960
    height = 560
    left = 78
    right = 34
    top = 42
    bottom = 86
    plot_w = width - left - right
    plot_h = height - top - bottom
    x_min = 24
    x_max = 40
    raw_rows = [row for row in rows if row["estimator"] == "raw_delta_600"]
    strict_rows = [row for row in rows if row["estimator"] == "strict_policy_1k_mild_recomputed"]

    y_values: list[float] = [0.0]
    for row in raw_rows + strict_rows:
        y_values.append(fnum(row[config["value"]]))
        y_values.append(fnum(row[config["low"]], fnum(row[config["value"]])))
        y_values.append(fnum(row[config["high"]], fnum(row[config["value"]])))
    y_min = math.floor((min(y_values) - 0.5) / 1.0) * 1.0
    y_max = math.ceil((max(y_values) + 0.5) / 1.0) * 1.0

    def x_scale(value: float) -> float:
        return left + (value - x_min) / (x_max - x_min) * plot_w

    def y_scale(value: float) -> float:
        return top + (y_max - value) / (y_max - y_min) * plot_h

    out: list[str] = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect x="0" y="0" width="{width}" height="{height}" fill="white"/>',
        "<style>"
        "text{font-family:Arial,sans-serif;font-size:13px;fill:#222}"
        ".axis{stroke:#333;stroke-width:1}.grid{stroke:#ddd;stroke-width:1}"
        ".label{font-size:14px;font-weight:600}.legend{font-size:13px}.note{font-size:12px;fill:#555}"
        "</style>",
    ]

    for tick in range(x_min, x_max + 1, 2):
        x = x_scale(tick)
        out.append(f'<line class="grid" x1="{x:.1f}" y1="{top}" x2="{x:.1f}" y2="{top + plot_h}"/>')
        out.append(f'<text x="{x:.1f}" y="{top + plot_h + 24}" text-anchor="middle">{tick}</text>')

    y_tick_start = math.ceil(y_min / 2.0) * 2
    tick = y_tick_start
    while tick <= y_max + 0.01:
        y = y_scale(tick)
        out.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left + plot_w}" y2="{y:.1f}"/>')
        tick_label = "0" if abs(tick) < 1e-9 else f"{tick:+.0f}"
        out.append(f'<text x="{left - 8}" y="{y + 4:.1f}" text-anchor="end">{tick_label}</text>')
        tick += 2

    out.append(f'<line class="axis" x1="{left}" y1="{top + plot_h}" x2="{left + plot_w}" y2="{top + plot_h}"/>')
    out.append(f'<line class="axis" x1="{left}" y1="{top}" x2="{left}" y2="{top + plot_h}"/>')
    zero_y = y_scale(0.0)
    out.append(f'<line x1="{left}" y1="{zero_y:.1f}" x2="{left + plot_w}" y2="{zero_y:.1f}" stroke="#777" stroke-dasharray="4 4"/>')
    out.append(f'<circle cx="{x_scale(32):.1f}" cy="{zero_y:.1f}" r="4" fill="#555"/>')
    out.append(f'<text x="{x_scale(32) + 8:.1f}" y="{zero_y - 8:.1f}" class="note">32-land champion baseline</text>')

    out.append(f'<text class="label" x="{left + plot_w / 2:.1f}" y="{height - 32}" text-anchor="middle">Simulator land count</text>')
    out.append(f'<text class="label" transform="translate(22 {top + plot_h / 2:.1f}) rotate(-90)" text-anchor="middle">{escape(config["ylabel"])}</text>')
    out.append(f'<text class="label" x="{left + plot_w / 2:.1f}" y="23" text-anchor="middle">{escape(config["title"])}</text>')

    for branch in ["add_rainbow_over_interaction", "lands_to_birds", "add_rainbow_over_gas"]:
        branch_rows = sorted(
            [row for row in raw_rows if row["branch"] == branch],
            key=lambda row: int(row["land_count"]),
        )
        if not branch_rows:
            continue
        color = BRANCH_COLORS[branch]
        points = [(x_scale(int(row["land_count"])), y_scale(fnum(row[config["value"]]))) for row in branch_rows]
        out.append(f'<path d="{svg_line(points)}" fill="none" stroke="{color}" stroke-width="2.5"/>')
        for row in branch_rows:
            x = x_scale(int(row["land_count"]))
            y = y_scale(fnum(row[config["value"]]))
            y_low = y_scale(fnum(row[config["low"]], fnum(row[config["value"]])))
            y_high = y_scale(fnum(row[config["high"]], fnum(row[config["value"]])))
            out.append(f'<line x1="{x:.1f}" y1="{y_low:.1f}" x2="{x:.1f}" y2="{y_high:.1f}" stroke="{color}" stroke-width="1.2" opacity="0.55"/>')
            out.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="{color}"/>')

    for row in sorted(strict_rows, key=lambda row: (str(row["branch"]), int(row["land_count"]))):
        x = x_scale(int(row["land_count"]))
        y = y_scale(fnum(row[config["value"]]))
        y_low = y_scale(fnum(row[config["low"]], fnum(row[config["value"]])))
        y_high = y_scale(fnum(row[config["high"]], fnum(row[config["value"]])))
        out.append(f'<line x1="{x:.1f}" y1="{y_low:.1f}" x2="{x:.1f}" y2="{y_high:.1f}" stroke="#000" stroke-width="1.4"/>')
        out.append(
            '<polygon points="{:.1f},{:.1f} {:.1f},{:.1f} {:.1f},{:.1f} {:.1f},{:.1f}" fill="#000"/>'.format(
                x,
                y - 7,
                x + 7,
                y,
                x,
                y + 7,
                x - 7,
                y,
            )
        )

    legend_x = width - 350
    legend_y = 55
    for index, branch in enumerate(["add_rainbow_over_interaction", "lands_to_birds", "add_rainbow_over_gas"]):
        y = legend_y + index * 22
        color = BRANCH_COLORS[branch]
        out.append(f'<line x1="{legend_x}" y1="{y}" x2="{legend_x + 28}" y2="{y}" stroke="{color}" stroke-width="2.5"/>')
        out.append(f'<circle cx="{legend_x + 14}" cy="{y}" r="4" fill="{color}"/>')
        out.append(f'<text class="legend" x="{legend_x + 36}" y="{y + 4}">{escape(BRANCH_LABELS[branch])}</text>')
    strict_y = legend_y + 66
    out.append(
        '<polygon points="{0},{1} {2},{3} {4},{5} {6},{7}" fill="#000"/>'.format(
            legend_x + 14,
            strict_y - 7,
            legend_x + 21,
            strict_y,
            legend_x + 14,
            strict_y + 7,
            legend_x + 7,
            strict_y,
        )
    )
    out.append(f'<text class="legend" x="{legend_x + 36}" y="{strict_y + 4}">{escape(config["strict_label"])}</text>')
    out.append(
        f'<text class="note" x="{left}" y="{height - 10}">'
        f'{escape(config["note"])}'
        "</text>"
    )
    out.append("</svg>")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(out) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--experiment-dir", type=Path, default=DEFAULT_EXPERIMENT_DIR)
    parser.add_argument("--out-csv", type=Path, default=None)
    parser.add_argument("--out-svg", type=Path, default=None)
    parser.add_argument("--metric", choices=["score_delta", "hit_delta"], default="score_delta")
    args = parser.parse_args()

    experiment_dir = args.experiment_dir.resolve()
    out_csv = args.out_csv or experiment_dir / "land_count_curve_corrected.csv"
    out_svg = args.out_svg or experiment_dir / "land_count_curve_corrected.svg"

    rows = load_rows(experiment_dir)
    write_csv(rows, out_csv)
    write_svg(rows, out_svg, metric=args.metric)
    print(f"wrote {out_csv}")
    print(f"wrote {out_svg}")


if __name__ == "__main__":
    main()
