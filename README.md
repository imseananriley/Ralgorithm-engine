# Ralgorithm Engine

Ralgorithm is a high-throughput, visible-information simulator for comparing early-engine
lines in Commander decks. Its current production objective is resolving Rhystic Study or
Heartwood Storyteller by turn two, with exact colored mana, London mulligans, tutors,
random outcomes, and paired deck-variant evaluation.

This is an objective-specific research engine, not a general Magic rules engine. Every card
that can affect the opening objective needs explicit semantics. Unsupported interaction and
post-engine cards may be intentionally inert; unsupported acceleration, tutors, lands, or
draw effects must be audited before results are trusted.

## Requirements

- Rust stable with Cargo
- Python 3.10 or newer; Python 3.12+ enables the much faster exact multinomial bootstrap.
  Runtime scripts use only the standard library.
- macOS or Linux for the tested multiprocessing and benchmark workflows

## Quick Start

```bash
python3 scripts/ralgorithm.py doctor
cargo build --release --locked
RUST_MIN_STACK=16777216 cargo test --locked --workspace
python3 -m unittest discover -s tests
```

Run the included 100-game paired smoke experiment:

```bash
python3 scripts/ralgorithm.py compare --preset smoke
```

Use `--preset local` for 2,000 games per variant or `--preset publication` for 10,000.
The preset name is historical: 10,000 games alone do not establish publication-quality
evidence. Use the [sample-size guide](docs/statistical_experiment_design.md) to choose a
confirmatory sample for the effect size and number of candidates.
Results are written under `benchmarks/results/`, while threshold and baseline caches are
shared under `.cache/ralgorithm/` so later experiments do not recompute identical work.
Cache keys include a digest of the Rust engine and orchestration sources, preventing stale
results from surviving a code change.

Each invocation generates a fresh root seed and prints it. Pass `--seed <recorded-seed>`
and the same output directory to resume or reproduce an experiment. Repeated identical
games provide no additional statistical evidence. Two workers are used by default to keep
local CPU use moderate; increase `--workers` explicitly when appropriate.

## Bring A Deck

Export a deck as plain text with one `quantity card name` per line, then convert it:

```bash
python3 scripts/ralgorithm.py import-deck deck.txt fixtures/decks/my_deck.json \
  --name "My deck" \
  --commander "Commander Name"
```

Repeat `--commander` for partners. `SIDEBOARD:` starts the sideboard section. Structural
validation enforces the expected 99-card or 98-card mainboard and Commander singleton
rules.

Production commander models currently cover Nick Fury, Rograkh/Silas, and
Rograkh/Thrasios. Other lists can be imported, but comparison is rejected until their
commander semantics are implemented. Structural validation allows repeated basic lands;
it is not a live Commander ban-list, color-identity, or partner-legality check.

Audit semantic coverage before simulating a new list:

```bash
python3 scripts/ralgorithm.py check-deck fixtures/decks/my_deck.json \
  --semantic \
  --swap-file benchmarks/my_swaps.txt
```

The strict audit fails if a base-deck or added card lacks an explicit classification. See
[Adding decks and cards](docs/adding_decks_and_cards.md) for the fixture format, swap-file
syntax, modeling workflow, and correctness requirements.

## Compare Swaps

Each non-comment line in a swap file is either a single replacement or a grouped package:

```text
probe_to_land: Gitaxian Probe=City of Brass
breach_package: Rain of Filth=Underworld Breach; Gitaxian Probe=Brain Freeze
```

Run it with the optimized Rust path:

```bash
python3 scripts/ralgorithm.py compare \
  --deck fixtures/decks/my_deck.json \
  --swap-file benchmarks/my_swaps.txt \
  --preset local \
  --workers 4
```

The runner builds the release binary, checks semantic coverage, enables paired stage orders and stochastic Gamble,
uses a shared mulligan policy and common random numbers, reruns capped actual hands at a
higher state limit, bootstraps weighted-score intervals, and reuses validated caches. Use
`--native` for a machine-specific binary compiled with `-C target-cpu=native`.

Policy randomness is derived from the root seed and global game index. Changing process or
internal shard boundaries therefore preserves the exact sampled games while allowing CPU
parallelism to reduce wall time.
Rust full-policy hands use card-identity random priorities: input order and the replaced
card's array slot do not determine where the added card appears. Legacy raw-slot screens
retain their explicitly conditional design and must be confirmed with full-policy games.

## Validate Recorded Games

```bash
python3 validator_ui/server.py --host 127.0.0.1 --port 8765
```

Open `http://127.0.0.1:8765`. Simulator runs intended for manual replay should include
`--include-validation-records --include-cap-replay-records`.

## Architecture

- `fast_engine`: production fixed-library Rust solver, compiled strategic dispatch, packed
  state search, full-library witness validation, and policy simulation.
- `nextgen`: observable information states, explicit chance transitions, packed permanent
  instances, reference search, and multi-fidelity correction infrastructure.
- `scripts`: experiment orchestration, paired statistics, coverage audits, replay tools,
  continuous optimization, and RunPod helpers.
- `validator_ui`: separate browser UI for manually auditing mulligans and lines.
- `fixtures`: deck and action-parity corpora.
- `docs`: architecture, methodology, benchmark generations, and retained experiments.

The fixed-library solver is a high-recall search oracle. Its results depend on modeled
opponents, search caps, mulligan training, and the treatment of post-shuffle draws.
Witness validation checks line legality; it is not proof of an optimal nonanticipating
play policy or of real tournament win probability. See the
[publication audit](docs/publication_audit_20260911.md) for fixes and remaining limitations.
Read [architecture](docs/architecture.md) and
[paired-rate methodology](docs/rhystic_paired_rate_methodology.md) before interpreting
results. Use the [statistical experiment design](docs/statistical_experiment_design.md)
guide to plan sample sizes, candidate-family corrections, and fresh-seed confirmation.

## Performance And Reproducibility

```bash
python3 scripts/benchmark_engine_pass.py \
  --repeats 5 \
  --nextgen-iterations 20000 \
  --out benchmarks/results/local_pass.json

python3 scripts/source_manifest.py --out benchmarks/source_manifest.json
```

Retained experiments should record the source manifest digest, deck digest, root seed,
policy version, objective weights, worker count, and state-cap behavior. Compare performance
on the same machine and power mode. Use the Rust full-sim path and shared caches for bulk
runs; use Python/reference paths for diagnostics and parity checks.

## Development

Run `make check` before committing. Contributions that add opening-relevant cards need a
focused rules test, a positive solver witness, and a negative control. See
[CONTRIBUTING.md](CONTRIBUTING.md).
