#!/usr/bin/env bash
set -euo pipefail

RUNPOD_HOST="${RUNPOD_HOST:-}"
RUNPOD_PORT="${RUNPOD_PORT:-}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
KNOWN_HOSTS="${KNOWN_HOSTS:-/tmp/ralgorithm_runpod_known_hosts}"
REMOTE_DIR="${REMOTE_DIR:-/workspace/ralgorithm_engine}"
DECK_JSON="${DECK_JSON:-fixtures/decks/champion_working_list.json}"
OUT_DIR="${OUT_DIR:-benchmarks/results/generation12_large}"
SEED="${SEED:-2026071301}"
SAMPLES="${SAMPLES:-2000}"
SHARD_SAMPLES="${SHARD_SAMPLES:-100}"
DEPTH="${DEPTH:-14}"
PILOT_SAMPLES="${PILOT_SAMPLES:-64}"
WORKERS="${WORKERS:-32}"
BOTTOM_CANDIDATES="${BOTTOM_CANDIDATES:-1}"
DISCREPANCY_BUDGET="${DISCREPANCY_BUDGET:-1}"
ACTION_CANDIDATES="${ACTION_CANDIDATES:-2}"

if [[ -z "$RUNPOD_HOST" || -z "$RUNPOD_PORT" ]]; then
  echo "RUNPOD_HOST and RUNPOD_PORT are required" >&2
  exit 2
fi
if [[ ! -f "$DECK_JSON" || ! -d rust/rhystic_core ]]; then
  echo "Run from the Ralgorithm-engine repository root" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"
tmpdir="$(mktemp -d /tmp/ralgorithm-opening.XXXXXX)"
archive="$(mktemp /tmp/ralgorithm-opening.XXXXXX.tar.gz)"
trap 'rm -rf "$tmpdir" "$archive"' EXIT
mkdir -p "$tmpdir/rust" "$tmpdir/fixtures/decks"
mkdir -p "$tmpdir/rust/rhystic_core"
rsync -a --exclude target/ rust/rhystic_core/ "$tmpdir/rust/rhystic_core/"
cp Cargo.toml Cargo.lock "$tmpdir/"
cp "$DECK_JSON" "$tmpdir/fixtures/decks/deck.json"
COPYFILE_DISABLE=1 tar --no-xattrs -C "$tmpdir" -czf "$archive" .

ssh_opts=(-i "$SSH_KEY" -p "$RUNPOD_PORT" -o BatchMode=yes -o ServerAliveInterval=30 -o ServerAliveCountMax=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile="$KNOWN_HOSTS")
scp_opts=(-i "$SSH_KEY" -P "$RUNPOD_PORT" -o ServerAliveInterval=30 -o ServerAliveCountMax=10 -o StrictHostKeyChecking=no -o UserKnownHostsFile="$KNOWN_HOSTS")

ssh "${ssh_opts[@]}" "$RUNPOD_HOST" "rm -rf '$REMOTE_DIR' && mkdir -p '$REMOTE_DIR'"
scp "${scp_opts[@]}" "$archive" "$RUNPOD_HOST:$REMOTE_DIR/source.tar.gz"
ssh "${ssh_opts[@]}" "$RUNPOD_HOST" "
  set -euo pipefail
  cd '$REMOTE_DIR'
  tar --no-same-owner -xzf source.tar.gz
  if ! command -v cargo >/dev/null 2>&1; then
    apt-get update
    apt-get install -y --no-install-recommends ca-certificates curl build-essential
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    export PATH=/root/.cargo/bin:\$PATH
  fi
  cargo build --release --manifest-path Cargo.toml
"

binary="$REMOTE_DIR/target/release/rhystic-core-smoke"
calibration="$OUT_DIR/mulligan_calibration.json"
if [[ ! -s "$calibration" ]]; then
  python3 - "$DECK_JSON" "$SEED" "$DEPTH" "$PILOT_SAMPLES" "$WORKERS" "$BOTTOM_CANDIDATES" "$DISCREPANCY_BUDGET" "$ACTION_CANDIDATES" > "$OUT_DIR/calibration_request.json" <<'PY'
import json, sys
deck = json.load(open(sys.argv[1]))["deck"]
print(json.dumps({
    "variants": [{"name": "champion", "deck": deck, "influence_slots": []}],
    "seed": int(sys.argv[2]), "sample_start": 0, "samples": 1,
    "max_turn": 2, "depth": int(sys.argv[3]), "strict_reference": False,
    "discordance_limit": 10, "progress_interval": 1,
    "correction_numerator": 0, "correction_denominator": 1,
    "workers": int(sys.argv[5]), "exact_slot_draws": False,
    "mulligan_pilot_samples": int(sys.argv[4]), "strict_mulligan_pilot_samples": 1,
    "fixture_mode": False, "commander_identity_mask": 31,
    "publication_mode": True, "work_chunk_size": 1,
    "policy_bottom_candidate_limit": int(sys.argv[6]),
    "policy_discrepancy_budget": int(sys.argv[7]),
    "policy_action_candidate_limit": int(sys.argv[8])
}))
PY
  ssh "${ssh_opts[@]}" "$RUNPOD_HOST" "$binary opening-batch-jsonl" \
    < "$OUT_DIR/calibration_request.json" > "$calibration.tmp"
  mv "$calibration.tmp" "$calibration"
fi

start=0
while (( start < SAMPLES )); do
  count=$((SAMPLES - start))
  if (( count > SHARD_SAMPLES )); then count=$SHARD_SAMPLES; fi
  shard="$OUT_DIR/shard_$(printf '%08d' "$start")_$(printf '%08d' "$((start + count))").json"
  if [[ -s "$shard" ]]; then
    echo "[resume] $((start + count))/$SAMPLES"
    start=$((start + count))
    continue
  fi
  python3 - "$DECK_JSON" "$calibration" "$SEED" "$start" "$count" "$DEPTH" "$WORKERS" "$BOTTOM_CANDIDATES" "$DISCREPANCY_BUDGET" "$ACTION_CANDIDATES" > "$OUT_DIR/current_request.json" <<'PY'
import json, sys
deck = json.load(open(sys.argv[1]))["deck"]
calibration = json.load(open(sys.argv[2]))
print(json.dumps({
    "variants": [{"name": "champion", "deck": deck, "influence_slots": []}],
    "seed": int(sys.argv[3]), "sample_start": int(sys.argv[4]), "samples": int(sys.argv[5]),
    "max_turn": 2, "depth": int(sys.argv[6]), "strict_reference": False,
    "discordance_limit": 100, "progress_interval": 10,
    "correction_numerator": 0, "correction_denominator": 1,
    "workers": int(sys.argv[7]), "exact_slot_draws": False,
    "mulligan_pilot_samples": 1, "strict_mulligan_pilot_samples": 1,
    "fixture_mode": False, "commander_identity_mask": 31,
    "publication_mode": True, "work_chunk_size": 1,
    "policy_bottom_candidate_limit": int(sys.argv[8]),
    "policy_discrepancy_budget": int(sys.argv[9]),
    "policy_action_candidate_limit": int(sys.argv[10]),
    "frozen_low_mulligan_continuation_ev": calibration["low_mulligan_continuation_ev"]
}))
PY
  ssh "${ssh_opts[@]}" "$RUNPOD_HOST" "$binary opening-batch-jsonl" \
    < "$OUT_DIR/current_request.json" > "$shard.tmp"
  python3 - "$shard.tmp" "$((start + count))" <<'PY'
import json, sys
row = json.load(open(sys.argv[1]))
if row.get("error"):
    raise SystemExit(row["error"])
if row["report"]["next_sample"] != int(sys.argv[2]):
    raise SystemExit("unexpected resume boundary")
PY
  mv "$shard.tmp" "$shard"
  start=$((start + count))
  echo "[progress] $start/$SAMPLES"
done

python3 - "$OUT_DIR" "$SAMPLES" <<'PY'
import json, math, pathlib, sys
out = pathlib.Path(sys.argv[1]); expected = int(sys.argv[2])
rows = [json.load(open(path)) for path in sorted(out.glob("shard_*.json"))]
digests = {row["model_digest"] for row in rows}
if len(digests) != 1: raise SystemExit("model digest mismatch across shards")
n = sum(row["report"]["accumulators"][0]["samples"] for row in rows)
if n != expected: raise SystemExit(f"merged {n} samples, expected {expected}")
value_sum = sum(row["report"]["accumulators"][0]["value_sum"] for row in rows)
value_sq = sum(row["report"]["accumulators"][0]["value_sum_squares"] for row in rows)
mean = value_sum / n
variance = max(0.0, (value_sq - value_sum * value_sum / n) / (n - 1)) if n > 1 else 0.0
keys = ["weighted_ev", "rhystic_turn_1", "rhystic_by_turn_2", "heartwood_turn_1", "heartwood_by_turn_2", "any_engine_by_turn_1", "any_engine_by_turn_2", "weighted_ev_upper", "capped_rate"]
outcomes = {key: sum(row["outcomes"][0][key] * row["outcomes"][0]["samples"] for row in rows) / n for key in keys}
summary = {
    "samples": n, "model_digest": next(iter(digests)), "weighted_ev": mean,
    "weighted_standard_error": math.sqrt(variance / n), "outcomes": outcomes,
    "measured_worker_seconds": sum(row["report"]["elapsed_seconds"] for row in rows),
    "shards": len(rows), "next_sample": max(row["report"]["next_sample"] for row in rows),
}
(out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
print(json.dumps(summary, indent=2, sort_keys=True))
PY
