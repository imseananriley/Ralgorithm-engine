from __future__ import annotations

import hashlib
from pathlib import Path


def engine_source_digest(root: Path) -> str:
    paths = [
        root / "Cargo.lock",
        root / "Cargo.toml",
        root / "rust" / "rhystic_core" / "Cargo.toml",
        root / "scripts" / "rhystic_belief_mulligan_sim.py",
        root / "scripts" / "rhystic_paired_rate_compare.py",
        root / "scripts" / "rhystic_quick_compare.py",
        root / "scripts" / "rhystic_study_calc.py",
        root / "scripts" / "source_digest.py",
        *(root / "rust" / "rhystic_core" / "src").rglob("*.rs"),
    ]
    digest = hashlib.blake2b(digest_size=16)
    for path in sorted(set(paths)):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()
