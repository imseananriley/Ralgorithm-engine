# Generation 35: Underworld Breach package experiment

## Question

Does Underworld Breach, alone or with Brain Freeze, improve the probability of resolving
Rhystic Study or Heartwood Storyteller by turn two in the Generation 32 balanced Nick Fury
list? A deterministic Underworld Breach win also counts as success because it dominates
resolving an engine.

The objective weights are Rhystic T1 1.00, Rhystic T2 0.75, Heartwood T1 0.70, and
Heartwood T2 0.55. A deterministic Breach win receives the Rhystic timing weight.

## Model additions

The Rust engine now models Brain Freeze's colored cost, storm count, and actual ordered
mill of three cards per copy. Underworld Breach grants relevant graveyard cards escape for
their normal mana cost plus exiling three other graveyard cards. Escaped instants and
sorceries return to the graveyard after resolving, escaped permanents enter the battlefield,
and Breach is sacrificed at the beginning of the end step.

The escape closure includes the opening-relevant engines, tutors, rituals, cantrips, Lion's
Eye Diamond, Lotus Petal, Brain Freeze, and creature/tutor intermediates. Tutors can find
Breach or Brain Freeze when their card-type restrictions allow it. Lion's Eye Diamond may
discard the hand and produce a selected color while Breach is active.

The terminal loop detector requires Breach on the battlefield, Brain Freeze and Lion's Eye
Diamond in the graveyard, and at least eight total graveyard cards. From that state the
solver can escape and crack LED, escape Brain Freeze, replace the exiled fodder through
storm mill, net mana, and repeat through the library. A focused complete-solver test finds a
turn-one loop from Command Tower, Lotus Petal, Breach, LED, Brain Freeze, and two blanks.

To bound branching, escape-exile choices use a deterministic preservation order instead of
enumerating every combination of three cards. This preserves Brain Freeze, LED, engines,
tutors, and mana before expendable cards. Opponent interaction is not modeled.

## Experiment

A 500-game paired screen tested ten two-card cut pairs plus Breach-only and Brain
Freeze-only controls. The best raw screen result was only +0.20 percentage points and was
negative on weighted score, with every weighted confidence interval crossing zero.

Four finalists then used a fresh, independent 2,000-game paired validation. All candidates
shared the exact baseline shuffles, stochastic Gamble outcomes, 75% pregame Gemstone
Caverns states, London mulligans through three cards, learned baseline thresholds, and
actual-state cap reruns to 100,000 states. The baseline was reused rather than recomputed.

| Change | Success | Raw delta (paired 95% CI) | Weighted delta (bootstrap 95% CI) | Breach combo wins |
|---|---:|---:|---:|---:|
| Baseline | 72.95% | - | - | 0 |
| Culling -> Breach | 71.70% | -1.25 pp (-1.96, -0.54) | -0.84 pp (-1.34, -0.35) | 0 |
| Rain + Probe -> Breach + Freeze | 71.70% | -1.25 pp (-2.25, -0.25) | -1.18 pp (-1.91, -0.42) | 5 |
| Culling + Probe -> Breach + Freeze | 71.05% | -1.90 pp (-2.90, -0.90) | -1.48 pp (-2.19, -0.80) | 9 |
| Culling + Rain -> Breach + Freeze | 70.15% | -2.80 pp (-3.84, -1.76) | -1.95 pp (-2.69, -1.22) | 7 |

Cap misses were nearly balanced: six for baseline and four to seven per candidate. They are
too few to explain the observed differences.

The least damaging package, Rain + Probe -> Breach + Freeze, gained 22 Heartwood
resolutions but lost 52 Rhystic resolutions relative to baseline, then added five deterministic
combo wins. Breach alone lost eight Heartwood and seventeen Rhystic resolutions. The
package therefore does create genuine recursive and terminal lines, but not often enough to
replace the standalone mana and velocity removed from this list.

## Recommendation

Do not add Underworld Breach or the Breach/Brain Freeze package for the turn-one/turn-two
Rhystic/Heartwood objective. Every independently validated configuration is statistically
worse in both raw success and mildly weighted value.

Breach may still be desirable for post-engine resiliency or as a conventional win condition,
but that is a different objective and should be tested with an explicit late-game or overall
win-rate model rather than credited indirectly in this opening-engine experiment.

Artifacts:

- `benchmarks/results/generation35_breach_package_screen_500/paired_summary.csv`
- `benchmarks/results/generation35_breach_package_finalists_shared_2000/paired_summary.csv`
- `benchmarks/results/generation35_breach_package_finalists_shared_2000/generation35_breach_package_combined.csv`
