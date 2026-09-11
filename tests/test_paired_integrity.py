from __future__ import annotations

import argparse
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from rhystic_paired_rate_compare import (
    bootstrap_mean_ci, input_fingerprint, mcnemar_exact_p, normal_mean_ci,
    paired_stats, records_by_game, reusable_result_json,
)
from rhystic_quick_compare import Variant
from ralgorithm import parser, command_compare, require_supported_commanders


def payload(*indices: int) -> dict:
    return {"seed": 42, "evaluation": {"game_records": [
        {"game_index": i, "hit": False} for i in indices
    ]}}


class PairedIntegrityTests(unittest.TestCase):
    def test_unknown_commander_cannot_silently_use_nick_fury_model(self):
        with self.assertRaisesRegex(ValueError, "no audited production model"):
            require_supported_commanders({"commander": "Thrasios, Triton Hero"})
        require_supported_commanders({"commanders": ["Rograkh, Son of Rohgahh", "Thrasios, Triton Hero"]})

    def compare(self, base: dict, candidate: dict):
        return paired_stats(
            base, candidate, variant=Variant("test", "A", "B"), weights={},
            bootstrap_samples=0, bootstrap_seed=1, rate_half_width=0.01,
            score_half_width=0.01,
        )

    def test_duplicate_game_indices_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            records_by_game(payload(0, 0))

    def test_missing_pairs_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "identical nonempty"):
            self.compare(payload(0, 1), payload(0, 2))

    def test_different_seeds_are_not_paired(self):
        candidate = payload(0, 1)
        candidate["seed"] = 43
        with self.assertRaisesRegex(ValueError, "matching root seeds"):
            self.compare(payload(0, 1), candidate)

    def test_matching_pairs_are_counted_once(self):
        stats, rows = self.compare(payload(0, 1), payload(1, 0))
        self.assertEqual(stats.games, 2)
        self.assertEqual(len(rows), 2)

    def test_cache_requires_deck_and_policy_fingerprint(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            deck = root / "deck.json"
            thresholds = root / "policy.json"
            result = root / "result.json"
            deck.write_text(json.dumps({"deck": ["A"]}))
            thresholds.write_text(json.dumps({"thresholds": [0.5]}))
            original = input_fingerprint(deck, thresholds)
            result.write_text(json.dumps({"comparison_inputs": original}))
            with patch("rhystic_paired_rate_compare.result_payload_mismatches", side_effect=lambda *a, **k: []):
                self.assertTrue(reusable_result_json(result, argparse.Namespace(), "test", original))
                deck.write_text(json.dumps({"deck": ["B"]}))
                self.assertFalse(reusable_result_json(result, argparse.Namespace(), "test", input_fingerprint(deck, thresholds)))
                deck.write_text(json.dumps({"deck": ["A"]}))
                thresholds.write_text(json.dumps({"thresholds": [0.6]}))
                self.assertFalse(reusable_result_json(result, argparse.Namespace(), "test", input_fingerprint(deck, thresholds)))
                self.assertFalse(reusable_result_json(result, argparse.Namespace(), "test", input_fingerprint(deck, None)))

    def test_compare_checks_semantics_before_launch(self):
        args = parser().parse_args(["compare", "--no-build", "--seed", "42"])
        with patch("ralgorithm.run") as run:
            command_compare(args)
        commands = [call.args[0] for call in run.call_args_list]
        self.assertEqual(len(commands), 2)
        self.assertIn("rhystic_rust_coverage_audit.py", commands[0][1])
        self.assertIn("--fail-on-unsupported", commands[0])
        self.assertIn("rhystic_paired_rate_compare.py", commands[1][1])

    def test_bootstrap_matches_exact_binomial_quantiles(self):
        # Resampling [-1, 1] twenty times yields mean (2*K-20)/20,
        # K ~ Binomial(20, 1/2), whose 2.5% and 97.5% quantiles are 6 and 14.
        interval = bootstrap_mean_ci([-1.0, 1.0] * 10, samples=20000, seed=137)
        self.assertEqual(interval, (-0.4, 0.4))
        self.assertEqual(interval, bootstrap_mean_ci([1.0, -1.0] * 10, samples=20000, seed=137))

    def test_one_observation_does_not_claim_zero_uncertainty(self):
        self.assertEqual(normal_mean_ci([1.0]), (1.0, None, None))
        self.assertEqual(bootstrap_mean_ci([1.0], samples=100, seed=1), (None, None))
        self.assertEqual(mcnemar_exact_p(0, 0), 1.0)


if __name__ == "__main__":
    unittest.main()
