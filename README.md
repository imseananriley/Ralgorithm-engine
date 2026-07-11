# Ralgorithm Engine

High-performance, deck-specific simulation and optimization for early Rhystic Study and Heartwood Storyteller deployment in the five-color Nick Fury Commander shell.

This repository is a clean fork of the active simulator implementation. Historical experiment outputs and the unrelated Ral/ML implementation remain in the original `Ralgorithm` repository.

## Engine Layers

1. `fast_engine`: the existing fixed-library Rust solver used as a parity oracle during migration.
2. `nextgen`: fixed-size information-state and resource-planning primitives for the nonanticipating reference solver.
3. Future policy evaluator: a compiled visible-information policy for bulk paired experiments.

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
