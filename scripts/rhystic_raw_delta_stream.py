#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
RUST_MANIFEST = ROOT / "rust/rhystic_core/Cargo.toml"
RUST_BIN = ROOT / "target/release/rhystic-core-smoke"
MERGE_SCRIPT = ROOT / "scripts/rhystic_merge_raw_delta_stream.py"
DEFAULT_REGISTRY = ROOT / "data/rhystic_study_turn12/experiment_loop/registry.json"
EXPERIMENT_LOOP = ROOT / "scripts/rhystic_experiment_loop.py"
CARD_IDENTITY_ALIASES = {
    "Glittering Caves of Aglarond": "Gemstone Caverns",
    "Zidane Tribal": "Ragavan, Nimble Pilferer",
}


def now_tag() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")


def repo_path(path: str | Path) -> Path:
    p = Path(path)
    return p if p.is_absolute() else ROOT / p


def rel(path: str | Path) -> str:
    p = repo_path(path).resolve()
    try:
        return p.relative_to(ROOT).as_posix()
    except ValueError:
        return p.as_posix()


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text())


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


def read_name_file(path: str | None) -> list[str]:
    if not path:
        return []
    names: list[str] = []
    for line in repo_path(path).read_text().splitlines():
        clean = line.split("#", 1)[0].strip()
        if clean:
            names.append(clean)
    return names


def read_swap_json_files(paths: list[str]) -> list[dict[str, Any]]:
    swaps: list[dict[str, Any]] = []
    for path in paths:
        payload = read_json(repo_path(path))
        if isinstance(payload, dict):
            payload = payload.get("swaps", [])
        if not isinstance(payload, list):
            raise SystemExit(f"--swap-json-file must contain a list or {{'swaps': [...]}}: {path}")
        for index, swap in enumerate(payload):
            if not isinstance(swap, dict):
                raise SystemExit(f"swap entry {index} in {path} is not an object")
            if not swap.get("name") or not swap.get("cut") or not swap.get("add"):
                raise SystemExit(f"swap entry {index} in {path} must include name, cut, and add")
            swaps.append(dict(swap))
    return swaps


def unique_in_order(names: list[str]) -> list[str]:
    seen: set[str] = set()
    output: list[str] = []
    for name in names:
        if name not in seen:
            output.append(name)
            seen.add(name)
    return output


def card_identity(name: str) -> str:
    return CARD_IDENTITY_ALIASES.get(name, name)


def registry_universe(path: Path) -> tuple[list[str], list[str], set[str], set[str]]:
    if not path.exists():
        return [], [], set(), set()
    registry = read_json(path)
    cuts: list[str] = []
    adds: list[str] = []
    known: set[str] = set()
    protected = set(registry.get("constraints", {}).get("protected_cuts", []))
    for candidate in registry.get("candidates", {}).values():
        cut = candidate.get("cut") or ""
        add = candidate.get("add") or ""
        if not cut or not add:
            continue
        cuts.append(cut)
        adds.append(add)
        if candidate.get("raw_runs") or candidate.get("policy_runs"):
            known.add(f"{cut} -> {add}")
    return unique_in_order(cuts), unique_in_order(adds), known, protected


def split_evenly(items: list[dict[str, str]], shards: int) -> list[list[dict[str, str]]]:
    shards = max(1, min(shards, len(items) or 1))
    buckets: list[list[dict[str, str]]] = [[] for _ in range(shards)]
    for index, item in enumerate(items):
        buckets[index % shards].append(item)
    return [bucket for bucket in buckets if bucket]


def count_variants(path: Path) -> int:
    if not path.exists():
        return 0
    count = 0
    with path.open() as handle:
        for line in handle:
            if '"record_type":"variant"' in line or '"record_type": "variant"' in line:
                count += 1
    return count


def build_request(args: argparse.Namespace, deck: list[str], swaps: list[dict[str, str]]) -> dict[str, Any]:
    request: dict[str, Any] = {
        "deck": deck,
        "swaps": swaps,
        "samples_per_stage": args.samples_per_stage,
        "seed": args.seed,
        "gemstone_caverns_live_rate": args.gemstone_caverns_live_rate,
        "state_limit": args.state_limit,
        "max_turns": args.max_turns,
        "goal": "engine",
        "engine_target_count": 1,
        "engine_success_policy": args.engine_success_policy,
        "remora_upkeep_payments": args.remora_upkeep_payments,
        "action_sort": not args.disable_action_sort,
        "gamble_mode": args.gamble_mode,
        "simplified_gamble": args.simplified_gamble,
        "cap_weight": args.cap_weight,
        "rhystic_t1_weight": args.rhystic_t1_weight,
        "rhystic_t2_weight": args.rhystic_t2_weight,
        "heartwood_t1_weight": args.heartwood_t1_weight,
        "heartwood_t2_weight": args.heartwood_t2_weight,
        "draw_window": args.draw_window,
        "relevance_mode": args.relevance_mode,
        "run_naive": args.run_naive,
    }
    if args.stages:
        request["stages"] = args.stages
    return request


def run_shards(args: argparse.Namespace, out_dir: Path, shard_count: int) -> None:
    if args.build or not RUST_BIN.exists():
        subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "--manifest-path",
                str(RUST_MANIFEST),
                "--bin",
                "rhystic-core-smoke",
            ],
            cwd=ROOT,
            check=True,
        )

    pending = list(range(shard_count))
    running: list[dict[str, Any]] = []
    failures: list[tuple[int, int]] = []
    total_variants = 0
    for index in range(shard_count):
        request = read_json(out_dir / f"request_shard{index}.json")
        total_variants += len(request.get("swaps", []))

    last_progress = 0.0
    started_at = time.perf_counter()
    while pending or running:
        while pending and len(running) < args.workers:
            index = pending.pop(0)
            stdin_path = out_dir / f"request_shard{index}.jsonl"
            stdout_path = out_dir / f"stream_shard{index}.jsonl"
            stderr_path = out_dir / f"run_shard{index}.log"
            command = [str(RUST_BIN), "raw-delta-fast-stream-jsonl"]
            if args.nice is not None:
                command = ["nice", "-n", str(args.nice), *command]
            stdin_handle = stdin_path.open()
            stdout_handle = stdout_path.open("w")
            stderr_handle = stderr_path.open("w")
            process = subprocess.Popen(
                command,
                cwd=ROOT,
                stdin=stdin_handle,
                stdout=stdout_handle,
                stderr=stderr_handle,
            )
            running.append(
                {
                    "index": index,
                    "process": process,
                    "stdin": stdin_handle,
                    "stdout": stdout_handle,
                    "stderr": stderr_handle,
                    "started_at": time.perf_counter(),
                }
            )
            print(f"started shard {index}", flush=True)

        time.sleep(args.poll_interval)
        still_running: list[dict[str, Any]] = []
        for item in running:
            process = item["process"]
            code = process.poll()
            if code is None:
                still_running.append(item)
                continue
            item["stdin"].close()
            item["stdout"].close()
            item["stderr"].close()
            elapsed = time.perf_counter() - item["started_at"]
            print(f"finished shard {item['index']} code={code} elapsed={elapsed:.1f}s", flush=True)
            if code != 0:
                failures.append((item["index"], code))
        running = still_running

        now = time.perf_counter()
        if now - last_progress >= args.progress_interval:
            completed = sum(count_variants(out_dir / f"stream_shard{index}.jsonl") for index in range(shard_count))
            elapsed = now - started_at
            rate = completed / elapsed if elapsed > 0 else 0.0
            remaining = (total_variants - completed) / rate if rate > 0 else None
            progress = {
                "elapsed_seconds": elapsed,
                "completed_variants": completed,
                "total_variants": total_variants,
                "variants_per_second": rate,
                "eta_seconds": remaining,
                "running_shards": [item["index"] for item in running],
                "pending_shards": pending,
            }
            (out_dir / "progress.json").write_text(json.dumps(progress, indent=2, sort_keys=True) + "\n")
            eta = "unknown" if remaining is None else f"{remaining / 60:.1f}m"
            print(f"progress {completed}/{total_variants} variants, eta {eta}", flush=True)
            last_progress = now

    if failures:
        raise SystemExit(f"raw-delta shard failures: {failures}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Run sharded Rust raw-delta swap screens.")
    parser.add_argument("--deck-json", required=True)
    parser.add_argument("--out-dir", required=True)
    parser.add_argument("--registry", default=str(DEFAULT_REGISTRY))
    parser.add_argument("--from-registry-universe", action="store_true")
    parser.add_argument("--skip-known", action="store_true")
    parser.add_argument("--exclude-protected", action="store_true")
    parser.add_argument("--cut", action="append", default=[])
    parser.add_argument("--add", action="append", default=[])
    parser.add_argument("--cut-file")
    parser.add_argument("--add-file")
    parser.add_argument("--swap-json-file", action="append", default=[])
    parser.add_argument("--protected-cut", action="append", default=[])
    parser.add_argument("--max-swaps", type=int, default=0)
    parser.add_argument("--samples-per-stage", type=int, default=100)
    parser.add_argument("--seed", type=int, default=2026070601)
    parser.add_argument("--shards", type=int, default=1)
    parser.add_argument("--workers", type=int, default=0)
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--merge", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--prefix", default="raw_delta")
    parser.add_argument("--ingest-registry", action="store_true")
    parser.add_argument("--ingest-label", default="")
    parser.add_argument("--force-ingest", action="store_true")
    parser.add_argument("--refresh-registry", action="store_true")
    parser.add_argument("--nice", type=int, default=20)
    parser.add_argument("--poll-interval", type=float, default=2.0)
    parser.add_argument("--progress-interval", type=float, default=30.0)
    parser.add_argument("--gemstone-caverns-live-rate", type=float, default=0.75)
    parser.add_argument("--state-limit", type=int, default=50000)
    parser.add_argument("--max-turns", type=int, default=2)
    parser.add_argument("--engine-success-policy", default="resilient")
    parser.add_argument("--remora-upkeep-payments", type=int, default=2)
    parser.add_argument("--disable-action-sort", action="store_true")
    parser.add_argument("--gamble-mode", choices=["off", "optimistic", "stochastic"], default="stochastic")
    parser.add_argument("--simplified-gamble", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument("--cap-weight", type=float, default=0.0)
    parser.add_argument("--rhystic-t1-weight", type=float, default=1.0)
    parser.add_argument("--rhystic-t2-weight", type=float, default=0.75)
    parser.add_argument("--heartwood-t1-weight", type=float, default=0.65)
    parser.add_argument("--heartwood-t2-weight", type=float, default=0.5)
    parser.add_argument("--draw-window", type=int, default=2)
    parser.add_argument("--relevance-mode", default="typed_direct")
    parser.add_argument("--stages", type=int, action="append", default=[])
    parser.add_argument("--run-naive", action="store_true")
    args = parser.parse_args()

    deck_path = repo_path(args.deck_json)
    deck = load_deck_names(deck_path)
    registry_path = repo_path(args.registry)
    registry_cuts, registry_adds, known_swaps, registry_protected = registry_universe(registry_path)
    cuts = list(args.cut) + read_name_file(args.cut_file)
    adds = list(args.add) + read_name_file(args.add_file)
    explicit_swaps = read_swap_json_files(args.swap_json_file)
    if args.from_registry_universe:
        cuts.extend(registry_cuts)
        adds.extend(registry_adds)
    cuts = unique_in_order(cuts)
    adds = unique_in_order(adds)
    protected = set(args.protected_cut) | registry_protected

    if not explicit_swaps and (not cuts or not adds):
        raise SystemExit("provide --cut/--add lists or use --from-registry-universe")

    deck_set = set(deck)
    deck_identities = {card_identity(card) for card in deck}
    swaps: list[dict[str, Any]] = list(explicit_swaps)
    skipped: dict[str, int] = {
        "cut_not_in_deck": 0,
        "add_already_in_deck": 0,
        "protected_cut": 0,
        "known": 0,
        "identity": 0,
    }
    for cut in cuts:
        for add in adds:
            label = f"{cut} -> {add}"
            if card_identity(cut) == card_identity(add):
                skipped["identity"] += 1
                continue
            if cut not in deck_set:
                skipped["cut_not_in_deck"] += 1
                continue
            if add in deck_set or card_identity(add) in deck_identities:
                skipped["add_already_in_deck"] += 1
                continue
            if args.exclude_protected and cut in protected:
                skipped["protected_cut"] += 1
                continue
            if args.skip_known and label in known_swaps:
                skipped["known"] += 1
                continue
            swaps.append({"name": label, "cut": cut, "add": add})
    swaps = unique_in_order([json.dumps(s, sort_keys=True) for s in swaps])
    swap_dicts = [json.loads(item) for item in swaps]
    if args.max_swaps > 0:
        swap_dicts = swap_dicts[: args.max_swaps]
    if not swap_dicts:
        raise SystemExit(f"no swaps to test; skipped={skipped}")

    out_dir = repo_path(args.out_dir.replace("$(date +%Y%m%d_%H%M%S)", now_tag()))
    out_dir.mkdir(parents=True, exist_ok=True)

    shard_swaps = split_evenly(swap_dicts, args.shards)
    worker_default = max(1, min(len(shard_swaps), max(1, (os.cpu_count() or 2) // 2)))
    args.workers = args.workers or worker_default
    args.workers = max(1, min(args.workers, len(shard_swaps)))
    for index, shard in enumerate(shard_swaps):
        request = build_request(args, deck, shard)
        request_path = out_dir / f"request_shard{index}.json"
        request_json = json.dumps(request, indent=2, sort_keys=True) + "\n"
        request_path.write_text(request_json)
        (out_dir / f"request_shard{index}.jsonl").write_text(json.dumps(request, sort_keys=True) + "\n")

    manifest = {
        "deck_json": rel(deck_path),
        "registry": rel(registry_path),
        "swap_count": len(swap_dicts),
        "shards": len(shard_swaps),
        "shard_sizes": [len(shard) for shard in shard_swaps],
        "workers": args.workers,
        "samples_per_stage": args.samples_per_stage,
        "total_raw_samples_per_swap": args.samples_per_stage * (len(args.stages) if args.stages else 6),
        "seed": args.seed,
        "skipped": skipped,
        "command": "raw-delta-fast-stream-jsonl",
        "run": args.run,
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    (out_dir / "swaps.json").write_text(json.dumps(swap_dicts, indent=2, sort_keys=True) + "\n")
    print(json.dumps(manifest, indent=2, sort_keys=True))

    if args.run:
        run_shards(args, out_dir, len(shard_swaps))
        if args.merge:
            subprocess.run([sys.executable, str(MERGE_SCRIPT), str(out_dir), "--prefix", args.prefix], cwd=ROOT, check=True)
            if args.ingest_registry:
                ranked_path = out_dir / f"{args.prefix}_ranked.csv"
                ingest_cmd = [
                    sys.executable,
                    str(EXPERIMENT_LOOP),
                    "--registry",
                    str(registry_path),
                    "ingest-raw",
                    "--path",
                    str(ranked_path),
                    "--label",
                    args.ingest_label or out_dir.name,
                ]
                if args.force_ingest:
                    ingest_cmd.append("--force")
                subprocess.run(ingest_cmd, cwd=ROOT, check=True)
            if args.refresh_registry:
                subprocess.run(
                    [
                        sys.executable,
                        str(EXPERIMENT_LOOP),
                        "--registry",
                        str(registry_path),
                        "refresh",
                        "--min-accept-games",
                        "6000",
                    ],
                    cwd=ROOT,
                    check=True,
                )
                subprocess.run(
                    [sys.executable, str(EXPERIMENT_LOOP), "--registry", str(registry_path), "status"],
                    cwd=ROOT,
                    check=True,
                )
                subprocess.run(
                    [sys.executable, str(EXPERIMENT_LOOP), "--registry", str(registry_path), "suggest"],
                    cwd=ROOT,
                    check=True,
                )
    else:
        print("dry run only; pass --run to execute shards")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
