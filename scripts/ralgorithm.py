#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from deck_io import load_deck, parse_text_export, validate_deck_payload, write_deck


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DECK = ROOT / "fixtures" / "decks" / "nick_fury_generation32_balanced_optimized.json"
DEFAULT_SWAPS = ROOT / "benchmarks" / "generation35_breach_package_finalists.txt"

PRESETS = {
    "smoke": {
        "threshold_hands": 32,
        "eval_games": 100,
        "samples": 2,
        "state_limit": 5_000,
        "rerun_limit": 50_000,
        "bootstrap_samples": 1_000,
        "games_per_shard": 25,
    },
    "local": {
        "threshold_hands": 256,
        "eval_games": 2_000,
        "samples": 4,
        "state_limit": 5_000,
        "rerun_limit": 100_000,
        "bootstrap_samples": 10_000,
        "games_per_shard": 500,
    },
    "publication": {
        "threshold_hands": 1_000,
        "eval_games": 10_000,
        "samples": 4,
        "state_limit": 5_000,
        "rerun_limit": 100_000,
        "bootstrap_samples": 20_000,
        "games_per_shard": 500,
    },
}


def resolve(path: str | Path) -> Path:
    value = Path(path)
    return value if value.is_absolute() else ROOT / value


def require_valid_deck(path: Path) -> dict[str, object]:
    payload = load_deck(path)
    errors = validate_deck_payload(payload)
    if errors:
        raise ValueError("invalid deck:\n- " + "\n- ".join(errors))
    return payload


def run(command: list[str], *, env: dict[str, str] | None = None, dry_run: bool = False) -> None:
    print("+ " + " ".join(command), flush=True)
    if not dry_run:
        subprocess.run(command, cwd=ROOT, env=env, check=True)


def command_doctor(_args: argparse.Namespace) -> int:
    tools = {name: shutil.which(name) for name in ("python3", "cargo", "rustc", "git")}
    report = {
        "python": sys.version.split()[0],
        "repository": str(ROOT),
        "release_binary": str(ROOT / "target" / "release" / "rhystic-core-smoke"),
        "release_binary_exists": (ROOT / "target" / "release" / "rhystic-core-smoke").exists(),
        "tools": tools,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    return 0 if all(tools.values()) and sys.version_info >= (3, 10) else 1


def command_import(args: argparse.Namespace) -> int:
    source = resolve(args.input)
    destination = resolve(args.output)
    payload = parse_text_export(
        source.read_text(),
        name=args.name or source.stem,
        commanders=args.commander,
    )
    errors = validate_deck_payload(payload)
    if errors and not args.allow_invalid:
        raise ValueError("invalid imported deck:\n- " + "\n- ".join(errors))
    write_deck(destination, payload)
    print(json.dumps({"deck": str(destination), "errors": errors}, indent=2))
    return 0 if not errors else 2


def command_check(args: argparse.Namespace) -> int:
    deck = resolve(args.deck)
    payload = load_deck(deck)
    errors = validate_deck_payload(payload)
    report = {
        "deck": str(deck),
        "mainboard_cards": len(payload.get("deck", [])),
        "errors": errors,
    }
    print(json.dumps(report, indent=2, sort_keys=True))
    if errors:
        return 2
    if not args.semantic:
        return 0

    binary = ROOT / "target" / "release" / "rhystic-core-smoke"
    if not binary.exists() and not args.no_build:
        run(["cargo", "build", "--release", "--locked", "-p", "rhystic_core", "--bin", "rhystic-core-smoke"])
    command = [
        sys.executable,
        str(ROOT / "scripts" / "rhystic_rust_coverage_audit.py"),
        "--deck-json",
        str(deck),
        "--binary",
        str(binary),
        "--fail-on-unsupported",
    ]
    if args.swap_file:
        command.extend(["--swap-file", str(resolve(args.swap_file))])
    return subprocess.run(command, cwd=ROOT).returncode


def command_compare(args: argparse.Namespace) -> int:
    deck = resolve(args.deck)
    swap_file = resolve(args.swap_file)
    require_valid_deck(deck)
    if not swap_file.exists():
        raise FileNotFoundError(f"swap file not found: {swap_file}")

    preset = dict(PRESETS[args.preset])
    games = args.games or preset["eval_games"]
    workers = args.workers or max(1, min(8, (os.cpu_count() or 2) - 1))
    out_dir = (
        resolve(args.out)
        if args.out
        else ROOT / "benchmarks" / "results" / datetime.now(timezone.utc).strftime("run_%Y%m%dT%H%M%SZ")
    )
    env = dict(os.environ)
    env["RHYSTIC_SIMPLIFIED_GAMBLE"] = "1"
    if args.native:
        env["RUSTFLAGS"] = " ".join(filter(None, [env.get("RUSTFLAGS", ""), "-C target-cpu=native"]))

    if not args.no_build:
        run(
            ["cargo", "build", "--release", "--locked", "-p", "rhystic_core", "--bin", "rhystic-core-smoke"],
            env=env,
            dry_run=args.dry_run,
        )

    command = [
        sys.executable,
        str(ROOT / "scripts" / "rhystic_paired_rate_compare.py"),
        "--deck-json",
        str(deck),
        "--swap-file",
        str(swap_file),
        "--out-dir",
        str(out_dir),
        "--target",
        "rhystic_heartwood",
        "--threshold-hands",
        str(preset["threshold_hands"]),
        "--eval-games",
        str(games),
        "--samples-per-bottom",
        str(preset["samples"]),
        "--validation-samples",
        str(preset["samples"]),
        "--state-limit",
        str(preset["state_limit"]),
        "--actual-rerun-state-limit",
        str(preset["rerun_limit"]),
        "--workers",
        str(workers),
        "--seed",
        str(args.seed),
        "--gemstone-caverns-live-rate",
        "0.75",
        "--engine-success-policy",
        "resilient",
        "--gamble-mode",
        "stochastic",
        "--weighted-policy-ev",
        "--rhystic-t1-weight",
        "1.0",
        "--rhystic-t2-weight",
        "0.75",
        "--heartwood-t1-weight",
        "0.70",
        "--heartwood-t2-weight",
        "0.55",
        "--rust-full-sim",
        "--rust-full-sim-games-per-shard",
        str(preset["games_per_shard"]),
        "--rust-full-sim-shard-workers",
        str(workers),
        "--shared-thresholds",
        "--bootstrap-samples",
        str(preset["bootstrap_samples"]),
        "--threshold-cache-dir",
        str(ROOT / ".cache" / "ralgorithm" / "thresholds"),
        "--baseline-cache-dir",
        str(ROOT / ".cache" / "ralgorithm" / "baselines"),
    ]
    run(command, env=env, dry_run=args.dry_run)
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description="Ralgorithm deck preparation and experiment runner")
    commands = root.add_subparsers(dest="command", required=True)

    doctor = commands.add_parser("doctor", help="check local prerequisites")
    doctor.set_defaults(func=command_doctor)

    importer = commands.add_parser("import-deck", help="convert a Moxfield/MTGO text export to simulator JSON")
    importer.add_argument("input")
    importer.add_argument("output")
    importer.add_argument("--name")
    importer.add_argument("--commander", action="append", required=True, help="repeat for partner commanders")
    importer.add_argument("--allow-invalid", action="store_true", help="write an incomplete list for later editing")
    importer.set_defaults(func=command_import)

    check = commands.add_parser("check-deck", help="validate deck structure and optional Rust semantic coverage")
    check.add_argument("deck")
    check.add_argument("--semantic", action="store_true")
    check.add_argument("--swap-file")
    check.add_argument("--no-build", action="store_true")
    check.set_defaults(func=command_check)

    compare = commands.add_parser("compare", help="run an optimized paired Rust card-swap experiment")
    compare.add_argument("--deck", default=str(DEFAULT_DECK))
    compare.add_argument("--swap-file", default=str(DEFAULT_SWAPS))
    compare.add_argument("--out")
    compare.add_argument("--preset", choices=tuple(PRESETS), default="smoke")
    compare.add_argument("--games", type=int)
    compare.add_argument("--workers", type=int)
    compare.add_argument("--seed", type=int, default=2026071601)
    compare.add_argument("--native", action="store_true", help="compile for this CPU; resulting binary is not portable")
    compare.add_argument("--no-build", action="store_true")
    compare.add_argument("--dry-run", action="store_true")
    compare.set_defaults(func=command_compare)
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        return int(args.func(args))
    except (FileNotFoundError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
