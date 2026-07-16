#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXCLUDED_PARTS = {".git", "target", "__pycache__", "artifacts", "benchmarks", "data", "results"}


def tracked_source_files() -> list[Path]:
    completed = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        check=True,
    )
    files: list[Path] = []
    for raw in completed.stdout.split(b"\0"):
        if not raw:
            continue
        relative = Path(raw.decode())
        path = ROOT / relative
        if not path.is_file() or any(part in EXCLUDED_PARTS for part in relative.parts):
            continue
        if path.suffix in {".pyc", ".prof"}:
            continue
        files.append(path)
    return sorted(files, key=lambda item: item.relative_to(ROOT).as_posix())


def digest_file(path: Path) -> str:
    return hashlib.blake2b(path.read_bytes(), digest_size=20).hexdigest()


def git_value(*args: str) -> str | None:
    completed = subprocess.run(
        ["git", *args],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return completed.stdout.strip() if completed.returncode == 0 else None


def main() -> int:
    parser = argparse.ArgumentParser(description="Write a reproducible simulator source manifest.")
    parser.add_argument("--out", default="benchmarks/source_manifest.json")
    args = parser.parse_args()

    entries = [
        {"path": path.relative_to(ROOT).as_posix(), "blake2b_160": digest_file(path), "bytes": path.stat().st_size}
        for path in tracked_source_files()
    ]
    aggregate = hashlib.blake2b(digest_size=24)
    for entry in entries:
        aggregate.update(entry["path"].encode())
        aggregate.update(b"\0")
        aggregate.update(entry["blake2b_160"].encode())
        aggregate.update(b"\n")

    payload = {
        "schema": 1,
        "source_digest": aggregate.hexdigest(),
        "git_commit": git_value("rev-parse", "HEAD"),
        "git_status_porcelain": git_value("status", "--porcelain"),
        "files": entries,
    }
    out = Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    print(out)
    print(payload["source_digest"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
