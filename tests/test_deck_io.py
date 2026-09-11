from __future__ import annotations

import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from deck_io import parse_text_export, validate_deck_payload  # noqa: E402


class DeckIoTests(unittest.TestCase):
    def test_basic_lands_can_repeat_but_nonbasics_cannot(self) -> None:
        payload = {"commander": "Commander", "deck": ["Forest"] * 99}
        self.assertEqual(validate_deck_payload(payload), [])
        payload["deck"] = ["Tropical Island"] * 99
        self.assertTrue(any("duplicate" in error for error in validate_deck_payload(payload)))

    def test_invalid_unhashable_entries_report_errors_instead_of_crashing(self) -> None:
        payload = {"commanders": [None], "deck": [{"name": "Forest"}, []]}
        errors = validate_deck_payload(payload)
        self.assertTrue(any("every deck entry" in error for error in errors))
        self.assertTrue(any("commanders must" in error for error in errors))

    def test_legacy_secondary_commander_counts_toward_size(self) -> None:
        payload = {"commander": "Rograkh", "secondary_commander": "Thrasios", "deck": ["Forest"] * 98}
        self.assertEqual(validate_deck_payload(payload), [])

    def test_whitespace_cannot_hide_a_singleton_violation(self) -> None:
        payload = {"commander": "Commander", "deck": ["Sol Ring", " Sol Ring"]}
        self.assertTrue(any("duplicate" in error for error in validate_deck_payload(payload)))

    def test_import_removes_partner_commanders_and_preserves_sideboard(self) -> None:
        payload = parse_text_export(
            "\n".join(
                [
                    "1 Rograkh, Son of Rohgahh",
                    "1 Silas Renn, Seeker Adept",
                    *[f"1 Main Card {index}" for index in range(98)],
                    "SIDEBOARD:",
                    "1 Sideboard Card",
                ]
            ),
            name="Partner test",
            commanders=["Rograkh, Son of Rohgahh", "Silas Renn, Seeker Adept"],
        )
        self.assertEqual(len(payload["deck"]), 98)
        self.assertEqual(payload["sideboard"], ["Sideboard Card"])
        self.assertEqual(validate_deck_payload(payload), [])

    def test_validation_reports_size_and_singleton_errors(self) -> None:
        payload = {
            "commander": "Commander",
            "deck": ["Repeated", "Repeated"],
            "sideboard": [],
        }
        errors = validate_deck_payload(payload)
        self.assertTrue(any("expected 99" in error for error in errors))
        self.assertTrue(any("duplicate" in error for error in errors))

    def test_invalid_export_line_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "line 1"):
            parse_text_export("Rhystic Study", name="Bad", commanders=["Commander"])


if __name__ == "__main__":
    unittest.main()
