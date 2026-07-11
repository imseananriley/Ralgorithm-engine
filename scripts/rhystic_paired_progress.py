#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import re
import shlex
import subprocess
import time
from pathlib import Path
from typing import Any

from rhystic_paired_rate_compare import (
    ROOT,
    Variant,
    load_payload,
    paired_stats,
    read_swap_files,
    write_detail_csv,
    write_summary_csv,
)


RUNNING_RE = re.compile(r"^(running|reusing)\s+(.+?)\s*$")
SHARD_RE = re.compile(r"^rust shard\s+(\d+)\s+games=(\d+)\s+successes=(\d+)\s+elapsed=([0-9.]+)s")


def resolve_repo_path(raw: str | None, *, default: Path | None = None) -> Path | None:
    if raw is None:
        return default
    path = Path(raw)
    if not path.is_absolute():
        path = ROOT / path
    return path.resolve()


def atomic_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(f".{path.name}.{time.time_ns()}.tmp")
    tmp_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp_path.replace(path)


def parse_log(path: Path | None, completed: set[str]) -> dict[str, Any]:
    if path is None or not path.exists():
        return {"current_variant": None, "last_log_line": "", "shards": []}
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return {"current_variant": None, "last_log_line": "", "shards": []}

    last_running: str | None = None
    last_running_index = -1
    for index, line in enumerate(lines):
        match = RUNNING_RE.match(line.strip())
        if match:
            last_running = match.group(2)
            last_running_index = index

    shards: list[dict[str, Any]] = []
    if last_running_index >= 0:
        for line in lines[last_running_index + 1 :]:
            match = SHARD_RE.match(line.strip())
            if match:
                shards.append(
                    {
                        "shard": int(match.group(1)),
                        "games": int(match.group(2)),
                        "successes": int(match.group(3)),
                        "elapsed_seconds": float(match.group(4)),
                    }
                )
    if last_running in completed:
        last_running = None
        shards = []
    return {
        "current_variant": last_running,
        "last_log_line": next((line for line in reversed(lines) if line.strip()), ""),
        "shards": shards,
    }


def paired_runner_active(out_dir: Path) -> bool:
    try:
        result = subprocess.run(
            ["ps", "-axo", "command="],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
    except Exception:
        return False
    out_text = out_dir.as_posix()
    for line in result.stdout.splitlines():
        if "rhystic_paired_rate_compare.py" not in line or "rhystic_paired_progress.py" in line:
            continue
        try:
            parts = shlex.split(line)
        except ValueError:
            parts = line.split()
        for index, part in enumerate(parts):
            if part != "--out-dir" or index + 1 >= len(parts):
                continue
            value = parts[index + 1]
            if value == out_text or value == out_dir.name or value.endswith(f"/{out_dir.name}"):
                return True
    return False


def result_payload(path: Path) -> dict[str, Any] | None:
    try:
        payload = load_payload(path)
    except Exception:
        return None
    evaluation = payload.get("evaluation")
    if not isinstance(evaluation, dict):
        return None
    games = int(payload.get("eval_games") or evaluation.get("games") or 0)
    records = evaluation.get("game_records")
    if not isinstance(records, list) or (games and len(records) != games):
        return None
    return payload


def result_summary(name: str, path: Path, payload: dict[str, Any]) -> dict[str, Any]:
    evaluation = payload.get("evaluation") or {}
    games = int(payload.get("eval_games") or evaluation.get("games") or 0)
    successes = int(evaluation.get("successes") or 0)
    return {
        "name": name,
        "path": path.relative_to(ROOT).as_posix(),
        "mtime": path.stat().st_mtime,
        "games": games,
        "successes": successes,
        "success_rate": successes / games if games else None,
        "turn_counts": evaluation.get("turn_counts") or {},
        "cap_misses": evaluation.get("cap_misses"),
        "elapsed_seconds": payload.get("elapsed_seconds"),
        "eval_elapsed_seconds": payload.get("eval_elapsed_seconds"),
        "threshold_elapsed_seconds": payload.get("threshold_elapsed_seconds"),
    }


def seconds_text(seconds: float | None) -> str:
    if seconds is None or not math.isfinite(seconds) or seconds < 0:
        return "unknown"
    seconds = int(round(seconds))
    hours, rem = divmod(seconds, 3600)
    minutes, secs = divmod(rem, 60)
    if hours:
        return f"{hours}h {minutes}m"
    if minutes:
        return f"{minutes}m {secs}s"
    return f"{secs}s"


def progress_bar(percent: float, width: int = 30) -> str:
    filled = max(0, min(width, int(round(width * percent))))
    return "[" + "#" * filled + "-" * (width - filled) + "]"


def analysis_weights(baseline: dict[str, Any]) -> dict[tuple[str, int], float]:
    return {
        ("rhystic", 1): float(baseline.get("rhystic_t1_weight", 1.0)),
        ("rhystic", 2): float(baseline.get("rhystic_t2_weight", 0.75)),
        ("heartwood", 1): float(baseline.get("heartwood_t1_weight", 0.65)),
        ("heartwood", 2): float(baseline.get("heartwood_t2_weight", 0.5)),
    }


def write_intermediate_analysis(
    *,
    out_dir: Path,
    variants: list[Variant],
    payloads: dict[str, dict[str, Any]],
    prefix_name: str,
    rate_half_width: float,
    score_half_width: float,
) -> list[dict[str, Any]]:
    baseline = payloads.get("baseline")
    if baseline is None:
        return []
    weights = analysis_weights(baseline)
    stats_rows = []
    detail_rows: list[dict[str, Any]] = []
    for index, variant in enumerate(variants[1:], start=1):
        candidate = payloads.get(variant.name)
        if candidate is None:
            continue
        stats, rows = paired_stats(
            baseline,
            candidate,
            variant=variant,
            weights=weights,
            bootstrap_samples=0,
            bootstrap_seed=2026070400 + index,
            rate_half_width=rate_half_width,
            score_half_width=score_half_width,
        )
        stats_rows.append(stats)
        detail_rows.extend(rows)
    stats_rows.sort(key=lambda row: (row.score_delta_mean, row.rate_delta), reverse=True)
    prefix = out_dir / prefix_name
    write_summary_csv(prefix.with_suffix(".csv"), stats_rows)
    write_detail_csv(Path(str(prefix) + "_game_deltas.csv"), detail_rows)

    lines = [
        "# Intermediate Paired Analysis",
        "",
        f"Completed candidate variants: {len(stats_rows)}.",
        "Bootstrap is intentionally disabled in the live monitor; final reports can rerun it.",
        "",
        "| variant | swap | raw delta | score delta | score CI95 | cand-only | base-only |",
        "|---|---|---:|---:|---:|---:|---:|",
    ]
    for row in stats_rows:
        lines.append(
            "| {variant} | {cut} -> {add} | {rate:+.2%} | {score:+.4f} | [{low:+.4f}, {high:+.4f}] | {win} | {loss} |".format(
                variant=row.variant,
                cut=row.cut,
                add=row.add,
                rate=row.rate_delta,
                score=row.score_delta_mean,
                low=row.score_delta_ci_low if row.score_delta_ci_low is not None else 0.0,
                high=row.score_delta_ci_high if row.score_delta_ci_high is not None else 0.0,
                win=row.candidate_only_successes,
                loss=row.baseline_only_successes,
            )
        )
    prefix.with_suffix(".md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return [
        {
            "variant": row.variant,
            "cut": row.cut,
            "add": row.add,
            "rate_delta": row.rate_delta,
            "score_delta": row.score_delta_mean,
            "score_delta_ci_low": row.score_delta_ci_low,
            "score_delta_ci_high": row.score_delta_ci_high,
            "candidate_only_successes": row.candidate_only_successes,
            "baseline_only_successes": row.baseline_only_successes,
            "candidate_successes": row.candidate_successes,
            "candidate_rate": row.candidate_rate,
        }
        for row in stats_rows[:12]
    ]


def write_progress_markdown(path: Path, payload: dict[str, Any]) -> None:
    outputs = payload.get("outputs") or {}
    top_rows = payload.get("top_intermediate_rows") or []
    percent = float(payload.get("percent_complete") or 0.0)
    lines = [
        "# Run Progress",
        "",
        f"{progress_bar(percent)} {payload.get('completed_variants', 0)}/{payload.get('total_variants', 0)} ({percent:.1%})",
        "",
        f"Current: `{payload.get('current_variant') or 'idle'}`",
        f"Last completed: `{(payload.get('latest_completed') or {}).get('name', 'none')}`",
        f"ETA: `{payload.get('eta_text', 'unknown')}`",
        "",
        "## Outputs",
        "",
    ]
    for label, value in outputs.items():
        lines.append(f"- {label}: `{value}`")
    if top_rows:
        lines.extend(
            [
                "",
                "## Top Intermediate Rows",
                "",
                "| variant | swap | raw delta | score delta | score CI95 |",
                "|---|---|---:|---:|---:|",
            ]
        )
        for row in top_rows:
            low = row.get("score_delta_ci_low")
            high = row.get("score_delta_ci_high")
            lines.append(
                "| {variant} | {cut} -> {add} | {rate:+.2%} | {score:+.4f} | [{low:+.4f}, {high:+.4f}] |".format(
                    variant=row["variant"],
                    cut=row["cut"],
                    add=row["add"],
                    rate=float(row["rate_delta"]),
                    score=float(row["score_delta"]),
                    low=float(low) if low is not None else 0.0,
                    high=float(high) if high is not None else 0.0,
                )
            )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Write live progress artifacts for a paired Rhystic sweep.")
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--swap-file", action="append", default=[])
    parser.add_argument("--log-file", default=None)
    parser.add_argument("--progress-json", default=None)
    parser.add_argument("--progress-md", default=None)
    parser.add_argument("--analysis-prefix", default="intermediate_summary")
    parser.add_argument("--rate-half-width", type=float, default=0.0025)
    parser.add_argument("--score-half-width", type=float, default=0.002)
    parser.add_argument("--no-analysis", action="store_true")
    args = parser.parse_args()

    out_dir = resolve_repo_path(args.out_dir)
    if out_dir is None:
        raise ValueError("--out-dir is required")
    result_dir = out_dir / "results"
    log_file = resolve_repo_path(args.log_file, default=out_dir / "run_local.log")
    progress_json = resolve_repo_path(args.progress_json, default=out_dir / "progress.json")
    progress_md = resolve_repo_path(args.progress_md, default=out_dir / "progress.md")
    swap_files = [str(resolve_repo_path(path)) for path in args.swap_file]
    variants = [Variant("baseline", None, None), *read_swap_files(swap_files)]

    payloads: dict[str, dict[str, Any]] = {}
    summaries: list[dict[str, Any]] = []
    for variant in variants:
        path = result_dir / f"{variant.name}.json"
        if not path.exists():
            continue
        payload = result_payload(path)
        if payload is None:
            continue
        payloads[variant.name] = payload
        summaries.append(result_summary(variant.name, path, payload))

    completed = set(payloads)
    summaries.sort(key=lambda row: row["mtime"])
    latest_completed = summaries[-1] if summaries else None
    candidate_durations = [
        float(row["elapsed_seconds"])
        for row in summaries
        if row["name"] != "baseline" and isinstance(row.get("elapsed_seconds"), (int, float))
    ]
    avg_variant_seconds = sum(candidate_durations) / len(candidate_durations) if candidate_durations else None
    total = len(variants)
    done = len(completed)
    log_state = parse_log(log_file, completed)
    active = paired_runner_active(out_dir)
    if done >= total:
        run_status = "complete"
    elif active:
        run_status = "running"
    elif log_state["current_variant"]:
        run_status = "paused"
    else:
        run_status = "idle"
    remaining = max(0, total - done)
    eta_seconds = avg_variant_seconds * remaining if avg_variant_seconds is not None else None

    top_rows: list[dict[str, Any]] = []
    if not args.no_analysis and "baseline" in payloads:
        top_rows = write_intermediate_analysis(
            out_dir=out_dir,
            variants=variants,
            payloads=payloads,
            prefix_name=args.analysis_prefix,
            rate_half_width=args.rate_half_width,
            score_half_width=args.score_half_width,
        )

    outputs = {
        "progress_json": progress_json.relative_to(ROOT).as_posix() if progress_json else "",
        "progress_md": progress_md.relative_to(ROOT).as_posix() if progress_md else "",
        "intermediate_summary_csv": (out_dir / f"{args.analysis_prefix}.csv").relative_to(ROOT).as_posix(),
        "intermediate_report_md": (out_dir / f"{args.analysis_prefix}.md").relative_to(ROOT).as_posix(),
        "intermediate_game_deltas_csv": (out_dir / f"{args.analysis_prefix}_game_deltas.csv").relative_to(ROOT).as_posix(),
    }
    progress_payload = {
        "generated_at": time.time(),
        "out_dir": out_dir.relative_to(ROOT).as_posix(),
        "result_dir": result_dir.relative_to(ROOT).as_posix(),
        "swap_files": [Path(path).relative_to(ROOT).as_posix() for path in swap_files],
        "total_variants": total,
        "completed_variants": done,
        "completed_candidates": max(0, done - int("baseline" in completed)),
        "remaining_variants": remaining,
        "percent_complete": done / total if total else 0.0,
        "run_status": run_status,
        "runner_active": active,
        "current_variant": log_state["current_variant"],
        "latest_completed": latest_completed,
        "last_log_line": log_state["last_log_line"],
        "running_shards": log_state["shards"],
        "average_variant_seconds": avg_variant_seconds,
        "eta_seconds": eta_seconds,
        "eta_text": seconds_text(eta_seconds),
        "results": summaries,
        "top_intermediate_rows": top_rows,
        "outputs": outputs,
    }
    if progress_json is not None:
        atomic_json(progress_json, progress_payload)
    if progress_md is not None:
        write_progress_markdown(progress_md, progress_payload)
    print(progress_json)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
