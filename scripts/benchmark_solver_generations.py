#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import platform
import random
import statistics
import subprocess
import time
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Compare paired solver outcomes and speed between two Rust engine generations."
    )
    parser.add_argument("--baseline-bin", required=True)
    parser.add_argument(
        "--candidate-bin", default="target/release/rhystic-core-smoke"
    )
    parser.add_argument(
        "--deck-json", default="fixtures/decks/champion_working_list.json"
    )
    parser.add_argument("--hands", type=int, default=100)
    parser.add_argument("--policy-games", type=int, default=50)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--cooldown-seconds", type=float, default=0.0)
    parser.add_argument("--seed", type=int, default=2026071101)
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument("--out", default="benchmarks/results/solver_generation.json")
    return parser.parse_args()


def resolve(path: str) -> Path:
    candidate = Path(path)
    return candidate if candidate.is_absolute() else ROOT / candidate


def load_deck(path: Path) -> list[str]:
    payload = json.loads(path.read_text())
    deck = payload.get("deck")
    if not isinstance(deck, list) or not all(isinstance(card, str) for card in deck):
        raise ValueError(f"{path} must contain a string deck array")
    return deck


def source_manifest() -> dict[str, Any]:
    path = ROOT / "benchmarks" / "source_manifest.json"
    subprocess.run(
        ["python3", str(ROOT / "scripts" / "source_manifest.py"), "--out", str(path)],
        cwd=ROOT,
        stdout=subprocess.DEVNULL,
        check=True,
    )
    return json.loads(path.read_text())


def solve_request(
    hand: list[str], library: list[str], gemstone_live: bool, state_limit: int, seed: int
) -> dict[str, Any]:
    return {
        "hand": hand,
        "library": library,
        "gemstone_live": gemstone_live,
        "state_limit": state_limit,
        "max_turns": 2,
        "goal": "engine",
        "engine_target_count": 1,
        "engine_success_policy": "resilient",
        "remora_upkeep_payments": 2,
        "action_sort": True,
        "gamble_mode": "stochastic",
        "gamble_seed": seed,
        "simplified_gamble": True,
    }


def fixed_hand_requests(
    deck: list[str], hands: int, seed: int, state_limit: int
) -> list[dict[str, Any]]:
    rng = random.Random(seed)
    requests = []
    for index in range(hands):
        order = deck.copy()
        rng.shuffle(order)
        requests.append(
            solve_request(
                sorted(order[:7]),
                order[7:],
                rng.random() < 0.75,
                state_limit,
                seed + index,
            )
        )
    return requests


def policy_request(
    deck: list[str], games: int, seed: int, state_limit: int, rerun_limit: int
) -> dict[str, Any]:
    # Fixed thresholds make this a paired evaluator benchmark, not a threshold-training benchmark.
    thresholds = [0.75, 0.65, 0.50, 0.35, 0.15, 0.0]
    return {
        "deck": deck,
        "thresholds_dead": thresholds,
        "thresholds_live": thresholds,
        "games": games,
        "seed": seed,
        "gemstone_caverns_live_rate": 0.75,
        "state_limit": state_limit,
        "actual_rerun_state_limit": rerun_limit,
        "samples_per_bottom": 1,
        "validation_samples": 2,
        "cap_weight": 0.5,
        "max_turns": 2,
        "goal": "engine",
        "engine_target_count": 1,
        "engine_success_policy": "resilient",
        "remora_upkeep_payments": 2,
        "action_sort": True,
        "adaptive_threshold_sampling": True,
        "include_game_records": True,
        "include_cap_replay_records": False,
        "include_validation_records": False,
        "trace_lines": False,
        "gamble_mode": "stochastic",
        "simplified_gamble": True,
        "internal_shards": 1,
        "internal_shard_workers": 1,
        "weighted_policy_ev": True,
        "rhystic_t1_weight": 1.0,
        "rhystic_t2_weight": 0.75,
        "heartwood_t1_weight": 0.70,
        "heartwood_t2_weight": 0.55,
    }


def invoke(binary: Path, command: str, payload: Any) -> tuple[float, Any]:
    started = time.perf_counter()
    completed = subprocess.run(
        [str(binary), command],
        cwd=ROOT,
        input=json.dumps(payload, separators=(",", ":")) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    elapsed = time.perf_counter() - started
    if completed.returncode != 0:
        raise RuntimeError(
            f"{binary} {command} exited {completed.returncode}: {completed.stderr[-2000:]}"
        )
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise RuntimeError(f"expected one JSON response, received {len(lines)}")
    return elapsed, json.loads(lines[0])


def timed_pair(
    baseline_bin: Path,
    candidate_bin: Path,
    command: str,
    payload: Any,
    repeats: int,
    cooldown_seconds: float,
) -> tuple[list[float], Any, list[float], Any]:
    _, baseline_expected = invoke(baseline_bin, command, payload)
    _, candidate_expected = invoke(candidate_bin, command, payload)
    baseline_elapsed = []
    candidate_elapsed = []
    for repeat in range(repeats):
        order = (
            [(baseline_bin, baseline_elapsed, baseline_expected),
             (candidate_bin, candidate_elapsed, candidate_expected)]
            if repeat % 2 == 0
            else [(candidate_bin, candidate_elapsed, candidate_expected),
                  (baseline_bin, baseline_elapsed, baseline_expected)]
        )
        for binary, elapsed, expected in order:
            if cooldown_seconds > 0.0:
                time.sleep(cooldown_seconds)
            seconds, response = invoke(binary, command, payload)
            if response != expected:
                raise RuntimeError(f"non-deterministic response from {binary} {command}")
            elapsed.append(seconds)
    return baseline_elapsed, baseline_expected, candidate_elapsed, candidate_expected


def outcome_key(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        row.get("turn"),
        bool(row.get("capped")),
        row.get("label"),
        bool(row.get("unsupported")),
        row.get("unsupported_reason"),
    )


def compare_fixed(
    baseline: list[dict[str, Any]], candidate: list[dict[str, Any]]
) -> dict[str, Any]:
    if len(baseline) != len(candidate):
        raise RuntimeError("fixed-hand response lengths differ")
    for name, rows in [("baseline", baseline), ("candidate", candidate)]:
        unsupported = [index for index, row in enumerate(rows) if row.get("unsupported")]
        if unsupported:
            raise RuntimeError(f"{name} fixed-hand responses unsupported at {unsupported[:20]}")
    discordant = [
        index
        for index, (left, right) in enumerate(zip(baseline, candidate, strict=True))
        if outcome_key(left) != outcome_key(right)
    ]
    baseline_hits = sum(row.get("turn") is not None for row in baseline)
    candidate_hits = sum(row.get("turn") is not None for row in candidate)
    return {
        "games": len(baseline),
        "baseline_successes": baseline_hits,
        "candidate_successes": candidate_hits,
        "baseline_success_rate": baseline_hits / len(baseline) if baseline else 0.0,
        "candidate_success_rate": candidate_hits / len(candidate) if candidate else 0.0,
        "baseline_wilson95": wilson95(baseline_hits, len(baseline)),
        "candidate_wilson95": wilson95(candidate_hits, len(candidate)),
        "exact_outcomes": len(baseline) - len(discordant),
        "discordant_outcomes": len(discordant),
        "discordant_indices": discordant[:100],
        "baseline_caps": sum(bool(row.get("capped")) for row in baseline),
        "candidate_caps": sum(bool(row.get("capped")) for row in candidate),
    }


def timing_summary(
    baseline: list[float], candidate: list[float], units: int
) -> dict[str, Any]:
    baseline_median = statistics.median(baseline)
    candidate_median = statistics.median(candidate)
    paired_speedups = [
        baseline_seconds / candidate_seconds
        for baseline_seconds, candidate_seconds in zip(
            baseline, candidate, strict=True
        )
    ]
    return {
        "baseline_seconds": baseline,
        "candidate_seconds": candidate,
        "baseline_median_seconds": baseline_median,
        "candidate_median_seconds": candidate_median,
        "baseline_units_per_second": units / baseline_median,
        "candidate_units_per_second": units / candidate_median,
        "candidate_speedup": baseline_median / candidate_median,
        "paired_speedups": paired_speedups,
        "paired_speedup_median": statistics.median(paired_speedups),
    }


def wilson95(successes: int, games: int) -> list[float]:
    if games == 0:
        return [0.0, 0.0]
    z = 1.959963984540054
    rate = successes / games
    denominator = 1.0 + z * z / games
    center = (rate + z * z / (2.0 * games)) / denominator
    margin = (
        z
        * math.sqrt(rate * (1.0 - rate) / games + z * z / (4.0 * games * games))
        / denominator
    )
    return [center - margin, center + margin]


def policy_game_outcomes(response: dict[str, Any]) -> list[tuple[Any, ...]]:
    records = response.get("game_records") or []
    outcomes = [
        (
            record.get("game_index"),
            bool(record.get("hit")),
            bool(record.get("capped")),
            record.get("turn"),
            record.get("engine_label"),
            record.get("stage"),
            record.get("bottom_count"),
        )
        for record in records
    ]
    outcomes.sort(key=lambda row: row[0])
    return outcomes


def main() -> int:
    args = parse_args()
    if args.hands <= 0 or args.policy_games <= 0 or args.repeats <= 0:
        raise ValueError("--hands, --policy-games, and --repeats must be positive")
    if args.cooldown_seconds < 0.0:
        raise ValueError("--cooldown-seconds must be non-negative")
    baseline_bin = resolve(args.baseline_bin)
    candidate_bin = resolve(args.candidate_bin)
    for binary in [baseline_bin, candidate_bin]:
        if not binary.is_file():
            raise FileNotFoundError(binary)
    deck = load_deck(resolve(args.deck_json))
    requests = fixed_hand_requests(deck, args.hands, args.seed, args.state_limit)
    fixed_payload = requests
    (
        baseline_fixed_time,
        baseline_fixed,
        candidate_fixed_time,
        candidate_fixed,
    ) = timed_pair(
        baseline_bin,
        candidate_bin,
        "solve-keep-fast-batch-jsonl",
        fixed_payload,
        args.repeats,
        args.cooldown_seconds,
    )

    policy_payload = policy_request(
        deck,
        args.policy_games,
        args.seed + 1_000_000,
        args.state_limit,
        args.actual_rerun_state_limit,
    )
    (
        baseline_policy_time,
        baseline_policy,
        candidate_policy_time,
        candidate_policy,
    ) = timed_pair(
        baseline_bin,
        candidate_bin,
        "policy-eval-fast-jsonl",
        policy_payload,
        args.repeats,
        args.cooldown_seconds,
    )
    for name, response in [
        ("baseline", baseline_policy),
        ("candidate", candidate_policy),
    ]:
        if response.get("unsupported"):
            raise RuntimeError(
                f"{name} policy evaluation unsupported: {response.get('unsupported_reason')}"
            )
    baseline_policy_outcomes = policy_game_outcomes(baseline_policy)
    candidate_policy_outcomes = policy_game_outcomes(candidate_policy)
    if len(baseline_policy_outcomes) != args.policy_games:
        raise RuntimeError(
            f"baseline returned {len(baseline_policy_outcomes)} policy records; "
            f"expected {args.policy_games}"
        )
    if len(candidate_policy_outcomes) != args.policy_games:
        raise RuntimeError(
            f"candidate returned {len(candidate_policy_outcomes)} policy records; "
            f"expected {args.policy_games}"
        )
    policy_discordance = [
        index
        for index, (left, right) in enumerate(
            zip(baseline_policy_outcomes, candidate_policy_outcomes, strict=True)
        )
        if left != right
    ]

    manifest = source_manifest()
    result = {
        "schema": 1,
        "candidate_source": {
            "source_digest": manifest["source_digest"],
            "git_commit": manifest.get("git_commit"),
            "git_status_porcelain": manifest.get("git_status_porcelain"),
        },
        "machine": {
            "platform": platform.platform(),
            "machine": platform.machine(),
        },
        "settings": vars(args),
        "fixed_hands": {
            "outcomes": compare_fixed(baseline_fixed, candidate_fixed),
            "timing": timing_summary(
                baseline_fixed_time, candidate_fixed_time, args.hands
            ),
        },
        "full_policy": {
            "baseline_successes": baseline_policy.get("successes"),
            "candidate_successes": candidate_policy.get("successes"),
            "baseline_success_rate": baseline_policy.get("success_rate"),
            "candidate_success_rate": candidate_policy.get("success_rate"),
            "baseline_wilson95": wilson95(
                int(baseline_policy.get("successes") or 0), args.policy_games
            ),
            "candidate_wilson95": wilson95(
                int(candidate_policy.get("successes") or 0), args.policy_games
            ),
            "baseline_caps": baseline_policy.get("cap_misses"),
            "candidate_caps": candidate_policy.get("cap_misses"),
            "exact_game_outcomes": len(baseline_policy_outcomes)
            - len(policy_discordance),
            "discordant_game_outcomes": len(policy_discordance),
            "discordant_indices": policy_discordance[:100],
            "timing": timing_summary(
                baseline_policy_time, candidate_policy_time, args.policy_games
            ),
        },
    }
    out = resolve(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(out)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
