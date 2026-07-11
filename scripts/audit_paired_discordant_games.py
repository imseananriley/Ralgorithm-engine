#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SIM = ROOT / "scripts" / "rhystic_belief_mulligan_sim.py"

LABEL_ENGINE = {
    "Rhystic Study": "rhystic",
    "Heartwood Storyteller": "heartwood",
    "Mystic Remora": "remora",
    "Smothering Tithe": "tithe",
}


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def localize_path(path_text: str | None) -> Path | None:
    if not path_text:
        return None
    path = Path(path_text)
    if path.exists():
        return path
    marker = "/workspace/rhystic_paired_rate_compare/"
    if path_text.startswith(marker):
        candidate = ROOT / path_text[len(marker) :]
        if candidate.exists():
            return candidate
    if not path.is_absolute():
        candidate = ROOT / path
        if candidate.exists():
            return candidate
    return None


def parse_swap_file(path: Path | None) -> dict[str, tuple[str, str]]:
    if path is None or not path.exists():
        return {}
    swaps: dict[str, tuple[str, str]] = {}
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or ":" not in line or "=" not in line:
            continue
        name, rest = line.split(":", 1)
        cut, add = rest.split("=", 1)
        swaps[name.strip()] = (cut.strip(), add.strip())
    return swaps


def records_by_game(payload: dict[str, Any]) -> dict[int, dict[str, Any]]:
    records = payload.get("evaluation", {}).get("game_records") or []
    return {int(row["game_index"]): row for row in records}


def score_record(row: dict[str, Any], weights: dict[tuple[str, int], float]) -> float:
    if not row.get("hit"):
        return 0.0
    try:
        turn = int(row.get("turn"))
    except (TypeError, ValueError):
        return 0.0
    engine = LABEL_ENGINE.get(str(row.get("engine_label") or ""))
    return weights.get((engine or "unknown", turn), 0.0)


def weights_from_payload(payload: dict[str, Any]) -> dict[tuple[str, int], float]:
    return {
        ("rhystic", 1): float(payload.get("rhystic_t1_weight", 1.0)),
        ("rhystic", 2): float(payload.get("rhystic_t2_weight", 0.75)),
        ("heartwood", 1): float(payload.get("heartwood_t1_weight", 0.65)),
        ("heartwood", 2): float(payload.get("heartwood_t2_weight", 0.50)),
    }


def category_for(base: dict[str, Any], cand: dict[str, Any], base_score: float, cand_score: float) -> str | None:
    base_hit = bool(base.get("hit"))
    cand_hit = bool(cand.get("hit"))
    if cand_hit and not base_hit:
        return "candidate_only"
    if base_hit and not cand_hit:
        return "baseline_only"
    if cand_score > base_score:
        return "score_upgrade"
    if cand_score < base_score:
        return "score_downgrade"
    return None


def result_thresholds_file(payload: dict[str, Any], audit_dir: Path, variant: str) -> Path:
    path = localize_path(payload.get("threshold_cache_path") or payload.get("thresholds_json"))
    if path is not None:
        return path
    threshold_payload = {
        "thresholds": payload.get("thresholds"),
        "threshold_rows": payload.get("threshold_rows"),
        "thresholds_by_gemstone_caverns_live": payload.get("thresholds_by_gemstone_caverns_live"),
        "threshold_rows_by_gemstone_caverns_live": payload.get("threshold_rows_by_gemstone_caverns_live"),
    }
    out = audit_dir / "thresholds" / f"{variant}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(threshold_payload, indent=2, sort_keys=True) + "\n")
    return out


def result_deck_file(payload: dict[str, Any], out_dir: Path, variant: str) -> Path:
    path = localize_path(payload.get("deck_json"))
    if path is not None:
        return path
    candidate = out_dir / "decks" / f"{variant}.json"
    if candidate.exists():
        return candidate
    raise FileNotFoundError(f"Could not locate deck JSON for {variant}")


def replay_seed_and_index(payload: dict[str, Any], record: dict[str, Any]) -> tuple[int, int]:
    shard_index = record.get("shard_index")
    local_index = int(record.get("shard_local_game_index", record["game_index"]))
    shard_results = payload.get("rust_full_sim_shard_results") or []
    if shard_index is not None:
        for shard in shard_results:
            if int(shard.get("shard_index", -1)) == int(shard_index):
                return int(shard["seed"]) - 1_000_000, local_index
    return int(payload.get("seed", 0)), int(record["game_index"])


def replay_record(
    *,
    out_dir: Path,
    audit_dir: Path,
    variant: str,
    payload: dict[str, Any],
    record: dict[str, Any],
) -> dict[str, Any] | None:
    cli_seed, local_index = replay_seed_and_index(payload, record)
    deck_json = result_deck_file(payload, out_dir, variant)
    thresholds_json = result_thresholds_file(payload, audit_dir, variant)
    fd, temp_name = tempfile.mkstemp(prefix=f"{variant}_{local_index}_", suffix=".json", dir=audit_dir)
    os.close(fd)
    temp_path = Path(temp_name)
    cmd = [
        sys.executable,
        str(SIM),
        "--target",
        str(payload.get("target", "rhystic_heartwood")),
        "--deck-json",
        str(deck_json),
        "--thresholds-json",
        str(thresholds_json),
        "--eval-games",
        str(local_index + 1),
        "--samples-per-bottom",
        str(payload.get("samples_per_bottom", 2)),
        "--validation-samples",
        str(payload.get("validation_samples", 4)),
        "--state-limit",
        str(payload.get("state_limit", 40000)),
        "--actual-rerun-state-limit",
        str(payload.get("actual_rerun_state_limit", 120000)),
        "--workers",
        "1",
        "--chunks-per-worker",
        "1",
        "--seed",
        str(cli_seed),
        "--gemstone-caverns-live-rate",
        str(payload.get("gemstone_caverns_live_rate", 0.75)),
        "--engine-success-policy",
        str(payload.get("engine_success_policy", "resilient")),
        "--gamble-mode",
        str(payload.get("gamble_mode", "stochastic")),
        "--rust-full-sim",
        "--weighted-policy-ev",
        "--rhystic-t1-weight",
        str(payload.get("rhystic_t1_weight", 1.0)),
        "--rhystic-t2-weight",
        str(payload.get("rhystic_t2_weight", 0.75)),
        "--heartwood-t1-weight",
        str(payload.get("heartwood_t1_weight", 0.65)),
        "--heartwood-t2-weight",
        str(payload.get("heartwood_t2_weight", 0.50)),
        "--include-game-records",
        "--trace-lines",
        "--json-out",
        str(temp_path),
        "--suppress-json-stdout",
    ]
    if payload.get("normalize_no_caverns_gemstone_key", True):
        cmd.append("--normalize-no-caverns-gemstone-key")
    if payload.get("adaptive_threshold_sampling", False):
        cmd.append("--adaptive-threshold-sampling")
    env = os.environ.copy()
    env["RHYSTIC_SIMPLIFIED_GAMBLE"] = "1"
    subprocess.run(cmd, cwd=ROOT, env=env, check=True, stdout=subprocess.DEVNULL)
    replay_payload = load_json(temp_path)
    replay_records = replay_payload.get("evaluation", {}).get("game_records") or []
    for row in replay_records:
        if int(row["game_index"]) == local_index:
            return row
    return None


def list_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, list):
        return " | ".join(str(item) for item in value)
    return str(value)


def classify_trace(row: dict[str, Any], cut: str, add: str) -> str:
    candidate_cards = set(str(row.get("candidate_line_cards", "")).split(" | "))
    baseline_cards = set(str(row.get("baseline_line_cards", "")).split(" | "))
    if row["category"] in {"candidate_only", "score_upgrade"} and add and add in candidate_cards:
        return "real_added_card_line"
    if row["category"] in {"baseline_only", "score_downgrade"} and cut and cut in baseline_cards:
        return "real_lost_cut_card_line"
    if row.get("baseline_capped") or row.get("candidate_capped"):
        return "cap_artifact_risk"
    if row["category"] in {"candidate_only", "baseline_only"}:
        return "indirect_or_policy_shift_inspect"
    return "score_shift_inspect"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--swap-file", default="data/rhystic_study_turn12/mild_objective_stage1_paired_swaps_20260704.txt")
    parser.add_argument("--audit-dir", default=None)
    parser.add_argument("--only-variant", action="append", default=[])
    parser.add_argument("--trace-limit-per-variant", type=int, default=0)
    parser.add_argument("--trace-all", action="store_true")
    args = parser.parse_args()

    out_dir = (ROOT / args.out_dir).resolve() if not Path(args.out_dir).is_absolute() else Path(args.out_dir)
    audit_dir = Path(args.audit_dir) if args.audit_dir else out_dir / "discordant_audit"
    if not audit_dir.is_absolute():
        audit_dir = ROOT / audit_dir
    audit_dir.mkdir(parents=True, exist_ok=True)
    swap_map = parse_swap_file((ROOT / args.swap_file).resolve())

    baseline_path = out_dir / "results" / "baseline.json"
    if not baseline_path.exists():
        raise FileNotFoundError(f"Baseline result is not available yet: {baseline_path}")
    baseline = load_json(baseline_path)
    base_records = records_by_game(baseline)
    if not base_records:
        raise ValueError(f"Baseline has no game records: {baseline_path}")
    weights = weights_from_payload(baseline)

    variant_paths = sorted((out_dir / "results").glob("*.json"))
    variant_paths = [path for path in variant_paths if path.stem != "baseline"]
    if args.only_variant:
        wanted = set(args.only_variant)
        variant_paths = [path for path in variant_paths if path.stem in wanted]

    detail_rows: list[dict[str, Any]] = []
    summary_rows: list[dict[str, Any]] = []
    for variant_path in variant_paths:
        variant = variant_path.stem
        candidate = load_json(variant_path)
        candidate_records = records_by_game(candidate)
        if not candidate_records:
            continue
        cut, add = swap_map.get(variant, ("", ""))
        traced = 0
        counts = {
            "candidate_only": 0,
            "baseline_only": 0,
            "score_upgrade": 0,
            "score_downgrade": 0,
        }
        trace_class_counts: dict[str, int] = {}
        for game_index in sorted(set(base_records) & set(candidate_records)):
            base = base_records[game_index]
            cand = candidate_records[game_index]
            base_score = score_record(base, weights)
            cand_score = score_record(cand, weights)
            category = category_for(base, cand, base_score, cand_score)
            if category is None:
                continue
            counts[category] += 1
            row: dict[str, Any] = {
                "variant": variant,
                "cut": cut,
                "add": add,
                "game_index": game_index,
                "category": category,
                "baseline_hit": bool(base.get("hit")),
                "candidate_hit": bool(cand.get("hit")),
                "baseline_score": base_score,
                "candidate_score": cand_score,
                "score_delta": cand_score - base_score,
                "baseline_turn": base.get("turn"),
                "candidate_turn": cand.get("turn"),
                "baseline_engine_label": base.get("engine_label"),
                "candidate_engine_label": cand.get("engine_label"),
                "baseline_capped": bool(base.get("capped") or base.get("trace_capped")),
                "candidate_capped": bool(cand.get("capped") or cand.get("trace_capped")),
                "baseline_stage": base.get("stage"),
                "candidate_stage": cand.get("stage"),
                "baseline_bottom_count": base.get("bottom_count"),
                "candidate_bottom_count": cand.get("bottom_count"),
                "baseline_shard": base.get("shard_index"),
                "candidate_shard": cand.get("shard_index"),
                "baseline_shard_local_game_index": base.get("shard_local_game_index"),
                "candidate_shard_local_game_index": cand.get("shard_local_game_index"),
                "trace_class": "",
                "baseline_line_cards": "",
                "candidate_line_cards": "",
                "baseline_line_actions": "",
                "candidate_line_actions": "",
            }
            should_trace = args.trace_all or traced < args.trace_limit_per_variant
            if should_trace:
                base_trace = replay_record(out_dir=out_dir, audit_dir=audit_dir, variant="baseline", payload=baseline, record=base)
                cand_trace = replay_record(out_dir=out_dir, audit_dir=audit_dir, variant=variant, payload=candidate, record=cand)
                traced += 1
                if base_trace:
                    row["baseline_line_cards"] = list_text(base_trace.get("line_cards"))
                    row["baseline_line_actions"] = list_text(base_trace.get("line_actions"))
                if cand_trace:
                    row["candidate_line_cards"] = list_text(cand_trace.get("line_cards"))
                    row["candidate_line_actions"] = list_text(cand_trace.get("line_actions"))
                row["trace_class"] = classify_trace(row, cut, add)
                trace_class_counts[row["trace_class"]] = trace_class_counts.get(row["trace_class"], 0) + 1
            detail_rows.append(row)
        summary_rows.append(
            {
                "variant": variant,
                "cut": cut,
                "add": add,
                "discordant_or_score_changed_games": sum(counts.values()),
                **counts,
                "traced_games": traced,
                "trace_classes": json.dumps(trace_class_counts, sort_keys=True),
            }
        )

    detail_path = audit_dir / "discordant_games.csv"
    summary_path = audit_dir / "discordant_summary.csv"
    fields = [
        "variant",
        "cut",
        "add",
        "game_index",
        "category",
        "baseline_hit",
        "candidate_hit",
        "baseline_score",
        "candidate_score",
        "score_delta",
        "baseline_turn",
        "candidate_turn",
        "baseline_engine_label",
        "candidate_engine_label",
        "baseline_capped",
        "candidate_capped",
        "baseline_stage",
        "candidate_stage",
        "baseline_bottom_count",
        "candidate_bottom_count",
        "baseline_shard",
        "candidate_shard",
        "baseline_shard_local_game_index",
        "candidate_shard_local_game_index",
        "trace_class",
        "baseline_line_cards",
        "candidate_line_cards",
        "baseline_line_actions",
        "candidate_line_actions",
    ]
    with detail_path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        for row in detail_rows:
            writer.writerow(row)
    with summary_path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(summary_rows[0].keys()) if summary_rows else ["variant"])
        writer.writeheader()
        for row in summary_rows:
            writer.writerow(row)
    print(f"wrote {summary_path}")
    print(f"wrote {detail_path}")
    print(f"variants={len(summary_rows)} changed_rows={len(detail_rows)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
