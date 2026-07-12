# Generation 19: Full-library witness validation

## Implementation

The existential discrepancy solver now records its successful state path,
including whether each transition was deterministic or a selected chance branch
and the branch probability.

Opening replay can receive the complete recorded library order and validate the
witness without adding the full permutation to every packed search state. Each
witness receives one of four statuses:

- `confirmed`: every relevant draw matches recorded order or is forced by a
  known-top effect
- `invalid`: a selected draw conflicts with recorded order
- `probabilistic`: the line consumes unresolved post-shuffle or stochastic
  information
- `unavailable`: no complete library order or witness was provided

An unresolved post-shuffle draw does not make a line probabilistic when the drawn
card remains unused in the final hand. This preserves lines that win independently
of the random card while rejecting lines that require a favorable shuffled draw.

When validation is enabled, only `confirmed` packed witnesses count as binary
hits. Invalid and probabilistic witnesses remain unresolved and enter exact
rescue. Exact rescue automatically enables strict hidden-shuffle handling so it
cannot reintroduce tutor-order lookahead.

## Corpus classification

Using the selected D2 packed profile on 1,000 final keeps:

- Raw packed positive branches: 703
- Full-library confirmed witnesses: 351
- Invalid witnesses: 80
- Probabilistic witnesses: 272

Of the 351 packed confirmations, 338 overlap the old engine's wins and 13 are new
complete witnesses. The new game indices are 174, 213, 344, 360, 436, 493, 518,
638, 697, 702, 743, 894, and 919.

Strict exact rescue evaluates the 649 unconfirmed hands and confirms 271 more
wins. The resulting deterministic lower bound is 622/1,000:

- 609 old-engine wins retained under strict validation
- 13 newly confirmed packed witnesses
- 46 exact-rescue searches still capped

Ten old binary labels are not deterministic under the corrected semantics: games
91, 115, 296, 416, 552, 614, 628, 792, 901, and 989. Seven use Gamble outcomes;
the other three consume a draw after a fetch/search shuffle. These should
contribute integrated probability mass rather than count as binary certainties.

## Performance

Three fresh repetitions recomputed both engines from scratch.

| Metric | Pre-fork | Validated hybrid |
|---|---:|---:|
| Median wall time | 57.727 s | 29.954 s |
| Median CPU time | 55.845 s | 44.176 s |
| Median paired speedup | 1.00x | 1.93x wall / 1.26x CPU |
| Deterministic confirmations | 619 old labels | 622 strict witnesses |

The old count is not semantically comparable to the strict count because it
includes the ten stochastic/lookahead-dependent labels above.

## Interpretation

Full-library witness validation is worthwhile: it removes 352 optimistic packed
positives from binary results while preserving a measurable speed advantage. It
also separates deterministic evidence from probabilistic evidence instead of
forcing both into one success bit.

The next statistical step is to integrate probability for the 272 probabilistic
witnesses and the 46 capped rescue cases. The deterministic 62.2% result is a
lower bound, not the final engine-resolution estimate.
