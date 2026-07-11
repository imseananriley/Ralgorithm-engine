#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import shutil
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REGISTRY = ROOT / "data/rhystic_study_turn12/experiment_loop/registry.json"
DEFAULT_OUT_DIR = ROOT / "data/rhystic_study_turn12/experiment_loop"


def now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def repo_path(path: str | Path) -> Path:
    p = Path(path)
    if not p.is_absolute():
        p = ROOT / p
    return p


def rel(path: str | Path) -> str:
    p = repo_path(path).resolve()
    try:
        return p.relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{hashlib.blake2b(str(path).encode(), digest_size=6).hexdigest()}.tmp")
    tmp.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    tmp.replace(path)


def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle))


def write_csv(path: Path, rows: list[dict[str, Any]], fields: list[str] | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if not rows:
        path.write_text("")
        return
    if fields is None:
        fields = []
        for row in rows:
            for key in row:
                if key not in fields:
                    fields.append(key)
    with path.open("w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def fnum(value: Any, default: float = 0.0) -> float:
    if value in (None, ""):
        return default
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def inum(value: Any, default: int = 0) -> int:
    if value in (None, ""):
        return default
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return default


def candidate_key(cut: str, add: str) -> str:
    payload = json.dumps({"cut": cut, "add": add}, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.blake2b(payload, digest_size=8).hexdigest()


def swap_label(cut: str, add: str) -> str:
    return f"{cut} -> {add}"


def load_registry(path: Path) -> dict[str, Any]:
    if path.exists():
        return read_json(path)
    return {
        "schema_version": 1,
        "created_at": now_iso(),
        "updated_at": now_iso(),
        "champion": {},
        "constraints": {
            "commander_legal": True,
            "singleton": True,
            "deck_size": 99,
            "protected_cuts": [],
            "notes": [],
        },
        "candidates": {},
        "raw_sources": {},
        "policy_sources": {},
        "decisions": [],
    }


def deck_names_from_moxfield(path: Path) -> list[str]:
    payload = read_json(path)
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards") or {}
    names: list[str] = []
    for entry in cards.values():
        qty = int(entry.get("quantity", 1))
        names.extend([entry["card"]["name"]] * qty)
    return names


def deck_names_from_working(path: Path) -> list[str]:
    payload = read_json(path)
    if isinstance(payload.get("deck"), list):
        return list(payload["deck"])
    return deck_names_from_moxfield(path)


def deck_hash(path: Path) -> str:
    names = sorted(deck_names_from_working(path))
    encoded = json.dumps(names, separators=(",", ":")).encode()
    return hashlib.blake2b(encoded, digest_size=12).hexdigest()


def ensure_candidate(registry: dict[str, Any], cut: str, add: str) -> dict[str, Any]:
    key = candidate_key(cut, add)
    candidates = registry.setdefault("candidates", {})
    if key not in candidates:
        candidates[key] = {
            "key": key,
            "cut": cut,
            "add": add,
            "swap": swap_label(cut, add),
            "created_at": now_iso(),
            "tags": [],
            "notes": [],
            "raw_runs": [],
            "policy_runs": [],
            "pooled_policy": {},
            "classification": "new",
            "classification_reason": "",
        }
    return candidates[key]


def mean_ci(values: list[float], z: float = 1.96) -> tuple[float, float, float, float]:
    if not values:
        return 0.0, 0.0, 0.0, 0.0
    mean = sum(values) / len(values)
    if len(values) < 2:
        return mean, 0.0, mean, mean
    variance = sum((value - mean) ** 2 for value in values) / (len(values) - 1)
    se = math.sqrt(variance / len(values))
    return mean, se, mean - z * se, mean + z * se


def binom_two_sided(k: int, n: int) -> float | None:
    if n <= 0:
        return None
    from math import comb

    probs = [comb(n, i) * (0.5**n) for i in range(n + 1)]
    threshold = probs[k]
    return min(1.0, sum(prob for prob in probs if prob <= threshold + 1e-18))


def transition_summary(rows: list[dict[str, str]]) -> str:
    counts: Counter[tuple[str, str, str, str, str]] = Counter()
    for row in rows:
        score = fnum(row.get("score_delta"))
        if score == 0:
            continue
        direction = "positive" if score > 0 else "negative"
        counts[
            (
                row.get("baseline_engine_label") or "none",
                row.get("candidate_engine_label") or "none",
                row.get("baseline_turn") or "miss",
                row.get("candidate_turn") or "miss",
                direction,
            )
        ] += 1
    return "; ".join(
        f"{base}@{bt}->{cand}@{ct}/{direction}:{count}"
        for (base, cand, bt, ct, direction), count in counts.most_common(8)
    )


def pool_candidate_policy(candidate: dict[str, Any]) -> dict[str, Any]:
    detail_rows: list[dict[str, str]] = []
    for run in candidate.get("policy_runs", []):
        if run.get("exclude_from_pool"):
            continue
        detail_path = run.get("details_path")
        variant = run.get("variant")
        if not detail_path or not variant:
            continue
        path = repo_path(detail_path)
        if not path.exists():
            continue
        for row in read_csv(path):
            if row.get("variant") == variant:
                detail_rows.append(row)
    if not detail_rows:
        return {}

    rates = [fnum(row.get("rate_delta")) for row in detail_rows]
    scores = [fnum(row.get("score_delta")) for row in detail_rows]
    rate_mean, rate_se, rate_low, rate_high = mean_ci(rates)
    score_mean, score_se, score_low, score_high = mean_ci(scores)
    candidate_only = sum(
        1
        for row in detail_rows
        if row.get("candidate_hit") == "True" and row.get("baseline_hit") == "False"
    )
    baseline_only = sum(
        1
        for row in detail_rows
        if row.get("candidate_hit") == "False" and row.get("baseline_hit") == "True"
    )
    pooled = {
        "games": len(detail_rows),
        "rate_delta": rate_mean,
        "rate_se": rate_se,
        "rate_ci95_low": rate_low,
        "rate_ci95_high": rate_high,
        "score_delta": score_mean,
        "score_se": score_se,
        "score_ci95_low": score_low,
        "score_ci95_high": score_high,
        "candidate_only_successes": candidate_only,
        "baseline_only_successes": baseline_only,
        "mcnemar_exact_p": binom_two_sided(min(candidate_only, baseline_only), candidate_only + baseline_only),
        "score_positive": sum(1 for value in scores if value > 0),
        "score_negative": sum(1 for value in scores if value < 0),
        "score_tied": sum(1 for value in scores if value == 0),
        "transition_summary": transition_summary(detail_rows),
    }
    return pooled


def classify_candidate(candidate: dict[str, Any], *, min_accept_games: int) -> tuple[str, str]:
    pooled = candidate.get("pooled_policy") or {}
    raw_runs = candidate.get("raw_runs") or []
    policy_runs = candidate.get("policy_runs") or []
    protected = "protected_cut" in set(candidate.get("tags") or [])

    if pooled:
        games = inum(pooled.get("games"))
        score_low = fnum(pooled.get("score_ci95_low"))
        score_high = fnum(pooled.get("score_ci95_high"))
        rate_low = fnum(pooled.get("rate_ci95_low"))
        score = fnum(pooled.get("score_delta"))
        if games >= min_accept_games and score_low > 0 and rate_low > -0.0025:
            if protected:
                return "statistical_candidate_strategic_review", (
                    f"pooled score CI clears zero over {games} games, but cut is protected"
                )
            return "promote_challenger", f"pooled score CI clears zero over {games} games"
        if score_high < 0:
            return "reject_policy", "pooled policy score CI is negative"
        if games >= min_accept_games and 0 < score < 0.1:
            return "deprioritize_small_effect", "pooled mean is positive but small and CI is unresolved"
        if games >= min_accept_games and score > 0:
            if protected:
                return "strategic_review_policy", "protected cut has unresolved positive policy evidence"
            return "needs_more_policy", "pooled mean is positive but CI is unresolved"
        if policy_runs:
            if protected:
                return "strategic_review_policy", "protected cut has unresolved policy evidence"
            return "watch_policy", "policy evidence exists but is unresolved"

    pilot_runs = [run for run in policy_runs if run.get("exclude_from_pool")]
    if pilot_runs:
        best_pilot = max(pilot_runs, key=lambda run: fnum(run.get("score_delta")), default={})
        worst_high = min(fnum(run.get("score_ci95_high")) for run in pilot_runs)
        if worst_high < 0:
            return "deprioritize_pilot_negative", "exploratory policy score CI was negative"
        if fnum(best_pilot.get("score_delta")) > 0:
            if protected:
                return "strategic_review_policy", "protected cut has positive exploratory policy evidence"
            return "watch_policy", "exploratory policy mean is positive but not validation-pooled"

    best_raw_low = None
    best_raw = None
    for run in raw_runs:
        low = fnum(run.get("raw_ci95_low"))
        delta = fnum(run.get("raw_delta"))
        if best_raw_low is None or low > best_raw_low:
            best_raw_low = low
            best_raw = run
        if low > 0:
            if protected:
                return "raw_strategic_review", "raw-delta CI clears zero, but cut is protected"
            return "needs_policy", "raw-delta CI clears zero; needs full-policy validation"
    if best_raw and fnum(best_raw.get("raw_delta")) > 0:
        if protected:
            return "raw_strategic_review", "raw-delta mean is positive, but cut is protected"
        return "watch_raw", "raw-delta mean is positive but unresolved"
    return "new", "no actionable evidence yet"


def init_registry(args: argparse.Namespace) -> int:
    path = repo_path(args.registry)
    registry = load_registry(path)
    if args.reset_evidence:
        registry["candidates"] = {}
        registry["raw_sources"] = {}
        registry["policy_sources"] = {}
        registry["decisions"] = []
    champion = repo_path(args.champion_deck)
    registry["champion"] = {
        "name": args.name,
        "deck_json": rel(champion),
        "deck_hash": deck_hash(champion),
        "set_at": now_iso(),
        "notes": args.note or "",
    }
    if args.protected_cut:
        protected = registry.setdefault("constraints", {}).setdefault("protected_cuts", [])
        for card in args.protected_cut:
            if card not in protected:
                protected.append(card)
    registry["updated_at"] = now_iso()
    write_json(path, registry)
    print(path)
    return 0


def ingest_raw(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    csv_path = repo_path(args.path)
    source_id = args.label or csv_path.parent.name
    if source_id in registry.setdefault("raw_sources", {}) and not args.force:
        print(f"raw source already ingested: {source_id}")
        return 0
    rows = read_csv(csv_path)
    count = 0
    protected_cuts = set(registry.get("constraints", {}).get("protected_cuts", []))
    for row in rows:
        cut = row.get("cut") or ""
        add = row.get("add") or ""
        if not cut or not add:
            continue
        candidate = ensure_candidate(registry, cut, add)
        if cut in protected_cuts and "protected_cut" not in candidate.setdefault("tags", []):
            candidate["tags"].append("protected_cut")
        run = {
            "source_id": source_id,
            "path": rel(csv_path),
            "raw_delta": fnum(row.get("mean_delta_delta_method")),
            "raw_se": fnum(row.get("delta_method_se")),
            "raw_ci95_low": fnum(row.get("ci95_low"), fnum(row.get("mean_delta_delta_method")) - 1.96 * fnum(row.get("delta_method_se"))),
            "raw_ci95_high": fnum(row.get("ci95_high"), fnum(row.get("mean_delta_delta_method")) + 1.96 * fnum(row.get("delta_method_se"))),
            "raw_hit_delta": fnum(row.get("hit_rate_delta")),
            "relevance_rate": fnum(row.get("relevance_rate")),
            "samples": inum(row.get("samples")),
            "ingested_at": now_iso(),
        }
        candidate["raw_runs"] = [old for old in candidate.get("raw_runs", []) if old.get("source_id") != source_id]
        candidate["raw_runs"].append(run)
        count += 1
    registry["raw_sources"][source_id] = {"path": rel(csv_path), "rows": count, "ingested_at": now_iso()}
    registry["updated_at"] = now_iso()
    write_json(registry_path, registry)
    print(f"ingested raw rows: {count}")
    return 0


def ingest_policy(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    summary_path = repo_path(args.summary)
    details_path = repo_path(args.details) if args.details else summary_path.with_name("paired_game_deltas.csv")
    source_id = args.label or summary_path.parent.name
    if source_id in registry.setdefault("policy_sources", {}) and not args.force:
        print(f"policy source already ingested: {source_id}")
        return 0
    rows = read_csv(summary_path)
    count = 0
    protected_cuts = set(registry.get("constraints", {}).get("protected_cuts", []))
    for row in rows:
        cut = row.get("cut") or ""
        add = row.get("add") or ""
        if not cut or not add:
            continue
        candidate = ensure_candidate(registry, cut, add)
        if cut in protected_cuts and "protected_cut" not in candidate.setdefault("tags", []):
            candidate["tags"].append("protected_cut")
        run = {
            "source_id": source_id,
            "summary_path": rel(summary_path),
            "details_path": rel(details_path) if details_path.exists() else "",
            "variant": row.get("variant") or "",
            "role": args.role,
            "exclude_from_pool": args.exclude_from_pool or args.role != "validation",
            "games": inum(row.get("games")),
            "baseline_rate": fnum(row.get("baseline_rate")),
            "candidate_rate": fnum(row.get("candidate_rate")),
            "rate_delta": fnum(row.get("rate_delta")),
            "rate_ci95_low": fnum(row.get("rate_delta_ci_low")),
            "rate_ci95_high": fnum(row.get("rate_delta_ci_high")),
            "score_delta": fnum(row.get("score_delta_mean")),
            "score_ci95_low": fnum(row.get("score_delta_ci_low")),
            "score_ci95_high": fnum(row.get("score_delta_ci_high")),
            "candidate_only_successes": inum(row.get("candidate_only_successes")),
            "baseline_only_successes": inum(row.get("baseline_only_successes")),
            "mcnemar_p": fnum(row.get("mcnemar_p"), math.nan),
            "ingested_at": now_iso(),
        }
        candidate["policy_runs"] = [old for old in candidate.get("policy_runs", []) if old.get("source_id") != source_id]
        candidate["policy_runs"].append(run)
        count += 1
    registry["policy_sources"][source_id] = {
        "summary_path": rel(summary_path),
        "details_path": rel(details_path) if details_path.exists() else "",
        "role": args.role,
        "exclude_from_pool": args.exclude_from_pool or args.role != "validation",
        "rows": count,
        "ingested_at": now_iso(),
    }
    registry["updated_at"] = now_iso()
    write_json(registry_path, registry)
    print(f"ingested policy rows: {count}")
    return 0


def prune_source(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    source_id = args.source_id
    kind = args.kind
    source_bucket = "policy_sources" if kind == "policy" else "raw_sources"
    run_bucket = "policy_runs" if kind == "policy" else "raw_runs"
    if source_id not in registry.get(source_bucket, {}):
        print(f"{kind} source not present: {source_id}")
        return 0

    registry[source_bucket].pop(source_id, None)
    removed = 0
    for candidate in registry.get("candidates", {}).values():
        before = len(candidate.get(run_bucket, []))
        candidate[run_bucket] = [
            run for run in candidate.get(run_bucket, []) if run.get("source_id") != source_id
        ]
        removed += before - len(candidate.get(run_bucket, []))
    registry["updated_at"] = now_iso()
    write_json(registry_path, registry)
    print(f"removed {removed} {kind} candidate runs from source {source_id}")
    return 0


def refresh_registry(args: argparse.Namespace) -> int:
    path = repo_path(args.registry)
    registry = load_registry(path)
    protected_cuts = set(registry.get("constraints", {}).get("protected_cuts", []))
    for candidate in registry.get("candidates", {}).values():
        tags = [tag for tag in candidate.get("tags", []) if tag != "protected_cut"]
        if candidate.get("cut") in protected_cuts:
            tags.append("protected_cut")
        candidate["tags"] = tags
        candidate["pooled_policy"] = pool_candidate_policy(candidate)
        status, reason = classify_candidate(candidate, min_accept_games=args.min_accept_games)
        candidate["classification"] = status
        candidate["classification_reason"] = reason
    registry["updated_at"] = now_iso()
    write_json(path, registry)
    print(path)
    return 0


def candidate_status_rows(registry: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for candidate in registry.get("candidates", {}).values():
        pooled = candidate.get("pooled_policy") or {}
        raw_runs = candidate.get("raw_runs") or []
        best_raw = max(raw_runs, key=lambda row: fnum(row.get("raw_delta")), default={})
        rows.append(
            {
                "classification": candidate.get("classification", ""),
                "reason": candidate.get("classification_reason", ""),
                "swap": candidate.get("swap", ""),
                "cut": candidate.get("cut", ""),
                "add": candidate.get("add", ""),
                "tags": ";".join(candidate.get("tags", [])),
                "raw_runs": len(raw_runs),
                "best_raw_delta": fnum(best_raw.get("raw_delta")),
                "best_raw_ci95_low": fnum(best_raw.get("raw_ci95_low")),
                "policy_runs": len(candidate.get("policy_runs", [])),
                "pooled_games": inum(pooled.get("games")),
                "pooled_rate_delta": fnum(pooled.get("rate_delta")),
                "pooled_rate_ci95_low": fnum(pooled.get("rate_ci95_low")),
                "pooled_rate_ci95_high": fnum(pooled.get("rate_ci95_high")),
                "pooled_score_delta": fnum(pooled.get("score_delta")),
                "pooled_score_ci95_low": fnum(pooled.get("score_ci95_low")),
                "pooled_score_ci95_high": fnum(pooled.get("score_ci95_high")),
                "candidate_only_successes": inum(pooled.get("candidate_only_successes")),
                "baseline_only_successes": inum(pooled.get("baseline_only_successes")),
                "mcnemar_exact_p": pooled.get("mcnemar_exact_p", ""),
            }
        )
    order = {
        "promote_challenger": 0,
        "statistical_candidate_strategic_review": 1,
        "needs_more_policy": 2,
        "needs_policy": 3,
        "strategic_review_policy": 4,
        "raw_strategic_review": 5,
        "watch_policy": 6,
        "watch_raw": 7,
        "deprioritize_small_effect": 8,
        "deprioritize_pilot_negative": 9,
        "reject_policy": 10,
        "new": 11,
    }
    rows.sort(
        key=lambda row: (
            order.get(str(row["classification"]), 7),
            -fnum(row["pooled_score_delta"]),
            -fnum(row["best_raw_delta"]),
            str(row["swap"]),
        )
    )
    return rows


def status(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    out_dir = repo_path(args.out_dir)
    rows = candidate_status_rows(registry)
    fields = [
        "classification",
        "reason",
        "swap",
        "cut",
        "add",
        "tags",
        "raw_runs",
        "best_raw_delta",
        "best_raw_ci95_low",
        "policy_runs",
        "pooled_games",
        "pooled_rate_delta",
        "pooled_rate_ci95_low",
        "pooled_rate_ci95_high",
        "pooled_score_delta",
        "pooled_score_ci95_low",
        "pooled_score_ci95_high",
        "candidate_only_successes",
        "baseline_only_successes",
        "mcnemar_exact_p",
    ]
    csv_path = out_dir / "candidate_status.csv"
    write_csv(csv_path, rows, fields)
    md_path = out_dir / "candidate_status.md"
    lines = [
        "# Rhystic Experiment Loop Status",
        "",
        f"Registry: `{rel(registry_path)}`",
        f"Champion deck: `{registry.get('champion', {}).get('deck_json', '')}`",
        "",
        "| class | swap | pooled games | rate delta | score delta | reason |",
        "|---|---|---:|---:|---:|---|",
    ]
    for row in rows[: args.limit]:
        lines.append(
            "| {cls} | {swap} | {games} | {rate:+.3%} | {score:+.4f} | {reason} |".format(
                cls=row["classification"],
                swap=row["swap"],
                games=row["pooled_games"],
                rate=fnum(row["pooled_rate_delta"]),
                score=fnum(row["pooled_score_delta"]),
                reason=row["reason"],
            )
        )
    md_path.write_text("\n".join(lines) + "\n")
    print(csv_path)
    print(md_path)
    return 0


def suggest(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    out_dir = repo_path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = candidate_status_rows(registry)

    policy_candidates = [
        row for row in rows if row["classification"] in {"needs_policy", "needs_more_policy", "watch_policy"}
    ][: args.policy_limit]
    promotions = [
        row for row in rows if row["classification"] in {"promote_challenger", "statistical_candidate_strategic_review"}
    ]
    raw_watch = [row for row in rows if row["classification"] == "watch_raw"][: args.raw_limit]

    policy_swap_path = out_dir / "next_policy_swaps.txt"
    policy_swap_path.write_text("\n".join(f"{row['add'].lower().replace(' ', '_')}_over_{row['cut'].lower().replace(' ', '_')}: {row['swap']}" for row in policy_candidates) + ("\n" if policy_candidates else ""))
    raw_watch_path = out_dir / "raw_watchlist.csv"
    write_csv(raw_watch_path, raw_watch)

    paired_args = [
        "--deck-json",
        registry.get("champion", {}).get("deck_json", ""),
        "--target rhystic_heartwood",
        "--threshold-hands 80",
        "--eval-games 3000",
        "--samples-per-bottom 2",
        "--validation-samples 2",
        "--state-limit 20000",
        "--actual-rerun-state-limit 60000",
        "--workers 5",
        "--chunks-per-worker 16",
        "--gemstone-caverns-live-rate 0.75",
        "--engine-success-policy resilient",
        "--gamble-mode stochastic",
        "--normalize-no-caverns-gemstone-key",
        "--weighted-policy-ev",
        "--rhystic-t1-weight 100",
        "--rhystic-t2-weight 60",
        "--heartwood-t1-weight 20",
        "--heartwood-t2-weight 10",
        "--rust-full-sim",
        "--rust-full-sim-games-per-shard 500",
        "--rust-full-sim-shard-workers 5",
        "--independent-thresholds",
        "--bootstrap-samples 500",
    ]
    command = ""
    if policy_candidates:
        swaps = " ".join(f"--swap {json.dumps(row['swap'])}" for row in policy_candidates)
        command = (
            "RHYSTIC_SIMPLIFIED_GAMBLE=1 nice -n 20 python3 scripts/rhystic_paired_rate_compare.py "
            f"--out-dir data/rhystic_study_turn12/experiment_loop/next_policy_batch_$(date +%Y%m%d_%H%M%S) "
            + " ".join(paired_args)
            + " "
            + swaps
        )
    raw_command = (
        "python3 scripts/rhystic_raw_delta_stream.py "
        f"--deck-json {registry.get('champion', {}).get('deck_json', '')} "
        "--out-dir data/rhystic_study_turn12/experiment_loop/raw_delta_refresh_$(date +%Y%m%d_%H%M%S) "
        "--from-registry-universe --exclude-protected "
        "--samples-per-stage 500 --seed $(date +%Y%m%d%H) "
        "--shards 3 --workers 2 --prefix registry_refresh "
        "--ingest-registry --refresh-registry --run"
    )

    md_path = out_dir / "next_actions.md"
    lines = [
        "# Next Experiment Actions",
        "",
        "## Promotion Candidates",
    ]
    if promotions:
        for row in promotions:
            lines.append(
                f"- `{row['swap']}`: {row['classification']}; score {fnum(row['pooled_score_delta']):+.4f} "
                f"CI [{fnum(row['pooled_score_ci95_low']):+.4f}, {fnum(row['pooled_score_ci95_high']):+.4f}]"
            )
    else:
        lines.append("- None currently meets promotion criteria.")
    lines.extend(["", "## Next Policy Batch"])
    if policy_candidates:
        for row in policy_candidates:
            lines.append(f"- `{row['swap']}` ({row['classification']})")
        lines.extend(["", "Command:", "", "```bash", command, "```"])
    else:
        lines.append("- No unresolved policy candidates. Generate a fresh raw-delta screen from the current champion.")
        lines.extend(["", "Conservative local command:", "", "```bash", raw_command, "```"])
    lines.extend(["", "## Compute-Saving Rules Applied"])
    lines.extend(
        [
            "- Do not rerun candidates with pooled negative policy CIs.",
            "- Reuse baseline result JSON when extending an existing seed.",
            "- Prefer one-candidate replication when a result is narrow.",
            "- Use raw-delta only to prioritize policy tests, not to promote directly.",
        ]
    )
    md_path.write_text("\n".join(lines) + "\n")
    print(md_path)
    print(policy_swap_path)
    return 0


def apply_swap_to_moxfield(source: Path, out_path: Path, cut: str, add: str) -> None:
    payload = read_json(source)
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards")
    if not isinstance(cards, dict):
        raise ValueError("source deck is not a Moxfield-style mainboard JSON")
    cut_key = None
    for key, entry in cards.items():
        if entry.get("card", {}).get("name") == cut:
            cut_key = key
            break
    if cut_key is None:
        raise ValueError(f"cut card not found: {cut}")
    existing = [entry.get("card", {}).get("name") for key, entry in cards.items() if key != cut_key]
    if add in existing:
        raise ValueError(f"adding {add} would violate singleton")
    replacement = json.loads(json.dumps(cards[cut_key]))
    replacement["card"]["name"] = add
    replacement["quantity"] = 1
    cards[cut_key] = replacement
    payload["name"] = f"{payload.get('name', source.stem)} [{cut} -> {add}]"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")


def write_challenger(args: argparse.Namespace) -> int:
    registry_path = repo_path(args.registry)
    registry = load_registry(registry_path)
    source = repo_path(args.source_deck or registry.get("champion", {}).get("deck_json", ""))
    candidates = registry.get("candidates", {})
    selected = None
    for candidate in candidates.values():
        if candidate.get("swap") == args.swap or candidate.get("key") == args.swap:
            selected = candidate
            break
    if selected is None:
        raise SystemExit(f"candidate not found: {args.swap}")
    out_path = repo_path(args.out)
    apply_swap_to_moxfield(source, out_path, selected["cut"], selected["add"])
    print(out_path)
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Maintain and evolve a Rhystic/Heartwood deck experiment loop.")
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    sub = parser.add_subparsers(dest="cmd", required=True)

    init = sub.add_parser("init")
    init.add_argument("--champion-deck", required=True)
    init.add_argument("--name", default="working")
    init.add_argument("--note", default="")
    init.add_argument("--protected-cut", action="append", default=[])
    init.add_argument("--reset-evidence", action="store_true")
    init.set_defaults(func=init_registry)

    raw = sub.add_parser("ingest-raw")
    raw.add_argument("--path", required=True)
    raw.add_argument("--label", default="")
    raw.add_argument("--force", action="store_true")
    raw.set_defaults(func=ingest_raw)

    policy = sub.add_parser("ingest-policy")
    policy.add_argument("--summary", required=True)
    policy.add_argument("--details", default="")
    policy.add_argument("--label", default="")
    policy.add_argument("--role", choices=["validation", "pilot"], default="validation")
    policy.add_argument("--exclude-from-pool", action="store_true")
    policy.add_argument("--force", action="store_true")
    policy.set_defaults(func=ingest_policy)

    prune = sub.add_parser("prune-source")
    prune.add_argument("--kind", choices=["raw", "policy"], required=True)
    prune.add_argument("--source-id", required=True)
    prune.set_defaults(func=prune_source)

    refresh = sub.add_parser("refresh")
    refresh.add_argument("--min-accept-games", type=int, default=6000)
    refresh.set_defaults(func=refresh_registry)

    stat = sub.add_parser("status")
    stat.add_argument("--out-dir", default=str(DEFAULT_OUT_DIR))
    stat.add_argument("--limit", type=int, default=40)
    stat.set_defaults(func=status)

    sug = sub.add_parser("suggest")
    sug.add_argument("--out-dir", default=str(DEFAULT_OUT_DIR))
    sug.add_argument("--policy-limit", type=int, default=8)
    sug.add_argument("--raw-limit", type=int, default=50)
    sug.set_defaults(func=suggest)

    challenger = sub.add_parser("write-challenger")
    challenger.add_argument("--swap", required=True, help="Candidate key or exact 'Cut -> Add' swap label.")
    challenger.add_argument("--out", required=True)
    challenger.add_argument("--source-deck", default="")
    challenger.set_defaults(func=write_challenger)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
