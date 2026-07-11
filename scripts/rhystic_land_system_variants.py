#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import itertools
import json
import re
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]

LAND_TYPE_COLORS: dict[str, str] = {
    "Badlands": "BR",
    "Bayou": "BG",
    "Blood Crypt": "BR",
    "Hallowed Fountain": "UW",
    "Plateau": "RW",
    "Scrubland": "BW",
    "Steam Vents": "RU",
    "Tropical Island": "UG",
    "Tundra": "UW",
    "Underground Sea": "BU",
    "Volcanic Island": "RU",
    "Watery Grave": "BU",
}

FETCH_TARGETS: dict[str, set[str]] = {
    "Arid Mesa": {"Scrubland", "Tundra", "Volcanic Island", "Hallowed Fountain", "Plateau", "Steam Vents"},
    "Bloodstained Mire": {
        "Badlands",
        "Bayou",
        "Blood Crypt",
        "Plateau",
        "Scrubland",
        "Steam Vents",
        "Underground Sea",
        "Volcanic Island",
        "Watery Grave",
    },
    "Flooded Strand": {
        "Hallowed Fountain",
        "Plateau",
        "Scrubland",
        "Steam Vents",
        "Tropical Island",
        "Tundra",
        "Underground Sea",
        "Volcanic Island",
        "Watery Grave",
    },
    "Marsh Flats": {
        "Badlands",
        "Bayou",
        "Blood Crypt",
        "Hallowed Fountain",
        "Plateau",
        "Scrubland",
        "Tundra",
        "Underground Sea",
        "Watery Grave",
    },
    "Misty Rainforest": {
        "Bayou",
        "Hallowed Fountain",
        "Steam Vents",
        "Tropical Island",
        "Tundra",
        "Underground Sea",
        "Volcanic Island",
        "Watery Grave",
    },
    "Polluted Delta": {
        "Badlands",
        "Bayou",
        "Blood Crypt",
        "Hallowed Fountain",
        "Scrubland",
        "Steam Vents",
        "Tropical Island",
        "Tundra",
        "Underground Sea",
        "Volcanic Island",
        "Watery Grave",
    },
    "Scalding Tarn": {
        "Badlands",
        "Blood Crypt",
        "Hallowed Fountain",
        "Plateau",
        "Steam Vents",
        "Tropical Island",
        "Tundra",
        "Underground Sea",
        "Volcanic Island",
        "Watery Grave",
    },
    "Verdant Catacombs": {
        "Badlands",
        "Bayou",
        "Blood Crypt",
        "Scrubland",
        "Tropical Island",
        "Underground Sea",
        "Watery Grave",
    },
    "Windswept Heath": {"Bayou", "Hallowed Fountain", "Plateau", "Scrubland", "Tropical Island", "Tundra"},
    "Wooded Foothills": {
        "Badlands",
        "Bayou",
        "Blood Crypt",
        "Plateau",
        "Steam Vents",
        "Tropical Island",
        "Volcanic Island",
    },
}

COLOR_WEIGHTS = {
    "U": 4.0,
    "G": 3.0,
    "B": 2.6,
    "W": 1.8,
    "R": 1.6,
}

CARD_PENALTY = {
    "Badlands": 0,
    "Bayou": 0,
    "Plateau": 0,
    "Scrubland": 0,
    "Tropical Island": 0,
    "Tundra": 0,
    "Underground Sea": 0,
    "Volcanic Island": 0,
    "Blood Crypt": 1,
    "Hallowed Fountain": 1,
    "Steam Vents": 1,
    "Watery Grave": 1,
}


def repo_path(path: str | Path) -> Path:
    p = Path(path)
    return p if p.is_absolute() else ROOT / p


def slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9_]+", "_", text).strip("_").lower()


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def read_deck(path: Path) -> list[str]:
    payload = read_json(path)
    if isinstance(payload.get("deck"), list):
        return list(payload["deck"])
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards") or {}
    deck: list[str] = []
    for entry in cards.values():
        deck.extend([entry["card"]["name"]] * int(entry.get("quantity", 1)))
    return deck


def colors_value(colors: str) -> float:
    return sum(COLOR_WEIGHTS[color] for color in set(colors))


def package_score(package: tuple[str, ...], fetches: list[str]) -> dict[str, Any]:
    package_set = set(package)
    direct_color_counts = {color: 0 for color in COLOR_WEIGHTS}
    pair_counts: dict[str, int] = {}
    for land in package:
        colors = "".join(sorted(LAND_TYPE_COLORS[land]))
        pair_counts[colors] = pair_counts.get(colors, 0) + 1
        for color in set(colors):
            direct_color_counts[color] += 1

    direct_score = sum(COLOR_WEIGHTS[color] * count for color, count in direct_color_counts.items())
    fetch_score = 0.0
    fetch_color_counts = {color: 0 for color in COLOR_WEIGHTS}
    fetch_engine_count = 0
    fetch_blue_black_count = 0
    for fetch in fetches:
        available = set()
        for target in FETCH_TARGETS.get(fetch, set()) & package_set:
            available.update(LAND_TYPE_COLORS[target])
        for color in available:
            fetch_color_counts[color] += 1
        fetch_score += colors_value("".join(available))
        if {"U", "G"} <= available:
            fetch_engine_count += 1
        if {"U", "B"} <= available:
            fetch_blue_black_count += 1

    duplicate_pair_penalty = sum(max(0, count - 1) for count in pair_counts.values()) * 1.25
    score = direct_score + 1.9 * fetch_score + 1.5 * fetch_engine_count + 0.6 * fetch_blue_black_count - duplicate_pair_penalty
    return {
        "score": score,
        "direct_score": direct_score,
        "fetch_score": fetch_score,
        "fetch_engine_count": fetch_engine_count,
        "fetch_blue_black_count": fetch_blue_black_count,
        "duplicate_pair_penalty": duplicate_pair_penalty,
        "direct_color_counts": direct_color_counts,
        "fetch_color_counts": fetch_color_counts,
        "pair_counts": pair_counts,
    }


def package_key(package: tuple[str, ...]) -> str:
    counts: dict[str, int] = {}
    for land in package:
        pair = "".join(sorted(LAND_TYPE_COLORS[land]))
        counts[pair] = counts.get(pair, 0) + 1
    return json.dumps(counts, sort_keys=True, separators=(",", ":"))


def package_penalty(package: tuple[str, ...]) -> int:
    return sum(CARD_PENALTY.get(card, 2) for card in package)


def color_distance(cut: str, add: str) -> int:
    left = set(LAND_TYPE_COLORS[cut])
    right = set(LAND_TYPE_COLORS[add])
    return len(left ^ right)


def pair_swaps(current: tuple[str, ...], target: tuple[str, ...]) -> list[tuple[str, str]]:
    current_remaining = [card for card in current if card not in target]
    target_remaining = [card for card in target if card not in current]
    swaps: list[tuple[str, str]] = []
    while current_remaining:
        cut = current_remaining.pop(0)
        add = min(target_remaining, key=lambda candidate: (color_distance(cut, candidate), candidate))
        target_remaining.remove(add)
        swaps.append((cut, add))
    return swaps


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        path.write_text("")
        return
    fields: list[str] = []
    for row in rows:
        for key in row:
            if key not in fields:
                fields.append(key)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate typed-land package variants for Rhystic experiments.")
    parser.add_argument("--deck-json", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--top", type=int, default=30)
    parser.add_argument("--max-swaps-per-package", type=int, default=3)
    parser.add_argument("--include-watery-grave", action="store_true")
    args = parser.parse_args()

    deck_path = repo_path(args.deck_json)
    out_dir = repo_path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    deck = read_deck(deck_path)
    deck_set = set(deck)
    fetches = [card for card in deck if card in FETCH_TARGETS]
    current = tuple(card for card in deck if card in LAND_TYPE_COLORS)
    current_set = set(current)
    candidate_universe = set(current_set)
    candidate_universe.update({"Badlands", "Blood Crypt", "Plateau", "Steam Vents"})
    if args.include_watery_grave:
        candidate_universe.add("Watery Grave")
    candidate_universe = {card for card in candidate_universe if card not in deck_set or card in current_set}
    universe = tuple(sorted(candidate_universe))
    current_score = package_score(tuple(sorted(current)), fetches)

    rows: list[dict[str, Any]] = []
    for package in itertools.combinations(universe, len(current)):
        package_set = set(package)
        if package_set == current_set:
            continue
        swaps = pair_swaps(tuple(sorted(current)), tuple(sorted(package)))
        if len(swaps) > args.max_swaps_per_package:
            continue
        score = package_score(package, fetches)
        rows.append(
            {
                "package": ";".join(package),
                "swap_count": len(swaps),
                "swaps": "; ".join(f"{cut}={add}" for cut, add in swaps),
                "score": score["score"],
                "score_delta": score["score"] - current_score["score"],
                "direct_score_delta": score["direct_score"] - current_score["direct_score"],
                "fetch_score_delta": score["fetch_score"] - current_score["fetch_score"],
                "fetch_engine_count": score["fetch_engine_count"],
                "fetch_blue_black_count": score["fetch_blue_black_count"],
                "duplicate_pair_penalty": score["duplicate_pair_penalty"],
                "package_penalty": package_penalty(package),
                "direct_color_counts": json.dumps(score["direct_color_counts"], sort_keys=True),
                "fetch_color_counts": json.dumps(score["fetch_color_counts"], sort_keys=True),
                "pair_counts": json.dumps(score["pair_counts"], sort_keys=True),
            }
        )
    best_by_key: dict[str, dict[str, Any]] = {}
    for row in rows:
        key = str(row["pair_counts"])
        old = best_by_key.get(key)
        if old is None or (
            float(row["score_delta"]),
            -int(row["swap_count"]),
            -int(row["package_penalty"]),
            str(row["package"]),
        ) > (
            float(old["score_delta"]),
            -int(old["swap_count"]),
            -int(old["package_penalty"]),
            str(old["package"]),
        ):
            best_by_key[key] = row
    rows = list(best_by_key.values())
    rows.sort(
        key=lambda row: (
            -float(row["score_delta"]),
            int(row["swap_count"]),
            int(row["package_penalty"]),
            str(row["package"]),
        )
    )
    selected = rows[: args.top]

    write_csv(out_dir / "land_system_ranked.csv", rows)
    variant_lines: list[str] = []
    for index, row in enumerate(selected, start=1):
        label = f"landpkg_{index:03d}_{slug(row['swaps'])[:80]}"
        variant_lines.append(f"{label}: {row['swaps']}")
    (out_dir / "land_system_variants.txt").write_text("\n".join(variant_lines) + ("\n" if variant_lines else ""))

    analysis = {
        "deck_json": str(deck_path),
        "fetches": fetches,
        "current_typed_lands": list(current),
        "candidate_universe": list(universe),
        "current_score": current_score,
        "top": selected,
    }
    (out_dir / "land_system_analysis.json").write_text(json.dumps(analysis, indent=2, sort_keys=True) + "\n")
    lines = [
        "# Land System Analysis",
        "",
        f"Deck: `{deck_path}`",
        "",
        "## Current Typed Package",
        "",
        "- " + ", ".join(current),
        "",
        "## Fetches",
        "",
        "- " + ", ".join(fetches),
        "",
        "## Top Package Variants",
        "",
        "| rank | score delta | swaps | package |",
        "|---:|---:|---|---|",
    ]
    for index, row in enumerate(selected, start=1):
        lines.append(
            f"| {index} | {float(row['score_delta']):+.2f} | `{row['swaps']}` | {row['package']} |"
        )
    (out_dir / "land_system_analysis.md").write_text("\n".join(lines) + "\n")
    print(json.dumps({"rows": len(rows), "selected": len(selected), "out_dir": str(out_dir)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
