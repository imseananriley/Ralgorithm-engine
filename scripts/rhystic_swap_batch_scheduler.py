#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PAIR = ROOT / "scripts" / "rhystic_paired_rate_compare.py"


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser(
        description=(
            "Run many Rhystic paired swap comparisons in batched groups while sharing "
            "the validated baseline result cache across every group."
        )
    )
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--swap", action="append", required=True)
    parser.add_argument("--group-size", type=int, default=8)
    parser.add_argument("--baseline-cache-dir", default=None)
    parser.add_argument("--force", action="store_true")
    parser.add_argument(
        "paired_args",
        nargs=argparse.REMAINDER,
        help="Arguments after '--' are forwarded to rhystic_paired_rate_compare.py.",
    )
    args = parser.parse_args()
    forwarded = list(args.paired_args)
    if forwarded and forwarded[0] == "--":
        forwarded = forwarded[1:]
    if args.group_size <= 0:
        raise ValueError("--group-size must be positive")
    return args, forwarded


def chunks(items: list[str], size: int) -> list[list[str]]:
    return [items[index : index + size] for index in range(0, len(items), size)]


def read_csv_rows(path: Path) -> list[dict[str, Any]]:
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle))


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    if not rows:
        path.write_text("")
        return
    fields = sorted({key for row in rows for key in row})
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    args, forwarded = parse_args()
    out_dir = (ROOT / args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    cache_dir = (
        (ROOT / args.baseline_cache_dir).resolve()
        if args.baseline_cache_dir
        else out_dir / "baseline_cache"
    )
    cache_dir.mkdir(parents=True, exist_ok=True)

    group_rows: list[dict[str, Any]] = []
    summary_rows: list[dict[str, Any]] = []
    detail_rows: list[dict[str, Any]] = []
    for group_index, group_swaps in enumerate(chunks(args.swap, args.group_size), start=1):
        group_dir = out_dir / f"group_{group_index:03d}"
        cmd = [
            sys.executable,
            str(PAIR),
            "--out-dir",
            str(group_dir),
            "--baseline-cache-dir",
            str(cache_dir),
            *forwarded,
        ]
        for swap in group_swaps:
            cmd.extend(["--swap", swap])
        if args.force:
            cmd.append("--force")

        started = time.perf_counter()
        subprocess.run(cmd, cwd=ROOT, check=True)
        elapsed = time.perf_counter() - started
        group_rows.append(
            {
                "group": group_index,
                "swaps": ";".join(group_swaps),
                "swap_count": len(group_swaps),
                "elapsed_seconds": f"{elapsed:.6f}",
                "group_dir": str(group_dir),
            }
        )
        for row in read_csv_rows(group_dir / "paired_summary.csv"):
            row["group"] = group_index
            summary_rows.append(row)
        for row in read_csv_rows(group_dir / "paired_game_deltas.csv"):
            row["group"] = group_index
            detail_rows.append(row)

    write_csv(out_dir / "scheduler_groups.csv", group_rows)
    write_csv(out_dir / "scheduler_paired_summary.csv", summary_rows)
    write_csv(out_dir / "scheduler_game_deltas.csv", detail_rows)
    (out_dir / "run_config.json").write_text(
        json.dumps(
            {
                "swaps": args.swap,
                "group_size": args.group_size,
                "baseline_cache_dir": str(cache_dir),
                "forwarded_args": forwarded,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    print(out_dir / "scheduler_paired_summary.csv")
    print(out_dir / "scheduler_game_deltas.csv")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
