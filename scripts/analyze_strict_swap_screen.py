#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import math
from pathlib import Path
from typing import Any


Z95 = 1.959963984540054


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def collect(paths: list[Path]) -> tuple[set[int], set[int], float, float]:
    hits: set[int] = set()
    caps: set[int] = set()
    wall = 0.0
    cpu = 0.0
    for path in paths:
        current = load(path)["current"]
        hits.update(map(int, current["hit_game_indices"]))
        rescue = current.get("rescue") or {}
        caps.update(map(int, rescue.get("final_cap_game_indices") or []))
        wall += float(current.get("wall_seconds") or 0.0)
        cpu += float(current.get("cpu_seconds") or 0.0)
    return hits, caps, wall, cpu


def wilson(successes: int, n: int) -> tuple[float, float]:
    p = successes / n
    z2 = Z95 * Z95
    center = (p + z2 / (2 * n)) / (1 + z2 / n)
    half = Z95 * math.sqrt(p * (1 - p) / n + z2 / (4 * n * n)) / (1 + z2 / n)
    return center - half, center + half


def paired_ci(candidate_only: int, baseline_only: int, n: int) -> tuple[float, float]:
    delta = (candidate_only - baseline_only) / n
    variance = (candidate_only + baseline_only) / (n * n) - delta * delta / n
    half = Z95 * math.sqrt(max(0.0, variance))
    return delta - half, delta + half


def mcnemar_exact(candidate_only: int, baseline_only: int) -> float:
    discordant = candidate_only + baseline_only
    if discordant == 0:
        return 1.0
    tail = sum(math.comb(discordant, k) for k in range(min(candidate_only, baseline_only) + 1))
    return min(1.0, 2.0 * tail / (2**discordant))


def holm(rows: list[dict[str, Any]]) -> None:
    ordered = sorted(enumerate(rows), key=lambda pair: pair[1]["mcnemar_p"])
    running = 0.0
    m = len(rows)
    for rank, (index, row) in enumerate(ordered):
        running = max(running, min(1.0, (m - rank) * row["mcnemar_p"]))
        rows[index]["holm_p"] = running


def holm_field(rows: list[dict[str, Any]], source: str, target: str) -> None:
    ordered = sorted(enumerate(rows), key=lambda pair: pair[1][source])
    running = 0.0
    m = len(rows)
    for rank, (index, row) in enumerate(ordered):
        running = max(running, min(1.0, (m - rank) * row[source]))
        rows[index][target] = running


def main() -> int:
    parser = argparse.ArgumentParser(description="Analyze a paired strict raw-hand swap screen.")
    parser.add_argument("--baseline-glob", required=True)
    parser.add_argument("--screen-root", required=True)
    parser.add_argument("--out-csv", required=True)
    parser.add_argument("--out-json", required=True)
    args = parser.parse_args()

    baseline_paths = sorted(Path().glob(args.baseline_glob)) if not args.baseline_glob.startswith("/") else sorted(Path("/").glob(args.baseline_glob[1:]))
    if not baseline_paths:
        raise FileNotFoundError(f"No baseline files matched {args.baseline_glob}")
    baseline_hits, baseline_caps, baseline_wall, baseline_cpu = collect(baseline_paths)

    rows: list[dict[str, Any]] = []
    for complete_path in sorted(Path(args.screen_root).glob("*/complete.json")):
        metadata = load(complete_path)
        result_paths = sorted(complete_path.parent.glob("result_*.json"))
        candidate_hits, candidate_caps, wall, cpu = collect(result_paths)
        all_ids = baseline_hits | baseline_caps | candidate_hits | candidate_caps
        n = max(all_ids) + 1
        candidate_only_ids = candidate_hits - baseline_hits
        baseline_only_ids = baseline_hits - candidate_hits
        candidate_only = len(candidate_only_ids)
        baseline_only = len(baseline_only_ids)
        ci_low, ci_high = paired_ci(candidate_only, baseline_only, n)
        rate_low, rate_high = wilson(len(candidate_hits), n)
        complete_ids = set(range(n)) - baseline_caps - candidate_caps
        complete_candidate_hits = candidate_hits & complete_ids
        complete_baseline_hits = baseline_hits & complete_ids
        complete_candidate_only = len(complete_candidate_hits - complete_baseline_hits)
        complete_baseline_only = len(complete_baseline_hits - complete_candidate_hits)
        complete_n = len(complete_ids)
        complete_ci_low, complete_ci_high = paired_ci(
            complete_candidate_only, complete_baseline_only, complete_n
        )
        rows.append({
            "swap": metadata["swap"],
            "cut": metadata["cut"],
            "add": metadata["add"],
            "hands": n,
            "baseline_hits": len(baseline_hits),
            "candidate_hits": len(candidate_hits),
            "baseline_rate": len(baseline_hits) / n,
            "candidate_rate": len(candidate_hits) / n,
            "delta": (candidate_only - baseline_only) / n,
            "delta_ci_low": ci_low,
            "delta_ci_high": ci_high,
            "candidate_rate_ci_low": rate_low,
            "candidate_rate_ci_high": rate_high,
            "candidate_only": candidate_only,
            "baseline_only": baseline_only,
            "both_hits": len(candidate_hits & baseline_hits),
            "both_misses": n - len(candidate_hits | baseline_hits),
            "mcnemar_p": mcnemar_exact(candidate_only, baseline_only),
            "baseline_caps": len(baseline_caps),
            "candidate_caps": len(candidate_caps),
            "candidate_only_baseline_capped": len(candidate_only_ids & baseline_caps),
            "baseline_only_candidate_capped": len(baseline_only_ids & candidate_caps),
            "definitive_candidate_only": len(candidate_only_ids - baseline_caps),
            "definitive_baseline_only": len(baseline_only_ids - candidate_caps),
            "complete_case_hands": complete_n,
            "complete_case_candidate_only": complete_candidate_only,
            "complete_case_baseline_only": complete_baseline_only,
            "complete_case_delta": (complete_candidate_only - complete_baseline_only) / complete_n,
            "complete_case_delta_ci_low": complete_ci_low,
            "complete_case_delta_ci_high": complete_ci_high,
            "complete_case_mcnemar_p": mcnemar_exact(
                complete_candidate_only, complete_baseline_only
            ),
            "candidate_cap_aware_upper": (len(candidate_hits) + len(candidate_caps)) / n,
            "wall_seconds_sum": wall,
            "cpu_seconds_sum": cpu,
        })

    holm(rows)
    holm_field(rows, "complete_case_mcnemar_p", "complete_case_holm_p")
    rows.sort(key=lambda row: (-row["delta"], row["holm_p"], row["swap"]))
    for rank, row in enumerate(rows, 1):
        row["rank"] = rank
        row["supported_positive"] = row["holm_p"] < 0.05 and row["delta_ci_low"] > 0
        row["complete_case_supported_positive"] = (
            row["complete_case_holm_p"] < 0.05 and row["complete_case_delta_ci_low"] > 0
        )

    out_csv = Path(args.out_csv)
    out_json = Path(args.out_json)
    out_csv.parent.mkdir(parents=True, exist_ok=True)
    out_json.parent.mkdir(parents=True, exist_ok=True)
    fields = ["rank"] + [key for key in rows[0] if key != "rank"]
    with out_csv.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    out_json.write_text(json.dumps({
        "method": "paired strict raw seven-card hands; identical permutations and random seeds",
        "baseline_files": [str(path) for path in baseline_paths],
        "baseline_wall_seconds_sum": baseline_wall,
        "baseline_cpu_seconds_sum": baseline_cpu,
        "comparisons": len(rows),
        "rows": rows,
    }, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
