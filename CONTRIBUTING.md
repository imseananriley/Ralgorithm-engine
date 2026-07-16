# Contributing

## Before changing the solver

- State the objective and rules assumptions affected by the change.
- Keep card behavior explicit; do not silently treat an unknown opening-relevant card as a
  generic mana source, tutor, draw spell, or permanent.
- Preserve visible-information decisions. Fixed unknown library order is available only to
  outcome evaluation and witness validation.
- Keep deterministic shortcuts narrow and prove their sufficient conditions in comments or
  tests.

## Verification

```bash
make check
python3 scripts/ralgorithm.py check-deck fixtures/decks/nick_fury_generation32_balanced_optimized.json --semantic
```

Performance changes should include a repeated benchmark on the same machine and a seeded
outcome comparison against the previous implementation. Statistical experiments should use
an independent validation seed after exploratory selection.

## Generated artifacts

`benchmarks/results/`, `.cache/`, local validator notes, build targets, credentials, and
RunPod connection material are not committed. Retain small final tables and methodology
documents when they are needed to reproduce a conclusion.
