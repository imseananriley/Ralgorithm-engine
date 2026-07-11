#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter, deque
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor, as_completed
from dataclasses import dataclass
import hashlib
import importlib.util
import json
import math
import multiprocessing
import os
import cProfile
import random
import sys
import time
from functools import lru_cache
from itertools import combinations
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SOLVER_PATH = ROOT / "scripts" / "rhystic_study_calc.py"
COMMANDER_MULLIGAN_BOTTOMS = (0, 0, 1, 2, 3, 4)
GEMSTONE_CAVERNS_NAMES = {"Gemstone Caverns", "Glittering Caves of Aglarond"}
CARD_NAME_ALIASES = {
    "Zidane Tribal": "Ragavan, Nimble Pilferer",
}

RH: Any | None = None
SEARCH: Any | None = None
DECK: tuple[str, ...] = ()
TARGET_MODE = "rhystic"
STATE_LIMIT = 20_000
SAMPLES_PER_BOTTOM = 4
VALIDATION_SAMPLES = 4
CAP_WEIGHT = 0.0
TRACE_LINES = False
ENGINE_SUCCESS_POLICY = "count"
REMORA_UPKEEP_PAYMENTS = 2
DECK_KEY = "fury"
DECK_JSON: str | None = None
GAMBLE_MODE = "off"
ACTUAL_RERUN_STATE_LIMIT = 0
COUNTERFACTUAL_LINE_CARDS = False
COUNTERFACTUAL_STATE_LIMIT = 0
WORKER_PROFILE_DIR: str | None = None
WORKER_PROFILE_CHUNKS_PER_PROCESS = 0
WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND: dict[str, int] = {}
ACTION_SORT = True
HEURISTIC_BOTTOM_ORDER = False


def has_gemstone_caverns_alias(cards: tuple[str, ...] | list[str]) -> bool:
    return any(card in GEMSTONE_CAVERNS_NAMES for card in cards)


def canonical_card_name(card: str) -> str:
    if card.startswith("Theoretical Birds of Paradise "):
        return "Birds of Paradise"
    return CARD_NAME_ALIASES.get(card, card)


@dataclass(frozen=True)
class HandTask:
    key: str
    hand: tuple[str, ...]
    bottom_count: int
    seed: int
    gemstone_live: bool = False
    keep_threshold: float | None = None
    force_keep: bool = False
    adaptive_threshold_sampling: bool = False


@dataclass(frozen=True)
class ActualTask:
    game_index: int
    stage: int
    bottom_count: int
    keep: tuple[str, ...]
    library: tuple[str, ...]
    gemstone_live: bool = False
    seed: int = 0


def load_solver_module():
    spec = importlib.util.spec_from_file_location("rhystic_study_calc", SOLVER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Could not load {SOLVER_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def read_moxfield_deck(path: str | Path) -> tuple[str, list[str], list[str]]:
    raw = Path(path).read_text()
    payload = json.loads(raw[raw.find("{") :])
    name = (payload.get("name") or Path(path).stem).strip()
    commanders: list[str] = []
    mainboard: list[str] = []
    boards = payload.get("boards") or {}
    for entry in (boards.get("commanders") or {}).get("cards", {}).values():
        card = canonical_card_name(entry["card"]["name"])
        commanders.extend([card] * int(entry.get("quantity", 1)))
    for entry in (boards.get("mainboard") or {}).get("cards", {}).values():
        card = canonical_card_name(entry["card"]["name"])
        mainboard.extend([card] * int(entry.get("quantity", 1)))
    if not commanders:
        raise ValueError(f"No commanders found in {path}")
    if not mainboard:
        raise ValueError(f"No mainboard found in {path}")
    return name, commanders, mainboard


def register_deck(module: Any, deck_json: str | None) -> str:
    if not deck_json:
        return "fury"
    name, commanders, mainboard = read_moxfield_deck(deck_json)
    deck_key = "moxfield"
    module.DECKS[deck_key] = {"commanders": commanders, "mainboard": mainboard}
    return deck_key


def deck_identity(deck_json: str | None, deck: tuple[str, ...], commanders: list[str]) -> str:
    if deck_json:
        path = Path(deck_json)
        if path.exists():
            return hashlib.blake2b(path.read_bytes(), digest_size=12).hexdigest()
    payload = json.dumps({"commanders": commanders, "mainboard": list(deck)}, sort_keys=True).encode()
    return hashlib.blake2b(payload, digest_size=12).hexdigest()


def threshold_cache_key(
    *,
    deck_identity_value: str,
    target: str,
    threshold_hands: int,
    samples_per_bottom: int,
    validation_samples: int,
    state_limit: int,
    cap_weight: float,
    seed: int,
    gemstone_live_flags: tuple[bool, ...],
    normalize_no_caverns_gemstone_key: bool,
    engine_success_policy: str,
    remora_upkeep_payments: int,
    gamble_mode: str,
    action_sort: bool,
    weighted_policy_ev: bool,
    rhystic_t1_weight: float,
    rhystic_t2_weight: float,
    heartwood_t1_weight: float,
    heartwood_t2_weight: float,
) -> str:
    payload = {
        "deck": deck_identity_value,
        "target": target,
        "threshold_hands": threshold_hands,
        "samples_per_bottom": samples_per_bottom,
        "validation_samples": validation_samples,
        "state_limit": state_limit,
        "cap_weight": cap_weight,
        "seed": seed,
        "gemstone_live_flags": list(gemstone_live_flags),
        "normalize_no_caverns_gemstone_key": normalize_no_caverns_gemstone_key,
        "engine_success_policy": engine_success_policy,
        "remora_upkeep_payments": remora_upkeep_payments,
        "gamble_mode": gamble_mode,
        "weighted_policy_ev": weighted_policy_ev,
        "rhystic_t1_weight": rhystic_t1_weight,
        "rhystic_t2_weight": rhystic_t2_weight,
        "heartwood_t1_weight": heartwood_t1_weight,
        "heartwood_t2_weight": heartwood_t2_weight,
        "mulligan_bottoms": list(COMMANDER_MULLIGAN_BOTTOMS),
    }
    if not action_sort:
        payload["action_sort"] = False
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.blake2b(encoded, digest_size=12).hexdigest()


def load_threshold_payload(
    threshold_payload: dict[str, Any],
    *,
    gemstone_caverns_live_rate: float,
    source: str,
) -> tuple[dict[str, list[float]], dict[str, Any]]:
    if "thresholds_by_gemstone_caverns_live" in threshold_payload:
        thresholds_by_gemstone_key = {
            key: [float(value) for value in values]
            for key, values in threshold_payload["thresholds_by_gemstone_caverns_live"].items()
        }
        threshold_rows_by_gemstone_key = threshold_payload.get("threshold_rows_by_gemstone_caverns_live", {})
    elif gemstone_caverns_live_rate > 0:
        raise ValueError(f"{source} does not contain gemstone-live/dead thresholds")
    else:
        thresholds_by_gemstone_key = {"dead": [float(value) for value in threshold_payload["thresholds"]]}
        threshold_rows_by_gemstone_key = {"dead": threshold_payload.get("threshold_rows", [])}
    for key, thresholds in thresholds_by_gemstone_key.items():
        if len(thresholds) != len(COMMANDER_MULLIGAN_BOTTOMS):
            raise ValueError(f"Expected {len(COMMANDER_MULLIGAN_BOTTOMS)} thresholds for {key} in {source}")
    return thresholds_by_gemstone_key, threshold_rows_by_gemstone_key


def apply_threshold_offset(
    thresholds_by_gemstone_key: dict[str, list[float]],
    offset: float,
) -> dict[str, list[float]]:
    if offset == 0:
        return {key: list(values) for key, values in thresholds_by_gemstone_key.items()}
    adjusted: dict[str, list[float]] = {}
    for key, values in thresholds_by_gemstone_key.items():
        next_values = [min(1.0, max(0.0, float(value) + offset)) for value in values]
        if next_values:
            next_values[-1] = 0.0
        adjusted[key] = next_values
    return adjusted


def configure_target(module: Any, target_mode: str) -> None:
    if target_mode == "rhystic":
        return
    if target_mode == "engine3":
        wanted = {module.TARGET, "Mystic Remora", "Heartwood Storyteller"}
        module.ENGINE_NATIVE = {name: value for name, value in module.ENGINE_NATIVE.items() if name in wanted}
        module.ENGINE_NATIVE_CREATURES = {"Heartwood Storyteller"}
        module.ENCHANTMENT_TUTOR_TARGETS = {module.TARGET, "Mystic Remora"}
        module.ENLIGHTENED_TARGETS = {module.TARGET, "Mystic Remora"}
        return
    if target_mode == "heartwood":
        module.ENGINE_NATIVE = {
            "Heartwood Storyteller": module.ENGINE_NATIVE["Heartwood Storyteller"],
        }
        module.ENGINE_NATIVE_CREATURES = {"Heartwood Storyteller"}
        module.ENGINE_COPY_SPELLS = {}
        module.ENCHANTMENT_TUTOR_TARGETS = set()
        module.ENLIGHTENED_TARGETS = set()
        return
    if target_mode == "rhystic_heartwood":
        module.ENGINE_NATIVE = {
            module.TARGET: module.ENGINE_NATIVE[module.TARGET],
            "Heartwood Storyteller": module.ENGINE_NATIVE["Heartwood Storyteller"],
        }
        module.ENGINE_NATIVE_CREATURES = {"Heartwood Storyteller"}
        module.ENGINE_COPY_SPELLS = {}
        module.ENCHANTMENT_TUTOR_TARGETS = {module.TARGET}
        module.ENLIGHTENED_TARGETS = {module.TARGET}
        return
    if target_mode == "rhystic_tithe":
        module.ENGINE_NATIVE = {
            module.TARGET: module.ENGINE_NATIVE[module.TARGET],
            "Smothering Tithe": ((3, 0, 0, 0, 1, 0), "ENCH", module.Perm("ENGINE_ENCH")),
        }
        module.ENGINE_NATIVE_CREATURES = set()
        module.ENGINE_COPY_SPELLS = {}
        module.ENCHANTMENT_TUTOR_TARGETS = {module.TARGET, "Smothering Tithe"}
        module.ENLIGHTENED_TARGETS = {module.TARGET, "Smothering Tithe"}
        return
    if target_mode == "engine4":
        return
    raise ValueError(f"Unknown target mode: {target_mode}")


def target_assumption_note(target_mode: str) -> str:
    if target_mode == "rhystic":
        return "Resilient engine policy treats Rhystic Study as live through turn 2."
    if target_mode == "heartwood":
        return "Resilient engine policy treats Heartwood Storyteller as live through turn 2."
    if target_mode == "rhystic_heartwood":
        return "Resilient engine policy treats Rhystic Study or Heartwood Storyteller as live through turn 2."
    if target_mode == "rhystic_tithe":
        return "Resilient engine policy treats Rhystic Study or Smothering Tithe as live through turn 2."
    return (
        "Resilient engine policy treats Rhystic Study or Heartwood Storyteller as live through turn 2, "
        "and Mystic Remora as live only on turn 1 if its next configured cumulative-upkeep payments can "
        "be made without unknown future draws."
    )


def simplified_gamble_enabled() -> bool:
    return os.environ.get("RHYSTIC_SIMPLIFIED_GAMBLE", "").lower() in {"1", "true", "yes", "on"}


def write_json_atomic(path: str | Path, payload: dict[str, Any], *, trailing_newline: bool = False) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = target.with_name(f".{target.name}.{os.getpid()}.tmp")
    text = json.dumps(payload, indent=2, sort_keys=True)
    if trailing_newline:
        text += "\n"
    tmp_path.write_text(text)
    tmp_path.replace(target)


def emit_simulator_payload(args: argparse.Namespace, payload: dict[str, Any], evaluation: dict[str, Any]) -> None:
    if args.json_out:
        write_json_atomic(args.json_out, payload)
    if args.suppress_json_stdout:
        print(
            json.dumps(
                {
                    "json_out": args.json_out,
                    "target": args.target,
                    "gamble_mode": args.gamble_mode,
                    "successes": evaluation["successes"],
                    "eval_games": args.eval_games,
                    "success_rate": evaluation["success_rate"],
                    "cap_misses": evaluation["cap_misses"],
                    "initial_cap_misses_before_actual_rerun": evaluation["initial_cap_misses_before_actual_rerun"],
                    "actual_cap_rerun_attempts": evaluation["actual_cap_rerun_attempts"],
                    "actual_cap_rerun_successes": evaluation["actual_cap_rerun_successes"],
                    "turn_counts": evaluation["turn_counts"],
                },
                sort_keys=True,
            ),
            flush=True,
        )
    else:
        print(json.dumps(payload, indent=2, sort_keys=True))


def split_shard_counts(total: int, shards: int) -> list[int]:
    shards = max(1, shards)
    base, extra = divmod(total, shards)
    return [base + int(index < extra) for index in range(shards)]


def effective_rust_full_sim_shards(args: argparse.Namespace) -> int:
    games_per_shard = getattr(args, "rust_full_sim_games_per_shard", None)
    if games_per_shard is not None:
        return max(1, math.ceil(args.eval_games / games_per_shard))
    return max(1, int(args.rust_full_sim_shards))


def sorted_count_map(counter: Counter[str]) -> dict[str, int]:
    return dict(sorted((str(key), int(value)) for key, value in counter.items()))


def sum_response_count_maps(responses: list[dict[str, Any]], key: str) -> dict[str, int]:
    counter: Counter[str] = Counter()
    for response in responses:
        counter.update({str(item_key): int(value) for item_key, value in (response.get(key) or {}).items()})
    return sorted_count_map(counter)


def add_trace_summary_to_evaluation(evaluation: dict[str, Any], deck: tuple[str, ...]) -> None:
    records = evaluation.get("game_records") or []
    if not records:
        return
    successes = int(evaluation.get("successes") or 0)
    games = int(evaluation.get("games") or 0)
    all_cards = sorted(set(deck))
    traced_successes = sum(1 for row in records if row.get("hit") and row.get("trace_found") is True)
    trace_failed_successes = sum(1 for row in records if row.get("hit") and row.get("trace_found") is False)
    nick_fury_successes = sum(1 for row in records if row.get("hit") and row.get("line_casts_nick_fury") is True)
    line_card_counts: Counter[str] = Counter()
    for row in records:
        if row.get("hit") and row.get("line_cards"):
            line_card_counts.update(set(row["line_cards"]))
    if not traced_successes and not line_card_counts:
        return
    evaluation.update(
        {
            "traced_successes": traced_successes,
            "trace_failed_successes": trace_failed_successes,
            "nick_fury_success_lines": nick_fury_successes,
            "nick_fury_cast_rate_among_successes": nick_fury_successes / successes if successes else 0.0,
            "nick_fury_cast_rate_among_traced_successes": nick_fury_successes / traced_successes if traced_successes else 0.0,
            "nick_fury_cast_rate_all_games": nick_fury_successes / games if games else 0.0,
            "line_card_counts": dict(sorted(line_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "line_card_rates_among_successes": {
                card: line_card_counts.get(card, 0) / successes if successes else 0.0
                for card in all_cards
            },
            "zero_line_cards": [card for card in all_cards if line_card_counts.get(card, 0) == 0],
            "low_line_cards_le_1pct": [
                card
                for card in all_cards
                if successes and 0 < line_card_counts.get(card, 0) / successes <= 0.01
            ],
            "low_line_cards_le_2pct": [
                card
                for card in all_cards
                if successes and 0 < line_card_counts.get(card, 0) / successes <= 0.02
            ],
        }
    )


def merge_policy_eval_shards(
    shard_results: list[dict[str, Any]],
    *,
    deck: tuple[str, ...],
    actual_rerun_state_limit: int,
    include_game_records: bool,
) -> dict[str, Any]:
    shard_results = sorted(shard_results, key=lambda item: int(item["shard_index"]))
    evaluations = [dict(item["evaluation"]) for item in shard_results]
    games = sum(int(evaluation.get("games") or 0) for evaluation in evaluations)
    successes = sum(int(evaluation.get("successes") or 0) for evaluation in evaluations)
    cap_misses = sum(int(evaluation.get("cap_misses") or 0) for evaluation in evaluations)
    initial_cap_misses = sum(int(evaluation.get("initial_cap_misses_before_actual_rerun") or 0) for evaluation in evaluations)
    actual_cap_rerun_attempts = sum(int(evaluation.get("actual_cap_rerun_attempts") or 0) for evaluation in evaluations)
    actual_cap_rerun_successes = sum(int(evaluation.get("actual_cap_rerun_successes") or 0) for evaluation in evaluations)
    actual_cap_rerun_remaining_caps = sum(int(evaluation.get("actual_cap_rerun_remaining_caps") or 0) for evaluation in evaluations)
    gem_counts = Counter()
    gem_successes = Counter()
    for evaluation in evaluations:
        counts = evaluation.get("gemstone_caverns_live_counts") or {}
        rates = evaluation.get("success_rate_by_gemstone_caverns_live") or {}
        raw_successes = evaluation.get("gemstone_caverns_live_successes") or {}
        for key, count_value in counts.items():
            count = int(count_value)
            key_str = str(key)
            gem_counts[key_str] += count
            if key in raw_successes or key_str in raw_successes:
                gem_successes[key_str] += int(raw_successes.get(key, raw_successes.get(key_str, 0)) or 0)
            else:
                gem_successes[key_str] += int(round(float(rates.get(key, rates.get(key_str, 0.0))) * count))

    game_records: list[dict[str, Any]] = []
    cap_replay_records: list[dict[str, Any]] = []
    validation_records: list[dict[str, Any]] = []
    offset = 0
    for shard in shard_results:
        evaluation = shard["evaluation"]
        shard_index = int(shard["shard_index"])
        shard_games = int(evaluation.get("games") or 0)
        if include_game_records:
            for row in evaluation.get("game_records") or []:
                remapped = dict(row)
                local_game_index = int(remapped.get("game_index") or 0)
                remapped["shard_index"] = shard_index
                remapped["shard_local_game_index"] = local_game_index
                remapped["game_index"] = offset + local_game_index
                game_records.append(remapped)
        for row in evaluation.get("cap_replay_records") or []:
            remapped = dict(row)
            local_game_index = int(remapped.get("game_index") or 0)
            remapped["shard_index"] = shard_index
            remapped["shard_local_game_index"] = local_game_index
            remapped["game_index"] = offset + local_game_index
            cap_replay_records.append(remapped)
        for row in evaluation.get("validation_records") or []:
            remapped = dict(row)
            local_game_index = int(remapped.get("game_index") or 0)
            remapped["shard_index"] = shard_index
            remapped["shard_local_game_index"] = local_game_index
            remapped["game_index"] = offset + local_game_index
            validation_records.append(remapped)
        offset += shard_games

    merged = {
        "games": games,
        "successes": successes,
        "success_rate": successes / games if games else 0.0,
        "cap_misses": cap_misses,
        "initial_cap_misses_before_actual_rerun": initial_cap_misses,
        "actual_cap_rerun_state_limit": actual_rerun_state_limit,
        "actual_cap_rerun_attempts": actual_cap_rerun_attempts,
        "actual_cap_rerun_successes": actual_cap_rerun_successes,
        "actual_cap_rerun_remaining_caps": actual_cap_rerun_remaining_caps,
        "upper_success_rate_if_caps_hit": (successes + cap_misses) / games if games else 0.0,
        "wilson95": list(wilson(successes, games)),
        "turn_counts": sum_response_count_maps(evaluations, "turn_counts"),
        "keep_counts_by_stage": sum_response_count_maps(evaluations, "keep_counts_by_stage"),
        "keep_counts_by_bottom": sum_response_count_maps(evaluations, "keep_counts_by_bottom"),
        "gemstone_caverns_live_counts": sorted_count_map(gem_counts),
        "gemstone_caverns_live_successes": sorted_count_map(gem_successes),
        "success_rate_by_gemstone_caverns_live": {
            key: gem_successes.get(key, 0) / count if count else 0.0
            for key, count in sorted(gem_counts.items())
        },
        "visible_ev_cache_size": sum(int(evaluation.get("visible_ev_cache_size") or 0) for evaluation in evaluations),
        "sharded_visible_ev_cache_size_sum": sum(int(evaluation.get("visible_ev_cache_size") or 0) for evaluation in evaluations),
        "rng_metadata": evaluations[0].get("rng_metadata") if evaluations else {},
        "unsupported": False,
        "unsupported_reason": None,
    }
    if include_game_records:
        merged["game_records"] = sorted(game_records, key=lambda item: int(item["game_index"]))
    else:
        merged["game_records"] = None
    merged["cap_replay_records"] = (
        sorted(cap_replay_records, key=lambda item: int(item["game_index"])) if cap_replay_records else None
    )
    merged["validation_records"] = (
        sorted(validation_records, key=lambda item: int(item["game_index"])) if validation_records else None
    )
    add_trace_summary_to_evaluation(merged, deck)
    return merged


def run_rust_policy_eval_request(
    *,
    module: Any,
    args: argparse.Namespace,
    request: dict[str, Any],
) -> dict[str, Any]:
    accelerator = module.RustActionAccelerator(
        args.rust_action_bin,
        command=os.environ.get("RHYSTIC_RUST_POLICY_EVAL_COMMAND", "policy-eval-fast-jsonl"),
    )
    try:
        return accelerator.policy_eval(request)
    finally:
        accelerator.close()


def run_rust_policy_eval_with_thresholds(
    *,
    module: Any,
    args: argparse.Namespace,
    deck: tuple[str, ...],
    common_request: dict[str, Any],
    thresholds_by_gemstone_key: dict[str, list[float]],
    eval_seed: int,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    shards = effective_rust_full_sim_shards(args)
    if args.rust_full_sim_internal_shards and shards > 1:
        shard_workers = args.rust_full_sim_shard_workers
        if shard_workers is None:
            shard_workers = min(shards, max(1, int(args.workers)))
        request = {
            **common_request,
            "thresholds_dead": thresholds_by_gemstone_key["dead"],
            "thresholds_live": thresholds_by_gemstone_key.get("live", thresholds_by_gemstone_key["dead"]),
            "games": args.eval_games,
            "seed": eval_seed,
            "internal_shards": shards,
            "internal_shard_workers": max(1, int(shard_workers)),
        }
        evaluation = run_rust_policy_eval_request(module=module, args=args, request=request)
        evaluation["rust_full_sim_shards"] = shards
        evaluation["rust_full_sim_shard_workers"] = max(1, int(shard_workers))
        evaluation["rust_full_sim_internal_shards"] = True
        return evaluation, []
    shard_counts = [count for count in split_shard_counts(args.eval_games, shards) if count > 0]
    if len(shard_counts) <= 1:
        request = {
            **common_request,
            "thresholds_dead": thresholds_by_gemstone_key["dead"],
            "thresholds_live": thresholds_by_gemstone_key.get("live", thresholds_by_gemstone_key["dead"]),
            "games": args.eval_games,
            "seed": eval_seed,
        }
        evaluation = run_rust_policy_eval_request(module=module, args=args, request=request)
        return evaluation, []

    shard_workers = args.rust_full_sim_shard_workers
    if shard_workers is None:
        shard_workers = min(len(shard_counts), max(1, int(args.workers)))
    shard_workers = max(1, min(int(shard_workers), len(shard_counts)))
    shard_specs = []
    for shard_index, shard_games in enumerate(shard_counts):
        shard_seed = stable_seed(eval_seed, "rust-full-sim-shard", shard_index)
        shard_specs.append((shard_index, shard_games, shard_seed))
    print(
        f"rust full-sim policy eval shards={len(shard_specs)} workers={shard_workers} games={args.eval_games}",
        file=sys.stderr,
        flush=True,
    )

    def run_shard(spec: tuple[int, int, int]) -> dict[str, Any]:
        shard_index, shard_games, shard_seed = spec
        shard_request = {
            **common_request,
            "thresholds_dead": thresholds_by_gemstone_key["dead"],
            "thresholds_live": thresholds_by_gemstone_key.get("live", thresholds_by_gemstone_key["dead"]),
            "games": shard_games,
            "seed": shard_seed,
        }
        started = time.time()
        evaluation = run_rust_policy_eval_request(module=module, args=args, request=shard_request)
        if evaluation.get("unsupported"):
            raise RuntimeError(f"Rust policy eval shard {shard_index} unsupported: {evaluation.get('unsupported_reason')}")
        return {
            "shard_index": shard_index,
            "games": shard_games,
            "seed": shard_seed,
            "elapsed_seconds": time.time() - started,
            "evaluation": evaluation,
        }

    shard_results: list[dict[str, Any]] = []
    with ThreadPoolExecutor(max_workers=shard_workers) as executor:
        future_to_spec = {executor.submit(run_shard, spec): spec for spec in shard_specs}
        for future in as_completed(future_to_spec):
            shard = future.result()
            shard_results.append(shard)
            print(
                "rust shard "
                f"{shard['shard_index']} games={shard['games']} "
                f"successes={shard['evaluation'].get('successes')} "
                f"elapsed={shard['elapsed_seconds']:.2f}s",
                file=sys.stderr,
                flush=True,
            )
    evaluation = merge_policy_eval_shards(
        shard_results,
        deck=deck,
        actual_rerun_state_limit=args.actual_rerun_state_limit,
        include_game_records=args.include_game_records,
    )
    evaluation["rust_full_sim_shards"] = len(shard_specs)
    evaluation["rust_full_sim_shard_workers"] = shard_workers
    return evaluation, sorted(
        [
            {
                "shard_index": int(shard["shard_index"]),
                "games": int(shard["games"]),
                "seed": int(shard["seed"]),
                "elapsed_seconds": float(shard["elapsed_seconds"]),
                "successes": int(shard["evaluation"].get("successes") or 0),
                "cap_misses": int(shard["evaluation"].get("cap_misses") or 0),
                "turn_counts": shard["evaluation"].get("turn_counts") or {},
            }
            for shard in shard_results
        ],
        key=lambda item: item["shard_index"],
    )


def rust_full_sim_payload(
    *,
    args: argparse.Namespace,
    module: Any,
    deck_key: str,
    deck: tuple[str, ...],
    commanders: tuple[str, ...],
    threshold_cache_path: Path | None,
    threshold_cache_hit: bool,
    engine_success_policy: str,
    started: float,
) -> dict[str, Any]:
    if args.target != "rhystic_heartwood":
        raise ValueError("--rust-full-sim currently supports --target rhystic_heartwood only")
    if args.counterfactual_line_cards:
        raise ValueError("--rust-full-sim does not yet support counterfactual diagnostics")
    if args.heuristic_bottom_order:
        raise ValueError("--rust-full-sim does not yet support --heuristic-bottom-order")
    if args.reuse_worker_pool or args.worker_profile_dir:
        raise ValueError("--rust-full-sim runs in one Rust process and does not use Python worker pools/profiling")
    if args.gamble_mode == "stochastic" and not simplified_gamble_enabled():
        raise ValueError("--rust-full-sim with stochastic Gamble requires RHYSTIC_SIMPLIFIED_GAMBLE=1")

    effective_threshold_cache_hit = bool(threshold_cache_path and threshold_cache_path.exists() and not args.thresholds_json)
    goal = "engine"
    simplified_gamble = simplified_gamble_enabled()
    common_request = {
        "deck": list(deck),
        "seed": args.seed,
        "eval_seed": args.seed + 1_000_000,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "state_limit": args.state_limit,
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "cap_weight": args.cap_weight,
        "max_turns": 2,
        "goal": goal,
        "engine_target_count": 1,
        "engine_success_policy": engine_success_policy,
        "remora_upkeep_payments": args.remora_upkeep_payments,
        "action_sort": not args.disable_action_sort,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "include_game_records": args.include_game_records,
        "include_cap_replay_records": args.include_cap_replay_records,
        "include_validation_records": args.include_validation_records,
        "trace_lines": args.trace_lines,
        "gamble_mode": args.gamble_mode,
        "simplified_gamble": simplified_gamble,
        "weighted_policy_ev": args.weighted_policy_ev,
        "rhystic_t1_weight": args.rhystic_t1_weight,
        "rhystic_t2_weight": args.rhystic_t2_weight,
        "heartwood_t1_weight": args.heartwood_t1_weight,
        "heartwood_t2_weight": args.heartwood_t2_weight,
    }

    threshold_started = time.time()
    threshold_source = "rust"
    thresholds_by_gemstone_key: dict[str, list[float]]
    threshold_rows_by_gemstone_key: dict[str, Any]
    rust_response: dict[str, Any] | None = None
    rust_shard_results: list[dict[str, Any]] = []
    rust_shards = effective_rust_full_sim_shards(args)
    eval_seed = args.seed + 1_000_000
    if args.thresholds_json:
        threshold_source = args.thresholds_json
        threshold_payload = json.loads(Path(args.thresholds_json).read_text())
        thresholds_by_gemstone_key, threshold_rows_by_gemstone_key = load_threshold_payload(
            threshold_payload,
            gemstone_caverns_live_rate=args.gemstone_caverns_live_rate,
            source=args.thresholds_json,
        )
    elif effective_threshold_cache_hit and threshold_cache_path is not None:
        threshold_source = str(threshold_cache_path)
        threshold_payload = json.loads(threshold_cache_path.read_text())
        thresholds_by_gemstone_key, threshold_rows_by_gemstone_key = load_threshold_payload(
            threshold_payload,
            gemstone_caverns_live_rate=args.gemstone_caverns_live_rate,
            source=str(threshold_cache_path),
        )
    else:
        request = {
            **common_request,
            "threshold_hands": args.threshold_hands,
            "eval_games": 0 if rust_shards > 1 else args.eval_games,
            "include_game_records": args.include_game_records if rust_shards <= 1 else False,
            "trace_lines": args.trace_lines if rust_shards <= 1 else False,
        }
        accelerator = module.RustActionAccelerator(
            args.rust_action_bin,
            command=os.environ.get("RHYSTIC_RUST_POLICY_SIM_COMMAND", "policy-sim-fast-jsonl"),
        )
        try:
            rust_response = accelerator.policy_sim(request)
        finally:
            accelerator.close()
        if rust_response.get("unsupported"):
            raise RuntimeError(f"Rust full sim returned unsupported: {rust_response.get('unsupported_reason')}")
        thresholds_by_gemstone_key = {
            "dead": [float(value) for value in rust_response["thresholds_dead"]],
            "live": [float(value) for value in rust_response["thresholds_live"]],
        }
        threshold_rows_by_gemstone_key = {
            "dead": rust_response["threshold_rows_dead"],
            "live": rust_response["threshold_rows_live"],
        }
        if threshold_cache_path is not None:
            write_json_atomic(
                threshold_cache_path,
                {
                    "thresholds": thresholds_by_gemstone_key["dead"],
                    "threshold_rows": threshold_rows_by_gemstone_key["dead"],
                    "thresholds_by_gemstone_caverns_live": thresholds_by_gemstone_key,
                    "threshold_rows_by_gemstone_caverns_live": threshold_rows_by_gemstone_key,
                },
                trailing_newline=True,
            )

    threshold_elapsed = time.time() - threshold_started
    thresholds_by_gemstone_key = apply_threshold_offset(
        thresholds_by_gemstone_key,
        args.threshold_offset,
    )
    eval_started = time.time()
    if (
        rust_response is not None
        and rust_shards <= 1
        and not args.thresholds_json
        and not effective_threshold_cache_hit
        and args.threshold_offset == 0
    ):
        evaluation = dict(rust_response["evaluation"])
    else:
        evaluation, rust_shard_results = run_rust_policy_eval_with_thresholds(
            module=module,
            args=args,
            deck=deck,
            common_request=common_request,
            thresholds_by_gemstone_key=thresholds_by_gemstone_key,
            eval_seed=eval_seed,
        )
    evaluation_elapsed = time.time() - eval_started
    if evaluation.get("unsupported"):
        raise RuntimeError(f"Rust full sim evaluation returned unsupported: {evaluation.get('unsupported_reason')}")
    add_trace_summary_to_evaluation(evaluation, deck)
    evaluation.setdefault("actual_cap_rerun_state_limit", args.actual_rerun_state_limit)
    evaluation.setdefault("engine_success_policy", engine_success_policy)
    evaluation.setdefault("remora_upkeep_payments", args.remora_upkeep_payments)
    evaluation.setdefault("gamble_mode", args.gamble_mode)
    evaluation.setdefault("gemstone_caverns_live_rate", args.gemstone_caverns_live_rate)
    evaluation.setdefault("normalize_no_caverns_gemstone_key", args.normalize_no_caverns_gemstone_key)
    evaluation.setdefault("paired_stage_orders", True)
    evaluation.setdefault("adaptive_threshold_sampling", args.adaptive_threshold_sampling)
    evaluation.setdefault("reuse_worker_pool", False)
    evaluation.setdefault("action_sort", not args.disable_action_sort)
    evaluation.setdefault("policy_stage_timing", [])
    evaluation.setdefault("appearance_denominators", {})
    evaluation.setdefault("appearance_card_counts", {})
    evaluation.setdefault("appearance_card_rates", {})
    if args.include_game_records:
        evaluation.setdefault("mulligan_decision_records", [])

    threshold_summary = {
        "thresholds": thresholds_by_gemstone_key["dead"],
        "threshold_rows": threshold_rows_by_gemstone_key.get("dead", []),
        "thresholds_by_gemstone_caverns_live": thresholds_by_gemstone_key,
        "threshold_rows_by_gemstone_caverns_live": threshold_rows_by_gemstone_key,
    }
    return {
        "target": args.target,
        "deck": deck_key,
        "deck_json": args.deck_json,
        "commanders": commanders,
        "mulligan_bottom_sequence": list(COMMANDER_MULLIGAN_BOTTOMS),
        "threshold_hands_per_stage": args.threshold_hands,
        "eval_games": args.eval_games,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "state_limit": args.state_limit,
        "cap_weight_for_keep_decisions": args.cap_weight,
        "thresholds_json": args.thresholds_json,
        "threshold_cache_dir": args.threshold_cache_dir,
        "threshold_cache_path": str(threshold_cache_path) if threshold_cache_path else None,
        "threshold_cache_hit": threshold_cache_hit or effective_threshold_cache_hit,
        "threshold_source": threshold_source,
        "threshold_offset": args.threshold_offset,
        "trace_lines": args.trace_lines,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "engine_success_policy": engine_success_policy,
        "remora_upkeep_payments": args.remora_upkeep_payments,
        "gamble_mode": args.gamble_mode,
        "weighted_policy_ev": args.weighted_policy_ev,
        "rhystic_t1_weight": args.rhystic_t1_weight,
        "rhystic_t2_weight": args.rhystic_t2_weight,
        "heartwood_t1_weight": args.heartwood_t1_weight,
        "heartwood_t2_weight": args.heartwood_t2_weight,
        "rust_action_mode": args.rust_action_mode,
        "rust_close_mode": args.rust_close_mode,
        "rust_solver_mode": args.rust_solver_mode,
        "rust_action_bin": args.rust_action_bin,
        "rust_full_sim": True,
        "rust_full_sim_rng_metadata": evaluation.get("rng_metadata"),
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "counterfactual_line_cards": args.counterfactual_line_cards,
        "counterfactual_state_limit": args.counterfactual_state_limit,
        "include_game_records": args.include_game_records,
        "include_cap_replay_records": args.include_cap_replay_records,
        "include_validation_records": args.include_validation_records,
        "compact_game_records": args.compact_game_records,
        "checkpoint_out": args.checkpoint_out,
        "normalize_no_caverns_gemstone_key": args.normalize_no_caverns_gemstone_key,
        "paired_stage_orders": True,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "reuse_worker_pool": False,
        "threshold_and_policy_reuse_worker_pool": False,
        "worker_profile_dir": args.worker_profile_dir,
        "worker_profile_chunks_per_process": args.worker_profile_chunks_per_process,
        "worker_start_method": "rust",
        "action_sort": not args.disable_action_sort,
        "heuristic_bottom_order": args.heuristic_bottom_order,
        "seed": args.seed,
        "workers": args.rust_full_sim_shard_workers or (min(rust_shards, max(1, args.workers)) if rust_shards > 1 else 1),
        "rust_full_sim_shards": rust_shards,
        "rust_full_sim_games_per_shard": args.rust_full_sim_games_per_shard,
        "rust_full_sim_internal_shards": args.rust_full_sim_internal_shards,
        "rust_full_sim_shard_workers": args.rust_full_sim_shard_workers or (min(rust_shards, max(1, args.workers)) if rust_shards > 1 else 1),
        "rust_full_sim_shard_results": rust_shard_results,
        "cap_replay_record_count": len(evaluation.get("cap_replay_records") or []),
        "validation_record_count": len(evaluation.get("validation_records") or []),
        "chunks_per_worker": args.chunks_per_worker,
        **threshold_summary,
        "evaluation": evaluation,
        "elapsed_seconds": time.time() - started,
        "threshold_elapsed_seconds": threshold_elapsed,
        "eval_elapsed_seconds": evaluation_elapsed,
        "evaluation_elapsed_seconds": evaluation_elapsed,
        "assumptions": [
            "Commander London mulligan sequence is modeled as 7, free 7, 6, 5, 4, 3.",
            "This run used the Rust full policy simulator with ChaCha20 domain-separated shuffles.",
            "Rust full-sim shards use independent domain-separated shard seeds and are merged as independent sample blocks.",
            "For each visible hand, keep EV is estimated from random hidden-library rollouts and bottom choices are selected by visible-hand EV, not by the actual hidden library.",
            "When weighted_policy_ev is enabled, visible-hand score_ev averages weighted outcome value while raw hit rate remains binary.",
            "Bottom choices are selected on one hidden-library sample set, then the selected bottom is scored on an independent validation sample set to reduce max-over-bottom sampling bias.",
            "The actual kept hand is then tested against its real shuffled library with the same color-correct Rust Rhystic/Heartwood solver.",
            "Gemstone Caverns live/dead status is pre-sampled before the first visible hand and known for all mulligan decisions.",
            target_assumption_note(args.target),
            "Stochastic Gamble in Rust full-sim uses the simplified legal random discard model when RHYSTIC_SIMPLIFIED_GAMBLE=1.",
            "State-limit capped misses are counted as misses in the primary rate and reported separately as an upper bound.",
        ],
    }


def worker_init(
    target_mode: str,
    deck_json: str | None,
    state_limit: int,
    samples_per_bottom: int,
    validation_samples: int,
    cap_weight: float,
    trace_lines: bool,
    engine_success_policy: str,
    remora_upkeep_payments: int,
    gamble_mode: str,
    actual_rerun_state_limit: int = 0,
    counterfactual_line_cards: bool = False,
    counterfactual_state_limit: int = 0,
    worker_profile_dir: str | None = None,
    worker_profile_chunks_per_process: int = 0,
    action_sort: bool = True,
    heuristic_bottom_order: bool = False,
) -> None:
    global RH, SEARCH, DECK, TARGET_MODE, STATE_LIMIT, SAMPLES_PER_BOTTOM, VALIDATION_SAMPLES, CAP_WEIGHT, TRACE_LINES, ENGINE_SUCCESS_POLICY, REMORA_UPKEEP_PAYMENTS, DECK_KEY, DECK_JSON, GAMBLE_MODE, ACTUAL_RERUN_STATE_LIMIT, COUNTERFACTUAL_LINE_CARDS, COUNTERFACTUAL_STATE_LIMIT, WORKER_PROFILE_DIR, WORKER_PROFILE_CHUNKS_PER_PROCESS, WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND, ACTION_SORT, HEURISTIC_BOTTOM_ORDER
    TARGET_MODE = target_mode
    DECK_JSON = deck_json
    STATE_LIMIT = state_limit
    SAMPLES_PER_BOTTOM = samples_per_bottom
    VALIDATION_SAMPLES = validation_samples
    CAP_WEIGHT = cap_weight
    TRACE_LINES = trace_lines
    ENGINE_SUCCESS_POLICY = engine_success_policy
    REMORA_UPKEEP_PAYMENTS = remora_upkeep_payments
    GAMBLE_MODE = gamble_mode
    ACTUAL_RERUN_STATE_LIMIT = actual_rerun_state_limit
    COUNTERFACTUAL_LINE_CARDS = counterfactual_line_cards
    COUNTERFACTUAL_STATE_LIMIT = counterfactual_state_limit
    WORKER_PROFILE_DIR = worker_profile_dir
    WORKER_PROFILE_CHUNKS_PER_PROCESS = worker_profile_chunks_per_process
    WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND = {}
    ACTION_SORT = action_sort
    HEURISTIC_BOTTOM_ORDER = heuristic_bottom_order
    if WORKER_PROFILE_DIR:
        Path(WORKER_PROFILE_DIR).mkdir(parents=True, exist_ok=True)
    if SEARCH is not None and hasattr(SEARCH, "close"):
        SEARCH.close()
    RH = load_solver_module()
    DECK_KEY = register_deck(RH, deck_json)
    configure_target(RH, target_mode)
    goal = "rhystic" if target_mode == "rhystic" else "engine"
    SEARCH = RH.RhysticSearch(
        DECK_KEY,
        max_turns=2,
        state_limit=state_limit,
        goal=goal,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=remora_upkeep_payments,
        gamble_mode=gamble_mode,
        action_sort=action_sort,
    )
    DECK = tuple(SEARCH.mainboard)


def stable_seed(*parts: object) -> int:
    payload = "|".join(str(part) for part in parts).encode()
    return int.from_bytes(hashlib.blake2b(payload, digest_size=8).digest(), "big")


def gemstone_key(gemstone_live: bool) -> str:
    return "live" if gemstone_live else "dead"


def hand_key(
    hand: tuple[str, ...],
    bottom_count: int,
    gemstone_live: bool = False,
    *,
    normalize_no_caverns_gemstone_key: bool = False,
) -> str:
    effective_gemstone_live = gemstone_live and not (
        normalize_no_caverns_gemstone_key and not has_gemstone_caverns_alias(hand)
    )
    return f"{bottom_count}|G{int(effective_gemstone_live)}|" + "\x1f".join(sorted(hand))


@lru_cache(maxsize=None)
def bottom_index_choices(hand_size: int, bottom_count: int) -> tuple[tuple[int, ...], ...]:
    if bottom_count == 0:
        return ((),)
    return tuple(combinations(range(hand_size), bottom_count))


def bottom_choices(hand: tuple[str, ...], bottom_count: int) -> list[tuple[str, ...]]:
    cards = tuple(sorted(hand))
    return [tuple(cards[i] for i in idxs) for idxs in bottom_index_choices(len(cards), bottom_count)]


def bottom_card_keep_priority(card: str) -> int:
    if RH is not None:
        engine_native = getattr(RH, "ENGINE_NATIVE", {})
        engine_copy_spells = getattr(RH, "ENGINE_COPY_SPELLS", {})
        if card in engine_native:
            return 1000 if card == getattr(RH, "TARGET", "Rhystic Study") else 850
        if card in engine_copy_spells:
            return 600
        if card in getattr(RH, "HAND_TUTORS", {}) or card in getattr(RH, "TOP_TUTORS", {}):
            return 800
        if (
            card in getattr(RH, "LANDS", set())
            or card in getattr(RH, "MDFC_LANDS", {})
            or getattr(RH, "is_theoretical_rainbow_land", lambda _card: False)(card)
        ):
            return 500
    if card in {
        "Green Sun's Zenith",
        "Eldritch Evolution",
        "Neoform",
        "Summoner's Pact",
        "Crop Rotation",
        "Ranger-Captain of Eos",
    }:
        return 800
    if card in {
        "Chrome Mox",
        "Lion's Eye Diamond",
        "Lotus Petal",
        "Mana Vault",
        "Mox Amber",
        "Mox Diamond",
        "Mox Opal",
        "Sol Ring",
        "Dark Ritual",
        "Rite of Flame",
        "Culling the Weak",
        "Infernal Plunge",
        "Rain of Filth",
        "Simian Spirit Guide",
        "Elvish Spirit Guide",
        "Tinder Wall",
        "Birds of Paradise",
        "Deathrite Shaman",
        "Ragavan, Nimble Pilferer",
    }:
        return 700
    if card in {"Mystic Remora", "Esper Sentinel", "Smothering Tithe", "Heartwood Storyteller"}:
        return 850
    return 100


def indexed_bottom_choices_for_selection(
    bottom_choice_list: list[tuple[str, ...]],
) -> list[tuple[int, tuple[str, ...]]]:
    indexed = list(enumerate(bottom_choice_list))
    if not HEURISTIC_BOTTOM_ORDER or len(indexed) <= 2:
        return indexed
    return sorted(
        indexed,
        key=lambda item: (
            sum(bottom_card_keep_priority(card) for card in item[1]),
            item[0],
        ),
    )


def remove_bottom(hand: tuple[str, ...], bottom: tuple[str, ...]) -> tuple[str, ...]:
    bottom_set = set(bottom)
    return tuple(card for card in hand if card not in bottom_set)


def remaining_cards(hand: tuple[str, ...]) -> list[str]:
    hand_set = set(hand)
    return [card for card in DECK if card not in hand_set]


def solver_hit(
    keep: tuple[str, ...],
    library: tuple[str, ...],
    gemstone_live: bool = False,
    seed: int | None = None,
    gamble_mode: str | None = None,
    state_limit_override: int | None = None,
) -> tuple[bool, bool, int | None]:
    hit, capped, turn, _label = solver_outcome(
        keep,
        library,
        gemstone_live=gemstone_live,
        seed=seed,
        gamble_mode=gamble_mode,
        state_limit_override=state_limit_override,
    )
    return hit, capped, turn


def solver_outcome(
    keep: tuple[str, ...],
    library: tuple[str, ...],
    gemstone_live: bool = False,
    seed: int | None = None,
    gamble_mode: str | None = None,
    state_limit_override: int | None = None,
) -> tuple[bool, bool, int | None, str | None]:
    assert SEARCH is not None
    old_gamble_mode = SEARCH.gamble_mode
    old_optimistic_gamble = SEARCH.optimistic_gamble
    old_state_limit = SEARCH.state_limit
    if gamble_mode is not None:
        SEARCH.gamble_mode = gamble_mode
        SEARCH.optimistic_gamble = gamble_mode == "optimistic"
    if state_limit_override is not None:
        SEARCH.state_limit = state_limit_override
        SEARCH._pact_survival_cache.clear()
        SEARCH._remora_keep_cache.clear()
    try:
        if hasattr(SEARCH, "_earliest_labeled_for_keep"):
            turn, capped, label = SEARCH._earliest_labeled_for_keep(
                list(keep),
                list(library),
                gemstone_live=gemstone_live,
                gamble_seed=seed,
            )
        else:
            turn, capped = SEARCH._earliest_for_keep(list(keep), list(library), gemstone_live=gemstone_live, gamble_seed=seed)
            label = None
    finally:
        SEARCH.gamble_mode = old_gamble_mode
        SEARCH.optimistic_gamble = old_optimistic_gamble
        if state_limit_override is not None:
            SEARCH.state_limit = old_state_limit
            SEARCH._pact_survival_cache.clear()
            SEARCH._remora_keep_cache.clear()
    hit = turn is not None and turn <= 2
    return hit, bool(capped), turn, label if hit else None


def solver_hit_without_card(
    keep: tuple[str, ...],
    library: tuple[str, ...],
    card: str,
    gemstone_live: bool = False,
    seed: int | None = None,
    state_limit_override: int | None = None,
) -> tuple[bool, bool, int | None]:
    assert SEARCH is not None
    old_commander_specs = SEARCH.commander_specs
    if card in SEARCH.commanders:
        SEARCH.commander_specs = [spec for spec in SEARCH.commander_specs if spec[0] != card]
    blanked_keep = tuple("Blank" if item == card else item for item in keep)
    blanked_library = tuple("Blank" if item == card else item for item in library)
    try:
        return solver_hit(
            blanked_keep,
            blanked_library,
            gemstone_live=gemstone_live,
            seed=seed,
            state_limit_override=state_limit_override,
        )
    finally:
        SEARCH.commander_specs = old_commander_specs


def cards_in_trace_path(actions: tuple[str, ...]) -> tuple[str, ...]:
    assert SEARCH is not None
    card_names = set(DECK) | set(SEARCH.commanders)
    found: set[str] = set()
    aliases = {
        "Heartwood": "Heartwood Storyteller",
        "Rhystic": "Rhystic Study",
        "Remora": "Mystic Remora",
        "Beseech": "Beseech the Mirror",
        "Ranger-Captain": "Ranger-Captain of Eos",
        "Deathrite": "Deathrite Shaman",
        "AMBER": "Mox Amber",
        "CHROME": "Chrome Mox",
        "DIAMOND": "Mox Diamond",
        "OPAL": "Mox Opal",
        "SOL": "Sol Ring",
        "SIGNET": "Arcane Signet",
        "WISHCLAW": "Wishclaw Talisman",
        "Wishclaw": "Wishclaw Talisman",
        "LED": "Lion's Eye Diamond",
        "CCLAND": "Ancient Tomb",
        "VEIN": "Crystal Vein",
        "TOWER": "Phyrexian Tower",
        "PETAL": "Lotus Petal",
        "TREASURE": "Treasure",
        "MANTLE": "Paradise Mantle",
        "Relic": "Relic of Legends",
        "DRUM": "Springleaf Drum",
        "BIRD": "Birds of Paradise",
        "NOBLE": "Noble Hierarch",
        "IGNOBLE": "Ignoble Hierarch",
        "CANTOR": "Wild Cantor",
        "NATURE": "Nature's Chosen",
        "CAVERN": "Gemstone Caverns",
        "MINE": "Gemstone Mine",
        "GLIMMER": "Glimmervoid",
        "TINDER": "Tinder Wall",
        "RAGAVAN": "Ragavan, Nimble Pilferer",
        "TATARU": "Tataru Taru",
        "LOTHO": "Lotho, Corrupt Shirriff",
        "ESPER": "Esper Sentinel",
        "HEARTWOOD": "Heartwood Storyteller",
        "Nick Fury": "Nick Fury, Agent of S.H.I.E.L.D.",
    }
    for action in actions:
        scan_action = action
        if action.startswith("cast Gamble for ") and " discard " in action:
            scan_action = action.split(" discard ", 1)[0]
        for card in card_names:
            if card in scan_action:
                found.add(card)
        for alias, card in aliases.items():
            if alias in scan_action and card in card_names:
                found.add(card)
    return tuple(sorted(found))


def _known_card(name: str) -> str | None:
    assert SEARCH is not None
    card_names = set(DECK) | set(SEARCH.commanders)
    if name == "Treasure":
        return name
    aliases = {
        "Heartwood": "Heartwood Storyteller",
        "Rhystic": "Rhystic Study",
        "Remora": "Mystic Remora",
        "Beseech": "Beseech the Mirror",
        "Ranger-Captain": "Ranger-Captain of Eos",
        "Deathrite": "Deathrite Shaman",
        "AMBER": "Mox Amber",
        "CHROME": "Chrome Mox",
        "DIAMOND": "Mox Diamond",
        "OPAL": "Mox Opal",
        "SOL": "Sol Ring",
        "SIGNET": "Arcane Signet",
        "WISHCLAW": "Wishclaw Talisman",
        "Wishclaw": "Wishclaw Talisman",
        "LED": "Lion's Eye Diamond",
        "CCLAND": "Ancient Tomb",
        "VEIN": "Crystal Vein",
        "TOWER": "Phyrexian Tower",
        "PETAL": "Lotus Petal",
        "TREASURE": "Treasure",
        "MANTLE": "Paradise Mantle",
        "Relic": "Relic of Legends",
        "DRUM": "Springleaf Drum",
        "BIRD": "Birds of Paradise",
        "NOBLE": "Noble Hierarch",
        "IGNOBLE": "Ignoble Hierarch",
        "CANTOR": "Wild Cantor",
        "NATURE": "Nature's Chosen",
        "CAVERN": "Gemstone Caverns",
        "MINE": "Gemstone Mine",
        "GLIMMER": "Glimmervoid",
        "TINDER": "Tinder Wall",
        "RAGAVAN": "Ragavan, Nimble Pilferer",
        "TATARU": "Tataru Taru",
        "LOTHO": "Lotho, Corrupt Shirriff",
        "ESPER": "Esper Sentinel",
        "HEARTWOOD": "Heartwood Storyteller",
        "Nick Fury": "Nick Fury, Agent of S.H.I.E.L.D.",
    }
    if name in card_names:
        return name
    return aliases.get(name)


def trace_card_events(actions: tuple[str, ...]) -> dict[str, tuple[str, ...]]:
    assert SEARCH is not None
    assert RH is not None
    card_names = sorted(set(DECK) | set(SEARCH.commanders), key=len, reverse=True)
    casts: set[str] = set()
    engine_casts: set[str] = set()
    tutor_targets: set[str] = set()
    cost_cards: set[str] = set()
    played: set[str] = set()
    activated: set[str] = set()
    mana_sources: set[str] = set()
    generic_lands: list[dict[str, object]] = []

    def add_card(bucket: set[str], name: str) -> None:
        card = _known_card(name)
        if card is not None:
            bucket.add(card)

    def add_target(bucket: set[str], text: str) -> None:
        for card in card_names:
            if card in text:
                bucket.add(card)
                return
        add_card(bucket, text)

    def land_colors(card: str) -> str:
        if card in RH.MDFC_LANDS:
            return RH.MDFC_LANDS[card]
        if card in RH.FETCH_TYPES:
            return ""
        if card in RH.CC_LANDS:
            return "C"
        if card == "Crystal Vein":
            return "C"
        if card == "Phyrexian Tower":
            return "C"
        if card == "Glimmervoid":
            return RH.COLORS
        if card == "Gemstone Mine":
            return RH.COLORS
        if card in RH.ANY_COLOR_LANDS or RH.is_theoretical_rainbow_land(card):
            return RH.COLORS
        if card in RH.COLOR_LANDS:
            return RH.COLOR_LANDS[card]
        if card in RH.COLORLESS_LANDS:
            return "C"
        if card in RH.LAND_TYPES:
            land_types = RH.LAND_TYPES[card]
            return "".join(c for c in RH.COLORS if c in {RH.LAND_COLOR_BY_TYPE[t] for t in land_types})
        if card in RH.LANDS:
            return "C"
        return ""

    def is_generic_land_perm(card: str) -> bool:
        if card in RH.MDFC_LANDS:
            return True
        if card in RH.FETCH_TYPES:
            return False
        if card in RH.CC_LANDS or card in {"Crystal Vein", "Glimmervoid", "Gemstone Mine", "Phyrexian Tower"}:
            return False
        return card in RH.LANDS or RH.is_theoretical_rainbow_land(card)

    def add_generic_land(card: str) -> None:
        colors = land_colors(card)
        if colors and is_generic_land_perm(card):
            generic_lands.append({"card": card, "colors": colors, "tapped": False})

    def credit_generic_land_tap(color: str) -> bool:
        matches = [
            land
            for land in generic_lands
            if color in str(land["colors"]) or (color == "C" and "C" in str(land["colors"]))
        ]
        if not matches:
            return False
        untapped = [land for land in matches if not bool(land["tapped"])]
        land = untapped[0] if untapped else matches[0]
        add_card(mana_sources, str(land["card"]))
        land["tapped"] = True
        return True

    def untap_generic_land() -> bool:
        for land in generic_lands:
            if bool(land["tapped"]):
                land["tapped"] = False
                return True
        return False

    def sacrifice_generic_land() -> bool:
        if not generic_lands:
            return False
        land = generic_lands.pop(0)
        add_card(mana_sources, str(land["card"]))
        return True

    for action in actions:
        caverns_prefix = next(
            (
                f"begin with {name} exiling "
                for name in GEMSTONE_CAVERNS_NAMES
                if action.startswith(f"begin with {name} exiling ")
            ),
            None,
        )
        if caverns_prefix is not None:
            played.add(action.removeprefix("begin with ").split(" exiling ", 1)[0])
            add_target(cost_cards, action.removeprefix(caverns_prefix))
        elif action.startswith("cast ") and ", counter it with An Offer You Can't Refuse" in action:
            text = action.removeprefix("cast ")
            bait = text.split(", counter it with ", 1)[0]
            add_target(casts, bait)
            casts.add("An Offer You Can't Refuse")
        elif action.startswith("cast engine "):
            add_target(casts, action.removeprefix("cast engine "))
            add_target(engine_casts, action.removeprefix("cast engine "))
        elif action.startswith("cast Chrome Mox imprint "):
            casts.add("Chrome Mox")
            add_target(cost_cards, action.removeprefix("cast Chrome Mox imprint "))
        elif action.startswith("cast Mox Diamond discard "):
            casts.add("Mox Diamond")
            add_target(cost_cards, action.removeprefix("cast Mox Diamond discard "))
        elif action.startswith("cast bargained Beseech for "):
            casts.add("Beseech the Mirror")
            add_target(tutor_targets, action.removeprefix("cast bargained Beseech for "))
        elif action.startswith("cast Gamble for ") and " discard " in action:
            casts.add("Gamble")
            text = action.removeprefix("cast Gamble for ")
            target, discarded = text.split(" discard ", 1)
            # If Gamble discards the tutored card, that card was not actually
            # available to the winning line and should not get non-cost credit.
            if _known_card(target) != _known_card(discarded):
                add_target(tutor_targets, target)
            add_target(cost_cards, discarded)
        elif action.startswith("cast optimistic Gamble for "):
            casts.add("Gamble")
            add_target(tutor_targets, action.removeprefix("cast optimistic Gamble for "))
        elif action.startswith("cast Ranger-Captain for "):
            casts.add("Ranger-Captain of Eos")
            add_target(tutor_targets, action.removeprefix("cast Ranger-Captain for "))
        elif action.startswith("cast ") and ", crack LED for " in action and ", tutor " in action:
            text = action.removeprefix("cast ")
            spell, target = text.split(", tutor ", 1)
            spell = spell.split(", crack LED for ", 1)[0]
            add_target(casts, spell)
            mana_sources.add("Lion's Eye Diamond")
            add_target(tutor_targets, target)
        elif action.startswith("activate Wishclaw, crack LED for ") and ", tutor " in action:
            activated.add("Wishclaw Talisman")
            mana_sources.add("Lion's Eye Diamond")
            add_target(tutor_targets, action.split(", tutor ", 1)[1])
        elif action.startswith("cast "):
            text = action.removeprefix("cast ")
            if " copying engine" in text:
                add_target(casts, text.split(" copying engine", 1)[0])
            elif " for " in text:
                card, target = text.split(" for ", 1)
                add_target(casts, card)
                add_target(tutor_targets, target)
            else:
                add_target(casts, text)
        elif action.startswith("activate Wishclaw for "):
            activated.add("Wishclaw Talisman")
            add_target(tutor_targets, action.removeprefix("activate Wishclaw for "))
        elif action.startswith("play "):
            text = action.removeprefix("play ")
            if " fetch " in text:
                card, target = text.split(" fetch ", 1)
                add_target(played, card)
                add_target(tutor_targets, target)
                add_generic_land(target)
            else:
                card = text.removesuffix(" as land")
                add_target(played, card)
                add_generic_land(card)
        elif action.startswith("activate Nature's Chosen ") or action.startswith("tap Nature's Chosen "):
            activated.add("Nature's Chosen")
            if action.endswith(" untap LAND") or action.endswith(" to untap LAND"):
                untap_generic_land()
        elif action.startswith("attack Ragavan"):
            activated.add("Ragavan, Nimble Pilferer")
        elif action.startswith("tap ") or action.startswith("sac ") or action.startswith("exile "):
            text = action.split(" ", 1)[1]
            source = text.split(" for ", 1)[0]
            source = source.split(" sacrificing ", 1)[0]
            source = source.split(" to Rain of Filth", 1)[0]
            if source == "LAND" and " for " in text:
                color = text.rsplit(" for ", 1)[1]
                credit_generic_land_tap(color)
            elif source == "LAND" and " to Rain of Filth" in text:
                sacrifice_generic_land()
            else:
                add_card(mana_sources, source)

    return {
        "casts": tuple(sorted(casts)),
        "engine_casts": tuple(sorted(engine_casts)),
        "tutor_targets": tuple(sorted(tutor_targets)),
        "cost_cards": tuple(sorted(cost_cards)),
        "played": tuple(sorted(played)),
        "activated": tuple(sorted(activated)),
        "mana_sources": tuple(sorted(mana_sources)),
    }


def counterfactual_candidate_cards(line_events: dict[str, tuple[str, ...]]) -> tuple[str, ...]:
    candidates: set[str] = set()
    for bucket, cards in line_events.items():
        if bucket == "cost_cards":
            continue
        candidates.update(cards)
    candidates.discard("Treasure")
    return tuple(sorted(candidates))


def counterfactual_losses_for_line(
    keep: tuple[str, ...],
    library: tuple[str, ...],
    line_events: dict[str, tuple[str, ...]],
    gemstone_live: bool,
    seed: int | None,
    high_state_limit: int | None,
) -> dict[str, str]:
    outcomes: dict[str, str] = {}
    for card in counterfactual_candidate_cards(line_events):
        counterfactual_seed = stable_seed(seed, "counterfactual", card) if seed is not None else None
        hit, capped, _turn = solver_hit_without_card(
            keep,
            library,
            card,
            gemstone_live=gemstone_live,
            seed=counterfactual_seed,
        )
        if (
            not hit
            and capped
            and high_state_limit
            and SEARCH is not None
            and high_state_limit > SEARCH.state_limit
        ):
            hit, capped, _turn = solver_hit_without_card(
                keep,
                library,
                card,
                gemstone_live=gemstone_live,
                seed=counterfactual_seed,
                state_limit_override=high_state_limit,
            )
        if hit:
            outcomes[card] = "survives"
        elif capped:
            outcomes[card] = "capped"
        else:
            outcomes[card] = "loss"
    return outcomes


def trace_line_for_keep(
    keep: tuple[str, ...],
    library: tuple[str, ...],
    gemstone_live: bool = False,
    seed: int | None = None,
    state_limit_override: int | None = None,
) -> tuple[int | None, bool, tuple[str, ...]]:
    assert RH is not None
    assert SEARCH is not None
    old_state_limit = SEARCH.state_limit
    if state_limit_override is not None:
        SEARCH.state_limit = state_limit_override
        SEARCH._pact_survival_cache.clear()
        SEARCH._remora_keep_cache.clear()
    if seed is not None:
        SEARCH.gamble_seed = seed
    try:
        chancellor = "Chancellor of the Tangle" in keep
        states: dict[Any, tuple[str, ...]] = {}
        for start, path in SEARCH._starting_state_options(keep, library, gemstone_live=gemstone_live):
            states.setdefault(start, path)
        truncated = False
        for turn in range(1, SEARCH.max_turns + 1):
            turn_states: dict[Any, tuple[str, ...]] = {}
            for state, path in states.items():
                begun = SEARCH._begin_turn(state)
                begun = SEARCH._replace(begun, turn=turn)
                turn_path = (*path, f"begin turn {turn}")
                if begun.pact_debt <= 0:
                    drawn = SEARCH._draw(begun)
                    if turn == 1 and chancellor:
                        drawn = SEARCH._replace(drawn, mana=RH.add_mana(drawn.mana, (0, 0, 0, 0, 1, 0)))
                    turn_states.setdefault(drawn, turn_path)
                else:
                    for upkeep_paid in SEARCH._pay_upkeep_pacts(begun):
                        drawn = SEARCH._draw(upkeep_paid)
                        if turn == 1 and chancellor:
                            drawn = SEARCH._replace(drawn, mana=RH.add_mana(drawn.mana, (0, 0, 0, 0, 1, 0)))
                        turn_states.setdefault(drawn, turn_path)
            closed, success_path, hit_limit = close_turn_with_paths(turn_states)
            truncated = truncated or hit_limit
            if success_path is not None:
                return turn, truncated, success_path
            states = {SEARCH._end_turn(state): path for state, path in closed.items()}
        return None, truncated, ()
    finally:
        if state_limit_override is not None:
            SEARCH.state_limit = old_state_limit
            SEARCH._pact_survival_cache.clear()
            SEARCH._remora_keep_cache.clear()


def close_turn_with_paths(start_states: dict[Any, tuple[str, ...]]) -> tuple[dict[Any, tuple[str, ...]], tuple[str, ...] | None, bool]:
    assert SEARCH is not None
    queue = deque(start_states)
    seen: dict[Any, None] = {state: None for state in start_states}
    parents: dict[Any, tuple[Any, str]] = {}
    best_mana: dict[tuple, list[tuple[int, ...]]] = {}
    hit_limit = False

    def build_path(state: Any) -> tuple[str, ...]:
        actions: list[str] = []
        current = state
        while current in parents:
            previous, action = parents[current]
            actions.append(action)
            current = previous
        return (*start_states.get(current, ()), *reversed(actions))

    while queue:
        state = queue.pop()
        if SEARCH._success(state):
            path = build_path(state)
            if TARGET_MODE == "rhystic" and RH is not None and RH.TARGET in state.hand and RH.pay_options(state.mana, RH.RHYSTIC_COST):
                path = (*path, f"cast engine {RH.TARGET}")
            return seen, path, hit_limit
        if getattr(SEARCH, "action_sort", True):
            actions = list(SEARCH._actions(state))
            actions.sort(key=lambda item: SEARCH._priority(item[1]))
        else:
            actions = SEARCH._actions(state)
        for next_state, action in actions:
            if next_state in seen or SEARCH._mana_dominated(next_state, best_mana):
                continue
            if SEARCH._success(next_state):
                next_path = (*build_path(state), action)
                if TARGET_MODE == "rhystic" and RH is not None and RH.TARGET in next_state.hand and RH.pay_options(next_state.mana, RH.RHYSTIC_COST):
                    next_path = (*next_path, f"cast engine {RH.TARGET}")
                return seen, next_path, hit_limit
            if len(seen) >= SEARCH.state_limit:
                hit_limit = True
                continue
            seen[next_state] = None
            parents[next_state] = (state, action)
            queue.append(next_state)
    return {state: build_path(state) for state in seen}, None, hit_limit


def evaluate_visible_hand(task: HandTask) -> dict[str, Any]:
    rng = random.Random(task.seed)
    hand = tuple(sorted(task.hand))
    base_remaining = remaining_cards(hand)
    best_selection: dict[str, Any] | None = None
    total_solver_calls = 0
    deterministic_checked = 0
    bottom_candidates_checked = 0
    selection_early_stopped = False
    selection_pruned_candidates = 0
    selection_pruned_samples = 0
    bottom_choice_list = bottom_choices(hand, task.bottom_count)
    single_bottom_choice = len(bottom_choice_list) == 1
    selection_skipped_single_candidate_samples = 0

    def deterministic_hit_payload(bottom: tuple[str, ...]) -> dict[str, Any]:
        return {
            "key": task.key,
            "hand": list(hand),
            "bottom_count": task.bottom_count,
            "gemstone_caverns_live": task.gemstone_live,
            "ev": 1.0,
            "upper_ev": 1.0,
            "score_ev": 1.0,
            "hits": VALIDATION_SAMPLES,
            "cap_misses": 0,
            "samples": VALIDATION_SAMPLES,
            "best_bottom": list(bottom),
            "deterministic": True,
            "selection_hits": SAMPLES_PER_BOTTOM,
            "selection_cap_misses": 0,
            "selection_samples": SAMPLES_PER_BOTTOM,
            "selection_score": 1.0,
            "solver_calls": total_solver_calls,
            "deterministic_checked": deterministic_checked,
            "bottom_candidates_checked": bottom_candidates_checked,
            "selection_early_stopped": True,
            "selection_pruned_candidates": selection_pruned_candidates,
            "selection_pruned_samples": selection_pruned_samples,
            "selection_skipped_single_candidate_samples": selection_skipped_single_candidate_samples,
            "validation_samples_used": 0,
            "adaptive_threshold_resolved": bool(task.adaptive_threshold_sampling),
            "adaptive_threshold_resolution": "deterministic_hit" if task.adaptive_threshold_sampling else "full",
            "adaptive_validation_samples_saved": VALIDATION_SAMPLES if task.adaptive_threshold_sampling else 0,
        }

    def consume_selection_shuffles(count: int) -> None:
        for _ in range(count):
            skipped_library = list(base_remaining)
            rng.shuffle(skipped_library)

    if not HEURISTIC_BOTTOM_ORDER:
        for bottom in bottom_choice_list:
            bottom_candidates_checked += 1
            keep = remove_bottom(hand, bottom)
            blank_library = tuple(["Blank"] * 20 + base_remaining + list(bottom))
            deterministic_checked += 1
            total_solver_calls += 1
            deterministic_gamble_mode = "off" if GAMBLE_MODE == "stochastic" else None
            hit, capped, _turn = solver_hit(
                keep,
                blank_library,
                task.gemstone_live,
                seed=stable_seed(task.seed, "deterministic", bottom),
                gamble_mode=deterministic_gamble_mode,
            )
            if hit:
                return deterministic_hit_payload(bottom)

            selection_hits = 0
            selection_cap_misses = int(capped)
            if single_bottom_choice:
                # With only one legal bottom choice, selection rollouts cannot change
                # the chosen keep. Consume their shuffles so validation samples keep
                # the same seeded libraries as the unoptimized path.
                consume_selection_shuffles(SAMPLES_PER_BOTTOM)
                selection_skipped_single_candidate_samples += SAMPLES_PER_BOTTOM
                best_selection = {
                    "best_bottom": list(bottom),
                    "selection_hits": 0,
                    "selection_cap_misses": selection_cap_misses,
                    "selection_samples": 0,
                    "selection_score": 0.0,
                }
                break
            if best_selection is not None:
                max_remaining_value = max(1.0, CAP_WEIGHT)
                max_possible_score = min(
                    1.0,
                    (selection_cap_misses * CAP_WEIGHT + SAMPLES_PER_BOTTOM * max_remaining_value)
                    / SAMPLES_PER_BOTTOM,
                )
                if (
                    max_possible_score,
                    SAMPLES_PER_BOTTOM,
                    -len(bottom),
                ) <= (
                    best_selection["selection_score"],
                    best_selection["selection_hits"],
                    -len(best_selection["best_bottom"]),
                ):
                    consume_selection_shuffles(SAMPLES_PER_BOTTOM)
                    selection_pruned_candidates += 1
                    selection_pruned_samples += SAMPLES_PER_BOTTOM
                    continue

            candidate_pruned = False
            for sample_index in range(SAMPLES_PER_BOTTOM):
                library = list(base_remaining)
                rng.shuffle(library)
                sample_library = tuple(library + list(bottom))
                total_solver_calls += 1
                sample_hit, sample_capped, _sample_turn = solver_hit(
                    keep,
                    sample_library,
                    task.gemstone_live,
                    seed=stable_seed(task.seed, "selection", bottom, sample_index),
                )
                selection_hits += int(sample_hit)
                selection_cap_misses += int(sample_capped and not sample_hit)
                remaining_samples = SAMPLES_PER_BOTTOM - sample_index - 1
                if best_selection is not None and remaining_samples:
                    max_remaining_value = max(1.0, CAP_WEIGHT)
                    max_possible_score = min(
                        1.0,
                        (
                            selection_hits
                            + selection_cap_misses * CAP_WEIGHT
                            + remaining_samples * max_remaining_value
                        )
                        / SAMPLES_PER_BOTTOM,
                    )
                    max_possible_hits = selection_hits + remaining_samples
                    if (
                        max_possible_score,
                        max_possible_hits,
                        -len(bottom),
                    ) <= (
                        best_selection["selection_score"],
                        best_selection["selection_hits"],
                        -len(best_selection["best_bottom"]),
                    ):
                        # Preserve the seeded random stream for later candidates.
                        consume_selection_shuffles(remaining_samples)
                        selection_pruned_candidates += 1
                        selection_pruned_samples += remaining_samples
                        candidate_pruned = True
                        break

            if candidate_pruned:
                continue

            selection_score = min(1.0, (selection_hits + selection_cap_misses * CAP_WEIGHT) / SAMPLES_PER_BOTTOM)
            candidate = {
                "best_bottom": list(bottom),
                "selection_hits": selection_hits,
                "selection_cap_misses": selection_cap_misses,
                "selection_samples": SAMPLES_PER_BOTTOM,
                "selection_score": selection_score,
            }
            if best_selection is None or (
                candidate["selection_score"],
                candidate["selection_hits"],
                -len(candidate["best_bottom"]),
            ) > (
                best_selection["selection_score"],
                best_selection["selection_hits"],
                -len(best_selection["best_bottom"]),
            ):
                best_selection = candidate
    else:
        deterministic_cap_by_index: dict[int, int] = {}
        selection_libraries_by_index: dict[int, list[tuple[str, ...]]] = {}
        best_selection_index: int | None = None

        def selection_rank(selection: dict[str, Any], bottom_index: int) -> tuple[float, int, int, int]:
            return (
                float(selection["selection_score"]),
                int(selection["selection_hits"]),
                -len(selection["best_bottom"]),
                -bottom_index,
            )

        indexed_bottom_choice_list = list(enumerate(bottom_choice_list))
        for bottom_index, bottom in indexed_bottom_choice_list:
            bottom_candidates_checked += 1
            keep = remove_bottom(hand, bottom)
            blank_library = tuple(["Blank"] * 20 + base_remaining + list(bottom))
            deterministic_checked += 1
            total_solver_calls += 1
            deterministic_gamble_mode = "off" if GAMBLE_MODE == "stochastic" else None
            hit, capped, _turn = solver_hit(
                keep,
                blank_library,
                task.gemstone_live,
                seed=stable_seed(task.seed, "deterministic", bottom),
                gamble_mode=deterministic_gamble_mode,
            )
            if hit:
                return deterministic_hit_payload(bottom)
            deterministic_cap_by_index[bottom_index] = int(capped)

        for bottom_index, bottom in indexed_bottom_choice_list:
            sample_libraries: list[tuple[str, ...]] = []
            for _sample_index in range(SAMPLES_PER_BOTTOM):
                library = list(base_remaining)
                rng.shuffle(library)
                sample_libraries.append(tuple(library + list(bottom)))
            selection_libraries_by_index[bottom_index] = sample_libraries

        for bottom_index, bottom in indexed_bottom_choices_for_selection(bottom_choice_list):
            keep = remove_bottom(hand, bottom)
            selection_hits = 0
            selection_cap_misses = deterministic_cap_by_index[bottom_index]
            if single_bottom_choice:
                selection_skipped_single_candidate_samples += SAMPLES_PER_BOTTOM
                best_selection = {
                    "best_bottom": list(bottom),
                    "selection_hits": 0,
                    "selection_cap_misses": selection_cap_misses,
                    "selection_samples": 0,
                    "selection_score": 0.0,
                }
                break
            if best_selection is not None:
                max_remaining_value = max(1.0, CAP_WEIGHT)
                max_possible_score = min(
                    1.0,
                    (selection_cap_misses * CAP_WEIGHT + SAMPLES_PER_BOTTOM * max_remaining_value)
                    / SAMPLES_PER_BOTTOM,
                )
                if (
                    max_possible_score,
                    SAMPLES_PER_BOTTOM,
                    -len(bottom),
                    -bottom_index,
                ) <= selection_rank(best_selection, best_selection_index if best_selection_index is not None else 0):
                    selection_pruned_candidates += 1
                    selection_pruned_samples += SAMPLES_PER_BOTTOM
                    continue

            candidate_pruned = False
            for sample_index, sample_library in enumerate(selection_libraries_by_index[bottom_index]):
                total_solver_calls += 1
                sample_hit, sample_capped, _sample_turn = solver_hit(
                    keep,
                    sample_library,
                    task.gemstone_live,
                    seed=stable_seed(task.seed, "selection", bottom, sample_index),
                )
                selection_hits += int(sample_hit)
                selection_cap_misses += int(sample_capped and not sample_hit)
                remaining_samples = SAMPLES_PER_BOTTOM - sample_index - 1
                if best_selection is not None and remaining_samples:
                    max_remaining_value = max(1.0, CAP_WEIGHT)
                    max_possible_score = min(
                        1.0,
                        (
                            selection_hits
                            + selection_cap_misses * CAP_WEIGHT
                            + remaining_samples * max_remaining_value
                        )
                        / SAMPLES_PER_BOTTOM,
                    )
                    max_possible_hits = selection_hits + remaining_samples
                    if (
                        max_possible_score,
                        max_possible_hits,
                        -len(bottom),
                        -bottom_index,
                    ) <= selection_rank(best_selection, best_selection_index if best_selection_index is not None else 0):
                        selection_pruned_candidates += 1
                        selection_pruned_samples += remaining_samples
                        candidate_pruned = True
                        break

            if candidate_pruned:
                continue

            selection_score = min(1.0, (selection_hits + selection_cap_misses * CAP_WEIGHT) / SAMPLES_PER_BOTTOM)
            candidate = {
                "best_bottom": list(bottom),
                "selection_hits": selection_hits,
                "selection_cap_misses": selection_cap_misses,
                "selection_samples": SAMPLES_PER_BOTTOM,
                "selection_score": selection_score,
            }
            if best_selection is None or (
                candidate["selection_score"],
                candidate["selection_hits"],
                -len(candidate["best_bottom"]),
                -bottom_index,
            ) > selection_rank(best_selection, best_selection_index if best_selection_index is not None else 0):
                best_selection = candidate
                best_selection_index = bottom_index

    assert best_selection is not None
    bottom = tuple(best_selection["best_bottom"])
    keep = remove_bottom(hand, bottom)
    validation_hits = 0
    validation_cap_misses = 0
    validation_samples_used = 0
    adaptive_threshold_resolved = False
    adaptive_threshold_resolution = "full"
    if task.adaptive_threshold_sampling and task.force_keep:
        adaptive_threshold_resolved = True
        adaptive_threshold_resolution = "force_keep"
        ev = 0.0
        upper_ev = 1.0
        score_ev = 1.0
    else:
        for sample_index in range(VALIDATION_SAMPLES):
            library = list(base_remaining)
            rng.shuffle(library)
            sample_library = tuple(library + list(bottom))
            total_solver_calls += 1
            sample_hit, sample_capped, _sample_turn = solver_hit(
                keep,
                sample_library,
                task.gemstone_live,
                seed=stable_seed(task.seed, "validation", bottom, sample_index),
            )
            validation_hits += int(sample_hit)
            validation_cap_misses += int(sample_capped and not sample_hit)
            validation_samples_used += 1
            if task.adaptive_threshold_sampling and task.keep_threshold is not None:
                remaining_samples = VALIDATION_SAMPLES - validation_samples_used
                current_score_units = validation_hits + validation_cap_misses * CAP_WEIGHT
                max_remaining_value = max(1.0, CAP_WEIGHT)
                min_score = min(1.0, current_score_units / VALIDATION_SAMPLES)
                max_score = min(
                    1.0,
                    (current_score_units + remaining_samples * max_remaining_value) / VALIDATION_SAMPLES,
                )
                if min_score >= task.keep_threshold:
                    adaptive_threshold_resolved = True
                    adaptive_threshold_resolution = "keep"
                    break
                if max_score < task.keep_threshold:
                    adaptive_threshold_resolved = True
                    adaptive_threshold_resolution = "mulligan"
                    break

        ev = validation_hits / VALIDATION_SAMPLES
        upper_ev = min(1.0, (validation_hits + validation_cap_misses) / VALIDATION_SAMPLES)
        score_ev = min(1.0, (validation_hits + validation_cap_misses * CAP_WEIGHT) / VALIDATION_SAMPLES)
        if adaptive_threshold_resolution == "mulligan":
            remaining_samples = VALIDATION_SAMPLES - validation_samples_used
            max_remaining_value = max(1.0, CAP_WEIGHT)
            score_ev = min(
                1.0,
                (validation_hits + validation_cap_misses * CAP_WEIGHT + remaining_samples * max_remaining_value)
                / VALIDATION_SAMPLES,
            )
            upper_ev = min(1.0, (validation_hits + validation_cap_misses + remaining_samples) / VALIDATION_SAMPLES)
    return {
        "key": task.key,
        "hand": list(hand),
        "bottom_count": task.bottom_count,
        "gemstone_caverns_live": task.gemstone_live,
        "ev": ev,
        "upper_ev": upper_ev,
        "score_ev": score_ev,
        "hits": validation_hits,
        "cap_misses": validation_cap_misses,
        "samples": VALIDATION_SAMPLES,
        "best_bottom": list(bottom),
        "deterministic": False,
        "selection_hits": best_selection["selection_hits"],
        "selection_cap_misses": best_selection["selection_cap_misses"],
        "selection_samples": best_selection["selection_samples"],
        "selection_score": best_selection["selection_score"],
        "solver_calls": total_solver_calls,
        "deterministic_checked": deterministic_checked,
        "bottom_candidates_checked": bottom_candidates_checked,
        "selection_early_stopped": selection_early_stopped,
        "selection_pruned_candidates": selection_pruned_candidates,
        "selection_pruned_samples": selection_pruned_samples,
        "selection_skipped_single_candidate_samples": selection_skipped_single_candidate_samples,
        "validation_samples_used": validation_samples_used,
        "adaptive_threshold_resolved": adaptive_threshold_resolved,
        "adaptive_threshold_resolution": adaptive_threshold_resolution,
        "adaptive_validation_samples_saved": VALIDATION_SAMPLES - validation_samples_used,
    }


def actual_solve_request(task: ActualTask, state_limit: int) -> dict[str, Any]:
    assert SEARCH is not None
    return {
        "hand": list(task.keep),
        "library": list(task.library),
        "gemstone_live": task.gemstone_live,
        "state_limit": state_limit,
        "max_turns": SEARCH.max_turns,
        "goal": SEARCH.goal,
        "engine_target_count": SEARCH.engine_target_count,
        "engine_success_policy": SEARCH.engine_success_policy,
        "remora_upkeep_payments": SEARCH.remora_upkeep_payments,
        "action_sort": SEARCH.action_sort,
        "gamble_mode": SEARCH.gamble_mode,
        "gamble_seed": task.seed,
        "simplified_gamble": SEARCH.simplified_gamble,
    }


def batch_initial_actual_solves(tasks: list[ActualTask]) -> list[tuple[bool, bool, int | None, str | None, float]] | None:
    assert SEARCH is not None
    accelerator = getattr(SEARCH, "_rust_solver_batch_accelerator", None)
    if accelerator is None or getattr(SEARCH, "rust_solver_mode", "off") == "off":
        return None
    if not getattr(SEARCH, "_rust_solver_supported")():
        return None

    requests = [actual_solve_request(task, SEARCH.state_limit) for task in tasks]
    started = time.time()
    responses = accelerator.solve_keep_batch(requests)
    batch_elapsed = time.time() - started
    per_task_elapsed = batch_elapsed / len(tasks) if tasks else 0.0
    out: list[tuple[bool, bool, int | None, str | None, float]] = []

    for task, (turn, capped, label, unsupported, _reason) in zip(tasks, responses, strict=True):
        if unsupported:
            fallback_started = time.time()
            hit, capped, turn, label = solver_outcome(task.keep, task.library, task.gemstone_live, seed=task.seed)
            out.append((hit, capped, turn, label, time.time() - fallback_started))
            continue
        if getattr(SEARCH, "rust_solver_mode", "off") == "verify":
            py_turn, py_capped, py_label = SEARCH._earliest_labeled_for_keep_python(
                list(task.keep),
                list(task.library),
                gemstone_live=task.gemstone_live,
                gamble_seed=task.seed,
            )
            if (py_turn, py_capped, py_label) != (turn, capped, label):
                raise RuntimeError(
                    "Rust solve-keep batch parity mismatch: "
                    f"python={(py_turn, py_capped, py_label)} rust={(turn, capped, label)} "
                    f"game_index={task.game_index} stage={task.stage} bottom_count={task.bottom_count}"
                )
            turn, capped, label = py_turn, py_capped, py_label
        hit = turn is not None and turn <= 2
        out.append((hit, bool(capped), turn, label if hit else None, per_task_elapsed))
    return out


def evaluate_actual_keep_with_initial(
    task: ActualTask,
    initial: tuple[bool, bool, int | None, str | None, float] | None = None,
) -> dict[str, Any]:
    started = time.time()
    if initial is None:
        initial_started = time.time()
        hit, capped, turn, label = solver_outcome(task.keep, task.library, task.gemstone_live, seed=task.seed)
        initial_elapsed = time.time() - initial_started
    else:
        hit, capped, turn, label, initial_elapsed = initial
    initial_hit = hit
    initial_capped = capped
    initial_turn = turn
    initial_label = label
    cap_rerun_attempted = False
    cap_rerun_hit = None
    cap_rerun_capped = None
    cap_rerun_turn = None
    cap_rerun_label = None
    cap_rerun_elapsed = 0.0
    if (
        not hit
        and capped
        and ACTUAL_RERUN_STATE_LIMIT
        and SEARCH is not None
        and ACTUAL_RERUN_STATE_LIMIT > SEARCH.state_limit
    ):
        cap_rerun_attempted = True
        cap_rerun_started = time.time()
        cap_rerun_hit, cap_rerun_capped, cap_rerun_turn, cap_rerun_label = solver_outcome(
            task.keep,
            task.library,
            task.gemstone_live,
            seed=task.seed,
            state_limit_override=ACTUAL_RERUN_STATE_LIMIT,
        )
        cap_rerun_elapsed = time.time() - cap_rerun_started
        hit = cap_rerun_hit
        capped = bool(cap_rerun_capped)
        turn = cap_rerun_turn
        label = cap_rerun_label
    trace_turn = None
    trace_capped = False
    trace_path: tuple[str, ...] = ()
    line_cards: tuple[str, ...] = ()
    line_events: dict[str, tuple[str, ...]] = {}
    counterfactual_results: dict[str, str] = {}
    trace_elapsed = 0.0
    counterfactual_elapsed = 0.0
    if TRACE_LINES and hit:
        trace_state_limit_override = (
            ACTUAL_RERUN_STATE_LIMIT
            if cap_rerun_attempted and ACTUAL_RERUN_STATE_LIMIT and SEARCH is not None and ACTUAL_RERUN_STATE_LIMIT > SEARCH.state_limit
            else None
        )
        trace_started = time.time()
        trace_turn, trace_capped, trace_path = trace_line_for_keep(
            task.keep,
            task.library,
            task.gemstone_live,
            seed=task.seed,
            state_limit_override=trace_state_limit_override,
        )
        trace_elapsed = time.time() - trace_started
        line_cards = cards_in_trace_path(trace_path)
        line_events = trace_card_events(trace_path)
        if COUNTERFACTUAL_LINE_CARDS and line_events:
            counterfactual_started = time.time()
            counterfactual_results = counterfactual_losses_for_line(
                task.keep,
                task.library,
                line_events,
                task.gemstone_live,
                task.seed,
                COUNTERFACTUAL_STATE_LIMIT or ACTUAL_RERUN_STATE_LIMIT or None,
            )
            counterfactual_elapsed = time.time() - counterfactual_started
    return {
        "game_index": task.game_index,
        "stage": task.stage,
        "bottom_count": task.bottom_count,
        "gemstone_caverns_live": task.gemstone_live,
        "seed": task.seed,
        "library": list(task.library),
        "hit": hit,
        "capped": capped,
        "turn": turn,
        "engine_label": label,
        "initial_hit": initial_hit,
        "initial_capped": initial_capped,
        "initial_turn": initial_turn,
        "initial_engine_label": initial_label,
        "cap_rerun_attempted": cap_rerun_attempted,
        "cap_rerun_state_limit": ACTUAL_RERUN_STATE_LIMIT if cap_rerun_attempted else None,
        "cap_rerun_hit": cap_rerun_hit,
        "cap_rerun_capped": cap_rerun_capped,
        "cap_rerun_turn": cap_rerun_turn,
        "cap_rerun_engine_label": cap_rerun_label,
        "actual_elapsed_seconds": time.time() - started,
        "initial_solve_elapsed_seconds": initial_elapsed,
        "cap_rerun_elapsed_seconds": cap_rerun_elapsed,
        "trace_elapsed_seconds": trace_elapsed,
        "counterfactual_elapsed_seconds": counterfactual_elapsed,
        "trace_turn": trace_turn,
        "trace_capped": trace_capped,
        "trace_found": (trace_turn is not None and trace_turn <= 2) if TRACE_LINES and hit else None,
        "line_casts_nick_fury": any(action.startswith("cast Nick Fury, Agent of S.H.I.E.L.D.") for action in trace_path) if TRACE_LINES and hit else None,
        "line_action_count": len(trace_path) if TRACE_LINES and hit else None,
        "line_actions": list(trace_path) if TRACE_LINES and hit else None,
        "line_cards": list(line_cards) if TRACE_LINES and hit else None,
        "line_events": {key: list(value) for key, value in line_events.items()} if TRACE_LINES and hit else None,
        "counterfactual_line_card_results": counterfactual_results if TRACE_LINES and hit and COUNTERFACTUAL_LINE_CARDS else None,
    }


def evaluate_actual_keep(task: ActualTask) -> dict[str, Any]:
    return evaluate_actual_keep_with_initial(task)


def batch_visible_hands(tasks: list[HandTask]) -> list[dict[str, Any]] | None:
    assert SEARCH is not None
    if HEURISTIC_BOTTOM_ORDER:
        return None
    accelerator = getattr(SEARCH, "_rust_visible_accelerator", None)
    if accelerator is None or getattr(SEARCH, "rust_solver_mode", "off") == "off":
        return None
    if not getattr(SEARCH, "_rust_solver_supported")():
        return None
    payload = {
        "deck": list(DECK),
        "tasks": [
            {
                "key": task.key,
                "hand": list(task.hand),
                "bottom_count": task.bottom_count,
                "seed": task.seed,
                "gemstone_live": task.gemstone_live,
                "keep_threshold": task.keep_threshold,
                "force_keep": task.force_keep,
                "adaptive_threshold_sampling": task.adaptive_threshold_sampling,
            }
            for task in tasks
        ],
        "state_limit": SEARCH.state_limit,
        "samples_per_bottom": SAMPLES_PER_BOTTOM,
        "validation_samples": VALIDATION_SAMPLES,
        "cap_weight": CAP_WEIGHT,
        "max_turns": SEARCH.max_turns,
        "goal": SEARCH.goal,
        "engine_target_count": SEARCH.engine_target_count,
        "engine_success_policy": SEARCH.engine_success_policy,
        "remora_upkeep_payments": SEARCH.remora_upkeep_payments,
        "action_sort": SEARCH.action_sort,
        "gamble_mode": SEARCH.gamble_mode,
        "simplified_gamble": SEARCH.simplified_gamble,
    }
    return accelerator.visible_hand_batch(payload)


def wilson(k: int, n: int, z: float = 1.959963984540054) -> tuple[float, float]:
    if n == 0:
        return 0.0, 0.0
    p = k / n
    den = 1 + z * z / n
    center = (p + z * z / (2 * n)) / den
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
    return center - half, center + half


def chunk_size_for(total: int, workers: int, chunks_per_worker: int = 16) -> int:
    if total <= 0:
        return 1
    target_chunks = max(1, workers * chunks_per_worker)
    return max(1, math.ceil(total / target_chunks))


def default_chunks_per_worker(workers: int, reuse_worker_pool: bool = False) -> int:
    if reuse_worker_pool:
        return 16
    return 8 if workers <= 12 else 16


def chunks(items: list[Any], size: int) -> list[list[Any]]:
    return [items[i : i + size] for i in range(0, len(items), size)]


def multiprocessing_context() -> multiprocessing.context.BaseContext:
    method = os.environ.get("RHYSTIC_WORKER_START_METHOD")
    if method:
        return multiprocessing.get_context(method)
    return multiprocessing.get_context("fork") if "fork" in multiprocessing.get_all_start_methods() else multiprocessing.get_context()


def create_worker_pool(
    *,
    workers: int,
    target_mode: str,
    deck_json: str | None,
    state_limit: int,
    samples_per_bottom: int,
    validation_samples: int,
    cap_weight: float,
    trace_lines: bool,
    engine_success_policy: str,
    remora_upkeep_payments: int,
    gamble_mode: str,
    actual_rerun_state_limit: int,
    counterfactual_line_cards: bool,
    counterfactual_state_limit: int,
    worker_profile_dir: str | None = None,
    worker_profile_chunks_per_process: int = 0,
    action_sort: bool = True,
    heuristic_bottom_order: bool = False,
) -> ProcessPoolExecutor:
    return ProcessPoolExecutor(
        max_workers=workers,
        mp_context=multiprocessing_context(),
        initializer=worker_init,
        initargs=(
            target_mode,
            deck_json,
            state_limit,
            samples_per_bottom,
            validation_samples,
            cap_weight,
            trace_lines,
            engine_success_policy,
            remora_upkeep_payments,
            gamble_mode,
            actual_rerun_state_limit,
            counterfactual_line_cards,
            counterfactual_state_limit,
            worker_profile_dir,
            worker_profile_chunks_per_process,
            action_sort,
            heuristic_bottom_order,
        ),
    )


def maybe_profile_worker_chunk(kind: str, func: Any, tasks: list[Any]) -> list[dict[str, Any]]:
    global WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND
    profile_count = WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND.get(kind, 0)
    if (
        not WORKER_PROFILE_DIR
        or WORKER_PROFILE_CHUNKS_PER_PROCESS <= 0
        or profile_count >= WORKER_PROFILE_CHUNKS_PER_PROCESS
    ):
        return func(tasks)
    if sys.getprofile() is not None:
        sys.setprofile(None)
    profile_index = profile_count + 1
    WORKER_PROFILE_CHUNKS_WRITTEN_BY_KIND[kind] = profile_index
    profile_path = (
        Path(WORKER_PROFILE_DIR)
        / f"worker_{os.getpid()}_{kind}_chunk_{profile_index:03d}_tasks_{len(tasks)}.prof"
    )
    profiler = cProfile.Profile()
    result = profiler.runcall(func, tasks)
    profiler.dump_stats(str(profile_path))
    return result


def evaluate_visible_hand_chunk(tasks: list[HandTask]) -> list[dict[str, Any]]:
    def run(rows: list[HandTask]) -> list[dict[str, Any]]:
        rust_rows = batch_visible_hands(rows)
        if rust_rows is None:
            return [evaluate_visible_hand(task) for task in rows]
        out: list[dict[str, Any]] = []
        for task, row in zip(rows, rust_rows, strict=True):
            if row.get("unsupported"):
                out.append(evaluate_visible_hand(task))
            else:
                out.append(row)
        return out

    return maybe_profile_worker_chunk("visible", run, tasks)


def evaluate_actual_keep_chunk(tasks: list[ActualTask]) -> list[dict[str, Any]]:
    def run(rows: list[ActualTask]) -> list[dict[str, Any]]:
        initial_results = batch_initial_actual_solves(rows)
        if initial_results is None:
            return [evaluate_actual_keep(task) for task in rows]
        return [
            evaluate_actual_keep_with_initial(task, initial)
            for task, initial in zip(rows, initial_results, strict=True)
        ]

    return maybe_profile_worker_chunk("actual", run, tasks)


def evaluate_tasks(
    tasks: list[HandTask],
    *,
    target_mode: str,
    deck_json: str | None,
    state_limit: int,
    samples_per_bottom: int,
    validation_samples: int,
    cap_weight: float,
    workers: int,
    trace_lines: bool = False,
    engine_success_policy: str = "count",
    remora_upkeep_payments: int = 2,
    gamble_mode: str = "off",
    actual_rerun_state_limit: int = 0,
    counterfactual_line_cards: bool = False,
    counterfactual_state_limit: int = 0,
    chunks_per_worker: int = 16,
    pool: ProcessPoolExecutor | None = None,
    worker_profile_dir: str | None = None,
    worker_profile_chunks_per_process: int = 0,
    action_sort: bool = True,
    heuristic_bottom_order: bool = False,
) -> dict[str, dict[str, Any]]:
    if not tasks:
        return {}
    unique: dict[str, HandTask] = {}
    for task in tasks:
        unique.setdefault(task.key, task)
    unique_tasks = list(unique.values())
    if workers <= 1:
        worker_init(target_mode, deck_json, state_limit, samples_per_bottom, validation_samples, cap_weight, trace_lines, engine_success_policy, remora_upkeep_payments, gamble_mode, actual_rerun_state_limit, counterfactual_line_cards, counterfactual_state_limit, worker_profile_dir, worker_profile_chunks_per_process, action_sort, heuristic_bottom_order)
        return {
            result["key"]: result
            for result in (evaluate_visible_hand(task) for task in unique_tasks)
        }
    out: dict[str, dict[str, Any]] = {}
    task_chunks = chunks(unique_tasks, chunk_size_for(len(unique_tasks), workers, chunks_per_worker))
    def collect(worker_pool: ProcessPoolExecutor) -> dict[str, dict[str, Any]]:
        futures = [worker_pool.submit(evaluate_visible_hand_chunk, chunk) for chunk in task_chunks]
        done = 0
        next_report = 100
        for future in as_completed(futures):
            for result in future.result():
                out[result["key"]] = result
                done += 1
            if done >= next_report or done == len(unique_tasks):
                print(f"visible EV tasks {done}/{len(unique_tasks)}", flush=True)
                while next_report <= done:
                    next_report += 100
        return out

    if pool is not None:
        return collect(pool)
    with create_worker_pool(
        workers=workers,
        target_mode=target_mode,
        deck_json=deck_json,
        state_limit=state_limit,
        samples_per_bottom=samples_per_bottom,
        validation_samples=validation_samples,
        cap_weight=cap_weight,
        trace_lines=trace_lines,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=remora_upkeep_payments,
        gamble_mode=gamble_mode,
        actual_rerun_state_limit=actual_rerun_state_limit,
        counterfactual_line_cards=counterfactual_line_cards,
        counterfactual_state_limit=counterfactual_state_limit,
        worker_profile_dir=worker_profile_dir,
        worker_profile_chunks_per_process=worker_profile_chunks_per_process,
        action_sort=action_sort,
        heuristic_bottom_order=heuristic_bottom_order,
    ) as owned_pool:
        return collect(owned_pool)


def evaluate_actual_tasks(
    tasks: list[ActualTask],
    *,
    target_mode: str,
    deck_json: str | None,
    state_limit: int,
    samples_per_bottom: int,
    validation_samples: int,
    cap_weight: float,
    workers: int,
    trace_lines: bool,
    engine_success_policy: str = "count",
    remora_upkeep_payments: int = 2,
    gamble_mode: str = "off",
    actual_rerun_state_limit: int = 0,
    counterfactual_line_cards: bool = False,
    counterfactual_state_limit: int = 0,
    chunks_per_worker: int = 16,
    pool: ProcessPoolExecutor | None = None,
    worker_profile_dir: str | None = None,
    worker_profile_chunks_per_process: int = 0,
    action_sort: bool = True,
) -> list[dict[str, Any]]:
    if not tasks:
        return []
    if workers <= 1:
        worker_init(target_mode, deck_json, state_limit, samples_per_bottom, validation_samples, cap_weight, trace_lines, engine_success_policy, remora_upkeep_payments, gamble_mode, actual_rerun_state_limit, counterfactual_line_cards, counterfactual_state_limit, worker_profile_dir, worker_profile_chunks_per_process, action_sort)
        return evaluate_actual_keep_chunk(tasks)
    out: list[dict[str, Any]] = []
    task_chunks = chunks(tasks, chunk_size_for(len(tasks), workers, chunks_per_worker))
    def collect(worker_pool: ProcessPoolExecutor) -> list[dict[str, Any]]:
        futures = [worker_pool.submit(evaluate_actual_keep_chunk, chunk) for chunk in task_chunks]
        done = 0
        next_report = 50
        for future in as_completed(futures):
            rows = future.result()
            out.extend(rows)
            done += len(rows)
            if done >= next_report or done == len(tasks):
                print(f"actual kept tasks {done}/{len(tasks)}", flush=True)
                while next_report <= done:
                    next_report += 50
        return out

    if pool is not None:
        return collect(pool)
    with create_worker_pool(
        workers=workers,
        target_mode=target_mode,
        deck_json=deck_json,
        state_limit=state_limit,
        samples_per_bottom=samples_per_bottom,
        validation_samples=validation_samples,
        cap_weight=cap_weight,
        trace_lines=trace_lines,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=remora_upkeep_payments,
        gamble_mode=gamble_mode,
        actual_rerun_state_limit=actual_rerun_state_limit,
        counterfactual_line_cards=counterfactual_line_cards,
        counterfactual_state_limit=counterfactual_state_limit,
        worker_profile_dir=worker_profile_dir,
        worker_profile_chunks_per_process=worker_profile_chunks_per_process,
        action_sort=action_sort,
    ) as owned_pool:
        return collect(owned_pool)


def random_order(deck: tuple[str, ...], rng: random.Random) -> list[str]:
    order = list(deck)
    rng.shuffle(order)
    return order


def paired_stage_order(deck: tuple[str, ...], seed: int, game_index: int, stage: int) -> list[str]:
    return random_order(deck, random.Random(stable_seed(seed, "policy-stage-order", game_index, stage)))


def make_threshold_tasks(
    deck: tuple[str, ...],
    *,
    hands_per_stage: int,
    seed: int,
    gemstone_live_flags: tuple[bool, ...],
    normalize_no_caverns_gemstone_key: bool = False,
) -> tuple[list[HandTask], dict[str, dict[int, list[str]]]]:
    rng = random.Random(seed)
    tasks: list[HandTask] = []
    stage_keys: dict[str, dict[int, list[str]]] = {gemstone_key(flag): {} for flag in gemstone_live_flags}
    for stage, bottom_count in enumerate(COMMANDER_MULLIGAN_BOTTOMS):
        keys_by_flag: dict[bool, list[str]] = {flag: [] for flag in gemstone_live_flags}
        for i in range(hands_per_stage):
            order = random_order(deck, rng)
            hand = tuple(sorted(order[:7]))
            for flag in gemstone_live_flags:
                key = hand_key(
                    hand,
                    bottom_count,
                    flag,
                    normalize_no_caverns_gemstone_key=normalize_no_caverns_gemstone_key,
                )
                keys_by_flag[flag].append(key)
                tasks.append(HandTask(key=key, hand=hand, bottom_count=bottom_count, seed=stable_seed(seed, "threshold", stage, i, key), gemstone_live=flag))
        for flag, keys in keys_by_flag.items():
            stage_keys[gemstone_key(flag)][stage] = keys
    return tasks, stage_keys


def compute_thresholds(stage_keys: dict[int, list[str]], evs: dict[str, dict[str, Any]]) -> tuple[list[float], list[dict[str, Any]]]:
    thresholds = [0.0 for _ in COMMANDER_MULLIGAN_BOTTOMS]
    stage_values = [0.0 for _ in COMMANDER_MULLIGAN_BOTTOMS]
    rows: list[dict[str, Any]] = []
    future_value = 0.0
    for stage in reversed(range(len(COMMANDER_MULLIGAN_BOTTOMS))):
        values = [evs[key]["score_ev"] for key in stage_keys[stage]]
        raw_mean = sum(values) / len(values)
        thresholds[stage] = future_value
        if stage == len(COMMANDER_MULLIGAN_BOTTOMS) - 1:
            stage_value = raw_mean
        else:
            stage_value = sum(max(value, future_value) for value in values) / len(values)
        stage_values[stage] = stage_value
        rows.append(
            {
                "stage": stage,
                "bottom_count": COMMANDER_MULLIGAN_BOTTOMS[stage],
                "future_keep_threshold": future_value,
                "raw_mean_visible_ev": raw_mean,
                "stage_value": stage_value,
                "hands": len(values),
            }
        )
        future_value = stage_value
    rows.reverse()
    return thresholds, rows


def evaluate_policy(
    deck: tuple[str, ...],
    thresholds_by_gemstone_live: dict[bool, list[float]],
    *,
    games: int,
    seed: int,
    gemstone_caverns_live_rate: float,
    target_mode: str,
    deck_json: str | None,
    state_limit: int,
    samples_per_bottom: int,
    validation_samples: int,
    cap_weight: float,
    workers: int,
    trace_lines: bool,
    engine_success_policy: str,
    remora_upkeep_payments: int,
    gamble_mode: str,
    actual_rerun_state_limit: int,
    counterfactual_line_cards: bool,
    counterfactual_state_limit: int,
    include_game_records: bool,
    compact_game_records: bool = False,
    normalize_no_caverns_gemstone_key: bool = False,
    paired_stage_orders: bool = False,
    adaptive_threshold_sampling: bool = False,
    reuse_worker_pool: bool = False,
    chunks_per_worker: int = 16,
    checkpoint_out: str | None = None,
    worker_profile_dir: str | None = None,
    worker_profile_chunks_per_process: int = 0,
    worker_pool: ProcessPoolExecutor | None = None,
    action_sort: bool = True,
    heuristic_bottom_order: bool = False,
) -> dict[str, Any]:
    rng = random.Random(seed)
    gemstone_rng = random.Random(stable_seed(seed, "gemstone-caverns-live"))
    gemstone_live_by_game = {
        game_index: gemstone_rng.random() < gemstone_caverns_live_rate
        for game_index in range(games)
    }
    active = list(range(games))
    results: dict[int, dict[str, Any]] = {}
    final_keep_by_game: dict[int, tuple[str, ...]] = {}
    kept_visible_by_game: dict[int, tuple[str, ...]] = {}
    bottom_by_game: dict[int, tuple[str, ...]] = {}
    decision_records: list[dict[str, Any]] = []
    all_visible_evs: dict[str, dict[str, Any]] = {}
    generated_orders: dict[tuple[int, int], list[str]] = {}
    visible_hand_card_counts: Counter[str] = Counter()
    visible_hand_card_counts_by_stage: dict[str, Counter[str]] = {}
    mulliganed_hand_card_counts: Counter[str] = Counter()
    kept_visible_hand_card_counts: Counter[str] = Counter()
    bottomed_card_counts: Counter[str] = Counter()
    kept_hand_card_counts: Counter[str] = Counter()
    visible_hand_total = 0
    mulliganed_hand_total = 0
    kept_hand_total = 0
    policy_stage_timing: list[dict[str, Any]] = []
    normalized_no_caverns_key_total = 0
    shared_worker_pool: ProcessPoolExecutor | None = worker_pool
    owns_shared_worker_pool = False
    if shared_worker_pool is None and reuse_worker_pool and workers > 1:
        shared_worker_pool = create_worker_pool(
            workers=workers,
            target_mode=target_mode,
            deck_json=deck_json,
            state_limit=state_limit,
            samples_per_bottom=samples_per_bottom,
            validation_samples=validation_samples,
            cap_weight=cap_weight,
            trace_lines=trace_lines,
            engine_success_policy=engine_success_policy,
            remora_upkeep_payments=remora_upkeep_payments,
            gamble_mode=gamble_mode,
            actual_rerun_state_limit=actual_rerun_state_limit,
            counterfactual_line_cards=counterfactual_line_cards,
            counterfactual_state_limit=counterfactual_state_limit,
            worker_profile_dir=worker_profile_dir,
            worker_profile_chunks_per_process=worker_profile_chunks_per_process,
            action_sort=action_sort,
            heuristic_bottom_order=heuristic_bottom_order,
        )
        owns_shared_worker_pool = True

    def game_record_rows() -> list[dict[str, Any]]:
        if compact_game_records:
            return [
                {
                    "game_index": game_index,
                    "stage": row["stage"],
                    "bottom_count": row["bottom_count"],
                    "gemstone_caverns_live": row["gemstone_caverns_live"],
                    "hit": row["hit"],
                    "turn": row["turn"],
                    "engine_label": row.get("engine_label"),
                    "capped": row["capped"],
                    "initial_capped": row.get("initial_capped"),
                    "cap_rerun_attempted": row.get("cap_rerun_attempted"),
                }
                for game_index, row in sorted(results.items())
            ]
        return [
            {
                **row,
                "visible_hand": list(kept_visible_by_game.get(game_index, ())),
                "best_bottom": list(bottom_by_game.get(game_index, ())),
                "final_keep": list(final_keep_by_game.get(game_index, ())),
            }
            for game_index, row in sorted(results.items())
        ]

    def write_stage_checkpoint(completed_stage: int, active_remaining: list[int]) -> None:
        if not checkpoint_out:
            return
        checkpoint_path = Path(checkpoint_out)
        checkpoint_path.parent.mkdir(parents=True, exist_ok=True)
        checkpoint_payload = {
            "checkpoint_type": "rhystic_belief_mulligan_policy_stage",
            "target": target_mode,
            "deck_json": deck_json,
            "games": games,
            "seed": seed,
            "completed_stage": completed_stage,
            "completed_games": len(results),
            "active_remaining_games": list(active_remaining),
            "thresholds_by_gemstone_caverns_live": {
                gemstone_key(flag): values
                for flag, values in thresholds_by_gemstone_live.items()
            },
            "state_limit": state_limit,
            "samples_per_bottom": samples_per_bottom,
            "validation_samples": validation_samples,
            "workers": workers,
            "chunks_per_worker": chunks_per_worker,
            "trace_lines": trace_lines,
            "engine_success_policy": engine_success_policy,
            "remora_upkeep_payments": remora_upkeep_payments,
            "gamble_mode": gamble_mode,
            "actual_rerun_state_limit": actual_rerun_state_limit,
            "counterfactual_line_cards": counterfactual_line_cards,
            "counterfactual_state_limit": counterfactual_state_limit,
            "normalize_no_caverns_gemstone_key": normalize_no_caverns_gemstone_key,
            "paired_stage_orders": paired_stage_orders,
            "adaptive_threshold_sampling": adaptive_threshold_sampling,
            "reuse_worker_pool": reuse_worker_pool,
            "worker_profile_dir": worker_profile_dir,
            "worker_profile_chunks_per_process": worker_profile_chunks_per_process,
            "action_sort": action_sort,
            "heuristic_bottom_order": heuristic_bottom_order,
            "policy_stage_timing": policy_stage_timing,
            "game_records": game_record_rows(),
            "mulligan_decision_records": sorted(
                decision_records,
                key=lambda item: (item["game_index"], item["stage"]),
            ),
        }
        tmp_path = checkpoint_path.with_suffix(checkpoint_path.suffix + ".tmp")
        tmp_path.write_text(json.dumps(checkpoint_payload, indent=2, sort_keys=True))
        tmp_path.replace(checkpoint_path)

    for stage, bottom_count in enumerate(COMMANDER_MULLIGAN_BOTTOMS):
        if not active:
            break
        stage_started = time.time()
        print(f"policy stage {stage} bottom {bottom_count} active {len(active)}", flush=True)
        tasks: list[HandTask] = []
        stage_order: dict[int, list[str]] = {}
        for game_index in active:
            order = (
                paired_stage_order(deck, seed, game_index, stage)
                if paired_stage_orders
                else random_order(deck, rng)
            )
            generated_orders[(game_index, stage)] = order
            stage_order[game_index] = order
            hand = tuple(sorted(order[:7]))
            gemstone_live = gemstone_live_by_game[game_index]
            normalized_no_caverns_key = bool(
                normalize_no_caverns_gemstone_key
                and gemstone_live
                and not has_gemstone_caverns_alias(hand)
            )
            normalized_no_caverns_key_total += int(normalized_no_caverns_key)
            key = hand_key(
                hand,
                bottom_count,
                gemstone_live,
                normalize_no_caverns_gemstone_key=normalize_no_caverns_gemstone_key,
            )
            visible_hand_card_counts.update(set(hand))
            visible_hand_card_counts_by_stage.setdefault(str(stage), Counter()).update(set(hand))
            visible_hand_total += 1
            if key not in all_visible_evs:
                tasks.append(
                    HandTask(
                        key=key,
                        hand=hand,
                        bottom_count=bottom_count,
                        seed=stable_seed(seed, "eval", game_index, stage, key),
                        gemstone_live=gemstone_live,
                        keep_threshold=thresholds_by_gemstone_live[gemstone_live][stage],
                        force_keep=stage == len(COMMANDER_MULLIGAN_BOTTOMS) - 1,
                        adaptive_threshold_sampling=adaptive_threshold_sampling,
                    )
                )
        visible_eval_started = time.time()
        stage_evs = evaluate_tasks(
            tasks,
            target_mode=target_mode,
            deck_json=deck_json,
            state_limit=state_limit,
            samples_per_bottom=samples_per_bottom,
            validation_samples=validation_samples,
            cap_weight=cap_weight,
            workers=workers,
            trace_lines=False,
            engine_success_policy=engine_success_policy,
            remora_upkeep_payments=remora_upkeep_payments,
            gamble_mode=gamble_mode,
            counterfactual_line_cards=False,
            counterfactual_state_limit=0,
            chunks_per_worker=chunks_per_worker,
            pool=shared_worker_pool,
            worker_profile_dir=worker_profile_dir,
            worker_profile_chunks_per_process=worker_profile_chunks_per_process,
            action_sort=action_sort,
            heuristic_bottom_order=heuristic_bottom_order,
        )
        visible_eval_elapsed = time.time() - visible_eval_started
        all_visible_evs.update(stage_evs)

        next_active: list[int] = []
        actual_tasks: list[ActualTask] = []
        for game_index in active:
            order = stage_order[game_index]
            hand = tuple(sorted(order[:7]))
            gemstone_live = gemstone_live_by_game[game_index]
            key = hand_key(
                hand,
                bottom_count,
                gemstone_live,
                normalize_no_caverns_gemstone_key=normalize_no_caverns_gemstone_key,
            )
            ev_row = all_visible_evs[key]
            keep_threshold = thresholds_by_gemstone_live[gemstone_live][stage]
            keep_now = stage == len(COMMANDER_MULLIGAN_BOTTOMS) - 1 or ev_row["score_ev"] >= keep_threshold
            if include_game_records:
                decision_records.append(
                    {
                        "game_index": game_index,
                        "stage": stage,
                        "bottom_count": bottom_count,
                        "gemstone_caverns_live": gemstone_live,
                        "visible_hand": list(hand),
                        "score_ev": ev_row["score_ev"],
                        "ev": ev_row["ev"],
                        "upper_ev": ev_row["upper_ev"],
                        "keep_threshold": keep_threshold,
                        "keep": keep_now,
                        "best_bottom": list(ev_row["best_bottom"]),
                        "deterministic": ev_row.get("deterministic"),
                        "selection_hits": ev_row.get("selection_hits"),
                        "selection_cap_misses": ev_row.get("selection_cap_misses"),
                        "validation_hits": ev_row.get("hits"),
                        "validation_cap_misses": ev_row.get("cap_misses"),
                        "validation_samples_used": ev_row.get("validation_samples_used"),
                        "adaptive_threshold_resolved": ev_row.get("adaptive_threshold_resolved"),
                        "adaptive_threshold_resolution": ev_row.get("adaptive_threshold_resolution"),
                        "adaptive_validation_samples_saved": ev_row.get("adaptive_validation_samples_saved"),
                        "solver_calls": ev_row.get("solver_calls"),
                        "bottom_candidates_checked": ev_row.get("bottom_candidates_checked"),
                        "selection_early_stopped": ev_row.get("selection_early_stopped"),
                        "selection_pruned_candidates": ev_row.get("selection_pruned_candidates"),
                        "selection_pruned_samples": ev_row.get("selection_pruned_samples"),
                        "selection_skipped_single_candidate_samples": ev_row.get("selection_skipped_single_candidate_samples"),
                        "normalized_no_caverns_gemstone_key": bool(
                            normalize_no_caverns_gemstone_key
                            and gemstone_live
                            and not has_gemstone_caverns_alias(hand)
                        ),
                    }
                )
            if not keep_now:
                mulliganed_hand_card_counts.update(set(hand))
                mulliganed_hand_total += 1
                next_active.append(game_index)
                continue
            bottom = tuple(ev_row["best_bottom"])
            keep = remove_bottom(hand, bottom)
            library = tuple(order[7:] + list(bottom))
            final_keep_by_game[game_index] = keep
            kept_visible_by_game[game_index] = hand
            bottom_by_game[game_index] = bottom
            kept_visible_hand_card_counts.update(set(hand))
            bottomed_card_counts.update(bottom)
            kept_hand_card_counts.update(set(keep))
            kept_hand_total += 1
            actual_seed = stable_seed(seed, "actual", game_index, stage, key)
            actual_tasks.append(
                ActualTask(
                    game_index=game_index,
                    stage=stage,
                    bottom_count=bottom_count,
                    keep=keep,
                    library=library,
                    gemstone_live=gemstone_live,
                    seed=actual_seed,
                )
            )

        actual_eval_started = time.time()
        actual_rows = evaluate_actual_tasks(
            actual_tasks,
            target_mode=target_mode,
            deck_json=deck_json,
            state_limit=state_limit,
            samples_per_bottom=samples_per_bottom,
            validation_samples=validation_samples,
            cap_weight=cap_weight,
            workers=workers,
            trace_lines=trace_lines,
            engine_success_policy=engine_success_policy,
            remora_upkeep_payments=remora_upkeep_payments,
            gamble_mode=gamble_mode,
            actual_rerun_state_limit=actual_rerun_state_limit,
            counterfactual_line_cards=counterfactual_line_cards,
            counterfactual_state_limit=counterfactual_state_limit,
            chunks_per_worker=chunks_per_worker,
            pool=shared_worker_pool,
            worker_profile_dir=worker_profile_dir,
            worker_profile_chunks_per_process=worker_profile_chunks_per_process,
            action_sort=action_sort,
        )
        actual_eval_elapsed = time.time() - actual_eval_started
        for row in actual_rows:
            results[row["game_index"]] = row
        stage_elapsed = time.time() - stage_started
        stage_solver_calls = sum(int(row.get("solver_calls") or 0) for row in stage_evs.values())
        stage_bottom_candidates_checked = sum(int(row.get("bottom_candidates_checked") or 0) for row in stage_evs.values())
        stage_pruned_candidates = sum(int(row.get("selection_pruned_candidates") or 0) for row in stage_evs.values())
        stage_pruned_samples = sum(int(row.get("selection_pruned_samples") or 0) for row in stage_evs.values())
        stage_skipped_single_candidate_samples = sum(int(row.get("selection_skipped_single_candidate_samples") or 0) for row in stage_evs.values())
        stage_validation_samples_used = sum(int(row.get("validation_samples_used") or 0) for row in stage_evs.values())
        stage_validation_sample_budget = sum(int(row.get("samples") or 0) for row in stage_evs.values())
        stage_adaptive_validation_samples_saved = sum(int(row.get("adaptive_validation_samples_saved") or 0) for row in stage_evs.values())
        stage_adaptive_threshold_resolved = sum(1 for row in stage_evs.values() if row.get("adaptive_threshold_resolved"))
        stage_normalized_no_caverns_keys = sum(
            1
            for game_index in active
            if normalize_no_caverns_gemstone_key
            and gemstone_live_by_game[game_index]
            and not has_gemstone_caverns_alias(tuple(sorted(stage_order[game_index][:7])))
        )
        policy_stage_timing.append(
            {
                "stage": stage,
                "bottom_count": bottom_count,
                "active_games": len(active),
                "visible_tasks": len(tasks),
                "visible_eval_elapsed_seconds": visible_eval_elapsed,
                "actual_kept_tasks": len(actual_tasks),
                "actual_eval_elapsed_seconds": actual_eval_elapsed,
                "stage_elapsed_seconds": stage_elapsed,
                "stage_solver_calls": stage_solver_calls,
                "bottom_candidates_checked": stage_bottom_candidates_checked,
                "selection_early_stops": sum(1 for row in stage_evs.values() if row.get("selection_early_stopped")),
                "selection_pruned_candidates": stage_pruned_candidates,
                "selection_pruned_samples": stage_pruned_samples,
                "selection_skipped_single_candidate_samples": stage_skipped_single_candidate_samples,
                "validation_samples_used": stage_validation_samples_used,
                "validation_sample_budget": stage_validation_sample_budget,
                "adaptive_threshold_resolutions": stage_adaptive_threshold_resolved,
                "adaptive_validation_samples_saved": stage_adaptive_validation_samples_saved,
                "normalized_no_caverns_gemstone_keys": stage_normalized_no_caverns_keys,
            }
        )
        print(
            f"policy stage {stage} kept {len(actual_tasks)} mulliganed {len(next_active)} elapsed {stage_elapsed:.1f}s",
            flush=True,
        )
        write_stage_checkpoint(stage, next_active)
        active = next_active

    if owns_shared_worker_pool and shared_worker_pool is not None:
        shared_worker_pool.shutdown()

    if len(results) != games:
        raise RuntimeError(f"Only resolved {len(results)} of {games} games")

    winning_kept_hand_card_counts: Counter[str] = Counter()
    losing_kept_hand_card_counts: Counter[str] = Counter()
    winning_kept_visible_card_counts: Counter[str] = Counter()
    losing_kept_visible_card_counts: Counter[str] = Counter()
    winning_kept_hand_total = 0
    losing_kept_hand_total = 0
    for game_index, row in results.items():
        keep = final_keep_by_game.get(game_index, ())
        visible = kept_visible_by_game.get(game_index, ())
        if row["hit"]:
            winning_kept_hand_card_counts.update(set(keep))
            winning_kept_visible_card_counts.update(set(visible))
            winning_kept_hand_total += 1
        else:
            losing_kept_hand_card_counts.update(set(keep))
            losing_kept_visible_card_counts.update(set(visible))
            losing_kept_hand_total += 1

    successes = sum(1 for row in results.values() if row["hit"])
    cap_misses = sum(1 for row in results.values() if row["capped"] and not row["hit"])
    initial_cap_misses = sum(1 for row in results.values() if row.get("initial_capped") and not row.get("initial_hit"))
    actual_cap_rerun_attempts = sum(1 for row in results.values() if row.get("cap_rerun_attempted"))
    actual_cap_rerun_successes = sum(1 for row in results.values() if row.get("cap_rerun_attempted") and row.get("hit"))
    actual_cap_rerun_remaining_caps = sum(
        1
        for row in results.values()
        if row.get("cap_rerun_attempted") and row.get("capped") and not row.get("hit")
    )
    traced_successes = sum(1 for row in results.values() if row["hit"] and row.get("trace_found") is True)
    trace_failed_successes = sum(1 for row in results.values() if row["hit"] and row.get("trace_found") is False)
    nick_fury_successes = sum(1 for row in results.values() if row["hit"] and row.get("line_casts_nick_fury") is True)
    line_card_counts: Counter[str] = Counter()
    line_card_counts_excluding_costs: Counter[str] = Counter()
    counterfactual_test_counts: Counter[str] = Counter()
    counterfactual_loss_counts: Counter[str] = Counter()
    counterfactual_survive_counts: Counter[str] = Counter()
    counterfactual_capped_counts: Counter[str] = Counter()
    event_counts: dict[str, Counter[str]] = {
        "casts": Counter(),
        "engine_casts": Counter(),
        "tutor_targets": Counter(),
        "cost_cards": Counter(),
        "played": Counter(),
        "activated": Counter(),
        "mana_sources": Counter(),
    }
    for row in results.values():
        if row["hit"] and row.get("line_cards"):
            line_card_counts.update(set(row["line_cards"]))
        if row["hit"] and row.get("line_events"):
            non_cost_cards: set[str] = set()
            for bucket, cards in row["line_events"].items():
                event_counts.setdefault(bucket, Counter()).update(set(cards))
                if bucket != "cost_cards":
                    non_cost_cards.update(cards)
            non_cost_cards.discard("Treasure")
            line_card_counts_excluding_costs.update(non_cost_cards)
        if row["hit"] and row.get("counterfactual_line_card_results"):
            for card, outcome in row["counterfactual_line_card_results"].items():
                counterfactual_test_counts[card] += 1
                if outcome == "loss":
                    counterfactual_loss_counts[card] += 1
                elif outcome == "survives":
                    counterfactual_survive_counts[card] += 1
                elif outcome == "capped":
                    counterfactual_capped_counts[card] += 1
    all_cards = sorted(set(deck))
    keep_counts = Counter(str(row["bottom_count"]) for row in results.values())
    stage_counts = Counter(str(row["stage"]) for row in results.values())
    turn_counts = Counter(str(row["turn"] or "miss") for row in results.values())
    gemstone_counts = Counter(gemstone_key(bool(row["gemstone_caverns_live"])) for row in results.values())
    gemstone_successes = Counter(gemstone_key(bool(row["gemstone_caverns_live"])) for row in results.values() if row["hit"])
    actual_elapsed_values = [float(row.get("actual_elapsed_seconds") or 0.0) for row in results.values()]
    cap_rerun_elapsed_values = [float(row.get("cap_rerun_elapsed_seconds") or 0.0) for row in results.values()]
    initial_solve_elapsed_values = [float(row.get("initial_solve_elapsed_seconds") or 0.0) for row in results.values()]
    trace_elapsed_values = [float(row.get("trace_elapsed_seconds") or 0.0) for row in results.values()]
    counterfactual_elapsed_values = [float(row.get("counterfactual_elapsed_seconds") or 0.0) for row in results.values()]
    slowest_actual_tasks = sorted(
        (
            {
                "game_index": row["game_index"],
                "stage": row["stage"],
                "bottom_count": row["bottom_count"],
                "hit": row["hit"],
                "turn": row["turn"],
                "capped": row["capped"],
                "initial_capped": row.get("initial_capped"),
                "cap_rerun_attempted": row.get("cap_rerun_attempted"),
                "actual_elapsed_seconds": row.get("actual_elapsed_seconds", 0.0),
                "initial_solve_elapsed_seconds": row.get("initial_solve_elapsed_seconds", 0.0),
                "cap_rerun_elapsed_seconds": row.get("cap_rerun_elapsed_seconds", 0.0),
                "trace_elapsed_seconds": row.get("trace_elapsed_seconds", 0.0),
                "counterfactual_elapsed_seconds": row.get("counterfactual_elapsed_seconds", 0.0),
            }
            for row in results.values()
        ),
        key=lambda item: float(item["actual_elapsed_seconds"]),
        reverse=True,
    )[:10]
    payload = {
        "games": games,
        "successes": successes,
        "success_rate": successes / games,
        "cap_misses": cap_misses,
        "initial_cap_misses_before_actual_rerun": initial_cap_misses,
        "actual_cap_rerun_state_limit": actual_rerun_state_limit,
        "actual_cap_rerun_attempts": actual_cap_rerun_attempts,
        "actual_cap_rerun_successes": actual_cap_rerun_successes,
        "actual_cap_rerun_remaining_caps": actual_cap_rerun_remaining_caps,
        "upper_success_rate_if_caps_hit": (successes + cap_misses) / games,
        "wilson95": wilson(successes, games),
        "keep_counts_by_bottom": dict(sorted(keep_counts.items())),
        "keep_counts_by_stage": dict(sorted(stage_counts.items())),
        "turn_counts": dict(sorted(turn_counts.items())),
        "visible_ev_cache_size": len(all_visible_evs),
        "policy_stage_timing": policy_stage_timing,
        "actual_task_timing": {
            "total_elapsed_seconds": sum(actual_elapsed_values),
            "mean_elapsed_seconds": sum(actual_elapsed_values) / len(actual_elapsed_values) if actual_elapsed_values else 0.0,
            "max_elapsed_seconds": max(actual_elapsed_values) if actual_elapsed_values else 0.0,
            "initial_solve_total_elapsed_seconds": sum(initial_solve_elapsed_values),
            "cap_rerun_total_elapsed_seconds": sum(cap_rerun_elapsed_values),
            "trace_total_elapsed_seconds": sum(trace_elapsed_values),
            "counterfactual_total_elapsed_seconds": sum(counterfactual_elapsed_values),
            "slowest_tasks": slowest_actual_tasks,
        },
        "engine_success_policy": engine_success_policy,
        "remora_upkeep_payments": remora_upkeep_payments,
        "gamble_mode": gamble_mode,
        "gemstone_caverns_live_rate": gemstone_caverns_live_rate,
        "gemstone_caverns_live_counts": dict(sorted(gemstone_counts.items())),
        "success_rate_by_gemstone_caverns_live": {
            key: gemstone_successes.get(key, 0) / count if count else 0.0
            for key, count in sorted(gemstone_counts.items())
        },
        "appearance_denominators": {
            "visible_hands": visible_hand_total,
            "mulliganed_hands": mulliganed_hand_total,
            "kept_visible_hands": kept_hand_total,
            "kept_hands": kept_hand_total,
            "winning_kept_hands": winning_kept_hand_total,
            "losing_kept_hands": losing_kept_hand_total,
        },
        "appearance_card_counts": {
            "visible_hands": dict(sorted(visible_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "visible_hands_by_stage": {
                stage: dict(sorted(counter.items(), key=lambda item: (-item[1], item[0])))
                for stage, counter in sorted(visible_hand_card_counts_by_stage.items())
            },
            "mulliganed_hands": dict(sorted(mulliganed_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "kept_visible_hands": dict(sorted(kept_visible_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "bottomed_cards": dict(sorted(bottomed_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "kept_hands": dict(sorted(kept_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "winning_kept_hands": dict(sorted(winning_kept_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "losing_kept_hands": dict(sorted(losing_kept_hand_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "winning_kept_visible_hands": dict(sorted(winning_kept_visible_card_counts.items(), key=lambda item: (-item[1], item[0]))),
            "losing_kept_visible_hands": dict(sorted(losing_kept_visible_card_counts.items(), key=lambda item: (-item[1], item[0]))),
        },
        "appearance_card_rates": {
            "visible_hands": {
                card: visible_hand_card_counts.get(card, 0) / visible_hand_total if visible_hand_total else 0.0
                for card in all_cards
            },
            "mulliganed_hands": {
                card: mulliganed_hand_card_counts.get(card, 0) / mulliganed_hand_total if mulliganed_hand_total else 0.0
                for card in all_cards
            },
            "kept_visible_hands": {
                card: kept_visible_hand_card_counts.get(card, 0) / kept_hand_total if kept_hand_total else 0.0
                for card in all_cards
            },
            "bottomed_per_kept_visible": {
                card: bottomed_card_counts.get(card, 0) / kept_visible_hand_card_counts.get(card, 1)
                for card in all_cards
                if kept_visible_hand_card_counts.get(card, 0)
            },
            "kept_hands": {
                card: kept_hand_card_counts.get(card, 0) / kept_hand_total if kept_hand_total else 0.0
                for card in all_cards
            },
            "winning_kept_hands": {
                card: winning_kept_hand_card_counts.get(card, 0) / winning_kept_hand_total if winning_kept_hand_total else 0.0
                for card in all_cards
            },
            "losing_kept_hands": {
                card: losing_kept_hand_card_counts.get(card, 0) / losing_kept_hand_total if losing_kept_hand_total else 0.0
                for card in all_cards
            },
            "keep_rate_when_seen": {
                card: kept_visible_hand_card_counts.get(card, 0) / visible_hand_card_counts.get(card, 1)
                for card in all_cards
                if visible_hand_card_counts.get(card, 0)
            },
            "mulligan_rate_when_seen": {
                card: mulliganed_hand_card_counts.get(card, 0) / visible_hand_card_counts.get(card, 1)
                for card in all_cards
                if visible_hand_card_counts.get(card, 0)
            },
            "win_rate_when_kept": {
                card: winning_kept_hand_card_counts.get(card, 0) / kept_hand_card_counts.get(card, 1)
                for card in all_cards
                if kept_hand_card_counts.get(card, 0)
            },
        },
        "normalize_no_caverns_gemstone_key": normalize_no_caverns_gemstone_key,
        "paired_stage_orders": paired_stage_orders,
        "adaptive_threshold_sampling": adaptive_threshold_sampling,
        "reuse_worker_pool": reuse_worker_pool,
        "action_sort": action_sort,
        "normalized_no_caverns_gemstone_key_count": normalized_no_caverns_key_total,
    }
    if trace_lines:
        payload.update(
            {
                "traced_successes": traced_successes,
                "trace_failed_successes": trace_failed_successes,
                "nick_fury_success_lines": nick_fury_successes,
                "nick_fury_cast_rate_among_successes": nick_fury_successes / successes if successes else 0.0,
                "nick_fury_cast_rate_among_traced_successes": nick_fury_successes / traced_successes if traced_successes else 0.0,
                "nick_fury_cast_rate_all_games": nick_fury_successes / games,
                "line_card_counts": dict(sorted(line_card_counts.items(), key=lambda item: (-item[1], item[0]))),
                "line_card_counts_excluding_costs": dict(
                    sorted(line_card_counts_excluding_costs.items(), key=lambda item: (-item[1], item[0]))
                ),
                "line_card_rates_among_successes": {
                    card: line_card_counts.get(card, 0) / successes if successes else 0.0
                    for card in all_cards
                },
                "line_card_rates_excluding_costs_among_successes": {
                    card: line_card_counts_excluding_costs.get(card, 0) / successes if successes else 0.0
                    for card in all_cards
                },
                "line_event_counts": {
                    bucket: dict(sorted(counter.items(), key=lambda item: (-item[1], item[0])))
                    for bucket, counter in sorted(event_counts.items())
                },
                "line_event_rates_among_successes": {
                    bucket: {
                        card: counter.get(card, 0) / successes if successes else 0.0
                        for card in all_cards
                    }
                    for bucket, counter in sorted(event_counts.items())
                },
                "zero_line_cards": [card for card in all_cards if line_card_counts.get(card, 0) == 0],
                "low_line_cards_le_1pct": [
                    card
                    for card in all_cards
                    if 0 < line_card_counts.get(card, 0) / successes <= 0.01
                ] if successes else [],
                "low_line_cards_le_2pct": [
                    card
                    for card in all_cards
                    if 0 < line_card_counts.get(card, 0) / successes <= 0.02
                ] if successes else [],
            }
        )
        if counterfactual_line_cards:
            payload.update(
                {
                    "counterfactual_line_card_state_limit": counterfactual_state_limit or actual_rerun_state_limit or state_limit,
                    "counterfactual_line_card_test_counts": dict(
                        sorted(counterfactual_test_counts.items(), key=lambda item: (-item[1], item[0]))
                    ),
                    "counterfactual_line_card_loss_counts": dict(
                        sorted(counterfactual_loss_counts.items(), key=lambda item: (-item[1], item[0]))
                    ),
                    "counterfactual_line_card_survive_counts": dict(
                        sorted(counterfactual_survive_counts.items(), key=lambda item: (-item[1], item[0]))
                    ),
                    "counterfactual_line_card_capped_counts": dict(
                        sorted(counterfactual_capped_counts.items(), key=lambda item: (-item[1], item[0]))
                    ),
                    "counterfactual_line_card_loss_rates_when_tested": {
                        card: counterfactual_loss_counts.get(card, 0) / counterfactual_test_counts.get(card, 1)
                        for card in all_cards
                        if counterfactual_test_counts.get(card, 0)
                    },
                    "counterfactual_line_card_loss_rates_among_successes": {
                        card: counterfactual_loss_counts.get(card, 0) / successes if successes else 0.0
                        for card in all_cards
                    },
                }
            )
    if include_game_records:
        payload["game_records"] = game_record_rows()
        payload["mulligan_decision_records"] = sorted(
            decision_records,
            key=lambda item: (item["game_index"], item["stage"]),
        )
    return payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", choices=("rhystic", "engine3", "engine4", "heartwood", "rhystic_heartwood", "rhystic_tithe"), default="rhystic")
    parser.add_argument("--deck-json", default=None)
    parser.add_argument("--threshold-hands", type=int, default=60)
    parser.add_argument("--eval-games", type=int, default=300)
    parser.add_argument("--samples-per-bottom", type=int, default=4)
    parser.add_argument("--validation-samples", type=int, default=4)
    parser.add_argument("--state-limit", type=int, default=20_000)
    parser.add_argument("--workers", type=int, default=max(1, min(8, (multiprocessing.cpu_count() or 2) - 1)))
    parser.add_argument(
        "--chunks-per-worker",
        type=int,
        default=None,
        help="Executor chunks per worker for visible/actual task batches. Defaults to 16 with --reuse-worker-pool, otherwise 8 for <=12 workers and 16 above that.",
    )
    parser.add_argument("--seed", type=int, default=20260625)
    parser.add_argument("--cap-weight", type=float, default=0.0)
    parser.add_argument("--weighted-policy-ev", action="store_true")
    parser.add_argument("--rhystic-t1-weight", type=float, default=1.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=0.75)
    parser.add_argument("--heartwood-t1-weight", type=float, default=0.65)
    parser.add_argument("--heartwood-t2-weight", type=float, default=0.50)
    parser.add_argument("--thresholds-json", default=None)
    parser.add_argument(
        "--threshold-offset",
        type=float,
        default=0.0,
        help="Add this value to every non-final keep threshold after loading/computing thresholds. Positive values make mulligans stricter.",
    )
    parser.add_argument(
        "--threshold-cache-dir",
        default=None,
        help="Optional directory for exact threshold result reuse keyed by deck and threshold-affecting simulator settings.",
    )
    parser.add_argument("--trace-lines", action="store_true")
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.0)
    parser.add_argument("--engine-success-policy", choices=("count", "resilient"), default=None)
    parser.add_argument("--remora-upkeep-payments", type=int, default=2)
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="off")
    parser.add_argument("--rust-action-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_ACTIONS", "off"))
    parser.add_argument("--rust-close-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_CLOSE", "off"))
    parser.add_argument("--rust-solver-mode", choices=("off", "verify", "require"), default=os.environ.get("RHYSTIC_RUST_SOLVER", "off"))
    parser.add_argument("--rust-action-bin", default=os.environ.get("RHYSTIC_RUST_ACTION_BIN"))
    parser.add_argument(
        "--rust-full-sim",
        action="store_true",
        help="Run the threshold and policy simulation through the Rust full-sim engine. Currently supports rhystic_heartwood core-rate runs.",
    )
    parser.add_argument(
        "--rust-full-sim-shards",
        type=int,
        default=1,
        help="Split Rust full-sim policy evaluation into independent shard processes and merge the results. Thresholds are computed or loaded once.",
    )
    parser.add_argument(
        "--rust-full-sim-games-per-shard",
        type=int,
        default=None,
        help="Choose Rust full-sim shard count automatically as ceil(eval_games / value). Overrides --rust-full-sim-shards.",
    )
    parser.add_argument(
        "--rust-full-sim-shard-workers",
        type=int,
        default=None,
        help="Maximum concurrent Rust full-sim shard processes. Defaults to min(shards, --workers).",
    )
    parser.add_argument(
        "--rust-full-sim-internal-shards",
        action="store_true",
        help="Run full-sim policy shards inside one Rust process instead of spawning one Rust process per shard.",
    )
    parser.add_argument("--actual-rerun-state-limit", type=int, default=0)
    parser.add_argument("--counterfactual-line-cards", action="store_true")
    parser.add_argument("--counterfactual-state-limit", type=int, default=0)
    parser.add_argument("--include-game-records", action="store_true")
    parser.add_argument(
        "--include-cap-replay-records",
        action="store_true",
        help="Emit compact solve inputs for remaining capped actual kept hands so they can be replayed at a higher state limit without rerunning all games.",
    )
    parser.add_argument(
        "--include-validation-records",
        action="store_true",
        help="Emit compact kept-hand/library records for every final keep for manual UI validation.",
    )
    parser.add_argument("--compact-game-records", action="store_true")
    parser.add_argument("--normalize-no-caverns-gemstone-key", action="store_true")
    parser.add_argument(
        "--paired-stage-orders",
        action="store_true",
        help="Sample each potential game/stage hand from a stable per-game seed, so variant runs stay paired after divergent mulligan decisions.",
    )
    parser.add_argument(
        "--adaptive-threshold-sampling",
        action="store_true",
        help="During policy evaluation, stop validation rollouts as soon as the keep/mull decision is fixed by the current threshold. Threshold construction remains exact.",
    )
    parser.add_argument(
        "--reuse-worker-pool",
        action="store_true",
        help="Reuse one initialized worker process pool across policy stages instead of creating a new pool for every visible/actual batch.",
    )
    parser.add_argument(
        "--worker-profile-dir",
        default=None,
        help="Optional directory where worker processes write cProfile files for their first profiled chunks.",
    )
    parser.add_argument(
        "--worker-profile-chunks-per-process",
        type=int,
        default=1,
        help="Maximum profiled chunks written by each worker process when --worker-profile-dir is set.",
    )
    parser.add_argument(
        "--worker-start-method",
        choices=tuple(multiprocessing.get_all_start_methods()),
        default=None,
        help="Optional multiprocessing start method override. Normal benchmarks default to fork for speed; profile-all runs can use spawn to avoid inherited cProfile state.",
    )
    parser.add_argument(
        "--disable-action-sort",
        action="store_true",
        help="Experimental: expand generated actions in yield order instead of sorting by heuristic priority.",
    )
    parser.add_argument(
        "--heuristic-bottom-order",
        action="store_true",
        help="Evaluate non-deterministic bottom choices in a keep-quality heuristic order while preserving original deterministic order, shuffle streams, and tie breaks.",
    )
    parser.add_argument("--suppress-json-stdout", action="store_true")
    parser.add_argument("--json-out", default=None)
    parser.add_argument("--checkpoint-out", default=None)
    args = parser.parse_args()
    if not 0.0 <= args.gemstone_caverns_live_rate <= 1.0:
        raise ValueError("--gemstone-caverns-live-rate must be between 0 and 1")
    if args.remora_upkeep_payments < 0:
        raise ValueError("--remora-upkeep-payments must be non-negative")
    if args.actual_rerun_state_limit < 0:
        raise ValueError("--actual-rerun-state-limit must be non-negative")
    if args.counterfactual_state_limit < 0:
        raise ValueError("--counterfactual-state-limit must be non-negative")
    for weight_name in (
        "rhystic_t1_weight",
        "rhystic_t2_weight",
        "heartwood_t1_weight",
        "heartwood_t2_weight",
    ):
        if getattr(args, weight_name) < 0:
            raise ValueError(f"--{weight_name.replace('_', '-')} must be non-negative")
    if args.rust_full_sim_shards <= 0:
        raise ValueError("--rust-full-sim-shards must be positive")
    if args.rust_full_sim_games_per_shard is not None and args.rust_full_sim_games_per_shard <= 0:
        raise ValueError("--rust-full-sim-games-per-shard must be positive")
    if args.rust_full_sim_shard_workers is not None and args.rust_full_sim_shard_workers <= 0:
        raise ValueError("--rust-full-sim-shard-workers must be positive")
    if (args.rust_full_sim_shards > 1 or args.rust_full_sim_games_per_shard is not None or args.rust_full_sim_internal_shards) and not args.rust_full_sim:
        raise ValueError("--rust-full-sim-shards/--rust-full-sim-games-per-shard require --rust-full-sim")
    if args.chunks_per_worker is None:
        args.chunks_per_worker = default_chunks_per_worker(args.workers, args.reuse_worker_pool)
    if args.chunks_per_worker <= 0:
        raise ValueError("--chunks-per-worker must be positive")
    if args.worker_profile_chunks_per_process < 0:
        raise ValueError("--worker-profile-chunks-per-process must be non-negative")
    if args.worker_start_method:
        os.environ["RHYSTIC_WORKER_START_METHOD"] = args.worker_start_method
    if args.counterfactual_line_cards and not args.trace_lines:
        raise ValueError("--counterfactual-line-cards requires --trace-lines")
    engine_success_policy = args.engine_success_policy or ("count" if args.target == "rhystic" else "resilient")
    os.environ["RHYSTIC_RUST_ACTIONS"] = args.rust_action_mode
    os.environ["RHYSTIC_RUST_CLOSE"] = args.rust_close_mode
    os.environ["RHYSTIC_RUST_SOLVER"] = args.rust_solver_mode
    if args.rust_action_bin:
        os.environ["RHYSTIC_RUST_ACTION_BIN"] = args.rust_action_bin

    started = time.time()
    module = load_solver_module()
    deck_key = register_deck(module, args.deck_json)
    configure_target(module, args.target)
    goal = "rhystic" if args.target == "rhystic" else "engine"
    search = module.RhysticSearch(
        deck_key,
        max_turns=2,
        state_limit=args.state_limit,
        goal=goal,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=args.remora_upkeep_payments,
        gamble_mode=args.gamble_mode,
        action_sort=not args.disable_action_sort,
    )
    deck = tuple(search.mainboard)
    commanders = tuple(search.commanders)
    search.close()
    threshold_cache_path: Path | None = None
    threshold_cache_hit = False
    if args.threshold_cache_dir and not args.thresholds_json:
        threshold_cache_root = Path(args.threshold_cache_dir)
        if not threshold_cache_root.is_absolute():
            threshold_cache_root = ROOT / threshold_cache_root
        threshold_cache_root.mkdir(parents=True, exist_ok=True)
        threshold_cache_path = threshold_cache_root / (
            "thresholds_"
            + threshold_cache_key(
                deck_identity_value=deck_identity(args.deck_json, deck, commanders),
                target=args.target,
                threshold_hands=args.threshold_hands,
                samples_per_bottom=args.samples_per_bottom,
                validation_samples=args.validation_samples,
                state_limit=args.state_limit,
                cap_weight=args.cap_weight,
                seed=args.seed,
                gemstone_live_flags=(False, True) if args.gemstone_caverns_live_rate > 0 else (False,),
                normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
                engine_success_policy=engine_success_policy,
                remora_upkeep_payments=args.remora_upkeep_payments,
                gamble_mode=args.gamble_mode,
                action_sort=not args.disable_action_sort,
                weighted_policy_ev=args.weighted_policy_ev,
                rhystic_t1_weight=args.rhystic_t1_weight,
                rhystic_t2_weight=args.rhystic_t2_weight,
                heartwood_t1_weight=args.heartwood_t1_weight,
                heartwood_t2_weight=args.heartwood_t2_weight,
            )
            + ".json"
        )

    if args.rust_full_sim:
        payload = rust_full_sim_payload(
            args=args,
            module=module,
            deck_key=deck_key,
            deck=deck,
            commanders=commanders,
            threshold_cache_path=threshold_cache_path,
            threshold_cache_hit=threshold_cache_hit,
            engine_success_policy=engine_success_policy,
            started=started,
        )
        emit_simulator_payload(args, payload, payload["evaluation"])
        return 0

    main_worker_pool: ProcessPoolExecutor | None = None
    if args.reuse_worker_pool and args.workers > 1:
        main_worker_pool = create_worker_pool(
            workers=args.workers,
            target_mode=args.target,
            deck_json=args.deck_json,
            state_limit=args.state_limit,
            samples_per_bottom=args.samples_per_bottom,
            validation_samples=args.validation_samples,
            cap_weight=args.cap_weight,
            trace_lines=args.trace_lines,
            engine_success_policy=engine_success_policy,
            remora_upkeep_payments=args.remora_upkeep_payments,
            gamble_mode=args.gamble_mode,
            actual_rerun_state_limit=args.actual_rerun_state_limit,
            counterfactual_line_cards=args.counterfactual_line_cards,
            counterfactual_state_limit=args.counterfactual_state_limit,
            worker_profile_dir=args.worker_profile_dir,
            worker_profile_chunks_per_process=args.worker_profile_chunks_per_process,
            action_sort=not args.disable_action_sort,
            heuristic_bottom_order=args.heuristic_bottom_order,
        )

    threshold_started = time.time()
    gemstone_live_flags = (False, True) if args.gemstone_caverns_live_rate > 0 else (False,)
    if args.thresholds_json:
        threshold_payload = json.loads(Path(args.thresholds_json).read_text())
        thresholds_by_gemstone_key, threshold_rows_by_gemstone_key = load_threshold_payload(
            threshold_payload,
            gemstone_caverns_live_rate=args.gemstone_caverns_live_rate,
            source=args.thresholds_json,
        )
    elif threshold_cache_path is not None and threshold_cache_path.exists():
        threshold_payload = json.loads(threshold_cache_path.read_text())
        thresholds_by_gemstone_key, threshold_rows_by_gemstone_key = load_threshold_payload(
            threshold_payload,
            gemstone_caverns_live_rate=args.gemstone_caverns_live_rate,
            source=str(threshold_cache_path),
        )
        threshold_cache_hit = True
    else:
        threshold_tasks, stage_keys_by_gemstone_key = make_threshold_tasks(
            deck,
            hands_per_stage=args.threshold_hands,
            seed=args.seed,
            gemstone_live_flags=gemstone_live_flags,
            normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
        )
        threshold_evs = evaluate_tasks(
            threshold_tasks,
            target_mode=args.target,
            deck_json=args.deck_json,
            state_limit=args.state_limit,
            samples_per_bottom=args.samples_per_bottom,
            validation_samples=args.validation_samples,
            cap_weight=args.cap_weight,
            workers=args.workers,
            trace_lines=False,
            engine_success_policy=engine_success_policy,
            remora_upkeep_payments=args.remora_upkeep_payments,
            gamble_mode=args.gamble_mode,
            chunks_per_worker=args.chunks_per_worker,
            pool=main_worker_pool,
            worker_profile_dir=args.worker_profile_dir,
            worker_profile_chunks_per_process=args.worker_profile_chunks_per_process,
            action_sort=not args.disable_action_sort,
            heuristic_bottom_order=args.heuristic_bottom_order,
        )
        thresholds_by_gemstone_key = {}
        threshold_rows_by_gemstone_key = {}
        for key, stage_keys in stage_keys_by_gemstone_key.items():
            thresholds, threshold_rows = compute_thresholds(stage_keys, threshold_evs)
            thresholds_by_gemstone_key[key] = thresholds
            threshold_rows_by_gemstone_key[key] = threshold_rows
    thresholds_by_gemstone_live = {
        key == "live": values
        for key, values in thresholds_by_gemstone_key.items()
    }
    for flag in gemstone_live_flags:
        if flag not in thresholds_by_gemstone_live:
            raise ValueError(f"Missing thresholds for gemstone_caverns_live={flag}")
    dead_thresholds = thresholds_by_gemstone_key.get("dead")
    dead_threshold_rows = threshold_rows_by_gemstone_key.get("dead", [])
    threshold_summary = {
        "thresholds": dead_thresholds,
        "threshold_rows": dead_threshold_rows,
        "thresholds_by_gemstone_caverns_live": thresholds_by_gemstone_key,
        "threshold_rows_by_gemstone_caverns_live": threshold_rows_by_gemstone_key,
    }
    if threshold_cache_path is not None and not threshold_cache_hit:
        write_json_atomic(threshold_cache_path, threshold_summary, trailing_newline=True)
    thresholds_by_gemstone_key = apply_threshold_offset(
        thresholds_by_gemstone_key,
        args.threshold_offset,
    )
    thresholds_by_gemstone_live = {
        key == "live": values
        for key, values in thresholds_by_gemstone_key.items()
    }
    dead_thresholds = thresholds_by_gemstone_key.get("dead")
    threshold_summary = {
        "thresholds": dead_thresholds,
        "threshold_rows": dead_threshold_rows,
        "thresholds_by_gemstone_caverns_live": thresholds_by_gemstone_key,
        "threshold_rows_by_gemstone_caverns_live": threshold_rows_by_gemstone_key,
    }
    if args.suppress_json_stdout:
        print(
            json.dumps(
                {
                    "thresholds": threshold_summary["thresholds"],
                    "thresholds_by_gemstone_caverns_live": threshold_summary["thresholds_by_gemstone_caverns_live"],
                },
                sort_keys=True,
            ),
            flush=True,
        )
    else:
        print(json.dumps(threshold_summary, indent=2), flush=True)

    eval_started = time.time()
    evaluation = evaluate_policy(
        deck,
        thresholds_by_gemstone_live,
        games=args.eval_games,
        seed=args.seed + 1_000_000,
        gemstone_caverns_live_rate=args.gemstone_caverns_live_rate,
        target_mode=args.target,
        deck_json=args.deck_json,
        state_limit=args.state_limit,
        samples_per_bottom=args.samples_per_bottom,
        validation_samples=args.validation_samples,
        cap_weight=args.cap_weight,
        workers=args.workers,
        trace_lines=args.trace_lines,
        engine_success_policy=engine_success_policy,
        remora_upkeep_payments=args.remora_upkeep_payments,
        gamble_mode=args.gamble_mode,
        actual_rerun_state_limit=args.actual_rerun_state_limit,
        counterfactual_line_cards=args.counterfactual_line_cards,
        counterfactual_state_limit=args.counterfactual_state_limit,
        include_game_records=args.include_game_records,
        compact_game_records=args.compact_game_records,
        normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
        paired_stage_orders=args.paired_stage_orders,
        adaptive_threshold_sampling=args.adaptive_threshold_sampling,
        reuse_worker_pool=args.reuse_worker_pool,
        chunks_per_worker=args.chunks_per_worker,
        checkpoint_out=args.checkpoint_out,
        worker_profile_dir=args.worker_profile_dir,
        worker_profile_chunks_per_process=args.worker_profile_chunks_per_process,
        worker_pool=main_worker_pool,
        action_sort=not args.disable_action_sort,
        heuristic_bottom_order=args.heuristic_bottom_order,
    )
    eval_elapsed = time.time() - eval_started
    payload = {
        "target": args.target,
        "deck": deck_key,
        "deck_json": args.deck_json,
        "commanders": commanders,
        "mulligan_bottom_sequence": list(COMMANDER_MULLIGAN_BOTTOMS),
        "threshold_hands_per_stage": args.threshold_hands,
        "eval_games": args.eval_games,
        "samples_per_bottom": args.samples_per_bottom,
        "validation_samples": args.validation_samples,
        "state_limit": args.state_limit,
        "cap_weight_for_keep_decisions": args.cap_weight,
        "thresholds_json": args.thresholds_json,
        "threshold_cache_dir": args.threshold_cache_dir,
        "threshold_cache_path": str(threshold_cache_path) if threshold_cache_path else None,
        "threshold_cache_hit": threshold_cache_hit,
        "threshold_offset": args.threshold_offset,
        "trace_lines": args.trace_lines,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "engine_success_policy": engine_success_policy,
        "remora_upkeep_payments": args.remora_upkeep_payments,
        "gamble_mode": args.gamble_mode,
        "weighted_policy_ev": args.weighted_policy_ev,
        "rhystic_t1_weight": args.rhystic_t1_weight,
        "rhystic_t2_weight": args.rhystic_t2_weight,
        "heartwood_t1_weight": args.heartwood_t1_weight,
        "heartwood_t2_weight": args.heartwood_t2_weight,
        "rust_action_mode": args.rust_action_mode,
        "rust_close_mode": args.rust_close_mode,
        "rust_solver_mode": args.rust_solver_mode,
        "rust_action_bin": args.rust_action_bin,
        "actual_rerun_state_limit": args.actual_rerun_state_limit,
        "counterfactual_line_cards": args.counterfactual_line_cards,
        "counterfactual_state_limit": args.counterfactual_state_limit,
        "include_game_records": args.include_game_records,
        "compact_game_records": args.compact_game_records,
        "checkpoint_out": args.checkpoint_out,
        "normalize_no_caverns_gemstone_key": args.normalize_no_caverns_gemstone_key,
        "paired_stage_orders": args.paired_stage_orders,
        "adaptive_threshold_sampling": args.adaptive_threshold_sampling,
        "reuse_worker_pool": args.reuse_worker_pool,
        "threshold_and_policy_reuse_worker_pool": main_worker_pool is not None,
        "worker_profile_dir": args.worker_profile_dir,
        "worker_profile_chunks_per_process": args.worker_profile_chunks_per_process,
        "worker_start_method": args.worker_start_method or os.environ.get("RHYSTIC_WORKER_START_METHOD") or "fork",
        "action_sort": not args.disable_action_sort,
        "heuristic_bottom_order": args.heuristic_bottom_order,
        "seed": args.seed,
        "workers": args.workers,
        "chunks_per_worker": args.chunks_per_worker,
        "thresholds": dead_thresholds,
        "threshold_rows": dead_threshold_rows,
        "thresholds_by_gemstone_caverns_live": thresholds_by_gemstone_key,
        "threshold_rows_by_gemstone_caverns_live": threshold_rows_by_gemstone_key,
        "evaluation": evaluation,
        "elapsed_seconds": time.time() - started,
        "threshold_elapsed_seconds": eval_started - threshold_started,
        "eval_elapsed_seconds": eval_elapsed,
        "evaluation_elapsed_seconds": eval_elapsed,
        "assumptions": [
            "Commander London mulligan sequence is modeled as 7, free 7, 6, 5, 4, 3.",
            "For each visible hand, keep EV is estimated from random hidden-library rollouts and bottom choices are selected by visible-hand EV, not by the actual hidden library.",
            "Bottom choices are selected on one hidden-library sample set, then the selected bottom is scored on an independent validation sample set to reduce max-over-bottom sampling bias.",
            "The actual kept hand is then tested against its real shuffled library with the same color-correct Rhystic solver.",
            "Gemstone Caverns live/dead status is pre-sampled before the first visible hand and known for all mulligan decisions.",
            "If --paired-stage-orders is set, potential mulligan hands are sampled independently by game index and stage so paired variant runs do not desynchronize after different keep decisions.",
            "If --heuristic-bottom-order is set, bottom-choice pruning evaluates promising choices first while preserving original deterministic checks, shuffle streams, and tie breaks.",
            target_assumption_note(args.target),
            "Gamble mode is off for conservative runs, stochastic for sampled legal random discard, and optimistic for a ceiling.",
            "If --actual-rerun-state-limit is set, capped actual kept hands are retried at that higher cap before final rate accounting.",
            "If --counterfactual-line-cards is set, each non-cost card in a traced winning line is blanked and the kept hand is re-solved to estimate causal dependence.",
            "If --include-game-records is set, final kept-hand records and visible mulligan decisions are included for line auditing.",
            "State-limit capped misses are counted as misses in the primary rate and reported separately as an upper bound.",
        ],
    }
    if args.json_out:
        write_json_atomic(args.json_out, payload)
    if args.suppress_json_stdout:
        print(
            json.dumps(
                {
                    "json_out": args.json_out,
                    "target": args.target,
                    "gamble_mode": args.gamble_mode,
                    "successes": evaluation["successes"],
                    "eval_games": args.eval_games,
                    "success_rate": evaluation["success_rate"],
                    "cap_misses": evaluation["cap_misses"],
                    "initial_cap_misses_before_actual_rerun": evaluation["initial_cap_misses_before_actual_rerun"],
                    "actual_cap_rerun_attempts": evaluation["actual_cap_rerun_attempts"],
                    "actual_cap_rerun_successes": evaluation["actual_cap_rerun_successes"],
                    "turn_counts": evaluation["turn_counts"],
                },
                sort_keys=True,
            )
        )
    else:
        print(json.dumps(payload, indent=2, sort_keys=True))
    if main_worker_pool is not None:
        main_worker_pool.shutdown()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
