# Adding decks and cards

## Deck fixture

The preferred fixture is a small JSON object:

```json
{
  "name": "Example deck",
  "commander": "Commander Name",
  "deck": ["Card 1", "Card 2"],
  "deck_size": 99,
  "sideboard": []
}
```

Partner decks also include `"commanders": ["Commander A", "Commander B"]`, retain the
first commander in `commander` for compatibility, and contain 98 mainboard cards. Card names
must match the names used by the Rust registry.

Use `scripts/ralgorithm.py import-deck` for Moxfield or MTGO-style text exports and
`check-deck` for structural validation.

## Semantic coverage

The simulator does not infer card behavior from Oracle text. Run:

```bash
python3 scripts/ralgorithm.py check-deck fixtures/decks/my_deck.json --semantic
```

Coverage statuses mean:

- `modeled`: opening-objective behavior has a dedicated implementation.
- `partial`: only the explicitly documented portion is modeled.
- `passive_by_design`: the card is deliberately inert for this objective.
- `missing_review`: no trustworthy classification exists; do not use publication results.

An interaction spell can often be passive when the experiment measures only proactive
engine resolution. Lands, mana, tutors, draw, recursion, cost modification, and cards used
as sacrifice, imprint, pitch, convoke, or tap resources generally require semantics even if
they never appear in a winning line by name.

## Adding semantics

1. Add or update the card specification in `rust/rhystic_core/src/nextgen/card_spec.rs`.
2. Implement strategic actions in `rust/rhystic_core/src/fast_engine.rs` using the existing
   generator for the relevant rules family when possible.
3. Add the card to tutor predicates and canonicalization preservation rules where relevant.
4. Update `scripts/rhystic_rust_coverage_audit.py` with an honest support status.
5. Add a focused unit test for costs, colors, zones, timing, and summoning sickness.
6. Add a complete-solver witness that reaches the intended engine or terminal line.
7. Add a negative control proving the card is necessary to that witness.
8. Run action parity, full-library witness, and seeded policy tests before a large experiment.

Random effects must use the seeded simulator stream. Unknown library order must never be
available to mulligan or action decisions. Top tutors may expose the selected card, while
shuffles must erase hidden order. Any deterministic shortcut for a loop needs a documented
proof of sufficiency.

## Swap experiments

Use `CUT=ADD` for one slot and semicolons for grouped swaps:

```text
single: Culling the Weak=Dispel
package: Rain of Filth=Underworld Breach; Gitaxian Probe=Brain Freeze
```

Paired experiments preserve slot positions and random streams across the baseline and each
candidate. This reduces variance but does not make a small sample conclusive. Screen broadly,
then validate finalists with an independent seed and report paired confidence intervals,
discordant counts, cap misses, and weighted-score effects.
