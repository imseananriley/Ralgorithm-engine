#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
SIM_PATH = ROOT / "scripts" / "rhystic_belief_mulligan_sim.py"


def default_rust_bin() -> Path:
    return ROOT / "rust" / "rhystic_core" / "target" / "release" / "rhystic-core-smoke"


def load_sim_module() -> Any:
    spec = importlib.util.spec_from_file_location("rhystic_belief_mulligan_sim", SIM_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Could not load {SIM_PATH}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def chi_square_z(counts: list[int], expected: float) -> tuple[float, float]:
    if expected <= 0:
        return 0.0, 0.0
    chi2 = sum((count - expected) ** 2 / expected for count in counts)
    df = max(1, len(counts) - 1)
    z = (chi2 - df) / math.sqrt(2 * df)
    return chi2, z


def run_audit(args: argparse.Namespace, deck: list[str]) -> dict[str, Any]:
    request = {
        "deck": deck,
        "samples": args.samples,
        "seed": args.seed,
        "domain": args.domain,
    }
    completed = subprocess.run(
        [str(args.rust_bin), "rng-shuffle-audit-jsonl"],
        input=json.dumps(request, sort_keys=True, separators=(",", ":")) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"Rust RNG audit failed with code {completed.returncode}:\n{completed.stderr}"
        )
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise RuntimeError(f"Expected one JSONL response, got {len(lines)} lines")
    response = json.loads(lines[0])
    if response.get("unsupported"):
        raise RuntimeError(f"Rust RNG audit returned unsupported: {response.get('unsupported_reason')}")
    return response


def summarize(response: dict[str, Any], z_threshold: float) -> dict[str, Any]:
    samples = int(response["samples"])
    deck_size = int(response["deck_size"])
    expected = samples / deck_size
    first_counts = [int(value) for value in response["first_card_counts"].values()]
    first_chi2, first_z = chi_square_z(first_counts, expected)

    card_position_rows: list[dict[str, Any]] = []
    max_abs_card_position_z = 0.0
    for card, counts in response["position_counts_by_card"].items():
        chi2, z = chi_square_z([int(value) for value in counts], expected)
        max_abs_card_position_z = max(max_abs_card_position_z, abs(z))
        card_position_rows.append(
            {
                "card": card,
                "chi_square": chi2,
                "normalized_z": z,
                "max_position_count": max(counts),
                "min_position_count": min(counts),
            }
        )
    card_position_rows.sort(key=lambda row: abs(float(row["normalized_z"])), reverse=True)

    pass_uniformity = abs(first_z) <= z_threshold and max_abs_card_position_z <= z_threshold
    return {
        "samples": samples,
        "deck_size": deck_size,
        "expected_per_bucket": expected,
        "z_threshold": z_threshold,
        "pass_uniformity_smoke": pass_uniformity,
        "first_card_chi_square": first_chi2,
        "first_card_normalized_z": first_z,
        "max_abs_card_position_z": max_abs_card_position_z,
        "worst_card_position_rows": card_position_rows[:10],
        "rng_metadata": response.get("rng_metadata", {}),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Smoke-test Rust shuffle RNG uniformity.")
    parser.add_argument("--deck-json", default="data/moxfield_tPWeAfl5uXGJdejEnaIwYw_current_rain_over_mindbreak_20260701.json")
    parser.add_argument("--rust-bin", type=Path, default=default_rust_bin())
    parser.add_argument("--samples", type=int, default=20_000)
    parser.add_argument("--seed", type=int, default=2026070404)
    parser.add_argument("--domain", default="rng_audit_shuffle")
    parser.add_argument("--z-threshold", type=float, default=6.0)
    parser.add_argument("--json-out", default="")
    args = parser.parse_args()

    if args.samples <= 0:
        raise ValueError("--samples must be positive")
    if not args.rust_bin.exists():
        raise FileNotFoundError(f"Rust binary not found: {args.rust_bin}")

    sim = load_sim_module()
    _name, _commanders, mainboard = sim.read_moxfield_deck(ROOT / args.deck_json)
    response = run_audit(args, mainboard)
    summary = summarize(response, args.z_threshold)

    if args.json_out:
        out_path = Path(args.json_out)
        if not out_path.is_absolute():
            out_path = ROOT / out_path
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")

    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0 if summary["pass_uniformity_smoke"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
