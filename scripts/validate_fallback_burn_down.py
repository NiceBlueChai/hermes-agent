#!/usr/bin/env python3
"""Validate the release evidence registry for fallback burn-down work.

The Phase 8 rule is evidence-driven: fallback branches can only be deleted
after a release proves the Rust replacement path across supported platforms.
This validator keeps every retained fallback tied to a stable source marker and
an explicit removal gate so the list remains auditable.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REGISTRY = REPO_ROOT / "docs" / "release" / "fallback-burn-down.json"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")


def validate_registry(registry_path: Path, repo_root: Path) -> int:
    """Validate fallback entries and return the number of checked entries."""

    payload = load_registry(registry_path)
    if payload.get("schemaVersion") != 1:
        raise RuntimeError("schemaVersion must be 1")

    entries = payload.get("entries")
    if not isinstance(entries, list) or not entries:
        raise RuntimeError("entries must be a non-empty list")

    seen_ids: set[str] = set()
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise RuntimeError(f"entry {index} must be an object")
        validate_entry(entry, index, repo_root, seen_ids)
    return len(entries)


def load_registry(registry_path: Path) -> dict[str, Any]:
    """Load a JSON fallback registry from disk."""

    try:
        payload = json.loads(registry_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"registry not found: {registry_path}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"registry is not valid JSON: {exc}") from exc
    if not isinstance(payload, dict):
        raise RuntimeError("registry root must be an object")
    return payload


def validate_entry(
    entry: dict[str, Any],
    index: int,
    repo_root: Path,
    seen_ids: set[str],
) -> None:
    """Validate one fallback entry and its source marker."""

    entry_id = require_string(entry, "id", index)
    if not ID_RE.match(entry_id):
        raise RuntimeError(f"entry {index} has invalid id: {entry_id}")
    if entry_id in seen_ids:
        raise RuntimeError(f"duplicate id: {entry_id}")
    seen_ids.add(entry_id)

    for key in ("owner", "fallback", "removalGate"):
        require_string(entry, key, index)

    marker = require_string(entry, "marker", index)
    if entry_id not in marker:
        raise RuntimeError(f"entry {entry_id} marker must include the id")

    evidence = entry.get("evidence")
    if not isinstance(evidence, list):
        raise RuntimeError(f"entry {entry_id} evidence must be a list")

    source_path = resolve_repo_file(repo_root, require_string(entry, "file", index), entry_id)
    source_text = source_path.read_text(encoding="utf-8")
    if marker not in source_text:
        raise RuntimeError(f"entry {entry_id} marker not found in {source_path}")


def require_string(entry: dict[str, Any], key: str, index: int) -> str:
    """Return a required non-empty string field from an entry."""

    value = entry.get(key)
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"entry {index} field {key} must be a non-empty string")
    return value


def resolve_repo_file(repo_root: Path, raw_path: str, entry_id: str) -> Path:
    """Resolve a registry file path and reject paths outside the repository."""

    candidate = Path(raw_path)
    if candidate.is_absolute() or ".." in candidate.parts:
        raise RuntimeError(f"entry {entry_id} has unsafe file path: {raw_path}")

    root = repo_root.resolve()
    resolved = (root / candidate).resolve()
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise RuntimeError(f"entry {entry_id} has unsafe file path: {raw_path}") from exc
    if not resolved.is_file():
        raise RuntimeError(f"entry {entry_id} file not found: {raw_path}")
    return resolved


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command line arguments."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, default=DEFAULT_REGISTRY)
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the fallback burn-down registry validator."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        count = validate_registry(args.registry, args.repo_root)
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    print(f"validated {count} fallback burn-down entries")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
