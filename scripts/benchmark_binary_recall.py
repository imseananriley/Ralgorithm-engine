#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import resource
import subprocess
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Benchmark pre-fork and adaptive packed binary recall on aligned keeps."
    )
    parser.add_argument("--corpus", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument(
        "--prefork-bin",
        default="/tmp/ralgorithm-prefork/target/release/rhystic-core-smoke",
    )
    parser.add_argument(
        "--current-bin", default="target/release/rhystic-core-smoke"
    )
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--depth", type=int, default=14)
    parser.add_argument("--action-candidates", type=int, default=2)
    parser.add_argument("--max-discrepancy", type=int, default=14)
    parser.add_argument("--recorded-top", type=int, default=2)
    parser.add_argument("--reuse-prefork-result")
    return parser.parse_args()


def resolve(path: str) -> Path:
    candidate = Path(path)
    return candidate if candidate.is_absolute() else ROOT / candidate


def child_cpu_seconds() -> float:
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_utime + usage.ru_stime


def invoke(binary: Path, command: str, payload: Any) -> tuple[Any, float, float]:
    cpu_start = child_cpu_seconds()
    wall_start = time.perf_counter()
    completed = subprocess.run(
        [str(binary), command],
        cwd=ROOT,
        input=json.dumps(payload, separators=(",", ":")) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    wall = time.perf_counter() - wall_start
    cpu = child_cpu_seconds() - cpu_start
    if completed.returncode != 0:
        raise RuntimeError(
            f"{binary} {command} exited {completed.returncode}: {completed.stderr[-2000:]}"
        )
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise RuntimeError(f"expected one JSON response, received {len(lines)}")
    response = json.loads(lines[0])
    if isinstance(response, dict) and response.get("error"):
        raise RuntimeError(response["error"])
    return response, wall, cpu


def old_request(record: dict[str, Any], state_limit: int) -> dict[str, Any]:
    return {
        "hand": record["keep"],
        "library": record["library"],
        "gemstone_live": record["gemstone_caverns_live"],
        "state_limit": state_limit,
        "max_turns": record.get("max_turns") or 2,
        "goal": record.get("goal") or "engine",
        "engine_target_count": record.get("engine_target_count") or 1,
        "engine_success_policy": record.get("engine_success_policy") or "resilient",
        "remora_upkeep_payments": record.get("remora_upkeep_payments") or 2,
        "action_sort": record.get("action_sort", True),
        "gamble_mode": record.get("gamble_mode") or "stochastic",
        "gamble_seed": record.get("gamble_seed"),
        "simplified_gamble": record.get("simplified_gamble", True),
    }


def replay_game(record: dict[str, Any], recorded_top: int) -> dict[str, Any]:
    return {
        "game_index": record["game_index"],
        "hand": record["keep"],
        "bottomed": record["bottomed"],
        "library_top": record["library"][:recorded_top],
        "gemstone_caverns_live": record["gemstone_caverns_live"],
        "legacy_hit": record["hit"],
        "legacy_capped": record["capped"],
        "legacy_turn": record["turn"],
        "legacy_engine_label": record["engine_label"],
    }


def main() -> int:
    args = parse_args()
    if not 0 <= args.recorded_top <= 3:
        raise SystemExit(
            "--recorded-top must be between 0 and 3; the fourth packed slot is "
            "reserved for top-deck tutor effects"
        )
    corpus = json.loads(resolve(args.corpus).read_text())
    records = corpus["response"]["validation_records"]
    deck = corpus["request"]["deck"]
    prefork_bin = resolve(args.prefork_bin)
    current_bin = resolve(args.current_bin)

    if args.reuse_prefork_result:
        cached = json.loads(resolve(args.reuse_prefork_result).read_text())
        old_hits = {
            record["game_index"] for record in records if record.get("hit")
        }
        old_wall = cached["prefork"]["wall_seconds"]
        old_cpu = cached["prefork"]["cpu_seconds"]
        rerun_indices = [None] * cached["prefork"]["initial_caps"]
    else:
        old_initial, old_wall, old_cpu = invoke(
            prefork_bin,
            "solve-keep-fast-batch-jsonl",
            [old_request(record, 20_000) for record in records],
        )
        rerun_indices = [
            index
            for index, outcome in enumerate(old_initial)
            if outcome.get("turn") is None and outcome.get("capped")
        ]
        old_final = list(old_initial)
        if rerun_indices:
            reruns, rerun_wall, rerun_cpu = invoke(
                prefork_bin,
                "solve-keep-fast-batch-jsonl",
                [old_request(records[index], 60_000) for index in rerun_indices],
            )
            old_wall += rerun_wall
            old_cpu += rerun_cpu
            for index, outcome in zip(rerun_indices, reruns, strict=True):
                old_final[index] = outcome
        old_hits = {
            records[index]["game_index"]
            for index, outcome in enumerate(old_final)
            if outcome.get("turn") is not None
        }

    unresolved = list(records)
    current_hits: set[int] = set()
    tiers = []
    current_wall = 0.0
    current_cpu = 0.0
    for budget in range(args.max_discrepancy + 1):
        if not unresolved:
            break
        response, wall, cpu = invoke(
            current_bin,
            "opening-replay-jsonl",
            {
                "deck": deck,
                "games": [replay_game(record, args.recorded_top) for record in unresolved],
                "max_turn": 2,
                "depth": args.depth,
                "discrepancy_budgets": [budget],
                "action_candidate_limit": args.action_candidates,
                "workers": min(args.workers, len(unresolved)),
                "existence_only": True,
            },
        )
        current_wall += wall
        current_cpu += cpu
        found = {
            row["game_index"] for row in response["games"] if row["tiers"][0]["found"]
        }
        current_hits.update(found)
        unresolved = [record for record in unresolved if record["game_index"] not in found]
        recalled_old = len(current_hits & old_hits)
        tiers.append(
            {
                "budget": budget,
                "evaluated": len(response["games"]),
                "new_hits": len(found),
                "cumulative_hits": len(current_hits),
                "old_hits_recalled": recalled_old,
                "old_hits_missed": len(old_hits) - recalled_old,
                "wall_seconds": wall,
                "cpu_seconds": cpu,
            }
        )
        print(
            f"[D={budget}] evaluated={len(response['games'])} new={len(found)} "
            f"old_recall={recalled_old}/{len(old_hits)} wall={wall:.3f}s",
            flush=True,
        )

    result = {
        "games": len(records),
        "seed": corpus["request"]["seed"],
        "settings": {
            "depth": args.depth,
            "action_candidates": args.action_candidates,
            "recorded_top": args.recorded_top,
            "workers": args.workers,
            "max_discrepancy": args.max_discrepancy,
        },
        "prefork": {
            "hits": len(old_hits),
            "hit_game_indices": sorted(old_hits),
            "initial_caps": len(rerun_indices),
            "wall_seconds": old_wall,
            "cpu_seconds": old_cpu,
            "games_per_wall_second": len(records) / old_wall,
        },
        "current": {
            "hits": len(current_hits),
            "old_hits_recalled": len(current_hits & old_hits),
            "old_hits_missed": sorted(old_hits - current_hits),
            "new_hits_over_prefork": len(current_hits - old_hits),
            "wall_seconds": current_wall,
            "cpu_seconds": current_cpu,
            "games_per_wall_second": len(records) / current_wall,
            "tiers": tiers,
        },
        "ratios": {
            "current_over_prefork_wall": current_wall / old_wall,
            "current_over_prefork_cpu": current_cpu / old_cpu,
            "current_speedup_wall": old_wall / current_wall,
            "current_speedup_cpu": old_cpu / current_cpu,
        },
    }
    out = resolve(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
