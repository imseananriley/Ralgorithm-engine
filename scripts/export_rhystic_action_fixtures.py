#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import deque
import hashlib
import json
import random
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.rhystic_belief_mulligan_sim import configure_target, load_solver_module, register_deck  # noqa: E402


CRAFTED_HANDS: tuple[tuple[str, ...], ...] = (
    (
        "Rhystic Study",
        "Ancient Tomb",
        "Underground Sea",
        "Chrome Mox",
        "Force of Will",
        "Lotus Petal",
        "Demonic Tutor",
    ),
    (
        "Heartwood Storyteller",
        "Tropical Island",
        "Elvish Spirit Guide",
        "Summoner's Pact",
        "Green Sun's Zenith",
        "Tinder Wall",
        "Mox Diamond",
    ),
    (
        "Beseech the Mirror",
        "Dark Ritual",
        "Underground Sea",
        "Lion's Eye Diamond",
        "Lotus Petal",
        "Rhystic Study",
        "Culling the Weak",
    ),
    (
        "Crop Rotation",
        "Bayou",
        "Mox Diamond",
        "City of Traitors",
        "Rhystic Study",
        "Elvish Spirit Guide",
        "Enlightened Tutor",
    ),
    (
        "An Offer You Can't Refuse",
        "Lotus Petal",
        "Sol Ring",
        "Mana Vault",
        "Rhystic Study",
        "City of Brass",
        "Mystic Remora",
    ),
    (
        "Ragavan, Nimble Pilferer",
        "Birds of Paradise",
        "Deathrite Shaman",
        "Mox Amber",
        "Command Tower",
        "Rite of Flame",
        "Mystical Tutor",
    ),
    (
        "Diabolic Intent",
        "Tinder Wall",
        "Culling the Weak",
        "Bayou",
        "Lotho, Corrupt Shirriff",
        "Mox Diamond",
        "Rhystic Study",
    ),
    (
        "Gemstone Caverns",
        "Chrome Mox",
        "Demonic Tutor",
        "Beseech the Mirror",
        "Dark Ritual",
        "Rhystic Study",
        "Force of Negation",
    ),
)


DIRECT_STATES: tuple[dict[str, Any], ...] = (
    {
        "name": "diabolic_intent",
        "hand": ["Diabolic Intent"],
        "library": ["Rhystic Study", "Heartwood Storyteller"],
        "battlefield": [{"name": "TINDER", "tapped": True, "extra": "G*"}],
        "mana": [1, 0, 0, 0, 0, 1],
        "turn": 1,
    },
    {
        "name": "diabolic_intent_led",
        "hand": ["Diabolic Intent"],
        "library": ["Rhystic Study", "Heartwood Storyteller"],
        "battlefield": [
            {"name": "LED", "tapped": False, "extra": ""},
            {"name": "TINDER", "tapped": True, "extra": "G*"},
        ],
        "mana": [1, 0, 0, 0, 0, 1],
        "turn": 1,
    },
    {
        "name": "enlightened_tutor",
        "hand": ["Enlightened Tutor"],
        "library": ["Rhystic Study", "Mystic Remora", "Esper Sentinel", "Copy Enchantment", "Mirrormade"],
        "mana": [0, 0, 0, 1, 0, 0],
        "turn": 1,
    },
    {
        "name": "vampiric_tutor",
        "hand": ["Vampiric Tutor"],
        "library": ["Rhystic Study", "Mystic Remora", "Esper Sentinel", "Heartwood Storyteller"],
        "mana": [1, 0, 0, 0, 0, 0],
        "turn": 1,
    },
    {
        "name": "beseech_plain_bargain",
        "hand": ["Beseech the Mirror"],
        "library": ["Rhystic Study", "Heartwood Storyteller", "Mystic Remora"],
        "battlefield": [{"name": "PETAL", "tapped": True, "extra": ""}],
        "mana": [3, 0, 0, 0, 0, 1],
        "turn": 1,
    },
    {
        "name": "beseech_led",
        "hand": ["Beseech the Mirror"],
        "library": ["Rhystic Study", "Heartwood Storyteller"],
        "battlefield": [{"name": "LED", "tapped": False, "extra": ""}],
        "mana": [3, 0, 0, 0, 0, 1],
        "turn": 1,
    },
    {
        "name": "phyrexian_tower",
        "library": [],
        "battlefield": [
            {"name": "TOWER", "tapped": False, "extra": "C"},
            {"name": "TINDER", "tapped": True, "extra": "G*"},
        ],
        "turn": 1,
    },
    {
        "name": "deathrite_active",
        "library": [],
        "battlefield": [{"name": "DEATHRITE", "tapped": False, "extra": "BG"}],
        "land_grave_count": 1,
        "turn": 2,
    },
    {
        "name": "gemstone_mine",
        "library": [],
        "battlefield": [{"name": "MINE", "tapped": False, "extra": "3"}],
        "turn": 1,
    },
)


def perm_to_json(perm: Any) -> dict[str, Any]:
    return {
        "name": perm.name,
        "tapped": perm.tapped,
        "extra": perm.extra,
    }


def state_to_json(state: Any) -> dict[str, Any]:
    return {
        "hand": list(state.hand),
        "library": list(state.library),
        "battlefield": [perm_to_json(perm) for perm in state.battlefield],
        "mana": list(state.mana),
        "land_played": state.land_played,
        "land_grave_count": state.land_grave_count,
        "mantle_attached": list(state.mantle_attached),
        "nature_attached": list(state.nature_attached),
        "nature_untap_used": state.nature_untap_used,
        "nature_tap_used": state.nature_tap_used,
        "rain_active": state.rain_active,
        "spells_this_turn": state.spells_this_turn,
        "pact_debt": state.pact_debt,
        "turn": state.turn,
        "engine_count": state.engine_count,
        "engine_targets": list(state.engine_targets),
        "engine_names": list(state.engine_names),
    }


def stable_digest(payload: dict[str, Any]) -> str:
    encoded = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.blake2b(encoded, digest_size=12).hexdigest()


def state_signature(state: Any) -> str:
    return stable_digest(state_to_json(state))


def action_to_json(search: Any, action: tuple[Any, str]) -> dict[str, Any]:
    next_state, label = action
    next_json = state_to_json(next_state)
    return {
        "label": label,
        "priority": search._priority(label),
        "next_state_signature": stable_digest(next_json),
        "next_state": next_json,
    }


def direct_state(module: Any, spec: dict[str, Any]) -> Any:
    battlefield = module.norm_battlefield(
        module.Perm(item["name"], item.get("tapped", False), item.get("extra", ""))
        for item in spec.get("battlefield", [])
    )
    return module.State(
        hand=module.norm(spec.get("hand", [])),
        library=tuple(spec.get("library", [])),
        battlefield=battlefield,
        mana=tuple(spec.get("mana", [0, 0, 0, 0, 0, 0])),
        land_played=spec.get("land_played", False),
        land_grave_count=spec.get("land_grave_count", 0),
        mantle_attached=tuple(spec.get("mantle_attached", [])),
        nature_attached=tuple(spec.get("nature_attached", [])),
        nature_untap_used=spec.get("nature_untap_used", False),
        nature_tap_used=spec.get("nature_tap_used", False),
        rain_active=spec.get("rain_active", False),
        spells_this_turn=spec.get("spells_this_turn", 0),
        pact_debt=spec.get("pact_debt", 0),
        turn=spec.get("turn", 1),
        engine_count=spec.get("engine_count", 0),
        engine_targets=tuple(spec.get("engine_targets", [])),
        engine_names=tuple(spec.get("engine_names", [])),
    )


def direct_fixture(module: Any, search: Any, spec: dict[str, Any], fixture_index: int, max_actions_per_state: int) -> dict[str, Any]:
    state = direct_state(module, spec)
    actions = list(search._actions(state))
    if search.action_sort:
        actions.sort(key=lambda item: search._priority(item[1]))
    state_json = state_to_json(state)
    fixture_actions = actions[:max_actions_per_state]
    return {
        "fixture_index": fixture_index,
        "game_index": None,
        "source": f"direct:{spec['name']}",
        "turn": state.turn,
        "depth": 0,
        "gemstone_live": False,
        "state_signature": stable_digest(state_json),
        "success_label": search._success_label(state),
        "state": state_json,
        "action_count": len(actions),
        "actions_truncated": len(actions) > max_actions_per_state,
        "actions": [action_to_json(search, action) for action in fixture_actions],
    }


def draw_turn_states(search: Any, states: dict[Any, None], turn: int) -> dict[Any, None]:
    out: dict[Any, None] = {}
    for state in states:
        begun = search._begin_turn(state)
        begun = search._replace(begun, turn=turn)
        if begun.pact_debt <= 0:
            out[search._draw(begun)] = None
        else:
            for upkeep_paid in search._pay_upkeep_pacts(begun):
                out[search._draw(upkeep_paid)] = None
    return out


def collect_from_keep(
    search: Any,
    *,
    hand: list[str],
    library: list[str],
    gemstone_live: bool,
    game_index: int,
    source: str,
    max_fixtures: int,
    max_actions_per_state: int,
) -> list[dict[str, Any]]:
    fixtures: list[dict[str, Any]] = []
    states: dict[Any, None] = {
        state: None for state, _path in search._starting_state_options(hand, library, gemstone_live=gemstone_live)
    }
    for turn in range(1, search.max_turns + 1):
        turn_states = draw_turn_states(search, states, turn)
        queue = deque(turn_states)
        seen = dict(turn_states)
        depth_by_state = {state: 0 for state in turn_states}
        best_mana: dict[tuple[Any, ...], list[tuple[int, ...]]] = {}
        while queue and len(fixtures) < max_fixtures:
            state = queue.pop()
            actions = list(search._actions(state))
            if search.action_sort:
                actions.sort(key=lambda item: search._priority(item[1]))
            if actions:
                state_json = state_to_json(state)
                fixture_actions = actions[:max_actions_per_state]
                fixtures.append(
                    {
                        "fixture_index": len(fixtures),
                        "game_index": game_index,
                        "source": source,
                        "turn": turn,
                        "depth": depth_by_state.get(state, 0),
                        "gemstone_live": gemstone_live,
                        "state_signature": stable_digest(state_json),
                        "success_label": search._success_label(state),
                        "state": state_json,
                        "action_count": len(actions),
                        "actions_truncated": len(actions) > max_actions_per_state,
                        "actions": [action_to_json(search, action) for action in fixture_actions],
                    }
                )
            for next_state, _label in actions:
                if next_state in seen or search._mana_dominated(next_state, best_mana):
                    continue
                if len(seen) >= search.state_limit:
                    continue
                seen[next_state] = None
                depth_by_state[next_state] = depth_by_state.get(state, 0) + 1
                queue.append(next_state)
        if len(fixtures) >= max_fixtures:
            break
        states = {search._end_turn(state): None for state in seen}
    return fixtures


def materialize_hand(deck: tuple[str, ...], wanted: tuple[str, ...]) -> tuple[list[str], list[str]] | None:
    remaining = list(deck)
    hand: list[str] = []
    for card in wanted:
        if card not in remaining:
            return None
        remaining.remove(card)
        hand.append(card)
    return hand, remaining


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--target", choices=("rhystic", "engine3", "engine4", "heartwood", "rhystic_heartwood", "rhystic_tithe"), default="rhystic_heartwood")
    parser.add_argument("--engine-success-policy", choices=("count", "resilient"), default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--state-limit", type=int, default=8000)
    parser.add_argument("--seed", type=int, default=2026070301)
    parser.add_argument("--games", type=int, default=8)
    parser.add_argument("--max-fixtures", type=int, default=64)
    parser.add_argument("--max-fixtures-per-hand", type=int, default=12)
    parser.add_argument("--max-actions-per-state", type=int, default=256)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--no-crafted-hands", action="store_true")
    parser.add_argument("--no-direct-states", action="store_true")
    parser.add_argument("--out", default="data/rhystic_study_turn12/benchmarks/action_fixtures_pass47_20260703.json")
    args = parser.parse_args()
    if args.games <= 0:
        raise ValueError("--games must be positive")
    if args.max_fixtures <= 0:
        raise ValueError("--max-fixtures must be positive")
    if args.max_fixtures_per_hand <= 0:
        raise ValueError("--max-fixtures-per-hand must be positive")
    if args.max_actions_per_state <= 0:
        raise ValueError("--max-actions-per-state must be positive")
    if not 0.0 <= args.gemstone_caverns_live_rate <= 1.0:
        raise ValueError("--gemstone-caverns-live-rate must be between 0 and 1")

    module = load_solver_module()
    deck_key = register_deck(module, args.deck_json)
    configure_target(module, args.target)
    search = module.RhysticSearch(
        deck_key,
        max_turns=2,
        state_limit=args.state_limit,
        goal="rhystic" if args.target == "rhystic" else "engine",
        engine_success_policy=args.engine_success_policy,
        gamble_mode=args.gamble_mode,
    )
    deck = tuple(search.mainboard)
    rng = random.Random(args.seed)
    fixtures: list[dict[str, Any]] = []
    if not args.no_direct_states:
        for spec in DIRECT_STATES:
            if len(fixtures) >= args.max_fixtures:
                break
            fixtures.append(direct_fixture(module, search, spec, len(fixtures), args.max_actions_per_state))
    if not args.no_crafted_hands:
        for crafted_index, wanted in enumerate(CRAFTED_HANDS):
            materialized = materialize_hand(deck, wanted)
            if materialized is None:
                continue
            hand, library = materialized
            fixtures.extend(
                collect_from_keep(
                    search,
                    hand=hand,
                    library=library,
                    gemstone_live=any(card in {"Gemstone Caverns", "Glittering Caves of Aglarond"} for card in hand),
                    game_index=-(crafted_index + 1),
                    source=f"crafted:{crafted_index + 1}",
                    max_fixtures=min(args.max_fixtures - len(fixtures), args.max_fixtures_per_hand),
                    max_actions_per_state=args.max_actions_per_state,
                )
            )
            if len(fixtures) >= args.max_fixtures:
                break
    for game_index in range(args.games):
        if len(fixtures) >= args.max_fixtures:
            break
        deck_order = list(deck)
        rng.shuffle(deck_order)
        hand = deck_order[:7]
        library = deck_order[7:]
        gemstone_live = rng.random() < args.gemstone_caverns_live_rate
        fixtures.extend(
            collect_from_keep(
                search,
                hand=hand,
                library=library,
                gemstone_live=gemstone_live,
                game_index=game_index,
                source="random",
                max_fixtures=min(args.max_fixtures - len(fixtures), args.max_fixtures_per_hand),
                max_actions_per_state=args.max_actions_per_state,
            )
        )

    for fixture_index, fixture in enumerate(fixtures):
        fixture["fixture_index"] = fixture_index

    payload = {
        "deck_json": args.deck_json,
        "deck_key": deck_key,
        "commanders": search.commanders,
        "target": args.target,
        "engine_success_policy": args.engine_success_policy,
        "gamble_mode": args.gamble_mode,
        "state_limit": args.state_limit,
        "seed": args.seed,
        "games_sampled": args.games,
        "direct_states_enabled": not args.no_direct_states,
        "crafted_hands_enabled": not args.no_crafted_hands,
        "max_fixtures_per_hand": args.max_fixtures_per_hand,
        "max_actions_per_state": args.max_actions_per_state,
        "fixture_count": len(fixtures),
        "fixtures": fixtures,
    }
    out = Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    print(out)
    print(json.dumps({"fixture_count": len(fixtures), "out": str(out)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
