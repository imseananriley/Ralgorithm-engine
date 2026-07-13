#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
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
    parser.add_argument(
        "--exact-rescue",
        action="store_true",
        help="run unresolved keeps through the current exact-order fast solver",
    )
    parser.add_argument("--rescue-initial-state-limit", type=int, default=20_000)
    parser.add_argument("--rescue-cap-state-limit", type=int, default=60_000)
    parser.add_argument(
        "--validate-witnesses",
        action="store_true",
        help="count packed hits only when their witness validates against full library order",
    )
    return parser.parse_args()


def resolve(path: str) -> Path:
    candidate = Path(path)
    return candidate if candidate.is_absolute() else ROOT / candidate


def child_cpu_seconds() -> float:
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_utime + usage.ru_stime


def invoke(
    binary: Path,
    command: str,
    payload: Any,
    extra_env: dict[str, str] | None = None,
) -> tuple[Any, float, float]:
    cpu_start = child_cpu_seconds()
    wall_start = time.perf_counter()
    completed = subprocess.run(
        [str(binary), command],
        cwd=ROOT,
        input=json.dumps(payload, separators=(",", ":")) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, **(extra_env or {})},
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
        "library_order": record["library"],
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
    witness_status_by_game: dict[int, str] = {}
    raw_packed_hits: set[int] = set()
    budgets = list(range(args.max_discrepancy + 1))
    response, packed_wall, packed_cpu = invoke(
        current_bin,
        "opening-replay-jsonl",
        {
            "deck": deck,
            "games": [replay_game(record, args.recorded_top) for record in unresolved],
            "max_turn": 2,
            "depth": args.depth,
            "discrepancy_budgets": budgets,
            "action_candidate_limit": args.action_candidates,
            "workers": min(args.workers, len(unresolved)),
            "existence_only": True,
            "validate_witnesses": args.validate_witnesses,
            "stop_after_confirmed_witness": True,
            "include_witness_actions": False,
        },
    )
    current_wall += packed_wall
    current_cpu += packed_cpu
    rows_by_budget = {
        budget: [
            (row, tier)
            for row in response["games"]
            for tier in row["tiers"]
            if tier["discrepancy_budget"] == budget
        ]
        for budget in budgets
    }
    last_budget = max((budget for budget, rows in rows_by_budget.items() if rows), default=0)
    for budget in budgets:
        rows = rows_by_budget[budget]
        if not rows:
            break
        found = set()
        for row, tier in rows:
            if not tier["found"]:
                continue
            raw_packed_hits.add(row["game_index"])
            validation = tier.get("witness_validation")
            status = validation["status"] if validation else "unavailable"
            previous_status = witness_status_by_game.get(row["game_index"])
            if previous_status != "confirmed" or status == "confirmed":
                witness_status_by_game[row["game_index"]] = status
            if not args.validate_witnesses or status == "confirmed":
                found.add(row["game_index"])
        found.difference_update(current_hits)
        current_hits.update(found)
        unresolved = [record for record in unresolved if record["game_index"] not in found]
        recalled_old = len(current_hits & old_hits)
        tiers.append(
            {
                "budget": budget,
                "evaluated": len(rows),
                "new_hits": len(found),
                "cumulative_hits": len(current_hits),
                "old_hits_recalled": recalled_old,
                "old_hits_missed": len(old_hits) - recalled_old,
                "wall_seconds": packed_wall if budget == last_budget else 0.0,
                "cpu_seconds": packed_cpu if budget == last_budget else 0.0,
                "timing_scope": "shared_all_tiers",
            }
        )
        print(
            f"[D={budget}] evaluated={len(rows)} new={len(found)} "
            f"old_recall={recalled_old}/{len(old_hits)}",
            flush=True,
        )
    print(f"[packed shared] wall={packed_wall:.3f}s cpu={packed_cpu:.3f}s", flush=True)

    packed_hits = set(current_hits)
    witness_statuses: dict[str, int] = {}
    for status in witness_status_by_game.values():
        witness_statuses[status] = witness_statuses.get(status, 0) + 1
    rescue = {
        "enabled": args.exact_rescue,
        "evaluated": 0,
        "initial_caps": 0,
        "final_caps": 0,
        "final_cap_game_indices": [],
        "unsupported": 0,
        "hits": 0,
        "hit_game_indices": [],
        "wall_seconds": 0.0,
        "cpu_seconds": 0.0,
    }
    if args.exact_rescue and unresolved:
        rescue_env = (
            {"RHYSTIC_STRICT_SHUFFLE_HIDDEN": "1"}
            if args.validate_witnesses
            else None
        )
        rescue["evaluated"] = len(unresolved)
        exact_initial, wall, cpu = invoke(
            current_bin,
            "solve-keep-fast-batch-jsonl",
            [
                old_request(record, args.rescue_initial_state_limit)
                for record in unresolved
            ],
            {**(rescue_env or {}), "RALGORITHM_BATCH_WORKERS": str(args.workers)},
        )
        rescue["wall_seconds"] += wall
        rescue["cpu_seconds"] += cpu
        exact_final = list(exact_initial)
        capped_indices = [
            index
            for index, outcome in enumerate(exact_initial)
            if outcome.get("turn") is None and outcome.get("capped")
        ]
        rescue["initial_caps"] = len(capped_indices)
        if capped_indices:
            reruns, wall, cpu = invoke(
                current_bin,
                "solve-keep-fast-batch-jsonl",
                [
                    old_request(unresolved[index], args.rescue_cap_state_limit)
                    for index in capped_indices
                ],
                {**(rescue_env or {}), "RALGORITHM_BATCH_WORKERS": str(args.workers)},
            )
            rescue["wall_seconds"] += wall
            rescue["cpu_seconds"] += cpu
            for index, outcome in zip(capped_indices, reruns, strict=True):
                exact_final[index] = outcome
        rescue_hits = {
            unresolved[index]["game_index"]
            for index, outcome in enumerate(exact_final)
            if outcome.get("turn") is not None
        }
        rescue["final_caps"] = sum(
            outcome.get("turn") is None and outcome.get("capped")
            for outcome in exact_final
        )
        rescue["final_cap_game_indices"] = [
            unresolved[index]["game_index"]
            for index, outcome in enumerate(exact_final)
            if outcome.get("turn") is None and outcome.get("capped")
        ]
        rescue["unsupported"] = sum(
            outcome.get("unsupported", False) for outcome in exact_final
        )
        current_hits.update(rescue_hits)
        rescue["hits"] = len(rescue_hits)
        rescue["hit_game_indices"] = sorted(rescue_hits)
        current_wall += rescue["wall_seconds"]
        current_cpu += rescue["cpu_seconds"]
        print(
            f"[exact rescue] evaluated={len(unresolved)} hits={len(rescue_hits)} "
            f"caps={len(capped_indices)} wall={rescue['wall_seconds']:.3f}s",
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
            "hit_game_indices": sorted(current_hits),
            "packed_hits": len(packed_hits),
            "raw_packed_hits": len(raw_packed_hits),
            "witness_statuses": witness_statuses,
            "witness_status_game_indices": {
                status: sorted(
                    game_index
                    for game_index, game_status in witness_status_by_game.items()
                    if game_status == status
                )
                for status in sorted(witness_statuses)
            },
            "rescue": rescue,
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
