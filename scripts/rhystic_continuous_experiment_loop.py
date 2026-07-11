#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import os
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REGISTRY = ROOT / "data/rhystic_study_turn12/experiment_loop/registry.json"
DEFAULT_OUT_ROOT = ROOT / "data/rhystic_study_turn12/experiment_loop/continuous"
DEFAULT_CHAMPION_DIR = ROOT / "data/rhystic_study_turn12/champion_glimmervoid_hallowed_20260706"
EXPERIMENT_LOOP = ROOT / "scripts/rhystic_experiment_loop.py"
RAW_STREAM = ROOT / "scripts/rhystic_raw_delta_stream.py"
PAIRED_COMPARE = ROOT / "scripts/rhystic_paired_rate_compare.py"
CARD_IDENTITY_ALIASES = {
    "Glittering Caves of Aglarond": "Gemstone Caverns",
    "Zidane Tribal": "Ragavan, Nimble Pilferer",
}


def now_tag() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S")


def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    tmp.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    tmp.replace(path)


def read_csv(path: Path) -> list[dict[str, str]]:
    if not path.exists() or path.stat().st_size == 0:
        return []
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle))


def read_name_file(path: Path) -> list[str]:
    names: list[str] = []
    for line in path.read_text().splitlines():
        clean = line.split("#", 1)[0].strip()
        if clean:
            names.append(clean)
    return names


def load_deck_names(path: Path) -> list[str]:
    payload = read_json(path)
    if isinstance(payload.get("deck"), list):
        return list(payload["deck"])
    cards = ((payload.get("boards") or {}).get("mainboard") or {}).get("cards") or {}
    names: list[str] = []
    for entry in cards.values():
        qty = int(entry.get("quantity", 1))
        names.extend([entry["card"]["name"]] * qty)
    return names


def card_identity(name: str) -> str:
    return CARD_IDENTITY_ALIASES.get(name, name)


def append_log(path: Path, line: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as handle:
        handle.write(f"{datetime.now().isoformat(timespec='seconds')} {line}\n")


def run_command(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def loop_status_path(out_root: Path) -> Path:
    return out_root / "loop_status.json"


def pause_path(out_root: Path) -> Path:
    return out_root / "PAUSE"


def write_status(out_root: Path, **payload: Any) -> None:
    current: dict[str, Any] = {}
    path = loop_status_path(out_root)
    if path.exists():
        try:
            current = read_json(path)
        except (OSError, json.JSONDecodeError):
            current = {}
    current.update(payload)
    current["updated_at"] = datetime.now().isoformat(timespec="seconds")
    write_json(path, current)


def champion_deck(registry_path: Path) -> Path:
    registry = read_json(registry_path)
    deck = registry.get("champion", {}).get("deck_json")
    if not deck:
        raise RuntimeError(f"registry has no champion deck: {registry_path}")
    path = Path(deck)
    return path if path.is_absolute() else ROOT / path


def unknown_raw_swaps(args: argparse.Namespace) -> tuple[int, dict[str, int]]:
    registry = read_json(args.registry)
    deck = load_deck_names(champion_deck(args.registry))
    deck_set = set(deck)
    deck_identities = {card_identity(card) for card in deck}
    protected = set(registry.get("constraints", {}).get("protected_cuts", []))
    known: set[str] = set()
    for candidate in registry.get("candidates", {}).values():
        cut = candidate.get("cut") or ""
        add = candidate.get("add") or ""
        if cut and add and (candidate.get("raw_runs") or candidate.get("policy_runs")):
            known.add(f"{cut} -> {add}")
    skipped = {
        "cut_not_in_deck": 0,
        "add_already_in_deck": 0,
        "protected_cut": 0,
        "known": 0,
        "identity": 0,
    }
    count = 0
    for cut in read_name_file(args.cut_file):
        for add in read_name_file(args.add_file):
            label = f"{cut} -> {add}"
            if card_identity(cut) == card_identity(add):
                skipped["identity"] += 1
            elif cut not in deck_set:
                skipped["cut_not_in_deck"] += 1
            elif add in deck_set or card_identity(add) in deck_identities:
                skipped["add_already_in_deck"] += 1
            elif cut in protected:
                skipped["protected_cut"] += 1
            elif label in known:
                skipped["known"] += 1
            else:
                count += 1
    return count, skipped


def refresh_status_suggest(args: argparse.Namespace) -> list[dict[str, str]]:
    run_command([sys.executable, str(EXPERIMENT_LOOP), "--registry", str(args.registry), "refresh", "--min-accept-games", str(args.min_accept_games)])
    run_command([sys.executable, str(EXPERIMENT_LOOP), "--registry", str(args.registry), "status", "--out-dir", str(args.loop_out)])
    run_command([sys.executable, str(EXPERIMENT_LOOP), "--registry", str(args.registry), "suggest", "--out-dir", str(args.loop_out)])
    return read_csv(args.loop_out / "candidate_status.csv")


def fnum(value: str | None, default: float = 0.0) -> float:
    if value in (None, ""):
        return default
    try:
        return float(value)
    except ValueError:
        return default


def inum(value: str | None, default: int = 0) -> int:
    if value in (None, ""):
        return default
    try:
        return int(float(value))
    except ValueError:
        return default


def policy_rows(rows: list[dict[str, str]], *, limit: int, min_accept_games: int) -> list[dict[str, str]]:
    selected: list[dict[str, str]] = []
    for row in rows:
        classification = row.get("classification")
        if classification in {"needs_policy", "needs_more_policy"}:
            selected.append(row)
        elif classification == "watch_policy":
            games = inum(row.get("pooled_games"))
            score = fnum(row.get("pooled_score_delta"))
            if games < min_accept_games or score > 0:
                selected.append(row)
        if len(selected) >= limit:
            break
    return selected


def promotion_rows(rows: list[dict[str, str]]) -> list[dict[str, str]]:
    wanted = {"promote_challenger", "statistical_candidate_strategic_review"}
    return [row for row in rows if row.get("classification") in wanted]


def run_policy_batch(args: argparse.Namespace, rows: list[dict[str, str]], iteration: int) -> Path:
    out_dir = args.loop_out / f"policy_iter{iteration:04d}_{now_tag()}"
    deck = champion_deck(args.registry)
    seed = args.seed_base + iteration * 1009 + int(time.time()) % 997
    command = [
        sys.executable,
        str(PAIRED_COMPARE),
        "--out-dir",
        str(out_dir),
        "--deck-json",
        rel(deck),
        "--target",
        "rhystic_heartwood",
        "--threshold-hands",
        str(args.policy_threshold_hands),
        "--eval-games",
        str(args.policy_eval_games),
        "--samples-per-bottom",
        "2",
        "--validation-samples",
        "2",
        "--state-limit",
        str(args.policy_state_limit),
        "--actual-rerun-state-limit",
        str(args.policy_actual_rerun_state_limit),
        "--workers",
        str(args.policy_workers),
        "--chunks-per-worker",
        str(args.policy_chunks_per_worker),
        "--seed",
        str(seed),
        "--gemstone-caverns-live-rate",
        "0.75",
        "--engine-success-policy",
        "resilient",
        "--gamble-mode",
        "stochastic",
        "--normalize-no-caverns-gemstone-key",
        "--weighted-policy-ev",
        "--rhystic-t1-weight",
        "100",
        "--rhystic-t2-weight",
        "60",
        "--heartwood-t1-weight",
        "20",
        "--heartwood-t2-weight",
        "10",
        "--rust-full-sim",
        "--rust-full-sim-games-per-shard",
        str(args.policy_rust_games_per_shard),
        "--rust-full-sim-shard-workers",
        str(args.policy_rust_shard_workers),
        "--independent-thresholds",
        "--bootstrap-samples",
        str(args.policy_bootstrap_samples),
        "--baseline-cache-dir",
        str(args.loop_out / "baseline_cache"),
    ]
    for row in rows:
        command.extend(["--swap", row["swap"]])
    env = os.environ.copy()
    env["RHYSTIC_SIMPLIFIED_GAMBLE"] = "1"
    if args.nice is not None:
        command = ["nice", "-n", str(args.nice), *command]
    run_command(command, env=env)
    summary = out_dir / "paired_summary.csv"
    details = out_dir / "paired_game_deltas.csv"
    label = out_dir.name
    if not args.no_ingest:
        run_command(
            [
                sys.executable,
                str(EXPERIMENT_LOOP),
                "--registry",
                str(args.registry),
                "ingest-policy",
                "--summary",
                str(summary),
                "--details",
                str(details),
                "--label",
                label,
                "--role",
                "validation",
                "--force",
            ]
        )
    return out_dir


def run_raw_screen(args: argparse.Namespace, iteration: int) -> Path | None:
    out_dir = args.loop_out / f"raw_iter{iteration:04d}_{now_tag()}"
    unknown_count, skipped = unknown_raw_swaps(args)
    if unknown_count == 0:
        write_status(
            args.loop_out,
            stage="raw_pool_exhausted",
            iteration=iteration,
            last_error="",
            raw_skipped=skipped,
            last_raw_out_dir="",
        )
        return None
    deck = champion_deck(args.registry)
    seed = args.seed_base + iteration * 4099 + int(time.time()) % 4093
    command = [
        sys.executable,
        str(RAW_STREAM),
        "--deck-json",
        rel(deck),
        "--registry",
        str(args.registry),
        "--out-dir",
        str(out_dir),
        "--cut-file",
        str(args.cut_file),
        "--add-file",
        str(args.add_file),
        "--skip-known",
        "--exclude-protected",
        "--samples-per-stage",
        str(args.raw_samples_per_stage),
        "--seed",
        str(seed),
        "--shards",
        str(args.raw_shards),
        "--workers",
        str(args.raw_workers),
        "--prefix",
        "continuous_raw",
        "--run",
        "--nice",
        str(args.nice),
    ]
    if not args.no_ingest:
        command.extend(["--ingest-registry", "--refresh-registry"])
    try:
        run_command(command)
    except subprocess.CalledProcessError as exc:
        # The raw stream exits non-zero when every strict-pool swap is already known.
        write_status(args.loop_out, stage="raw_screen_failed_or_empty", last_error=str(exc), last_raw_out_dir=str(out_dir))
        raise
    return out_dir


def run_loop(args: argparse.Namespace) -> int:
    args.registry = Path(args.registry).resolve()
    args.loop_out = Path(args.loop_out).resolve()
    args.cut_file = Path(args.cut_file).resolve()
    args.add_file = Path(args.add_file).resolve()
    args.loop_out.mkdir(parents=True, exist_ok=True)
    log_path = args.loop_out / "loop_events.log"
    write_status(
        args.loop_out,
        stage="starting",
        pid=os.getpid(),
        registry=rel(args.registry),
        pause_file=str(pause_path(args.loop_out)),
        max_iterations=args.max_iterations,
    )
    append_log(log_path, f"started pid={os.getpid()}")

    iteration = 0
    while args.max_iterations <= 0 or iteration < args.max_iterations:
        iteration += 1
        if pause_path(args.loop_out).exists():
            write_status(args.loop_out, stage="paused", iteration=iteration)
            append_log(log_path, f"paused before iteration={iteration}")
            return 0

        write_status(args.loop_out, stage="refreshing", iteration=iteration)
        rows = refresh_status_suggest(args)
        promotions = promotion_rows(rows)
        if promotions:
            write_status(args.loop_out, stage="promotion_pending", iteration=iteration, promotions=promotions)
            append_log(log_path, f"promotion_pending iteration={iteration} count={len(promotions)}")
            return 0

        policies = policy_rows(rows, limit=args.policy_limit, min_accept_games=args.min_accept_games)
        if policies:
            write_status(args.loop_out, stage="policy_batch", iteration=iteration, swaps=[row["swap"] for row in policies])
            append_log(log_path, f"policy iteration={iteration} swaps={len(policies)}")
            out_dir = run_policy_batch(args, policies, iteration)
            write_status(args.loop_out, stage="policy_complete", iteration=iteration, last_policy_out_dir=rel(out_dir))
        else:
            write_status(args.loop_out, stage="raw_screen", iteration=iteration)
            append_log(log_path, f"raw iteration={iteration}")
            out_dir = run_raw_screen(args, iteration)
            if out_dir is None:
                append_log(log_path, f"raw_pool_exhausted iteration={iteration}")
                return 0
            write_status(args.loop_out, stage="raw_complete", iteration=iteration, last_raw_out_dir=rel(out_dir) if out_dir else "")

        if args.sleep_seconds > 0:
            write_status(args.loop_out, stage="sleeping", iteration=iteration, sleep_seconds=args.sleep_seconds)
            time.sleep(args.sleep_seconds)

    write_status(args.loop_out, stage="finished_max_iterations", iteration=iteration)
    append_log(log_path, f"finished max_iterations={args.max_iterations}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run the Rhystic/Heartwood autonomous experiment loop.")
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    parser.add_argument("--loop-out", default=str(DEFAULT_OUT_ROOT))
    parser.add_argument("--cut-file", default=str(DEFAULT_CHAMPION_DIR / "strict_cut_pool.txt"))
    parser.add_argument("--add-file", default=str(DEFAULT_CHAMPION_DIR / "strict_add_pool.txt"))
    parser.add_argument("--max-iterations", type=int, default=0, help="0 means run until paused, empty, or promotion pending.")
    parser.add_argument("--sleep-seconds", type=float, default=0.0)
    parser.add_argument("--min-accept-games", type=int, default=6000)
    parser.add_argument("--policy-limit", type=int, default=4)
    parser.add_argument("--policy-eval-games", type=int, default=3000)
    parser.add_argument("--policy-threshold-hands", type=int, default=80)
    parser.add_argument("--policy-workers", type=int, default=3)
    parser.add_argument("--policy-chunks-per-worker", type=int, default=16)
    parser.add_argument("--policy-rust-games-per-shard", type=int, default=500)
    parser.add_argument("--policy-rust-shard-workers", type=int, default=3)
    parser.add_argument("--policy-state-limit", type=int, default=20000)
    parser.add_argument("--policy-actual-rerun-state-limit", type=int, default=60000)
    parser.add_argument("--policy-bootstrap-samples", type=int, default=500)
    parser.add_argument("--raw-samples-per-stage", type=int, default=20)
    parser.add_argument("--raw-shards", type=int, default=4)
    parser.add_argument("--raw-workers", type=int, default=2)
    parser.add_argument("--seed-base", type=int, default=2026070600)
    parser.add_argument("--nice", type=int, default=20)
    parser.add_argument("--no-ingest", action="store_true", help="Run commands but do not write raw/policy evidence to the registry.")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return run_loop(args)


if __name__ == "__main__":
    raise SystemExit(main())
