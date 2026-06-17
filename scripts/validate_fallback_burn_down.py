#!/usr/bin/env python3
"""Validate the release evidence registry for fallback burn-down work.

The Phase 8 rule is evidence-driven: fallback branches can only be deleted
after a signed release proves the Rust replacement path across supported
platforms. This validator keeps every retained fallback tied to a stable source
marker and an explicit removal gate so the list remains auditable.
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
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
VALID_PLATFORMS = frozenset(("windows", "macos", "linux"))


def validate_registry(
    registry_path: Path,
    repo_root: Path,
    require_complete: str | None = None,
    print_template: str | None = None,
) -> int:
    """Validate fallback entries and return the number of checked entries."""

    payload = load_registry(registry_path)
    if payload.get("schemaVersion") != 1:
        raise RuntimeError("schemaVersion must be 1")

    entries = payload.get("entries")
    if not isinstance(entries, list) or not entries:
        raise RuntimeError("entries must be a non-empty list")

    seen_ids: set[str] = set()
    entries_by_id: dict[str, dict[str, Any]] = {}
    requirements_by_id: dict[str, dict[str, set[str]]] = {}
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise RuntimeError(f"entry {index} must be an object")
        required = validate_entry(entry, index, repo_root, seen_ids)
        entry_id = entry["id"]
        entries_by_id[entry_id] = entry
        requirements_by_id[entry_id] = required
    if require_complete is not None:
        entry = entries_by_id.get(require_complete)
        if entry is None:
            raise RuntimeError(f"required complete entry not found: {require_complete}")
        validate_complete_evidence(entry, require_complete, requirements_by_id[require_complete])
    if print_template is not None and print_template not in entries_by_id:
        raise RuntimeError(f"template entry not found: {print_template}")
    return len(entries)


def evidence_template(registry_path: Path, entry_id: str) -> dict[str, Any]:
    """Return a signed release evidence skeleton for one fallback entry."""
    payload = load_registry(registry_path)
    for entry in payload.get("entries", []):
        if isinstance(entry, dict) and entry.get("id") == entry_id:
            required = validate_required_evidence(entry, entry_id)
            checks_by_platform = collected_evidence_checks(entry, required)
            evidence = []
            for platform, required_checks in required.items():
                missing = sorted(required_checks - checks_by_platform.get(platform, set()))
                if not missing:
                    continue
                evidence.append(
                    {
                        "platform": platform,
                        "release": "vX.Y.Z",
                        "url": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                        "commit": "<40-character-git-sha>",
                        "signed": True,
                        "checks": missing,
                    }
                )
            return {"entryId": entry_id, "evidence": evidence}
    raise RuntimeError(f"template entry not found: {entry_id}")


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
) -> dict[str, set[str]]:
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

    required_evidence = validate_required_evidence(entry, entry_id)
    validate_evidence(entry, entry_id, required_evidence)

    source_path = resolve_repo_file(repo_root, require_string(entry, "file", index), entry_id)
    source_text = source_path.read_text(encoding="utf-8")
    if marker not in source_text:
        raise RuntimeError(f"entry {entry_id} marker not found in {source_path}")
    return required_evidence


def validate_required_evidence(entry: dict[str, Any], entry_id: str) -> dict[str, set[str]]:
    """Validate required release evidence and return checks by platform."""

    requirements = entry.get("requiredEvidence")
    if not isinstance(requirements, list) or not requirements:
        raise RuntimeError(f"entry {entry_id} requiredEvidence must be a non-empty list")

    required: dict[str, set[str]] = {}
    for index, requirement in enumerate(requirements):
        if not isinstance(requirement, dict):
            raise RuntimeError(f"entry {entry_id} requiredEvidence {index} must be an object")
        platform = require_nested_string(requirement, "platform", entry_id, "requiredEvidence")
        if platform not in VALID_PLATFORMS:
            raise RuntimeError(f"entry {entry_id} requiredEvidence has invalid platform: {platform}")
        if platform in required:
            raise RuntimeError(f"entry {entry_id} has duplicate requiredEvidence platform: {platform}")
        checks = require_string_list(requirement, "checks", entry_id, "requiredEvidence")
        required[platform] = set(checks)
    return required


def validate_evidence(
    entry: dict[str, Any],
    entry_id: str,
    required_evidence: dict[str, set[str]],
) -> None:
    """Validate recorded release evidence against the declared removal gate."""

    evidence = entry.get("evidence")
    if not isinstance(evidence, list):
        raise RuntimeError(f"entry {entry_id} evidence must be a list")

    for index, item in enumerate(evidence):
        if not isinstance(item, dict):
            raise RuntimeError(f"entry {entry_id} evidence {index} must be an object")
        platform = require_nested_string(item, "platform", entry_id, "evidence")
        if platform not in required_evidence:
            raise RuntimeError(f"entry {entry_id} has undeclared evidence platform: {platform}")
        require_nested_string(item, "release", entry_id, "evidence")
        url = require_nested_string(item, "url", entry_id, "evidence")
        if not url.startswith("https://"):
            raise RuntimeError(f"entry {entry_id} evidence url must be HTTPS: {url}")
        commit = item.get("commit")
        if not isinstance(commit, str) or not COMMIT_RE.match(commit):
            raise RuntimeError(f"entry {entry_id} evidence commit must be a 40-character git SHA")
        if item.get("signed") is not True:
            raise RuntimeError(f"entry {entry_id} evidence signed must be true")
        checks = require_string_list(item, "checks", entry_id, "evidence")
        allowed_checks = required_evidence[platform]
        for check in checks:
            if check not in allowed_checks:
                raise RuntimeError(f"entry {entry_id} has undeclared evidence check: {check}")


def validate_complete_evidence(
    entry: dict[str, Any],
    entry_id: str,
    required_evidence: dict[str, set[str]],
) -> None:
    """Validate that one fallback entry has complete signed evidence for every required check."""
    checks_by_platform = collected_evidence_checks(entry, required_evidence)

    for platform, required_checks in required_evidence.items():
        evidence_checks = checks_by_platform.get(platform, set())
        missing = sorted(required_checks - evidence_checks)
        if missing:
            checks = ", ".join(missing)
            raise RuntimeError(
                f"entry {entry_id} missing complete evidence for {platform}: {checks}"
            )


def collected_evidence_checks(
    entry: dict[str, Any],
    required_evidence: dict[str, set[str]],
) -> dict[str, set[str]]:
    """Collect already-recorded signed checks by platform."""
    checks_by_platform: dict[str, set[str]] = {platform: set() for platform in required_evidence}
    for item in entry.get("evidence", []):
        if not isinstance(item, dict):
            continue
        platform = item.get("platform")
        checks = item.get("checks")
        if item.get("signed") is True and platform in checks_by_platform and isinstance(checks, list):
            checks_by_platform[platform].update(
                check for check in checks if isinstance(check, str)
            )
    return checks_by_platform


def require_string(entry: dict[str, Any], key: str, index: int) -> str:
    """Return a required non-empty string field from an entry."""

    value = entry.get(key)
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"entry {index} field {key} must be a non-empty string")
    return value


def require_nested_string(entry: dict[str, Any], key: str, entry_id: str, section: str) -> str:
    """Return a required non-empty string from a nested registry object."""

    value = entry.get(key)
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"entry {entry_id} {section} field {key} must be a non-empty string")
    return value


def require_string_list(entry: dict[str, Any], key: str, entry_id: str, section: str) -> list[str]:
    """Return a required non-empty string list from a nested registry object."""

    values = entry.get(key)
    if not isinstance(values, list) or not values:
        raise RuntimeError(f"entry {entry_id} {section} field {key} must be a non-empty list")

    strings: list[str] = []
    seen: set[str] = set()
    for value in values:
        if not isinstance(value, str) or not value.strip():
            raise RuntimeError(f"entry {entry_id} {section} field {key} has invalid value")
        if not ID_RE.match(value):
            raise RuntimeError(f"entry {entry_id} {section} field {key} has invalid id: {value}")
        if value in seen:
            raise RuntimeError(f"entry {entry_id} {section} field {key} has duplicate value: {value}")
        seen.add(value)
        strings.append(value)
    return strings


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
    parser.add_argument(
        "--require-complete",
        metavar="ENTRY_ID",
        default=None,
        help="Require complete signed evidence for one fallback entry.",
    )
    parser.add_argument(
        "--print-template",
        metavar="ENTRY_ID",
        default=None,
        help="Print a JSON skeleton for missing signed evidence on one fallback entry.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the fallback burn-down registry validator."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        count = validate_registry(
            args.registry,
            args.repo_root,
            args.require_complete,
            args.print_template,
        )
        if args.print_template:
            print(json.dumps(evidence_template(args.registry, args.print_template), indent=4))
            return 0
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    print(f"validated {count} fallback burn-down entries")
    if args.require_complete:
        print(f"complete evidence: {args.require_complete}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
