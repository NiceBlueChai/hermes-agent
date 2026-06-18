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
GITHUB_RELEASE_TAG_RE = re.compile(
    r"^https://github\.com/([^/\\?#\s]+)/([^/\\?#\s]+)/releases/tag/([^/\\?#\s]+)$"
)
VALID_PLATFORMS = frozenset(("windows", "macos", "linux"))
FULL_BOOTSTRAP_FALLBACK_ID = "desktop-bootstrap-script-fallback"
FULL_BOOTSTRAP_RELEASE_PLATFORMS = frozenset(("windows", "macos", "linux"))
FULL_BOOTSTRAP_SIGNATURES = {
    "windows": "authenticode",
    "macos": "developer-id-notarized",
    "linux": "sigstore",
}


def validate_registry(
    registry_path: Path,
    repo_root: Path,
    require_complete: str | None = None,
    require_all_complete: bool = False,
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
    if require_all_complete:
        for entry_id, entry in entries_by_id.items():
            validate_complete_evidence(entry, entry_id, requirements_by_id[entry_id])
    if print_template is not None and print_template not in entries_by_id:
        raise RuntimeError(f"template entry not found: {print_template}")
    return len(entries)


def release_status(registry_path: Path, repo_root: Path) -> dict[str, Any]:
    """Return machine-readable completion status for every fallback entry."""

    payload = load_registry(registry_path)
    if payload.get("schemaVersion") != 1:
        raise RuntimeError("schemaVersion must be 1")

    entries = payload.get("entries")
    if not isinstance(entries, list) or not entries:
        raise RuntimeError("entries must be a non-empty list")

    seen_ids: set[str] = set()
    status_entries: list[dict[str, Any]] = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            raise RuntimeError(f"entry {index} must be an object")
        required = validate_entry(entry, index, repo_root, seen_ids)
        entry_id = entry["id"]
        collected = collected_evidence_checks(entry, required)
        complete_keys = {
            platform: complete_evidence_release_keys(entry, platform, checks)
            for platform, checks in required.items()
        }
        shared_keys = (
            set.intersection(*complete_keys.values())
            if complete_keys and all(complete_keys.values())
            else set()
        )
        platforms = [
            {
                "platform": platform,
                "completeArtifact": bool(complete_keys[platform]),
                "missingChecks": sorted(checks - collected.get(platform, set())),
            }
            for platform, checks in sorted(required.items())
        ]
        status_entries.append(
            {
                "id": entry_id,
                "complete": bool(shared_keys),
                "sharedRelease": bool(shared_keys),
                "platforms": platforms,
            }
        )
    return {
        "allComplete": all(entry["complete"] for entry in status_entries),
        "entries": status_entries,
    }


def evidence_template(registry_path: Path, entry_id: str) -> dict[str, Any]:
    """Return a signed release evidence skeleton for one fallback entry."""

    payload = load_registry(registry_path)
    for entry in payload.get("entries", []):
        if isinstance(entry, dict) and entry.get("id") == entry_id:
            required = validate_required_evidence(entry, entry_id)
            evidence = []
            complete_release_keys_by_platform = {
                platform: complete_evidence_release_keys(entry, platform, required_checks)
                for platform, required_checks in required.items()
            }
            all_platforms_have_complete_artifact = all(complete_release_keys_by_platform.values())
            shared_release_keys = (
                set.intersection(*complete_release_keys_by_platform.values())
                if all_platforms_have_complete_artifact
                else set()
            )
            for platform, required_checks in required.items():
                complete_release_keys = complete_release_keys_by_platform[platform]
                if complete_release_keys and (
                    shared_release_keys or not all_platforms_have_complete_artifact
                ):
                    continue
                template = {
                    "platform": platform,
                    "release": "vX.Y.Z",
                    "url": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "commit": "<40-character-git-sha>",
                    "signed": True,
                    "checks": sorted(required_checks),
                }
                if entry_id == FULL_BOOTSTRAP_FALLBACK_ID:
                    template["signature"] = FULL_BOOTSTRAP_SIGNATURES[platform]
                evidence.append(template)
            return {"entryId": entry_id, "evidence": evidence}
    raise RuntimeError(f"template entry not found: {entry_id}")


def add_signed_evidence(
    registry_path: Path,
    repo_root: Path,
    entry_id: str,
    platform: str,
    release: str,
    url: str,
    release_notes: str | None,
    signature: str | None,
    commit: str,
    checks: list[str],
    all_required_checks: bool,
) -> None:
    """Record one signed release evidence item in the fallback registry."""

    payload, target, required, new_item = build_signed_evidence_item(
        registry_path,
        repo_root,
        entry_id,
        platform,
        release,
        url,
        release_notes,
        signature,
        commit,
        checks,
        all_required_checks,
    )

    merge_signed_evidence_item(target, entry_id, required, new_item)
    registry_path.write_text(json.dumps(payload, indent=4) + "\n", encoding="utf-8")


def add_evidence_json(registry_path: Path, repo_root: Path, evidence_path: Path) -> str:
    """Record signed release evidence from a workflow-generated JSON artifact."""

    validate_registry(registry_path, repo_root)
    payload = load_registry(registry_path)
    entries = payload.get("entries")
    if not isinstance(entries, list):
        raise RuntimeError("entries must be a list")

    try:
        imported = json.loads(evidence_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"evidence JSON not found: {evidence_path}") from exc
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"evidence JSON is not valid JSON: {exc}") from exc
    if not isinstance(imported, dict):
        raise RuntimeError("evidence JSON root must be an object")

    entry_id = imported.get("entryId")
    if not isinstance(entry_id, str) or not entry_id.strip():
        raise RuntimeError("evidence JSON field entryId must be a non-empty string")
    evidence_items = imported.get("evidence")
    if not isinstance(evidence_items, list) or not evidence_items:
        raise RuntimeError(f"entry {entry_id} evidence JSON field evidence must be a non-empty list")

    target = next(
        (entry for entry in entries if isinstance(entry, dict) and entry.get("id") == entry_id),
        None,
    )
    if target is None:
        raise RuntimeError(f"evidence entry not found: {entry_id}")

    required = validate_required_evidence(target, entry_id)
    for item in evidence_items:
        if not isinstance(item, dict):
            raise RuntimeError(f"entry {entry_id} evidence JSON items must be objects")
        merge_signed_evidence_item(target, entry_id, required, dict(item))

    validate_evidence(target, entry_id, required)
    registry_path.write_text(json.dumps(payload, indent=4) + "\n", encoding="utf-8")
    return entry_id


def merge_signed_evidence_item(
    target: dict[str, Any],
    entry_id: str,
    required: dict[str, set[str]],
    new_item: dict[str, Any],
) -> None:
    """Merge one validated signed evidence item into a registry entry."""

    validate_evidence({**target, "evidence": [new_item]}, entry_id, required)
    evidence = target.get("evidence")
    if not isinstance(evidence, list):
        raise RuntimeError(f"entry {entry_id} evidence must be a list")

    matching = find_matching_evidence(evidence, new_item)
    if matching is None:
        evidence.append(new_item)
    else:
        if "releaseNotes" in new_item:
            existing_release_notes = matching.get("releaseNotes")
            if (
                isinstance(existing_release_notes, str)
                and existing_release_notes != new_item["releaseNotes"]
            ):
                raise RuntimeError(
                    f"entry {entry_id} has conflicting releaseNotes for release artifact: {release}"
                )
            matching["releaseNotes"] = new_item["releaseNotes"]
        matching["checks"] = merge_checks(matching.get("checks"), new_item["checks"])


def build_signed_evidence_item(
    registry_path: Path,
    repo_root: Path,
    entry_id: str,
    platform: str,
    release: str,
    url: str,
    release_notes: str | None,
    signature: str | None,
    commit: str,
    checks: list[str],
    all_required_checks: bool,
) -> tuple[dict[str, Any], dict[str, Any], dict[str, set[str]], dict[str, Any]]:
    """Build and validate one signed release evidence item without mutating the registry file."""

    validate_registry(registry_path, repo_root)
    payload = load_registry(registry_path)
    entries = payload.get("entries")
    if not isinstance(entries, list):
        raise RuntimeError("entries must be a list")

    target = next(
        (entry for entry in entries if isinstance(entry, dict) and entry.get("id") == entry_id),
        None,
    )
    if target is None:
        raise RuntimeError(f"evidence entry not found: {entry_id}")

    required = validate_required_evidence(target, entry_id)
    resolved_checks = resolve_evidence_checks(required, platform, checks, all_required_checks)
    if "release-notes" in resolved_checks and not release_notes:
        raise RuntimeError("--add-evidence requires --release-notes when recording release-notes")
    new_item = {
        "platform": platform,
        "release": release,
        "url": url,
        "commit": commit,
        "signed": True,
        "checks": resolved_checks,
    }
    if release_notes is not None:
        new_item["releaseNotes"] = release_notes
    if signature is not None:
        new_item["signature"] = signature
    validate_evidence({**target, "evidence": [new_item]}, entry_id, required)
    return payload, target, required, new_item


def resolve_evidence_checks(
    required_evidence: dict[str, set[str]],
    platform: str,
    checks: list[str],
    all_required_checks: bool,
) -> list[str]:
    """Resolve explicit and platform-wide evidence checks into one ordered list."""

    resolved: list[str] = []
    if all_required_checks:
        resolved.extend(sorted(required_evidence.get(platform, set())))
    resolved.extend(checks)
    return merge_checks([], resolved)


def find_matching_evidence(
    evidence: list[Any],
    new_item: dict[str, Any],
) -> dict[str, Any] | None:
    """Find an existing evidence item for the same signed release artifact."""

    for item in evidence:
        if not isinstance(item, dict):
            continue
        if (
            item.get("platform") == new_item["platform"]
            and item.get("release") == new_item["release"]
            and item.get("url") == new_item["url"]
            and item.get("commit") == new_item["commit"]
            and item.get("signed") is True
        ):
            return item
    return None


def merge_checks(existing: Any, added: list[str]) -> list[str]:
    """Merge evidence checks while preserving first-seen order."""

    if not isinstance(existing, list):
        raise RuntimeError("matching evidence checks must be a list")
    merged: list[str] = []
    seen: set[str] = set()
    for check in [*existing, *added]:
        if not isinstance(check, str):
            raise RuntimeError("matching evidence checks must be strings")
        if check not in seen:
            seen.add(check)
            merged.append(check)
    return merged


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
    if entry_id == FULL_BOOTSTRAP_FALLBACK_ID:
        missing_platforms = sorted(FULL_BOOTSTRAP_RELEASE_PLATFORMS - set(required))
        if missing_platforms:
            missing = ", ".join(missing_platforms)
            raise RuntimeError(f"entry {entry_id} missing requiredEvidence platform: {missing}")
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

    release_notes_by_artifact: dict[tuple[str, str, str, str], str] = {}
    for index, item in enumerate(evidence):
        if not isinstance(item, dict):
            raise RuntimeError(f"entry {entry_id} evidence {index} must be an object")
        platform = require_nested_string(item, "platform", entry_id, "evidence")
        if platform not in required_evidence:
            raise RuntimeError(f"entry {entry_id} has undeclared evidence platform: {platform}")
        release = require_nested_string(item, "release", entry_id, "evidence")
        if release == "vX.Y.Z":
            raise RuntimeError(f"entry {entry_id} evidence release must replace vX.Y.Z placeholder")
        url = require_nested_string(item, "url", entry_id, "evidence")
        if not url.startswith("https://"):
            raise RuntimeError(f"entry {entry_id} evidence url must be HTTPS: {url}")
        if release not in url:
            raise RuntimeError(f"entry {entry_id} evidence url must include release tag: {release}")
        if not is_github_release_tag_url(url, release):
            raise RuntimeError(
                f"entry {entry_id} evidence url must be a GitHub release tag URL: {release}"
            )
        if github_release_repo(url) == "OWNER/REPO":
            raise RuntimeError(f"entry {entry_id} evidence url must replace OWNER/REPO placeholder")
        commit = item.get("commit")
        if not isinstance(commit, str) or not COMMIT_RE.match(commit):
            raise RuntimeError(f"entry {entry_id} evidence commit must be a 40-character git SHA")
        if item.get("signed") is not True:
            raise RuntimeError(f"entry {entry_id} evidence signed must be true")
        signature = item.get("signature")
        if signature is not None:
            expected_signature = FULL_BOOTSTRAP_SIGNATURES[platform]
            if signature != expected_signature:
                raise RuntimeError(
                    f"entry {entry_id} evidence signature must be {expected_signature}"
                )
        if entry_id == FULL_BOOTSTRAP_FALLBACK_ID:
            expected_signature = FULL_BOOTSTRAP_SIGNATURES[platform]
            if signature != expected_signature:
                raise RuntimeError(
                    f"entry {entry_id} evidence signature must be {expected_signature}"
                )
        checks = require_string_list(item, "checks", entry_id, "evidence")
        release_notes = item.get("releaseNotes")
        if release_notes is not None:
            if not isinstance(release_notes, str) or not release_notes.startswith("https://"):
                raise RuntimeError(f"entry {entry_id} evidence releaseNotes must be HTTPS")
            if release not in release_notes:
                raise RuntimeError(
                    f"entry {entry_id} evidence releaseNotes must include release tag: {release}"
                )
            if not is_github_release_tag_url(release_notes, release):
                raise RuntimeError(
                    f"entry {entry_id} evidence releaseNotes must be a GitHub release tag URL: {release}"
                )
            if github_release_repo(release_notes) != github_release_repo(url):
                raise RuntimeError(
                    f"entry {entry_id} evidence releaseNotes must reference the same GitHub repository"
                )
            if github_release_repo(release_notes) == "OWNER/REPO":
                raise RuntimeError(
                    f"entry {entry_id} evidence releaseNotes must replace OWNER/REPO placeholder"
                )
            artifact = (platform, release, url, commit)
            existing_release_notes = release_notes_by_artifact.setdefault(artifact, release_notes)
            if existing_release_notes != release_notes:
                raise RuntimeError(
                    f"entry {entry_id} has conflicting releaseNotes for release artifact: {release}"
                )
        if "release-notes" in checks and release_notes is None:
            raise RuntimeError(f"entry {entry_id} evidence releaseNotes must be HTTPS")
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

    complete_release_keys_by_platform: list[set[tuple[str, str, str]]] = []
    for platform, required_checks in required_evidence.items():
        complete_release_keys = complete_evidence_release_keys(entry, platform, required_checks)
        if not complete_release_keys:
            missing = sorted(required_checks)
            checks = ", ".join(missing)
            raise RuntimeError(
                f"entry {entry_id} missing complete evidence for {platform}: {checks}"
            )
        complete_release_keys_by_platform.append(complete_release_keys)

    shared_release_keys = set.intersection(*complete_release_keys_by_platform)
    if not shared_release_keys:
        raise RuntimeError(
            f"entry {entry_id} missing shared complete signed release evidence"
        )


def platform_has_complete_evidence_artifact(
    entry: dict[str, Any],
    platform: str,
    required_checks: set[str],
) -> bool:
    """Return whether one signed release artifact covers all checks for a platform."""

    return bool(complete_evidence_release_keys(entry, platform, required_checks))


def complete_evidence_release_keys(
    entry: dict[str, Any],
    platform: str,
    required_checks: set[str],
) -> set[tuple[str, str, str]]:
    """Return repository, release, and commit keys for complete signed artifacts."""

    checks_by_artifact: dict[tuple[str, str, str], set[str]] = {}
    for item in entry.get("evidence", []):
        if not isinstance(item, dict) or item.get("platform") != platform:
            continue
        if item.get("signed") is not True:
            continue
        release = item.get("release")
        url = item.get("url")
        commit = item.get("commit")
        checks = item.get("checks")
        release_notes = item.get("releaseNotes")
        if (
            not isinstance(release, str)
            or not release.strip()
            or release == "vX.Y.Z"
            or not isinstance(url, str)
            or not url.startswith("https://")
            or release not in url
            or not is_github_release_tag_url(url, release)
            or github_release_repo(url) == "OWNER/REPO"
            or not isinstance(commit, str)
            or not COMMIT_RE.match(commit)
            or not isinstance(checks, list)
            or not full_bootstrap_signature_matches_platform(entry, item, platform)
        ):
            continue
        if "release-notes" in checks and (
            not isinstance(release_notes, str) or not release_notes.startswith("https://")
        ):
            continue
        if "release-notes" in checks and release not in release_notes:
            continue
        if "release-notes" in checks and not is_github_release_tag_url(release_notes, release):
            continue
        if "release-notes" in checks and github_release_repo(release_notes) != github_release_repo(url):
            continue
        if "release-notes" in checks and github_release_repo(release_notes) == "OWNER/REPO":
            continue
        artifact = (release, url, commit)
        checks_by_artifact.setdefault(artifact, set()).update(
            check for check in checks if isinstance(check, str)
        )
    return {
        (github_release_repo(url), release, commit)
        for (release, url, commit), checks in checks_by_artifact.items()
        if required_checks.issubset(checks)
    }


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


def full_bootstrap_signature_matches_platform(
    entry: dict[str, Any],
    item: dict[str, Any],
    platform: str,
) -> bool:
    """Return whether full-bootstrap evidence carries the required signature type."""

    if entry.get("id") != FULL_BOOTSTRAP_FALLBACK_ID:
        return True
    return item.get("signature") == FULL_BOOTSTRAP_SIGNATURES.get(platform)


def is_github_release_tag_url(url: str, release: str) -> bool:
    """Return whether a URL points at the exact GitHub release tag."""

    match = GITHUB_RELEASE_TAG_RE.match(url)
    return bool(match and match.group(3) == release)


def github_release_repo(url: str) -> str | None:
    """Return the owner/repo pair for a GitHub release URL."""

    match = GITHUB_RELEASE_TAG_RE.match(url)
    if match is None:
        return None
    parts = url.split("/")
    return "/".join(parts[3:5])


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
        "--require-all-complete",
        action="store_true",
        help="Require complete signed evidence for every fallback entry.",
    )
    parser.add_argument(
        "--print-template",
        metavar="ENTRY_ID",
        default=None,
        help="Print a JSON skeleton for missing signed evidence on one fallback entry.",
    )
    parser.add_argument(
        "--print-status",
        action="store_true",
        help="Print JSON completion status for every fallback entry.",
    )
    parser.add_argument(
        "--add-evidence",
        metavar="ENTRY_ID",
        default=None,
        help="Append or merge signed release evidence for one fallback entry.",
    )
    parser.add_argument(
        "--print-evidence-item",
        metavar="ENTRY_ID",
        default=None,
        help="Print one validated signed release evidence item without mutating the registry.",
    )
    parser.add_argument(
        "--add-evidence-json",
        metavar="PATH",
        type=Path,
        action="append",
        default=None,
        help="Append or merge signed release evidence from a workflow-generated JSON file. Repeat for multiple files.",
    )
    parser.add_argument("--platform", choices=sorted(VALID_PLATFORMS), default=None)
    parser.add_argument("--release", default=None)
    parser.add_argument("--url", default=None)
    parser.add_argument(
        "--release-notes",
        default=None,
        help="HTTPS URL for the published release notes when recording the release-notes check.",
    )
    parser.add_argument(
        "--signature",
        default=None,
        help="Platform signing proof type for full-bootstrap release evidence.",
    )
    parser.add_argument("--commit", default=None)
    parser.add_argument(
        "--check",
        action="append",
        dest="checks",
        default=[],
        help="Evidence check to record. Repeat for multiple checks.",
    )
    parser.add_argument(
        "--all-required-checks",
        action="store_true",
        help="Record every required evidence check for the selected platform.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the fallback burn-down registry validator."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.add_evidence_json and (
            args.add_evidence
            or args.print_evidence_item
            or args.require_complete
            or args.require_all_complete
            or args.print_template
            or args.print_status
        ):
            raise RuntimeError("--add-evidence-json cannot be combined with other actions")
        if args.print_status and (
            args.add_evidence
            or args.print_evidence_item
            or args.require_complete
            or args.require_all_complete
            or args.print_template
        ):
            raise RuntimeError("--print-status cannot be combined with other actions")
        if args.add_evidence and args.print_evidence_item:
            raise RuntimeError("--add-evidence cannot be combined with --print-evidence-item")
        if args.add_evidence_json:
            for evidence_path in args.add_evidence_json:
                entry_id = add_evidence_json(args.registry, args.repo_root, evidence_path)
                print(f"added evidence JSON: {entry_id}")
            return 0
        if args.add_evidence or args.print_evidence_item:
            command_name = "--add-evidence" if args.add_evidence else "--print-evidence-item"
            entry_id = args.add_evidence or args.print_evidence_item
            missing = [
                name
                for name, value in (
                    ("--platform", args.platform),
                    ("--release", args.release),
                    ("--url", args.url),
                    ("--commit", args.commit),
                )
                if not value
            ]
            if not args.checks and not args.all_required_checks:
                missing.append("--check or --all-required-checks")
            if entry_id == FULL_BOOTSTRAP_FALLBACK_ID and not args.signature:
                missing.append("--signature")
            if missing:
                raise RuntimeError(f"{command_name} requires {', '.join(missing)}")
            if args.add_evidence:
                add_signed_evidence(
                    args.registry,
                    args.repo_root,
                    entry_id,
                    args.platform,
                    args.release,
                    args.url,
                    args.release_notes,
                    args.signature,
                    args.commit,
                    args.checks,
                    args.all_required_checks,
                )
                print(f"added evidence: {entry_id} {args.platform}")
                return 0
            _payload, _target, _required, new_item = build_signed_evidence_item(
                args.registry,
                args.repo_root,
                entry_id,
                args.platform,
                args.release,
                args.url,
                args.release_notes,
                args.signature,
                args.commit,
                args.checks,
                args.all_required_checks,
            )
            print(json.dumps({"entryId": entry_id, "evidence": [new_item]}, indent=4))
            return 0
        if args.print_status:
            print(json.dumps(release_status(args.registry, args.repo_root), indent=4))
            return 0

        count = validate_registry(
            args.registry,
            args.repo_root,
            args.require_complete,
            args.require_all_complete,
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
    if args.require_all_complete:
        print("complete evidence: all")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
