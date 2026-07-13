#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SIM = ROOT / "scripts" / "rhystic_belief_mulligan_sim.py"

DEFAULT_ACCEL_CUTS = (
    "Tataru Taru",
    "Nature's Chosen",
    "Infernal Plunge",
    "Strike It Rich",
    "Rite of Flame",
    "Rain of Filth",
    "Paradise Mantle",
    "Springleaf Drum",
    "Manamorphose",
    "Arcane Signet",
    "Mox Opal",
)
DEFAULT_ABSENT_TESTS = (
    "Neoform",
    "Idyllic Tutor",
    "Relic of Legends",
)
DEFAULT_INTERACTION_STAND_IN = "Swan Song"
COMMANDER_BANNED_OR_NOT_LEGAL = {
    "Chaos Emerald",
    "Jeweled Lotus",
}


@dataclass(frozen=True)
class Variant:
    name: str
    cut: str | None
    add: str | None
    swaps: tuple[tuple[str, str], ...] = ()


def slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9_]+", "_", text).strip("_").lower()


def read_moxfield(path: Path) -> dict[str, Any]:
    raw = path.read_text()
    return json.loads(raw[raw.find("{") :])


def mainboard_entries(payload: dict[str, Any]) -> dict[str, Any]:
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards")
    if not isinstance(cards, dict):
        raise ValueError("Moxfield payload does not contain boards.mainboard.cards")
    return cards


def mainboard_names(payload: dict[str, Any]) -> list[str]:
    names: list[str] = []
    for entry in mainboard_entries(payload).values():
        qty = int(entry.get("quantity", 1))
        names.extend([entry["card"]["name"]] * qty)
    return names


def write_variant_deck(
    source: Path,
    out_path: Path,
    cut: str | None,
    add: str | None,
    *,
    swaps: tuple[tuple[str, str], ...] = (),
) -> None:
    payload = read_moxfield(source)
    cards = mainboard_entries(payload)
    if not swaps:
        if cut is not None and add is not None:
            swaps = ((cut, add),)
        elif cut is None and add is None:
            swaps = ()
        else:
            raise ValueError("Both cut and add must be provided for a non-baseline variant.")

    if not swaps:
        out_path.write_text(json.dumps(payload, indent=2, sort_keys=True))
        return

    cuts = [pair[0] for pair in swaps]
    adds = [pair[1] for pair in swaps]
    if len(set(cuts)) != len(cuts):
        raise ValueError(f"Duplicate cut card in grouped variant: {cuts}")
    if len(set(adds)) != len(adds):
        raise ValueError(f"Duplicate add card in grouped variant: {adds}")
    for add_name in adds:
        if add_name in COMMANDER_BANNED_OR_NOT_LEGAL:
            raise ValueError(f"{add_name} is not legal for Commander testing in this harness.")

    replacements: list[tuple[str, dict[str, Any], str, str]] = []
    for cut_name, add_name in swaps:
        cut_key = None
        cut_entry = None
        for key, entry in cards.items():
            if entry.get("card", {}).get("name") == cut_name:
                cut_key = key
                cut_entry = entry
                break
        if cut_key is None or cut_entry is None:
            raise ValueError(f"Cut card is not in the mainboard: {cut_name}")
        replacements.append((cut_key, cut_entry, cut_name, add_name))

    cut_keys = {key for key, _entry, _cut_name, _add_name in replacements}
    existing = [entry.get("card", {}).get("name") for key, entry in cards.items() if key not in cut_keys]
    for add_name in adds:
        if add_name in existing:
            raise ValueError(f"Adding {add_name} would create a singleton violation after grouped cuts.")

    # Preserve the original mapping key/insertion position so paired stage
    # orders keep every unchanged card in the same shuffle slot.
    for cut_key, cut_entry, _cut_name, add_name in replacements:
        replacement = json.loads(json.dumps(cut_entry))
        replacement["card"]["name"] = add_name
        replacement["quantity"] = 1
        cards[cut_key] = replacement
    swap_label = ", ".join(f"{cut_name} -> {add_name}" for cut_name, add_name in swaps)
    payload["name"] = f"{payload.get('name', source.stem)} [{swap_label}]"
    out_path.write_text(json.dumps(payload, indent=2, sort_keys=True))


def parse_swap(text: str) -> Variant:
    label = ""
    body = text.strip()
    first_separator_positions = [pos for pos in (body.find("="), body.find("->")) if pos >= 0]
    first_separator = min(first_separator_positions) if first_separator_positions else -1
    colon = body.find(":")
    if 0 <= colon < first_separator:
        label = body[:colon].strip()
        body = body[colon + 1 :].strip()

    if ";" in body:
        raw_parts = body.split(";")
    else:
        comma_parts = [part.strip() for part in body.split(",") if part.strip()]
        raw_parts = comma_parts if len(comma_parts) > 1 and all("=" in part or "->" in part for part in comma_parts) else [body]

    swaps: list[tuple[str, str]] = []
    for raw_part in raw_parts:
        part = raw_part.strip()
        if not part:
            continue
        if "=" in part:
            cut, add = part.split("=", 1)
        elif "->" in part:
            cut, add = part.split("->", 1)
        else:
            raise argparse.ArgumentTypeError("Swap must use CUT=ADD or CUT->ADD")
        cut = cut.strip()
        add = add.strip()
        if not cut or not add:
            raise argparse.ArgumentTypeError("Swap cut and add must be non-empty")
        swaps.append((cut, add))

    if not swaps:
        raise argparse.ArgumentTypeError("Swap must include at least one cut/add pair.")
    if len(swaps) == 1:
        cut, add = swaps[0]
        return Variant(name=label or f"{slug(add)}_over_{slug(cut)}", cut=cut, add=add, swaps=tuple(swaps))

    cuts = " + ".join(cut for cut, _add in swaps)
    adds = " + ".join(add for _cut, add in swaps)
    default_name = "_and_".join(f"{slug(add)}_over_{slug(cut)}" for cut, add in swaps)
    return Variant(name=label or default_name, cut=cuts, add=adds, swaps=tuple(swaps))


def default_variants(payload: dict[str, Any], interaction_stand_in: str) -> list[Variant]:
    names = set(mainboard_names(payload))
    variants: list[Variant] = []
    for cut in DEFAULT_ACCEL_CUTS:
        if cut in names and interaction_stand_in not in names - {cut}:
            variants.append(Variant(f"{slug(interaction_stand_in)}_over_{slug(cut)}", cut, interaction_stand_in))
    for add in DEFAULT_ABSENT_TESTS:
        if add in names or add in COMMANDER_BANNED_OR_NOT_LEGAL:
            continue
        for cut in ("Tataru Taru", "Nature's Chosen", "Infernal Plunge", "Strike It Rich", "Rite of Flame"):
            if cut in names:
                variants.append(Variant(f"{slug(add)}_over_{slug(cut)}", cut, add))
    return variants


def run_sim(args: argparse.Namespace, deck_path: Path, json_out: Path, thresholds_json: Path | None = None) -> None:
    cmd = [
        sys.executable,
        str(SIM),
        "--target",
        args.target,
        "--deck-json",
        str(deck_path),
        "--threshold-hands",
        str(args.threshold_hands),
        "--eval-games",
        str(args.eval_games),
        "--samples-per-bottom",
        str(args.samples_per_bottom),
        "--validation-samples",
        str(args.validation_samples),
        "--state-limit",
        str(args.state_limit),
        "--workers",
        str(args.workers),
        "--seed",
        str(args.seed),
        "--gemstone-caverns-live-rate",
        str(args.gemstone_caverns_live_rate),
        "--gamble-mode",
        args.gamble_mode,
        "--json-out",
        str(json_out),
        "--suppress-json-stdout",
        "--trace-lines",
        "--include-game-records",
        "--paired-stage-orders",
    ]
    if args.engine_success_policy:
        cmd.extend(["--engine-success-policy", args.engine_success_policy])
    if thresholds_json is not None:
        cmd.extend(["--thresholds-json", str(thresholds_json)])
    if args.actual_rerun_state_limit:
        cmd.extend(["--actual-rerun-state-limit", str(args.actual_rerun_state_limit)])
    if args.normalize_no_caverns_gemstone_key:
        cmd.append("--normalize-no-caverns-gemstone-key")
    subprocess.run(cmd, cwd=ROOT, check=True)


def classify_engine(row: dict[str, Any]) -> str:
    label = row.get("engine_label")
    if label == "Rhystic Study":
        return "rhystic"
    if label == "Heartwood Storyteller":
        return "heartwood"
    actions = " | ".join(row.get("line_actions") or [])
    events = row.get("line_events") or {}
    engines = set(events.get("engine_casts") or [])
    targets = set(events.get("tutor_targets") or [])
    cards = set(row.get("line_cards") or [])
    if "Rhystic Study" in engines or "cast engine Rhystic Study" in actions:
        return "rhystic"
    if "Rhystic Study" in targets and "Rhystic Study" in cards:
        return "rhystic"
    if "Heartwood Storyteller" in engines:
        return "heartwood"
    if "Heartwood Storyteller" in targets or "Heartwood Storyteller" in cards or "Heartwood" in actions:
        return "heartwood"
    return "unknown"


def weighted_score(engine: str, turn: int | None, weights: dict[tuple[str, int], float]) -> float:
    if turn is None:
        return 0.0
    return weights.get((engine, int(turn)), 0.0)


def load_summary(path: Path, weights: dict[tuple[str, int], float]) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    ev = payload["evaluation"]
    records = ev.get("game_records") or []
    outcome_counts: dict[str, int] = {
        "rhystic_t1": 0,
        "rhystic_t2": 0,
        "heartwood_t1": 0,
        "heartwood_t2": 0,
        "unknown_t1": 0,
        "unknown_t2": 0,
    }
    total_score = 0.0
    for row in records:
        if not row.get("hit"):
            continue
        engine = classify_engine(row)
        turn = row.get("turn")
        key = f"{engine}_t{turn}"
        if key in outcome_counts:
            outcome_counts[key] += 1
        total_score += weighted_score(engine, turn, weights)

    games = int(ev["games"])
    successes = int(ev["successes"])
    return {
        "games": games,
        "successes": successes,
        "success_rate": successes / games if games else 0.0,
        "turn1": int((ev.get("turn_counts") or {}).get("1", 0)),
        "turn2": int((ev.get("turn_counts") or {}).get("2", 0)),
        "miss": int((ev.get("turn_counts") or {}).get("miss", 0)),
        "cap_misses": int(ev.get("cap_misses", 0)),
        "weighted_score": total_score,
        "weighted_score_per_game": total_score / games if games else 0.0,
        **outcome_counts,
        "json": str(path),
    }


def write_csv(path: Path, rows: list[dict[str, Any]]) -> None:
    fields = [
        "variant",
        "cut",
        "add",
        "successes",
        "success_rate",
        "success_delta",
        "rate_delta",
        "weighted_score_per_game",
        "weighted_delta_per_game",
        "rhystic_t1",
        "rhystic_t2",
        "heartwood_t1",
        "heartwood_t2",
        "unknown_t1",
        "unknown_t2",
        "turn1",
        "turn2",
        "miss",
        "cap_misses",
        "json",
    ]
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in rows:
            writer.writerow({field: row.get(field, "") for field in fields})


def fmt_pct(value: float) -> str:
    return f"{100 * value:.2f}%"


def write_markdown(path: Path, rows: list[dict[str, Any]], args: argparse.Namespace) -> None:
    lines = [
        "# Rhystic/Heartwood Quick Compare",
        "",
        f"Target: `{args.target}`. Paired stage orders enabled. Weights: Rhystic T1={args.rhystic_t1_weight}, Rhystic T2={args.rhystic_t2_weight}, Heartwood T1={args.heartwood_t1_weight}, Heartwood T2={args.heartwood_t2_weight}.",
        f"Run shape: threshold_hands={args.threshold_hands}, eval_games={args.eval_games}, samples_per_bottom={args.samples_per_bottom}, validation_samples={args.validation_samples}, state_limit={args.state_limit}.",
        f"Threshold policy: {'baseline thresholds reused for variants' if args.shared_thresholds else 'thresholds recomputed independently per variant'}.",
        "",
        "| Variant | Swap | Success | Delta | Weighted/game | Weighted delta | Rhystic T1 | Rhystic T2 | Heartwood T1 | Heartwood T2 | Caps |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in rows:
        swap = "baseline" if not row.get("cut") else f"{row['cut']} -> {row['add']}"
        lines.append(
            "| {variant} | {swap} | {successes}/{games} ({rate}) | {delta:+d} ({rate_delta:+.2%}) | {w:.3f} | {wd:+.3f} | {rt1} | {rt2} | {ht1} | {ht2} | {caps} |".format(
                variant=row["variant"],
                swap=swap,
                successes=row["successes"],
                games=row["games"],
                rate=fmt_pct(row["success_rate"]),
                delta=int(row["success_delta"]),
                rate_delta=float(row["rate_delta"]),
                w=float(row["weighted_score_per_game"]),
                wd=float(row["weighted_delta_per_game"]),
                rt1=row["rhystic_t1"],
                rt2=row["rhystic_t2"],
                ht1=row["heartwood_t1"],
                ht2=row["heartwood_t2"],
                caps=row["cap_misses"],
            )
        )
    path.write_text("\n".join(lines) + "\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--deck-json", default="data/moxfield_ggafAahWI3KipH2u48GdVQ.json")
    parser.add_argument("--out-dir", default="data/rhystic_study_turn12/quick_compare")
    parser.add_argument("--target", default="rhystic_heartwood", choices=("rhystic", "heartwood", "rhystic_heartwood"))
    parser.add_argument("--swap", action="append", type=parse_swap, default=[])
    parser.add_argument("--default-suite", action="store_true")
    parser.add_argument("--interaction-stand-in", default=DEFAULT_INTERACTION_STAND_IN)
    parser.add_argument("--threshold-hands", type=int, default=10)
    parser.add_argument("--eval-games", type=int, default=80)
    parser.add_argument("--samples-per-bottom", type=int, default=1)
    parser.add_argument("--validation-samples", type=int, default=1)
    parser.add_argument("--state-limit", type=int, default=15_000)
    parser.add_argument("--actual-rerun-state-limit", type=int, default=60_000)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--seed", type=int, default=2026062901)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--gamble-mode", choices=("off", "optimistic", "stochastic"), default="stochastic")
    parser.add_argument("--normalize-no-caverns-gemstone-key", action="store_true", default=True)
    parser.add_argument("--no-normalize-no-caverns-gemstone-key", dest="normalize_no_caverns_gemstone_key", action="store_false")
    parser.add_argument("--shared-thresholds", action="store_true", default=True)
    parser.add_argument("--independent-thresholds", dest="shared_thresholds", action="store_false")
    parser.add_argument("--rhystic-t1-weight", type=float, default=100.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=60.0)
    parser.add_argument("--heartwood-t1-weight", type=float, default=20.0)
    parser.add_argument("--heartwood-t2-weight", type=float, default=10.0)
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    deck_json = (ROOT / args.deck_json).resolve()
    out_dir = (ROOT / args.out_dir).resolve()
    deck_dir = out_dir / "decks"
    result_dir = out_dir / "results"
    deck_dir.mkdir(parents=True, exist_ok=True)
    result_dir.mkdir(parents=True, exist_ok=True)

    payload = read_moxfield(deck_json)
    variants = [Variant("baseline", None, None)]
    if args.default_suite:
        variants.extend(default_variants(payload, args.interaction_stand_in))
    variants.extend(args.swap)

    seen_names: set[str] = set()
    deduped: list[Variant] = []
    for variant in variants:
        if variant.name in seen_names:
            continue
        seen_names.add(variant.name)
        deduped.append(variant)

    weights = {
        ("rhystic", 1): args.rhystic_t1_weight,
        ("rhystic", 2): args.rhystic_t2_weight,
        ("heartwood", 1): args.heartwood_t1_weight,
        ("heartwood", 2): args.heartwood_t2_weight,
    }

    rows: list[dict[str, Any]] = []
    baseline_thresholds_json: Path | None = None
    for variant in deduped:
        variant_deck = deck_dir / f"{variant.name}.json"
        result_json = result_dir / f"{variant.name}.json"
        if args.force or not variant_deck.exists():
            write_variant_deck(deck_json, variant_deck, variant.cut, variant.add)
        if args.force or not result_json.exists():
            print(f"running {variant.name}", flush=True)
            thresholds_json = baseline_thresholds_json if args.shared_thresholds and variant.name != "baseline" else None
            run_sim(args, variant_deck, result_json, thresholds_json=thresholds_json)
        if variant.name == "baseline":
            baseline_thresholds_json = result_json
        summary = load_summary(result_json, weights)
        summary.update({"variant": variant.name, "cut": variant.cut or "", "add": variant.add or ""})
        rows.append(summary)

    baseline = rows[0]
    for row in rows:
        row["success_delta"] = int(row["successes"]) - int(baseline["successes"])
        row["rate_delta"] = float(row["success_rate"]) - float(baseline["success_rate"])
        row["weighted_delta_per_game"] = float(row["weighted_score_per_game"]) - float(baseline["weighted_score_per_game"])

    rows = [rows[0], *sorted(rows[1:], key=lambda row: (-float(row["weighted_delta_per_game"]), -int(row["success_delta"]), row["variant"]))]
    csv_path = out_dir / "quick_compare.csv"
    md_path = out_dir / "quick_compare.md"
    write_csv(csv_path, rows)
    write_markdown(md_path, rows, args)
    print(f"wrote {csv_path}")
    print(f"wrote {md_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
