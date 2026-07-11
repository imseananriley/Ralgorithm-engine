# Ralgorithm Engine

High-performance, deck-specific simulation and optimization for early Rhystic Study and Heartwood Storyteller deployment in the five-color Nick Fury Commander shell.

This repository is a clean fork of the active simulator implementation. Historical experiment outputs and the unrelated Ral/ML implementation remain in the original `Ralgorithm` repository.

## Engine Layers

1. `fast_engine`: the fixed-library Rust solver used as a parity oracle and production bridge during migration. Libraries use shared immutable storage, cached hashes, cached membership indexes, and cached structural-key canonicalization. Registry-compiled action masks skip strategic generators that cannot apply to the current hand.
2. `nextgen`: fixed-size information state, immutable card registry, resource planning, explicit-chance reference search, and compiled-policy evaluation primitives.
3. Production integration: card classification and strategic dispatch read from the registry. `RALGORITHM_PAYMENT_DIRECTED=packed` enables the generation-five cost-specific planner; `generic` retains the generation-four closure oracle. Full production search over the fixed-size `nextgen` state remains the next migration stage.

The reference solver and production policy intentionally have different jobs. The reference solver supplies strict labels and correction samples; the production policy supplies throughput.

## Build And Test

```bash
cargo test --release
cargo run --release -p rhystic_core --bin rhystic-core-smoke -- bench-nextgen 100000
python3 -m py_compile scripts/*.py validator_ui/server.py
```

## Reproducibility

Generate a source manifest before every retained experiment:

```bash
python3 scripts/source_manifest.py --out benchmarks/source_manifest.json
```

Run configurations must record the manifest digest, deck digest, root seed, policy version, objective weights, and state-cap behavior.

The current generation comparison is documented in [docs/generation5_benchmark.md](docs/generation5_benchmark.md).
