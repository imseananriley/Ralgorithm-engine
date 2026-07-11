#!/usr/bin/env python3
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import json
import math
import os
from pathlib import Path
import sys
import time
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from rhystic_study_calc import RustActionAccelerator, default_rust_action_bin  # noqa: E402


def wilson(k: int, n: int, z: float = 1.959963984540054) -> list[float]:
    if n <= 0:
        return [0.0, 0.0]
    p = k / n
    den = 1.0 + z * z / n
    center = (p + z * z / (2 * n)) / den
    margin = z * math.sqrt((p * (1.0 - p) + z * z / (4 * n)) / n) / den
    return [center - margin, center + margin]


def chunks(values: list[dict[str, Any]], size: int) -> list[list[dict[str, Any]]]:
    return [values[index : index + size] for index in range(0, len(values), size)]


def solve_request(record: dict[str, Any], state_limit: int) -> dict[str, Any]:
    return {
        "hand": list(record["keep"]),
        "library": list(record["library"]),
        "gemstone_live": bool(record["gemstone_caverns_live"]),
        "state_limit": state_limit,
        "max_turns": int(record.get("max_turns", 2)),
        "goal": str(record.get("goal", "engine")),
        "engine_target_count": int(record.get("engine_target_count", 1)),
        "engine_success_policy": str(record.get("engine_success_policy", "resilient")),
        "remora_upkeep_payments": int(record.get("remora_upkeep_payments", 2)),
        "action_sort": bool(record.get("action_sort", True)),
        "gamble_mode": record.get("gamble_mode"),
        "gamble_seed": int(record["gamble_seed"]),
        "simplified_gamble": bool(record.get("simplified_gamble", False)),
    }


def replay_chunk(
    chunk_index: int,
    records: list[dict[str, Any]],
    *,
    state_limit: int,
    rust_action_bin: str | None,
) -> dict[str, Any]:
    started = time.time()
    accelerator = RustActionAccelerator(
        rust_action_bin,
        command=os.environ.get("RHYSTIC_RUST_SOLVER_BATCH_COMMAND", "solve-keep-fast-batch-jsonl"),
    )
    try:
        responses = accelerator.solve_keep_batch([solve_request(record, state_limit) for record in records])
    finally:
        accelerator.close()
    replay_records = []
    for record, (turn, capped, label, unsupported, reason) in zip(records, responses, strict=True):
        if unsupported:
            raise RuntimeError(f"cap replay unsupported for game {record.get('game_index')}: {reason}")
        hit = turn is not None and int(turn) <= int(record.get("max_turns", 2))
        replay_records.append(
            {
                "game_index": int(record["game_index"]),
                "stage": int(record["stage"]),
                "bottom_count": int(record["bottom_count"]),
                "gemstone_caverns_live": bool(record["gemstone_caverns_live"]),
                "hit": hit,
                "capped": bool(capped),
                "turn": turn,
                "engine_label": label if hit else None,
            }
        )
    return {
        "chunk_index": chunk_index,
        "records": len(records),
        "elapsed_seconds": time.time() - started,
        "replay_records": replay_records,
    }


def adjusted_summary(source: dict[str, Any], replay_records: list[dict[str, Any]], state_limit: int) -> dict[str, Any]:
    evaluation = source["evaluation"]
    games = int(evaluation["games"])
    original_successes = int(evaluation["successes"])
    original_cap_misses = int(evaluation.get("cap_misses") or 0)
    replay_hits = sum(1 for row in replay_records if row["hit"])
    remaining_caps = sum(1 for row in replay_records if row["capped"] and not row["hit"])
    adjusted_successes = original_successes + replay_hits
    turn_counts = {str(key): int(value) for key, value in (evaluation.get("turn_counts") or {}).items()}
    turn_counts["miss"] = int(turn_counts.get("miss", 0)) - replay_hits
    for row in replay_records:
        if row["hit"]:
            turn_key = str(row["turn"])
            turn_counts[turn_key] = int(turn_counts.get(turn_key, 0)) + 1
    turn_counts = dict(sorted(turn_counts.items(), key=lambda item: (item[0] == "miss", item[0])))
    return {
        "games": games,
        "new_state_limit": state_limit,
        "original_successes": original_successes,
        "original_success_rate": original_successes / games if games else 0.0,
        "original_cap_misses": original_cap_misses,
        "cap_replay_records": len(replay_records),
        "replay_hits": replay_hits,
        "replay_uncapped_misses": sum(1 for row in replay_records if not row["hit"] and not row["capped"]),
        "remaining_cap_misses": remaining_caps,
        "adjusted_successes": adjusted_successes,
        "adjusted_success_rate": adjusted_successes / games if games else 0.0,
        "adjusted_wilson95": wilson(adjusted_successes, games),
        "adjusted_upper_success_rate_if_caps_hit": (adjusted_successes + remaining_caps) / games if games else 0.0,
        "adjusted_turn_counts": turn_counts,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Replay compact Rhystic/Heartwood cap records at a higher state limit.")
    parser.add_argument("--input-json", required=True)
    parser.add_argument("--json-out", default=None)
    parser.add_argument("--new-state-limit", type=int, required=True)
    parser.add_argument("--workers", type=int, default=max(1, os.cpu_count() or 1))
    parser.add_argument("--batch-size", type=int, default=32)
    parser.add_argument("--rust-action-bin", default=str(default_rust_action_bin()))
    parser.add_argument("--include-replay-records", action="store_true")
    parser.add_argument("--suppress-json-stdout", action="store_true")
    args = parser.parse_args()
    if args.new_state_limit <= 0:
        raise ValueError("--new-state-limit must be positive")
    if args.workers <= 0:
        raise ValueError("--workers must be positive")
    if args.batch_size <= 0:
        raise ValueError("--batch-size must be positive")

    source_path = Path(args.input_json)
    source = json.loads(source_path.read_text())
    evaluation = source.get("evaluation") or {}
    records = list(evaluation.get("cap_replay_records") or [])
    if not records:
        raise ValueError(f"{source_path} has no evaluation.cap_replay_records")

    started = time.time()
    chunk_list = chunks(records, args.batch_size)
    workers = min(args.workers, len(chunk_list))
    chunk_results: list[dict[str, Any]] = []
    with ThreadPoolExecutor(max_workers=workers) as executor:
        futures = {
            executor.submit(
                replay_chunk,
                chunk_index,
                chunk,
                state_limit=args.new_state_limit,
                rust_action_bin=args.rust_action_bin,
            ): chunk_index
            for chunk_index, chunk in enumerate(chunk_list)
        }
        for future in as_completed(futures):
            chunk_results.append(future.result())

    replay_records = [
        row
        for chunk in sorted(chunk_results, key=lambda item: int(item["chunk_index"]))
        for row in chunk["replay_records"]
    ]
    replay_records.sort(key=lambda row: int(row["game_index"]))
    summary = adjusted_summary(source, replay_records, args.new_state_limit)
    payload: dict[str, Any] = {
        "input_json": str(source_path),
        "elapsed_seconds": time.time() - started,
        "workers": workers,
        "batch_size": args.batch_size,
        "chunk_results": [
            {
                "chunk_index": int(chunk["chunk_index"]),
                "records": int(chunk["records"]),
                "elapsed_seconds": float(chunk["elapsed_seconds"]),
            }
            for chunk in sorted(chunk_results, key=lambda item: int(item["chunk_index"]))
        ],
        "summary": summary,
    }
    if args.include_replay_records:
        payload["replay_records"] = replay_records
    if args.json_out:
        out_path = Path(args.json_out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    if not args.suppress_json_stdout:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(
            json.dumps(
                {
                    "input_json": str(source_path),
                    "json_out": args.json_out,
                    "elapsed_seconds": payload["elapsed_seconds"],
                    **summary,
                },
                sort_keys=True,
            )
        )


if __name__ == "__main__":
    main()
