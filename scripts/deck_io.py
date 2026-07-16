#!/usr/bin/env python3
from __future__ import annotations

import json
import re
from collections import Counter
from pathlib import Path
from typing import Any


CARD_LINE = re.compile(r"^\s*(\d+)\s+(.+?)\s*$")


def commander_names(payload: dict[str, Any]) -> list[str]:
    commanders = payload.get("commanders")
    if isinstance(commanders, list) and commanders:
        return [str(name).strip() for name in commanders if str(name).strip()]
    commander = payload.get("commander")
    if isinstance(commander, str) and commander.strip():
        return [commander.strip()]
    return []


def validate_deck_payload(payload: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    deck = payload.get("deck")
    if not isinstance(deck, list):
        return ["deck must be a JSON array of card names"]
    if any(not isinstance(card, str) or not card.strip() for card in deck):
        errors.append("every deck entry must be a non-empty card name")

    commanders = commander_names(payload)
    if not commanders:
        errors.append("commander or commanders must identify at least one commander")
    if len(commanders) > 2:
        errors.append("at most two commanders are supported")
    if len(set(commanders)) != len(commanders):
        errors.append("commander names must be unique")

    expected_size = 100 - len(commanders) if commanders else None
    if expected_size is not None and len(deck) != expected_size:
        errors.append(
            f"mainboard has {len(deck)} cards; expected {expected_size} with "
            f"{len(commanders)} commander(s)"
        )

    duplicates = sorted(card for card, count in Counter(deck).items() if count > 1)
    if duplicates:
        errors.append(f"duplicate mainboard cards: {', '.join(duplicates)}")
    overlap = sorted(set(deck) & set(commanders))
    if overlap:
        errors.append(f"commanders also appear in the mainboard: {', '.join(overlap)}")

    sideboard = payload.get("sideboard", [])
    if not isinstance(sideboard, list) or any(not isinstance(card, str) for card in sideboard):
        errors.append("sideboard must be a JSON array of card names")
    recorded_size = payload.get("deck_size")
    if recorded_size is not None and recorded_size != len(deck):
        errors.append(f"deck_size is {recorded_size}, but deck contains {len(deck)} cards")
    return errors


def load_deck(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text())
    if not isinstance(payload, dict):
        raise ValueError("deck JSON must contain an object")
    return payload


def _parse_card_line(line: str, line_number: int) -> list[str]:
    match = CARD_LINE.match(line)
    if not match:
        raise ValueError(f"line {line_number}: expected '<quantity> <card name>'")
    quantity = int(match.group(1))
    name = match.group(2).strip()
    if quantity <= 0 or not name:
        raise ValueError(f"line {line_number}: invalid card quantity or name")
    return [name] * quantity


def parse_text_export(
    text: str,
    *,
    name: str,
    commanders: list[str],
) -> dict[str, Any]:
    mainboard: list[str] = []
    sideboard: list[str] = []
    section = "mainboard"
    for line_number, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        normalized = line.rstrip(":").strip().lower()
        if normalized in {"deck", "mainboard"}:
            section = "mainboard"
            continue
        if normalized in {"sideboard", "considering"}:
            section = "sideboard"
            continue
        cards = _parse_card_line(line, line_number)
        if section == "sideboard":
            sideboard.extend(cards)
        else:
            mainboard.extend(cards)

    normalized_commanders = [commander.strip() for commander in commanders if commander.strip()]
    for commander in normalized_commanders:
        if commander in mainboard:
            mainboard.remove(commander)
        if commander in sideboard:
            sideboard.remove(commander)

    payload: dict[str, Any] = {
        "name": name,
        "commander": normalized_commanders[0] if normalized_commanders else "",
        "deck": mainboard,
        "deck_size": len(mainboard),
        "sideboard": sideboard,
    }
    if len(normalized_commanders) > 1:
        payload["commanders"] = normalized_commanders
    return payload


def write_deck(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
