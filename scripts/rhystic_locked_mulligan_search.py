#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import multiprocessing
import random
import sys
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import rhystic_belief_mulligan_sim as sim  # noqa: E402
from rhystic_quick_compare import mainboard_names, read_moxfield, slug, write_variant_deck  # noqa: E402


LABEL_ENGINE = {
    "Rhystic Study": "rhystic",
    "Heartwood Storyteller": "heartwood",
}

PRESETS: dict[str, list[tuple[str, str]]] = {
    "fast": [
        ("Chrome Mox", "fast"),
        ("Lion's Eye Diamond", "fast"),
        ("Lotus Petal", "fast"),
        ("Mana Vault", "fast"),
        ("Mox Amber", "fast"),
        ("Mox Diamond", "fast"),
        ("Mox Opal", "fast"),
        ("Sol Ring", "fast"),
        ("Springleaf Drum", "fast"),
        ("Paradise Mantle", "fast"),
        ("Relic of Legends", "fast"),
        ("Arcane Signet", "fast"),
        ("Dark Ritual", "ritual"),
        ("Culling the Weak", "ritual"),
        ("Rain of Filth", "ritual"),
        ("Rite of Flame", "ritual"),
        ("Infernal Plunge", "ritual"),
        ("Strike It Rich", "ritual"),
        ("Manamorphose", "ritual"),
        ("Elvish Spirit Guide", "fast"),
        ("Simian Spirit Guide", "fast"),
        ("Tinder Wall", "dork"),
        ("Wild Cantor", "dork"),
    ],
    "dorks": [
        ("Birds of Paradise", "dork"),
        ("Deathrite Shaman", "dork"),
        ("Ragavan, Nimble Pilferer", "dork"),
        ("Lotho, Corrupt Shirriff", "dork"),
        ("Noble Hierarch", "dork"),
        ("Ignoble Hierarch", "dork"),
        ("Tinder Wall", "dork"),
        ("Wild Cantor", "dork"),
    ],
    "lands": [
        ("Ancient Tomb", "land"),
        ("Arid Mesa", "land"),
        ("Bayou", "land"),
        ("Bloodstained Mire", "land"),
        ("Boseiju, Who Endures", "land"),
        ("City of Brass", "land"),
        ("City of Traitors", "land"),
        ("Command Tower", "land"),
        ("Crystal Vein", "land"),
        ("Emergence Zone", "land"),
        ("Exotic Orchard", "land"),
        ("Flooded Strand", "land"),
        ("Forbidden Orchard", "land"),
        ("Gemstone Caverns", "land"),
        ("Gemstone Mine", "land"),
        ("Glimmervoid", "land"),
        ("Hallowed Fountain", "land"),
        ("Mana Confluence", "land"),
        ("Marsh Flats", "land"),
        ("Misty Rainforest", "land"),
        ("Phyrexian Tower", "land"),
        ("Plateau", "land"),
        ("Polluted Delta", "land"),
        ("Scalding Tarn", "land"),
        ("Scrubland", "land"),
        ("Sea of Clouds", "land"),
        ("Sink into Stupor", "land"),
        ("Starting Town", "land"),
        ("Steam Vents", "land"),
        ("Tarnished Citadel", "land"),
        ("Tropical Island", "land"),
        ("Tundra", "land"),
        ("Underground Sea", "land"),
        ("Verdant Catacombs", "land"),
        ("Volcanic Island", "land"),
        ("Windswept Heath", "land"),
        ("Wooded Foothills", "land"),
    ],
    "tutors": [
        ("Beseech the Mirror", "tutor"),
        ("Crop Rotation", "tutor"),
        ("Demonic Tutor", "tutor"),
        ("Diabolic Intent", "tutor"),
        ("Eldritch Evolution", "tutor"),
        ("Enlightened Tutor", "tutor"),
        ("Gamble", "tutor"),
        ("Green Sun's Zenith", "tutor"),
        ("Grim Tutor", "tutor"),
        ("Idyllic Tutor", "tutor"),
        ("Imperial Seal", "tutor"),
        ("Mystical Tutor", "tutor"),
        ("Neoform", "tutor"),
        ("Scheming Symmetry", "tutor"),
        ("Summoner's Pact", "tutor"),
        ("Vampiric Tutor", "tutor"),
        ("Wishclaw Talisman", "tutor"),
        ("Worldly Tutor", "tutor"),
    ],
}


@dataclass(frozen=True)
class Candidate:
    card: str
    category: str
    effective_cut: str
    deck_json: Path
    present_in_source: bool


def unique_presets(names: list[str]) -> list[tuple[str, str]]:
    seen: set[str] = set()
    out: list[tuple[str, str]] = []
    for preset in names:
        if preset == "all":
            parts = PRESETS["fast"] + PRESETS["dorks"] + PRESETS["lands"] + PRESETS["tutors"]
        else:
            parts = PRESETS[preset]
        for card, category in parts:
            if card in seen:
                continue
            seen.add(card)
            out.append((card, category))
    return out


def parse_candidate(text: str) -> tuple[str, str, str | None]:
    parts = [part.strip() for part in text.split("|")]
    if len(parts) == 1:
        return parts[0], "custom", None
    if len(parts) == 2:
        return parts[0], parts[1], None
    if len(parts) == 3:
        return parts[0], parts[1], parts[2]
    raise argparse.ArgumentTypeError("Candidates must be CARD, CARD|category, or CARD|category|cut")


def read_candidate_files(paths: list[str]) -> list[tuple[str, str, str | None]]:
    candidates: list[tuple[str, str, str | None]] = []
    for path_text in paths:
        path = Path(path_text)
        if not path.is_absolute():
            path = ROOT / path
        for raw_line in path.read_text().splitlines():
            line = raw_line.strip()
            if not line or line.startswith("#"):
                continue
            candidates.append(parse_candidate(line))
    return candidates


def candidate_cut(card: str, category: str, names: set[str], args: argparse.Namespace, explicit_cut: str | None) -> str:
    if explicit_cut:
        return explicit_cut
    if card in names:
        return card
    if category == "land":
        return args.absent_land_cut
    if category == "tutor":
        return args.absent_tutor_cut
    return args.absent_nonland_cut


def make_candidate_decks(args: argparse.Namespace, cards: list[tuple[str, str, str | None]], out_dir: Path) -> list[Candidate]:
    source = (ROOT / args.deck_json).resolve()
    payload = read_moxfield(source)
    names = set(mainboard_names(payload))
    deck_dir = out_dir / "decks"
    deck_dir.mkdir(parents=True, exist_ok=True)
    candidates: list[Candidate] = []
    seen_candidates: set[tuple[str, str]] = set()
    for card, category, explicit_cut in cards:
        if not card:
            continue
        cut = candidate_cut(card, category, names, args, explicit_cut)
        candidate_key = (card, cut)
        if candidate_key in seen_candidates:
            continue
        seen_candidates.add(candidate_key)
        if card in names:
            deck_path = source
            present = True
        else:
            deck_path = deck_dir / f"{slug(card)}_over_{slug(cut)}.json"
            if args.force or not deck_path.exists():
                write_variant_deck(source, deck_path, cut, card)
            present = False
        candidates.append(
            Candidate(
                card=card,
                category=category,
                effective_cut=cut,
                deck_json=deck_path,
                present_in_source=present,
            )
        )
    return candidates


def configure_worker(args: argparse.Namespace, deck_json: Path) -> None:
    sim.worker_init(
        args.target,
        str(deck_json),
        args.state_limit,
        args.samples_per_bottom,
        args.validation_samples,
        args.cap_weight,
        False,
        args.engine_success_policy,
        args.remora_upkeep_payments,
        args.gamble_mode,
        args.actual_rerun_state_limit,
        False,
        0,
    )


def candidate_order(deck: tuple[str, ...], card: str, seed: int, game_index: int, stage: int, namespace: str) -> list[str]:
    base = list(deck)
    try:
        base.remove(card)
    except ValueError as exc:
        raise ValueError(f"Locked card is not in candidate deck: {card}") from exc
    rng = random.Random(sim.stable_seed(seed, namespace, card, game_index, stage))
    rng.shuffle(base)
    return [card, *base]


def threshold_tasks_for_candidate(
    deck: tuple[str, ...],
    card: str,
    *,
    hands_per_stage: int,
    seed: int,
    gemstone_live_flags: tuple[bool, ...],
    normalize_no_caverns_gemstone_key: bool,
) -> tuple[list[sim.HandTask], dict[str, dict[int, list[str]]]]:
    tasks: list[sim.HandTask] = []
    stage_keys: dict[str, dict[int, list[str]]] = {sim.gemstone_key(flag): {} for flag in gemstone_live_flags}
    for stage, bottom_count in enumerate(sim.COMMANDER_MULLIGAN_BOTTOMS):
        keys_by_flag: dict[bool, list[str]] = {flag: [] for flag in gemstone_live_flags}
        for i in range(hands_per_stage):
            order = candidate_order(deck, card, seed, i, stage, "locked-threshold-order")
            hand = tuple(sorted(order[:7]))
            for flag in gemstone_live_flags:
                key = sim.hand_key(
                    hand,
                    bottom_count,
                    flag,
                    normalize_no_caverns_gemstone_key=normalize_no_caverns_gemstone_key,
                )
                keys_by_flag[flag].append(key)
                tasks.append(
                    sim.HandTask(
                        key=key,
                        hand=hand,
                        bottom_count=bottom_count,
                        seed=sim.stable_seed(seed, "locked-threshold", card, stage, i, key),
                        gemstone_live=flag,
                    )
                )
        for flag, keys in keys_by_flag.items():
            stage_keys[sim.gemstone_key(flag)][stage] = keys
    return tasks, stage_keys


def score_row(row: dict[str, Any], args: argparse.Namespace) -> float:
    if not row.get("hit"):
        return 0.0
    try:
        turn = int(row.get("turn") or 0)
    except (TypeError, ValueError):
        return 0.0
    engine = LABEL_ENGINE.get(str(row.get("engine_label") or ""))
    if engine == "rhystic" and turn == 1:
        return args.rhystic_t1_weight
    if engine == "rhystic" and turn == 2:
        return args.rhystic_t2_weight
    if engine == "heartwood" and turn == 1:
        return args.heartwood_t1_weight
    if engine == "heartwood" and turn == 2:
        return args.heartwood_t2_weight
    return 0.0


def compute_locked_thresholds(args: argparse.Namespace, candidate: Candidate) -> dict[bool, list[float]]:
    configure_worker(args, candidate.deck_json)
    deck = tuple(sim.DECK)
    tasks, stage_keys_by_gemstone_key = threshold_tasks_for_candidate(
        deck,
        candidate.card,
        hands_per_stage=args.threshold_hands,
        seed=args.seed,
        gemstone_live_flags=(False, True),
        normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
    )
    evs = sim.evaluate_tasks(
        tasks,
        target_mode=args.target,
        deck_json=str(candidate.deck_json),
        state_limit=args.state_limit,
        samples_per_bottom=args.samples_per_bottom,
        validation_samples=args.validation_samples,
        cap_weight=args.cap_weight,
        workers=args.workers,
        trace_lines=False,
        engine_success_policy=args.engine_success_policy,
        remora_upkeep_payments=args.remora_upkeep_payments,
        gamble_mode=args.gamble_mode,
        actual_rerun_state_limit=args.actual_rerun_state_limit,
        counterfactual_line_cards=False,
        counterfactual_state_limit=0,
        chunks_per_worker=args.chunks_per_worker,
    )
    out: dict[bool, list[float]] = {}
    for key, stage_keys in stage_keys_by_gemstone_key.items():
        thresholds, _rows = sim.compute_thresholds(stage_keys, evs)
        out[key == "live"] = thresholds
    return out


def evaluate_locked_candidate(args: argparse.Namespace, candidate: Candidate) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    started = time.time()
    configure_worker(args, candidate.deck_json)
    deck = tuple(sim.DECK)
    thresholds = compute_locked_thresholds(args, candidate)
    gemstone_rng = random.Random(sim.stable_seed(args.seed, "locked-gemstone", candidate.card))
    gemstone_live_by_game = {
        game_index: gemstone_rng.random() < args.gemstone_caverns_live_rate
        for game_index in range(args.games)
    }
    active = list(range(args.games))
    results: dict[int, dict[str, Any]] = {}
    decision_rows: list[dict[str, Any]] = []
    visible_cache: dict[str, dict[str, Any]] = {}
    kept_visible_count = 0
    locked_bottomed = 0
    locked_kept = 0
    stage_counts: Counter[str] = Counter()
    bottom_counts: Counter[str] = Counter()

    for stage, bottom_count in enumerate(sim.COMMANDER_MULLIGAN_BOTTOMS):
        if not active:
            break
        print(f"{candidate.card}: locked stage {stage} active {len(active)}", flush=True)
        stage_orders: dict[int, list[str]] = {}
        tasks: list[sim.HandTask] = []
        for game_index in active:
            order = candidate_order(deck, candidate.card, args.seed, game_index, stage, "locked-policy-order")
            stage_orders[game_index] = order
            hand = tuple(sorted(order[:7]))
            gemstone_live = gemstone_live_by_game[game_index]
            key = sim.hand_key(
                hand,
                bottom_count,
                gemstone_live,
                normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
            )
            if key not in visible_cache:
                tasks.append(
                    sim.HandTask(
                        key=key,
                        hand=hand,
                        bottom_count=bottom_count,
                        seed=sim.stable_seed(args.seed, "locked-eval", candidate.card, game_index, stage, key),
                        gemstone_live=gemstone_live,
                    )
                )
        visible_cache.update(
            sim.evaluate_tasks(
                tasks,
                target_mode=args.target,
                deck_json=str(candidate.deck_json),
                state_limit=args.state_limit,
                samples_per_bottom=args.samples_per_bottom,
                validation_samples=args.validation_samples,
                cap_weight=args.cap_weight,
                workers=args.workers,
                trace_lines=False,
                engine_success_policy=args.engine_success_policy,
                remora_upkeep_payments=args.remora_upkeep_payments,
                gamble_mode=args.gamble_mode,
                actual_rerun_state_limit=args.actual_rerun_state_limit,
                counterfactual_line_cards=False,
                counterfactual_state_limit=0,
                chunks_per_worker=args.chunks_per_worker,
            )
        )

        next_active: list[int] = []
        actual_tasks: list[sim.ActualTask] = []
        for game_index in active:
            order = stage_orders[game_index]
            hand = tuple(sorted(order[:7]))
            gemstone_live = gemstone_live_by_game[game_index]
            key = sim.hand_key(
                hand,
                bottom_count,
                gemstone_live,
                normalize_no_caverns_gemstone_key=args.normalize_no_caverns_gemstone_key,
            )
            ev_row = visible_cache[key]
            keep_threshold = thresholds[gemstone_live][stage]
            keep_now = stage == len(sim.COMMANDER_MULLIGAN_BOTTOMS) - 1 or ev_row["score_ev"] >= keep_threshold
            decision_rows.append(
                {
                    "card": candidate.card,
                    "category": candidate.category,
                    "effective_cut": candidate.effective_cut,
                    "game_index": game_index,
                    "stage": stage,
                    "bottom_count": bottom_count,
                    "gemstone_caverns_live": gemstone_live,
                    "score_ev": ev_row["score_ev"],
                    "keep_threshold": keep_threshold,
                    "keep": keep_now,
                    "best_bottom": "; ".join(ev_row["best_bottom"]),
                    "locked_card_bottomed": candidate.card in set(ev_row["best_bottom"]),
                    "visible_hand": "; ".join(hand),
                }
            )
            if not keep_now:
                next_active.append(game_index)
                continue
            bottom = tuple(ev_row["best_bottom"])
            keep = sim.remove_bottom(hand, bottom)
            library = tuple(order[7:] + list(bottom))
            kept_visible_count += 1
            locked_bottomed += int(candidate.card in set(bottom))
            locked_kept += int(candidate.card in set(keep))
            actual_tasks.append(
                sim.ActualTask(
                    game_index=game_index,
                    stage=stage,
                    bottom_count=bottom_count,
                    keep=keep,
                    library=library,
                    gemstone_live=gemstone_live,
                    seed=sim.stable_seed(args.seed, "locked-actual", candidate.card, game_index, stage, key),
                )
            )
        for row in sim.evaluate_actual_tasks(
            actual_tasks,
            target_mode=args.target,
            deck_json=str(candidate.deck_json),
            state_limit=args.state_limit,
            samples_per_bottom=args.samples_per_bottom,
            validation_samples=args.validation_samples,
            cap_weight=args.cap_weight,
            workers=args.workers,
            trace_lines=False,
            engine_success_policy=args.engine_success_policy,
            remora_upkeep_payments=args.remora_upkeep_payments,
            gamble_mode=args.gamble_mode,
            actual_rerun_state_limit=args.actual_rerun_state_limit,
            counterfactual_line_cards=False,
            counterfactual_state_limit=0,
            chunks_per_worker=args.chunks_per_worker,
        ):
            results[int(row["game_index"])] = row
            stage_counts[str(row["stage"])] += 1
            bottom_counts[str(row["bottom_count"])] += 1
        active = next_active

    if len(results) != args.games:
        raise RuntimeError(f"{candidate.card}: only resolved {len(results)} of {args.games} games")
    turns = Counter(str(row.get("turn") or "miss") for row in results.values())
    successes = sum(1 for row in results.values() if row.get("hit"))
    score_total = sum(score_row(row, args) for row in results.values())
    summary = {
        "card": candidate.card,
        "category": candidate.category,
        "effective_cut": candidate.effective_cut,
        "present_in_source": candidate.present_in_source,
        "games": args.games,
        "successes": successes,
        "success_rate": successes / args.games,
        "weighted_score_per_game": score_total / args.games,
        "turn1": turns.get("1", 0),
        "turn2": turns.get("2", 0),
        "miss": turns.get("miss", 0),
        "cap_misses": sum(1 for row in results.values() if row.get("capped") and not row.get("hit")),
        "initial_cap_misses_before_actual_rerun": sum(
            1 for row in results.values() if row.get("initial_capped") and not row.get("initial_hit")
        ),
        "actual_cap_rerun_attempts": sum(1 for row in results.values() if row.get("cap_rerun_attempted")),
        "actual_cap_rerun_successes": sum(
            1 for row in results.values() if row.get("cap_rerun_attempted") and row.get("hit")
        ),
        "kept_visible_count": kept_visible_count,
        "locked_card_kept_count": locked_kept,
        "locked_card_bottomed_count": locked_bottomed,
        "locked_card_bottomed_rate": locked_bottomed / kept_visible_count if kept_visible_count else 0.0,
        "keep_counts_by_stage": json.dumps(dict(sorted(stage_counts.items()))),
        "keep_counts_by_bottom": json.dumps(dict(sorted(bottom_counts.items()))),
        "elapsed_seconds": time.time() - started,
        "deck_json": str(candidate.deck_json),
    }
    return summary, decision_rows


def write_csv(path: Path, rows: list[dict[str, Any]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--out-dir", default="data/rhystic_study_turn12/locked_mulligan_search")
    parser.add_argument("--preset", action="append", choices=("fast", "dorks", "lands", "tutors", "all"), default=[])
    parser.add_argument("--candidate", action="append", type=parse_candidate, default=[])
    parser.add_argument("--candidate-file", action="append", default=[])
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--shard-count", type=int, default=1)
    parser.add_argument("--absent-nonland-cut", default="Rain of Filth")
    parser.add_argument("--absent-land-cut", default="Glimmervoid")
    parser.add_argument("--absent-tutor-cut", default="Rain of Filth")
    parser.add_argument("--games", type=int, default=250)
    parser.add_argument("--threshold-hands", type=int, default=24)
    parser.add_argument("--seed", type=int, default=2026070111)
    parser.add_argument("--target", default="rhystic_heartwood", choices=("rhystic", "heartwood", "rhystic_heartwood"))
    parser.add_argument("--state-limit", type=int, default=12_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=30_000)
    parser.add_argument("--samples-per-bottom", type=int, default=1)
    parser.add_argument("--validation-samples", type=int, default=1)
    parser.add_argument("--cap-weight", type=float, default=0.0)
    parser.add_argument("--workers", type=int, default=max(1, multiprocessing.cpu_count() // 2))
    parser.add_argument("--chunks-per-worker", type=int, default=8)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--remora-upkeep-payments", type=int, default=2)
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--normalize-no-caverns-gemstone-key", action="store_true", default=True)
    parser.add_argument("--no-normalize-no-caverns-gemstone-key", dest="normalize_no_caverns_gemstone_key", action="store_false")
    parser.add_argument("--rhystic-t1-weight", type=float, default=100.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=60.0)
    parser.add_argument("--heartwood-t1-weight", type=float, default=20.0)
    parser.add_argument("--heartwood-t2-weight", type=float, default=10.0)
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.shard_count <= 0:
        raise ValueError("--shard-count must be positive")
    if args.shard_index < 0 or args.shard_index >= args.shard_count:
        raise ValueError("--shard-index must satisfy 0 <= index < shard-count")
    if not args.preset and not args.candidate and not args.candidate_file:
        args.preset = ["all"]
    out_dir = (ROOT / args.out_dir).resolve()
    preset_cards = [(card, category, None) for card, category in unique_presets(args.preset)]
    all_cards = [*preset_cards, *read_candidate_files(args.candidate_file), *args.candidate]
    candidates = make_candidate_decks(args, all_cards, out_dir)
    candidates = [
        candidate
        for index, candidate in enumerate(candidates)
        if index % args.shard_count == args.shard_index
    ]
    if not candidates:
        raise ValueError(f"No candidates selected for shard {args.shard_index}/{args.shard_count}")

    summaries: list[dict[str, Any]] = []
    decisions: list[dict[str, Any]] = []
    for index, candidate in enumerate(candidates, start=1):
        print(f"[{index}/{len(candidates)}] locked mulligan {candidate.card} ({candidate.category}, cut={candidate.effective_cut})", flush=True)
        summary, decision_rows = evaluate_locked_candidate(args, candidate)
        summaries.append(summary)
        if args.games <= 1000:
            decisions.extend(decision_rows)
        summary_path = out_dir / "locked_mulligan_summary.csv"
        summary_fields = [
            "card",
            "category",
            "effective_cut",
            "present_in_source",
            "games",
            "successes",
            "success_rate",
            "weighted_score_per_game",
            "turn1",
            "turn2",
            "miss",
            "cap_misses",
            "initial_cap_misses_before_actual_rerun",
            "actual_cap_rerun_attempts",
            "actual_cap_rerun_successes",
            "kept_visible_count",
            "locked_card_kept_count",
            "locked_card_bottomed_count",
            "locked_card_bottomed_rate",
            "keep_counts_by_stage",
            "keep_counts_by_bottom",
            "elapsed_seconds",
            "deck_json",
        ]
        ranked = sorted(summaries, key=lambda row: (-float(row["weighted_score_per_game"]), -float(row["success_rate"]), row["card"]))
        write_csv(summary_path, ranked, summary_fields)
        print(summary_path, flush=True)

    if decisions:
        write_csv(
            out_dir / "locked_mulligan_decisions.csv",
            decisions,
            [
                "card",
                "category",
                "effective_cut",
                "game_index",
                "stage",
                "bottom_count",
                "gemstone_caverns_live",
                "score_ev",
                "keep_threshold",
                "keep",
                "best_bottom",
                "locked_card_bottomed",
                "visible_hand",
            ],
        )
    config = vars(args).copy()
    config["candidate"] = [
        {"card": card, "category": category, "cut": cut}
        for card, category, cut in args.candidate
    ]
    config["candidate_file"] = args.candidate_file
    (out_dir / "run_config.json").write_text(json.dumps(config, indent=2, sort_keys=True))
    print(out_dir / "locked_mulligan_summary.csv")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
