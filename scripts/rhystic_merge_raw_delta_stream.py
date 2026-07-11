#!/usr/bin/env python3
"""Merge rhystic-core raw-delta streaming JSONL outputs into ranked CSVs."""

from __future__ import annotations

import argparse
import csv
import json
import math
from collections import defaultdict
from pathlib import Path
from typing import Any


def read_stream_records(stream_dir: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    starts: list[dict[str, Any]] = []
    variants: list[dict[str, Any]] = []
    completes: list[dict[str, Any]] = []
    for path in sorted(stream_dir.glob("stream_shard*.jsonl")):
        shard = path.stem.replace("stream_shard", "")
        with path.open() as handle:
            for line_number, line in enumerate(handle, start=1):
                if not line.strip():
                    continue
                record = json.loads(line)
                record["_source_file"] = path.name
                record["_line_number"] = line_number
                record["_shard"] = shard
                kind = record.get("record_type")
                if kind == "start":
                    starts.append(record)
                elif kind == "variant":
                    row = dict(record["variant"])
                    row["variant_index"] = record.get("variant_index")
                    row["variants_total"] = record.get("variants_total")
                    row["source_file"] = path.name
                    row["shard"] = shard
                    variants.append(row)
                elif kind == "complete":
                    completes.append(record)
    return starts, variants, completes


def annotate_variant(row: dict[str, Any]) -> dict[str, Any]:
    delta = float(row.get("mean_delta_delta_method", 0.0))
    se = float(row.get("delta_method_se", 0.0))
    z = delta / se if se > 0 else (math.inf if delta > 0 else -math.inf if delta < 0 else 0.0)
    annotated = dict(row)
    annotated["ci95_low"] = delta - 1.96 * se
    annotated["ci95_high"] = delta + 1.96 * se
    annotated["z_score"] = z
    annotated["baseline_hit_rate"] = (
        row["baseline_hits"] / row["samples"] if row.get("samples") else 0.0
    )
    annotated["candidate_hit_rate_delta_method"] = (
        row["candidate_hits_delta_method"] / row["samples"] if row.get("samples") else 0.0
    )
    annotated["hit_rate_delta"] = (
        annotated["candidate_hit_rate_delta_method"] - annotated["baseline_hit_rate"]
    )
    return annotated


def aggregate_group(rows: list[dict[str, Any]], key: str) -> list[dict[str, Any]]:
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        groups[str(row[key])].append(row)
    output: list[dict[str, Any]] = []
    for name, group_rows in groups.items():
        weighted_numerator = 0.0
        weighted_denominator = 0.0
        simple_mean = sum(float(row["mean_delta_delta_method"]) for row in group_rows) / len(group_rows)
        positive = 0
        ci_positive = 0
        for row in group_rows:
            delta = float(row["mean_delta_delta_method"])
            se = float(row.get("delta_method_se", 0.0))
            if delta > 0:
                positive += 1
            if float(row.get("ci95_low", 0.0)) > 0:
                ci_positive += 1
            if se > 0:
                weight = 1.0 / (se * se)
                weighted_numerator += delta * weight
                weighted_denominator += weight
        if weighted_denominator > 0:
            weighted_delta = weighted_numerator / weighted_denominator
            weighted_se = math.sqrt(1.0 / weighted_denominator)
        else:
            weighted_delta = simple_mean
            weighted_se = 0.0
        output.append(
            {
                key: name,
                "rows": len(group_rows),
                "simple_mean_delta": simple_mean,
                "inverse_variance_delta": weighted_delta,
                "inverse_variance_se": weighted_se,
                "ci95_low": weighted_delta - 1.96 * weighted_se,
                "ci95_high": weighted_delta + 1.96 * weighted_se,
                "positive_rows": positive,
                "ci_positive_rows": ci_positive,
                "mean_relevance_rate": sum(float(row.get("relevance_rate", 0.0)) for row in group_rows)
                / len(group_rows),
                "mean_delta_elapsed_ms": sum(float(row.get("delta_elapsed_ms", 0.0)) for row in group_rows)
                / len(group_rows),
                "total_solver_calls": sum(int(row.get("candidate_delta_solver_calls", 0)) for row in group_rows),
            }
        )
    output.sort(key=lambda row: (-float(row["inverse_variance_delta"]), str(row[key])))
    return output


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        path.write_text("")
        return
    fieldnames: list[str] = []
    for row in rows:
        for key in row:
            if key not in fieldnames:
                fieldnames.append(key)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("stream_dir", type=Path)
    parser.add_argument("--prefix", default="raw_delta")
    args = parser.parse_args()

    starts, raw_variants, completes = read_stream_records(args.stream_dir)
    variants = [annotate_variant(row) for row in raw_variants]
    variants.sort(
        key=lambda row: (
            -float(row["mean_delta_delta_method"]),
            float(row.get("delta_method_se", 0.0)),
            str(row.get("name", "")),
        )
    )

    write_csv(args.stream_dir / f"{args.prefix}_ranked.csv", variants)
    write_csv(args.stream_dir / f"{args.prefix}_by_add.csv", aggregate_group(variants, "add"))
    write_csv(args.stream_dir / f"{args.prefix}_by_cut.csv", aggregate_group(variants, "cut"))

    summary = {
        "stream_dir": str(args.stream_dir),
        "stream_files": sorted(path.name for path in args.stream_dir.glob("stream_shard*.jsonl")),
        "start_records": len(starts),
        "variant_records": len(variants),
        "complete_records": len(completes),
        "completed": len(completes) == len(starts) and len(starts) > 0,
        "unsupported_completes": [
            record for record in completes if record.get("unsupported")
        ],
        "baseline": starts[0] if starts else None,
        "top_variants": variants[:20],
    }
    (args.stream_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True))

    print(f"merged {len(variants)} variants from {len(starts)} shards")
    if variants:
        best = variants[0]
        print(
            "best "
            f"{best['name']} delta={float(best['mean_delta_delta_method']):+.5f} "
            f"se={float(best['delta_method_se']):.5f}"
        )


if __name__ == "__main__":
    main()
