#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
from copy import deepcopy
from http import HTTPStatus
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
from itertools import combinations
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Any
from urllib.parse import parse_qs, quote, unquote, urlparse
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
STATIC_DIR = Path(__file__).resolve().parent / "static"
DATA_ROOT = ROOT / "data"
DEFAULT_RUN_ROOT = DATA_ROOT / "rhystic_study_turn12"
BENCHMARK_RUN_ROOT = ROOT / "benchmarks" / "results"
NOTES_PATH = Path(__file__).resolve().parent / "notes.jsonl"
MANIFEST_PATH = ROOT / "visualizer" / "card_manifest.json"
CARD_CACHE_PATH = Path(__file__).resolve().parent / "card_cache.json"
RUST_BIN = ROOT / "target" / "release" / "rhystic-core-smoke"
RUN_CACHE: dict[str, Any] = {"expires": 0.0, "runs": []}
PROGRESS_CACHE: dict[str, Any] = {"expires": 0.0, "runs": []}
RUN_SCAN_LIMIT = int(os.environ.get("VALIDATOR_RUN_SCAN_LIMIT", "140"))
MANIFEST_CACHE: dict[str, Any] | None = None
CARD_CACHE: dict[str, Any] | None = None

PROGRESS_FLOAT_FIELDS = {
    "baseline_rate",
    "candidate_rate",
    "rate_delta",
    "rate_delta_ci_low",
    "rate_delta_ci_high",
    "score_delta_mean",
    "score_delta",
    "score_delta_ci_low",
    "score_delta_ci_high",
    "bootstrap_score_ci_low",
    "bootstrap_score_ci_high",
    "mcnemar_p",
}
PROGRESS_INT_FIELDS = {
    "games",
    "baseline_successes",
    "candidate_successes",
    "candidate_only_successes",
    "baseline_only_successes",
    "both_successes",
    "both_misses",
    "score_positive",
    "score_negative",
    "score_tied",
    "candidate_cap_misses",
    "baseline_cap_misses",
    "n_for_rate_half_width",
    "n_for_score_half_width",
    "n_for_observed_rate_power",
    "n_for_observed_score_power",
}


def json_response(handler: SimpleHTTPRequestHandler, payload: Any, status: int = 200) -> None:
    body = json.dumps(browser_safe_payload(payload), sort_keys=True).encode("utf-8")
    handler.send_response(status)
    handler.send_header("Content-Type", "application/json; charset=utf-8")
    handler.send_header("Cache-Control", "no-store")
    handler.send_header("Content-Length", str(len(body)))
    handler.end_headers()
    handler.wfile.write(body)


def browser_safe_payload(payload: Any) -> Any:
    if isinstance(payload, list):
        return [browser_safe_payload(item) for item in payload]
    if isinstance(payload, dict):
        safe: dict[str, Any] = {}
        for key, value in payload.items():
            if (key == "seed" or key.endswith("_seed")) and isinstance(value, int):
                safe[key] = str(value)
            else:
                safe[key] = browser_safe_payload(value)
        return safe
    return payload


def error_response(handler: SimpleHTTPRequestHandler, message: str, status: int = 400) -> None:
    json_response(handler, {"error": message}, status)


def safe_repo_path(raw_path: str) -> Path:
    rel = Path(unquote(raw_path))
    if rel.is_absolute():
        candidate = rel.resolve()
    else:
        candidate = (ROOT / rel).resolve()
    if ROOT not in candidate.parents and candidate != ROOT:
        raise ValueError("path is outside the repository")
    if not candidate.exists():
        raise ValueError(f"path does not exist: {raw_path}")
    return candidate


def load_json_file(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def progress_csv_value(key: str, value: str) -> Any:
    if value == "":
        return None
    if key in PROGRESS_INT_FIELDS:
        try:
            return int(value)
        except ValueError:
            return value
    if key in PROGRESS_FLOAT_FIELDS:
        try:
            return float(value)
        except ValueError:
            return value
    return value


def progress_summary_csv_path(progress_path: Path, payload: dict[str, Any]) -> Path | None:
    candidates: list[Path] = []
    raw = ((payload.get("outputs") or {}).get("intermediate_summary_csv") or "").strip()
    if raw:
        raw_path = Path(raw)
        candidates.append(raw_path if raw_path.is_absolute() else ROOT / raw_path)
    candidates.append(progress_path.parent / "intermediate_summary.csv")
    for candidate in candidates:
        resolved = candidate.resolve()
        if ROOT not in resolved.parents and resolved != ROOT:
            continue
        if resolved.exists():
            return resolved
    return None


def load_progress_intermediate_rows(progress_path: Path, payload: dict[str, Any]) -> list[dict[str, Any]]:
    csv_path = progress_summary_csv_path(progress_path, payload)
    if csv_path is None:
        return []
    rows: list[dict[str, Any]] = []
    with csv_path.open("r", encoding="utf-8", newline="") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            rows.append({key: progress_csv_value(key, value) for key, value in row.items()})
    return rows


def load_manifest() -> dict[str, Any]:
    global MANIFEST_CACHE
    if MANIFEST_CACHE is None:
        payload = load_json_file(MANIFEST_PATH)
        MANIFEST_CACHE = payload if isinstance(payload, dict) else {}
    return MANIFEST_CACHE


def load_card_cache() -> dict[str, Any]:
    global CARD_CACHE
    if CARD_CACHE is None:
        if CARD_CACHE_PATH.exists():
            try:
                payload = load_json_file(CARD_CACHE_PATH)
                CARD_CACHE = payload if isinstance(payload, dict) else {}
            except Exception:
                CARD_CACHE = {}
        else:
            CARD_CACHE = {}
    return CARD_CACHE


def save_card_cache() -> None:
    if CARD_CACHE is None:
        return
    CARD_CACHE_PATH.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = CARD_CACHE_PATH.with_suffix(".json.tmp")
    tmp_path.write_text(json.dumps(CARD_CACHE, indent=2, sort_keys=True), encoding="utf-8")
    tmp_path.replace(CARD_CACHE_PATH)


def manifest_lookup(name: str) -> dict[str, Any] | None:
    manifest = load_manifest()
    exact = manifest.get(name)
    if isinstance(exact, dict) and (exact.get("image") or exact.get("image_large")):
        return exact
    if " // " in name:
        front = name.split(" // ", 1)[0]
        front_match = manifest.get(front)
        if isinstance(front_match, dict) and (front_match.get("image") or front_match.get("image_large")):
            return front_match
    for card in manifest.values():
        if not isinstance(card, dict):
            continue
        if card.get("name") == name or card.get("back_name") == name:
            if card.get("image") or card.get("image_large"):
                return card
    return None


def scryfall_card_payload(card: dict[str, Any], requested_name: str) -> dict[str, Any]:
    image_uris = card.get("image_uris") or {}
    faces = card.get("card_faces") or []
    back_face = faces[1] if len(faces) > 1 and isinstance(faces[1], dict) else {}
    if not image_uris and faces and isinstance(faces[0], dict):
        image_uris = faces[0].get("image_uris") or {}
    back_images = back_face.get("image_uris") or {}
    return {
        "name": card.get("name") or requested_name,
        "image": image_uris.get("normal") or image_uris.get("large") or image_uris.get("small"),
        "image_large": image_uris.get("large") or image_uris.get("normal"),
        "back_name": back_face.get("name"),
        "back_image": back_images.get("normal") or back_images.get("large") or back_images.get("small"),
        "back_image_large": back_images.get("large") or back_images.get("normal"),
        "layout": card.get("layout"),
        "mana_cost": card.get("mana_cost") or (faces[0].get("mana_cost") if faces and isinstance(faces[0], dict) else ""),
        "oracle_text": card.get("oracle_text") or (faces[0].get("oracle_text") if faces and isinstance(faces[0], dict) else ""),
        "scryfall_id": card.get("id"),
        "type_line": card.get("type_line") or (faces[0].get("type_line") if faces and isinstance(faces[0], dict) else ""),
    }


def fetch_scryfall_card(name: str) -> dict[str, Any] | None:
    headers = {
        "Accept": "application/json",
        "User-Agent": "RhysticRunValidator/0.1 local image cache",
    }
    for mode in ("exact", "fuzzy"):
        url = f"https://api.scryfall.com/cards/named?{mode}={quote(name, safe='')}"
        request = Request(url, headers=headers)
        try:
            with urlopen(request, timeout=8) as response:
                card = json.loads(response.read().decode("utf-8"))
        except Exception:
            continue
        payload = scryfall_card_payload(card, name)
        if payload.get("image") or payload.get("image_large"):
            return payload
    return None


def lookup_card_image(name: str) -> dict[str, Any] | None:
    name = name.strip()
    if not name:
        return None
    manifest_match = manifest_lookup(name)
    if manifest_match:
        return manifest_match
    cache = load_card_cache()
    cached = cache.get(name)
    if isinstance(cached, dict) and (cached.get("image") or cached.get("image_large")):
        return cached
    payload = fetch_scryfall_card(name)
    if payload:
        cache[name] = payload
        save_card_cache()
    return payload


def record_counts(payload: dict[str, Any]) -> dict[str, int]:
    evaluation = payload.get("evaluation") or {}
    return {
        "game_records": len(evaluation.get("game_records") or []),
        "cap_replay_records": len(evaluation.get("cap_replay_records") or []),
        "validation_records": len(evaluation.get("validation_records") or []),
    }


def summarize_run(path: Path, payload: dict[str, Any]) -> dict[str, Any] | None:
    counts = record_counts(payload)
    if not any(counts.values()):
        return None
    evaluation = payload.get("evaluation") or {}
    rel = path.relative_to(ROOT).as_posix()
    return {
        "path": rel,
        "name": path.name,
        "label": rel.removeprefix("data/rhystic_study_turn12/"),
        "mtime": path.stat().st_mtime,
        "size": path.stat().st_size,
        "counts": counts,
        "games": evaluation.get("games") or payload.get("eval_games"),
        "successes": evaluation.get("successes"),
        "success_rate": evaluation.get("success_rate"),
        "cap_misses": evaluation.get("cap_misses"),
        "turn_counts": evaluation.get("turn_counts") or {},
        "deck_json": payload.get("deck_json"),
        "target": payload.get("target"),
        "seed": payload.get("seed"),
    }


def list_runs() -> list[dict[str, Any]]:
    now = time.time()
    if RUN_CACHE["expires"] > now:
        return list(RUN_CACHE["runs"])
    runs: list[dict[str, Any]] = []
    roots = [DEFAULT_RUN_ROOT, BENCHMARK_RUN_ROOT]
    candidates: list[tuple[float, Path]] = []
    for root in roots:
        if not root.exists():
            continue
        for path in root.rglob("*.json"):
            try:
                candidates.append((path.stat().st_mtime, path))
            except Exception:
                continue
    candidates.sort(key=lambda item: item[0], reverse=True)
    for _, path in candidates[:RUN_SCAN_LIMIT]:
        try:
            if path.stat().st_size > 250 * 1024 * 1024:
                continue
            payload = load_json_file(path)
            if not isinstance(payload, dict):
                continue
            summary = summarize_run(path, payload)
            if summary:
                runs.append(summary)
        except (OSError, ValueError, TypeError):
            continue
    runs.sort(key=lambda item: (item["mtime"], item["path"]), reverse=True)
    RUN_CACHE["runs"] = runs
    RUN_CACHE["expires"] = now + 15.0
    return runs


def summarize_progress(path: Path, payload: dict[str, Any]) -> dict[str, Any] | None:
    rel = path.relative_to(ROOT).as_posix()
    out_dir = payload.get("out_dir") or rel.removesuffix("/progress.json")
    total = int(payload.get("total_variants") or 0)
    completed = int(payload.get("completed_variants") or 0)
    if total <= 0:
        return None
    return {
        "path": rel,
        "out_dir": out_dir,
        "label": str(out_dir).removeprefix("data/rhystic_study_turn12/"),
        "mtime": path.stat().st_mtime,
        "total_variants": total,
        "completed_variants": completed,
        "percent_complete": payload.get("percent_complete"),
        "current_variant": payload.get("current_variant"),
        "latest_completed": (payload.get("latest_completed") or {}).get("name"),
        "eta_text": payload.get("eta_text"),
    }


def list_progress_runs() -> list[dict[str, Any]]:
    now = time.time()
    if PROGRESS_CACHE["expires"] > now:
        return list(PROGRESS_CACHE["runs"])
    runs: list[dict[str, Any]] = []
    if DEFAULT_RUN_ROOT.exists():
        candidates = sorted(
            DEFAULT_RUN_ROOT.rglob("progress.json"),
            key=lambda path: path.stat().st_mtime if path.exists() else 0,
            reverse=True,
        )
        for path in candidates[:50]:
            try:
                payload = load_json_file(path)
                if not isinstance(payload, dict):
                    continue
                summary = summarize_progress(path, payload)
                if summary:
                    runs.append(summary)
            except Exception:
                continue
    PROGRESS_CACHE["runs"] = runs
    PROGRESS_CACHE["expires"] = now + 5.0
    return runs


def progress_path_from_query(raw_path: str) -> Path:
    if not raw_path:
        runs = list_progress_runs()
        if not runs:
            raise ValueError("no progress artifacts found")
        raw_path = runs[0]["path"]
    path = safe_repo_path(raw_path)
    if path.is_dir():
        path = path / "progress.json"
    if path.name != "progress.json":
        raise ValueError("progress path must be a run directory or progress.json")
    if not path.exists():
        raise ValueError(f"path does not exist: {path.relative_to(ROOT).as_posix()}")
    return path


def solve_request_from_payload(payload: dict[str, Any]) -> dict[str, Any]:
    record = payload.get("record") or {}
    state_limit = int(payload.get("state_limit") or record.get("actual_cap_rerun_state_limit") or record.get("state_limit") or 200000)
    hand = payload.get("hand", record.get("keep", []))
    library = payload.get("library", record.get("library", []))
    return {
        "hand": list(hand),
        "library": list(library),
        "gemstone_live": bool(payload.get("gemstone_live", record.get("gemstone_caverns_live", False))),
        "state_limit": state_limit,
        "max_turns": int(payload.get("max_turns", record.get("max_turns", 2))),
        "goal": str(payload.get("goal", record.get("goal", "engine"))),
        "engine_target_count": int(payload.get("engine_target_count", record.get("engine_target_count", 1))),
        "engine_success_policy": str(payload.get("engine_success_policy", record.get("engine_success_policy", "resilient"))),
        "remora_upkeep_payments": int(payload.get("remora_upkeep_payments", record.get("remora_upkeep_payments", 2))),
        "action_sort": bool(payload.get("action_sort", record.get("action_sort", True))),
        "gamble_mode": payload.get("gamble_mode", record.get("gamble_mode")),
        "gamble_seed": int(payload.get("gamble_seed", record.get("gamble_seed", 0)) or 0),
        "simplified_gamble": bool(payload.get("simplified_gamble", record.get("simplified_gamble", False))),
    }


def run_rust_solve(request: dict[str, Any]) -> dict[str, Any]:
    if not RUST_BIN.exists():
        raise RuntimeError(f"Rust binary not found: {RUST_BIN}")
    process = subprocess.run(
        [str(RUST_BIN), "solve-keep-fast-jsonl"],
        input=json.dumps(request, sort_keys=True) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=ROOT,
        env=rust_solver_env(None),
        check=False,
    )
    if process.returncode != 0:
        raise RuntimeError(process.stderr.strip() or f"rust solver exited {process.returncode}")
    lines = [line for line in process.stdout.splitlines() if line.strip()]
    if not lines:
        raise RuntimeError("rust solver returned no output")
    response = json.loads(lines[-1])
    if not isinstance(response, dict) or response.get("error"):
        raise RuntimeError(f"Rust solver rejected request: {response}")
    response["request"] = request
    response["hit"] = response.get("turn") is not None and int(response["turn"]) <= int(request["max_turns"])
    return response


def rust_solver_env(strict_hidden: bool | None) -> dict[str, str] | None:
    if strict_hidden is None:
        return None
    env = os.environ.copy()
    if strict_hidden:
        env["RHYSTIC_STRICT_SHUFFLE_HIDDEN"] = "1"
    else:
        env.pop("RHYSTIC_STRICT_SHUFFLE_HIDDEN", None)
    return env


def run_rust_solve_batch(requests: list[dict[str, Any]], strict_hidden: bool | None = None) -> list[dict[str, Any]]:
    if not RUST_BIN.exists():
        raise RuntimeError(f"Rust binary not found: {RUST_BIN}")
    process = subprocess.run(
        [str(RUST_BIN), "solve-keep-fast-batch-jsonl"],
        input=json.dumps(requests, sort_keys=True) + "\n",
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=ROOT,
        env=rust_solver_env(strict_hidden),
        check=False,
    )
    if process.returncode != 0:
        raise RuntimeError(process.stderr.strip() or f"rust solver exited {process.returncode}")
    lines = [line for line in process.stdout.splitlines() if line.strip()]
    if not lines:
        raise RuntimeError("rust solver returned no output")
    responses = json.loads(lines[-1])
    if not isinstance(responses, list):
        raise RuntimeError("rust solver batch returned non-list output")
    if len(responses) != len(requests):
        raise RuntimeError("rust solver batch returned an incorrect number of results")
    for response, request in zip(responses, requests):
        if not isinstance(response, dict) or response.get("error"):
            raise RuntimeError(f"Rust solver rejected batch request: {response}")
        response["request"] = request
        response["hit"] = response.get("turn") is not None and int(response["turn"]) <= int(request["max_turns"])
    return responses


def solve_variant(base: dict[str, Any], mode: str, simplified: bool, seed: int | None = None) -> dict[str, Any]:
    request = deepcopy(base)
    request["gamble_mode"] = mode
    request["simplified_gamble"] = simplified
    if seed is not None:
        request["gamble_seed"] = seed
    return request


def remove_cards_by_name(cards: list[str], removed: list[str]) -> list[str]:
    counts: dict[str, int] = {}
    for name in removed:
        counts[name] = counts.get(name, 0) + 1
    kept: list[str] = []
    for name in cards:
        count = counts.get(name, 0)
        if count > 0:
            counts[name] = count - 1
        else:
            kept.append(name)
    return kept


def same_card_multiset(left: list[str], right: list[str]) -> bool:
    if len(left) != len(right):
        return False
    counts: dict[str, int] = {}
    for name in left:
        counts[name] = counts.get(name, 0) + 1
    for name in right:
        count = counts.get(name, 0)
        if count <= 0:
            return False
        counts[name] = count - 1
    return all(count == 0 for count in counts.values())


def slim_solve_response(response: dict[str, Any]) -> dict[str, Any]:
    gamble_seed = (response.get("request") or {}).get("gamble_seed")
    return {
        "hit": bool(response.get("hit")),
        "turn": response.get("turn"),
        "capped": bool(response.get("capped")),
        "label": response.get("label") or "",
        "gamble_seed": str(gamble_seed) if gamble_seed is not None else None,
        "gamble_mode": (response.get("request") or {}).get("gamble_mode"),
    }


def audit_bottoms(payload: dict[str, Any]) -> dict[str, Any]:
    record = payload.get("record") or {}
    include_strict_diff = payload.get("include_strict_diff", True)
    if isinstance(include_strict_diff, str):
        include_strict_diff = include_strict_diff.lower() not in {"0", "false", "no", "off"}
    else:
        include_strict_diff = bool(include_strict_diff)
    visible = list(
        payload.get("visible_hand")
        or record.get("visible_hand")
        or [*(record.get("keep") or []), *(record.get("bottomed") or [])]
    )
    bottom_count = int(
        payload.get("bottom_count")
        or record.get("bottom_count")
        or len(record.get("bottomed") or [])
        or 0
    )
    if bottom_count < 0 or bottom_count > len(visible):
        raise ValueError(f"invalid bottom_count {bottom_count} for {len(visible)} visible cards")

    choices = list(combinations(range(len(visible)), bottom_count))
    if len(choices) > 256:
        raise ValueError(f"too many bottom choices to audit at once: {len(choices)}")

    record_library = list(record.get("library") or [])
    original_bottomed = list(record.get("bottomed") or [])
    base_library = record_library
    if original_bottomed and len(record_library) >= len(original_bottomed):
        base_library = record_library[: len(record_library) - len(original_bottomed)]

    requests: list[dict[str, Any]] = []
    candidates: list[dict[str, Any]] = []
    for indexes in choices:
        index_set = set(indexes)
        bottom = [visible[index] for index in indexes]
        keep = [name for index, name in enumerate(visible) if index not in index_set]
        library = [*base_library, *bottom]
        base = solve_request_from_payload({**payload, "record": record, "hand": keep, "library": library})
        candidate = {
            "bottom": bottom,
            "keep": keep,
            "recorded": same_card_multiset(bottom, original_bottomed),
        }
        candidates.append(candidate)
        requests.append(base)
        requests.append(solve_variant(base, "optimistic", False))
        requests.append(solve_variant(base, "off", False))

    responses = run_rust_solve_batch(requests, strict_hidden=False)
    strict_responses = run_rust_solve_batch(requests, strict_hidden=True) if include_strict_diff else []
    rows: list[dict[str, Any]] = []
    for index, candidate in enumerate(candidates):
        current = slim_solve_response(responses[index * 3])
        optimistic = slim_solve_response(responses[index * 3 + 1])
        no_gamble = slim_solve_response(responses[index * 3 + 2])
        strict_current = slim_solve_response(strict_responses[index * 3]) if strict_responses else None
        strict_optimistic = slim_solve_response(strict_responses[index * 3 + 1]) if strict_responses else None
        strict_no_gamble = slim_solve_response(strict_responses[index * 3 + 2]) if strict_responses else None
        shuffle_sensitive = any(
            normal["hit"] and strict is not None and not strict["hit"]
            for normal, strict in [
                (current, strict_current),
                (optimistic, strict_optimistic),
                (no_gamble, strict_no_gamble),
            ]
        )
        row = {
            **candidate,
            "current": current,
            "optimistic": optimistic,
            "no_gamble": no_gamble,
            "strict_current": strict_current,
            "strict_optimistic": strict_optimistic,
            "strict_no_gamble": strict_no_gamble,
            "shuffle_sensitive": shuffle_sensitive,
            "hit": bool(current["hit"] or optimistic["hit"] or no_gamble["hit"]),
            "capped": bool(current["capped"] or optimistic["capped"] or no_gamble["capped"]),
        }
        rows.append(row)

    def turn_value(summary: dict[str, Any]) -> int:
        return int(summary["turn"]) if summary.get("hit") and summary.get("turn") is not None else 99

    def sort_key(row: dict[str, Any]) -> tuple[Any, ...]:
        current = row["current"]
        optimistic = row["optimistic"]
        no_gamble = row["no_gamble"]
        best_turn = min(turn_value(current), turn_value(optimistic), turn_value(no_gamble))
        return (
            not bool(current["hit"]),
            not bool(optimistic["hit"]),
            not bool(no_gamble["hit"]),
            best_turn,
            not bool(row["recorded"]),
            bool(row["capped"]),
            ", ".join(row["bottom"]),
        )

    rows.sort(key=sort_key)
    return {
        "visible_hand": visible,
        "bottom_count": bottom_count,
        "state_limit": int(payload.get("state_limit") or record.get("actual_cap_rerun_state_limit") or record.get("state_limit") or 200000),
        "candidate_count": len(rows),
        "hit_count": sum(1 for row in rows if row["hit"]),
        "shuffle_sensitive_count": sum(1 for row in rows if row.get("shuffle_sensitive")),
        "include_strict_diff": include_strict_diff,
        "rows": rows,
    }


def audit_gamble(payload: dict[str, Any]) -> dict[str, Any]:
    base = solve_request_from_payload(payload)
    sample_count = max(1, min(512, int(payload.get("sample_count") or payload.get("samples") or 64)))
    base_seed = int(base.get("gamble_seed") or 0)
    has_gamble = "Gamble" in base.get("hand", [])

    requests = [
        deepcopy(base),
        solve_variant(base, "off", False),
        solve_variant(base, "optimistic", False),
    ]
    first_sample_index = len(requests)
    for index in range(sample_count):
        seed = (base_seed + index * 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        requests.append(solve_variant(base, "stochastic", True, seed))

    responses = run_rust_solve_batch(requests)
    samples = [slim_solve_response(response) for response in responses[first_sample_index:]]
    hits = [sample for sample in samples if sample["hit"]]
    misses = [sample for sample in samples if not sample["hit"]]
    return {
        "has_gamble": has_gamble,
        "hand": base.get("hand", []),
        "current": slim_solve_response(responses[0]),
        "no_gamble": slim_solve_response(responses[1]),
        "optimistic": slim_solve_response(responses[2]),
        "stochastic": {
            "samples": sample_count,
            "hits": len(hits),
            "misses": len(misses),
            "hit_rate": len(hits) / sample_count,
            "hit_examples": hits[:5],
            "miss_examples": misses[:5],
        },
        "request": base,
    }


class ValidatorHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, directory=str(STATIC_DIR), **kwargs)

    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()

    def log_message(self, format: str, *args: Any) -> None:
        sys.stderr.write("%s - - [%s] %s\n" % (self.client_address[0], self.log_date_time_string(), format % args))

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        if parsed.path == "/api/runs":
            json_response(self, {"runs": list_runs(), "root": str(ROOT)})
            return
        if parsed.path == "/api/progress-runs":
            json_response(self, {"runs": list_progress_runs(), "root": str(ROOT)})
            return
        if parsed.path == "/api/progress":
            params = parse_qs(parsed.query)
            raw_path = (params.get("path") or [""])[0]
            try:
                path = progress_path_from_query(raw_path)
                payload = load_json_file(path)
                if isinstance(payload, dict):
                    payload = dict(payload)
                    rows = load_progress_intermediate_rows(path, payload)
                    payload["intermediate_rows"] = rows
                    payload["intermediate_row_count"] = len(rows)
                json_response(self, {"path": path.relative_to(ROOT).as_posix(), "payload": payload})
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.BAD_REQUEST)
            return
        if parsed.path == "/api/run":
            params = parse_qs(parsed.query)
            raw_path = (params.get("path") or [""])[0]
            if not raw_path:
                error_response(self, "missing path")
                return
            try:
                path = safe_repo_path(raw_path)
                json_response(self, {"path": path.relative_to(ROOT).as_posix(), "payload": load_json_file(path)})
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.BAD_REQUEST)
            return
        if parsed.path == "/api/manifest":
            try:
                json_response(self, load_manifest())
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        if parsed.path == "/api/card":
            params = parse_qs(parsed.query)
            name = (params.get("name") or [""])[0].strip()
            if not name:
                error_response(self, "missing name")
                return
            try:
                payload = lookup_card_image(name)
                if not payload:
                    error_response(self, f"card image not found: {name}", HTTPStatus.NOT_FOUND)
                    return
                json_response(self, payload)
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        if parsed.path == "/api/notes":
            notes = []
            if NOTES_PATH.exists():
                for line in NOTES_PATH.read_text(encoding="utf-8").splitlines():
                    if line.strip():
                        notes.append(json.loads(line))
            json_response(self, {"notes": notes})
            return
        super().do_GET()

    def do_POST(self) -> None:
        parsed = urlparse(self.path)
        length = int(self.headers.get("Content-Length", "0") or "0")
        try:
            payload = json.loads(self.rfile.read(length).decode("utf-8") or "{}")
        except Exception as exc:
            error_response(self, f"invalid JSON: {exc}")
            return
        if parsed.path == "/api/solve":
            try:
                request = solve_request_from_payload(payload)
                json_response(self, run_rust_solve(request))
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        if parsed.path == "/api/gamble-audit":
            try:
                json_response(self, audit_gamble(payload))
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        if parsed.path == "/api/bottom-audit":
            try:
                json_response(self, audit_bottoms(payload))
            except Exception as exc:
                error_response(self, str(exc), HTTPStatus.INTERNAL_SERVER_ERROR)
            return
        if parsed.path == "/api/notes":
            note = {
                "run_path": payload.get("run_path"),
                "record_key": payload.get("record_key"),
                "record_type": payload.get("record_type"),
                "note": payload.get("note", ""),
                "zones": payload.get("zones", {}),
            }
            NOTES_PATH.parent.mkdir(parents=True, exist_ok=True)
            with NOTES_PATH.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps(note, sort_keys=True) + "\n")
            json_response(self, {"saved": True, "note": note})
            return
        error_response(self, f"unknown endpoint: {parsed.path}", HTTPStatus.NOT_FOUND)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the Rhystic simulator validation UI.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8765)
    args = parser.parse_args()
    os.chdir(ROOT)
    server = ThreadingHTTPServer((args.host, args.port), ValidatorHandler)
    print(f"validator UI: http://{args.host}:{args.port}")
    server.serve_forever()


if __name__ == "__main__":
    main()
