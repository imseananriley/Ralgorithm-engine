#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Replay a pre-fork validation corpus through the packed opening solver."
    )
    parser.add_argument("--corpus", required=True)
    parser.add_argument(
        "--current-bin", default="target/release/rhystic-core-smoke"
    )
    parser.add_argument(
        "--prefork-bin",
        default="/tmp/ralgorithm-prefork/target/release/rhystic-core-smoke",
    )
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--depth", type=int, default=14)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--chunk-size", type=int, default=50)
    parser.add_argument("--action-candidates", type=int, default=2)
    return parser.parse_args()


def resolve(path: str) -> Path:
    candidate = Path(path)
    return candidate if candidate.is_absolute() else ROOT / candidate


def invoke(binary: Path, command: str, payload: Any, env: dict[str, str] | None = None) -> Any:
    completed = subprocess.run(
        [str(binary), command],
        cwd=ROOT,
        input=json.dumps(payload, separators=(",", ":")) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{binary} {command} exited {completed.returncode}: {completed.stderr[-2000:]}"
        )
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise RuntimeError(f"expected one JSON response, received {len(lines)}")
    response = json.loads(lines[0])
    if response.get("error"):
        raise RuntimeError(response["error"])
    return response


def replay_game(record: dict[str, Any]) -> dict[str, Any]:
    return {
        "game_index": record["game_index"],
        "hand": record["keep"],
        "bottomed": record["bottomed"],
        "gemstone_caverns_live": record["gemstone_caverns_live"],
        "legacy_hit": record["hit"],
        "legacy_capped": record["capped"],
        "legacy_turn": record["turn"],
        "legacy_engine_label": record["engine_label"],
    }


def run_current_chunks(
    binary: Path,
    deck: list[str],
    games: list[dict[str, Any]],
    budgets: list[int],
    depth: int,
    workers: int,
    chunk_size: int,
    action_candidates: int,
    label: str,
) -> list[dict[str, Any]]:
    outcomes: list[dict[str, Any]] = []
    total = len(games)
    for start in range(0, total, chunk_size):
        chunk = games[start : start + chunk_size]
        response = invoke(
            binary,
            "opening-replay-jsonl",
            {
                "deck": deck,
                "games": chunk,
                "max_turn": 2,
                "depth": depth,
                "discrepancy_budgets": budgets,
                "action_candidate_limit": action_candidates,
                "workers": min(workers, len(chunk)),
            },
        )
        outcomes.extend(response["games"])
        print(f"[{label}] {min(start + len(chunk), total)}/{total}", flush=True)
    outcomes.sort(key=lambda row: row["game_index"])
    return outcomes


def strict_request(record: dict[str, Any]) -> dict[str, Any]:
    return {
        "hand": record["keep"],
        "library": record["library"],
        "gemstone_live": record["gemstone_caverns_live"],
        "state_limit": record.get("actual_cap_rerun_state_limit") or 60_000,
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


def tier_by_budget(outcome: dict[str, Any], budget: int) -> dict[str, Any]:
    return next(
        tier for tier in outcome["tiers"] if tier["discrepancy_budget"] == budget
    )


def positive(tier: dict[str, Any]) -> bool:
    return tier["outcome"]["weighted_ev"] > 1e-15


def top_cards(records: list[dict[str, Any]], limit: int = 30) -> list[dict[str, Any]]:
    counts = Counter(card for record in records for card in record["keep"])
    return [
        {"card": card, "games": games}
        for card, games in counts.most_common(limit)
    ]


def main() -> int:
    args = parse_args()
    corpus = json.loads(resolve(args.corpus).read_text())
    request = corpus["request"]
    records = corpus["response"]["validation_records"]
    if len(records) != request["games"]:
        raise RuntimeError("pre-fork corpus is missing validation records")
    out_dir = resolve(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    current_bin = resolve(args.current_bin)
    prefork_bin = resolve(args.prefork_bin)
    games = [replay_game(record) for record in records]

    current_d01 = run_current_chunks(
        current_bin,
        request["deck"],
        games,
        [0, 1],
        args.depth,
        args.workers,
        args.chunk_size,
        args.action_candidates,
        "current D0/D1",
    )
    by_index = {row["game_index"]: row for row in current_d01}
    legacy_hits = [record for record in records if record["hit"]]
    d1_misses = [
        record
        for record in legacy_hits
        if not positive(tier_by_budget(by_index[record["game_index"]], 1))
    ]
    current_d2 = run_current_chunks(
        current_bin,
        request["deck"],
        [replay_game(record) for record in d1_misses],
        [2],
        args.depth,
        args.workers,
        args.chunk_size,
        args.action_candidates,
        "current D2 legacy misses",
    ) if d1_misses else []
    d2_by_index = {row["game_index"]: row for row in current_d2}

    strict_env = os.environ.copy()
    strict_env["RALGORITHM_STRICT_SHUFFLE_HIDDEN"] = "1"
    strict_rows = invoke(
        prefork_bin,
        "solve-keep-fast-batch-jsonl",
        [strict_request(record) for record in legacy_hits],
        env=strict_env,
    )
    strict_by_index = {
        record["game_index"]: strict
        for record, strict in zip(legacy_hits, strict_rows, strict=True)
    }

    merged: list[dict[str, Any]] = []
    for record in records:
        index = record["game_index"]
        d01 = by_index[index]
        strict = strict_by_index.get(index)
        d2 = d2_by_index.get(index)
        merged.append(
            {
                "game_index": index,
                "legacy": record,
                "strict_prefork": strict,
                "current_d0": tier_by_budget(d01, 0),
                "current_d1": tier_by_budget(d01, 1),
                "current_d2": tier_by_budget(d2, 2) if d2 else None,
            }
        )

    lookahead = [
        record
        for record in legacy_hits
        if strict_by_index[record["game_index"]].get("turn") is None
        and not strict_by_index[record["game_index"]].get("capped")
    ]
    strict_caps = [
        record
        for record in legacy_hits
        if strict_by_index[record["game_index"]].get("turn") is None
        and strict_by_index[record["game_index"]].get("capped")
    ]
    legal_legacy_hits = [
        record
        for record in legacy_hits
        if strict_by_index[record["game_index"]].get("turn") is not None
    ]
    legal_d1_misses = [
        record
        for record in legal_legacy_hits
        if not positive(tier_by_budget(by_index[record["game_index"]], 1))
    ]
    legal_d2_misses = [
        record
        for record in legal_d1_misses
        if not positive(tier_by_budget(d2_by_index[record["game_index"]], 2))
    ]
    current_only_d1 = [
        record
        for record in records
        if not record["hit"] and positive(tier_by_budget(by_index[record["game_index"]], 1))
    ]

    summary = {
        "games": len(records),
        "legacy_hits": len(legacy_hits),
        "legacy_caps": sum(bool(record["capped"]) for record in records),
        "legacy_hits_failing_strict_hidden": len(lookahead),
        "legacy_hits_strict_hidden_caps": len(strict_caps),
        "legal_legacy_hits": len(legal_legacy_hits),
        "legacy_hits_recovered_d0": sum(
            positive(tier_by_budget(by_index[record["game_index"]], 0))
            for record in legacy_hits
        ),
        "legacy_hits_recovered_d1": len(legacy_hits) - len(d1_misses),
        "legal_legacy_hits_missed_d1": len(legal_d1_misses),
        "legal_legacy_hits_missed_d2": len(legal_d2_misses),
        "current_d1_positive_legacy_misses": len(current_only_d1),
        "lookahead_game_indices": [record["game_index"] for record in lookahead],
        "strict_cap_game_indices": [record["game_index"] for record in strict_caps],
        "legal_d1_miss_game_indices": [record["game_index"] for record in legal_d1_misses],
        "legal_d2_miss_game_indices": [record["game_index"] for record in legal_d2_misses],
        "legal_d1_miss_hand_cards": top_cards(legal_d1_misses),
        "legal_d2_miss_hand_cards": top_cards(legal_d2_misses),
        "settings": {
            "seed": request["seed"],
            "depth": args.depth,
            "action_candidates": args.action_candidates,
            "workers": args.workers,
        },
    }
    (out_dir / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )
    (out_dir / "games.json").write_text(
        json.dumps(merged, separators=(",", ":")) + "\n"
    )
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
