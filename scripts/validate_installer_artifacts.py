#!/usr/bin/env python3
"""Validate installer release artifacts before workflow upload.

The build workflows already use `actions/upload-artifact` with
`if-no-files-found: error`, but this helper gives the release job a local,
testable gate that also validates the retained bootstrap-tools manifest.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from prepare_bootstrap_tools import MANIFEST_NAME, sha256_file, validate_manifest
from prepare_python_wheelhouse import (
    MANIFEST_NAME as WHEELHOUSE_MANIFEST_NAME,
    validate_payload as validate_wheelhouse_payload,
)


REPO_ROOT = Path(__file__).resolve().parents[1]
ALLOWED_BOOTSTRAP_TOOLS_METADATA = {".gitignore", "README.md"}


def validate_bootstrap_tools_payload(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate that the bootstrap-tools directory contains only manifest-owned payloads."""

    archive_count = validate_manifest(output_dir, expected_platform, expected_arch)
    manifest_path = output_dir / MANIFEST_NAME
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    expected = {MANIFEST_NAME, *ALLOWED_BOOTSTRAP_TOOLS_METADATA}
    expected.update(archive["name"] for archive in payload["archives"])

    for entry in output_dir.iterdir():
        if entry.name not in expected:
            raise RuntimeError(f"unmanifested bootstrap tool payload: {entry.name}")
        if not entry.is_file():
            raise RuntimeError(f"bootstrap tool payload is not a file: {entry.name}")
    if expected_platform is not None:
        for archive in payload["archives"]:
            platform = archive.get("platform")
            if platform != expected_platform:
                raise RuntimeError(
                    f"unexpected bootstrap tools platform for {archive['name']}: "
                    f"expected {expected_platform}, got {platform}"
                )
    return archive_count


def validate_artifacts(
    root: Path,
    patterns: list[str],
    bootstrap_tools_dir: Path | None = None,
    bootstrap_tools_platform: str | None = None,
    bootstrap_tools_arch: str | None = None,
    wheelhouse_dir: Path | None = None,
    wheelhouse_platform: str | None = None,
    wheelhouse_arch: str | None = None,
) -> list[Path]:
    """Return matched artifact paths after enforcing non-empty required globs."""

    checked: list[Path] = []
    for pattern in patterns:
        matches = sorted(root.glob(pattern))
        if not matches:
            raise RuntimeError(f"missing installer artifact for pattern: {pattern}")
        for path in matches:
            validate_artifact_path(path)
            checked.append(path)

    manifest_paths = [path for path in checked if path.name == MANIFEST_NAME]
    if bootstrap_tools_dir is not None:
        validate_bootstrap_tools_payload(bootstrap_tools_dir, bootstrap_tools_platform, bootstrap_tools_arch)
    elif manifest_paths:
        validate_bootstrap_tools_payload(manifest_paths[0].parent, bootstrap_tools_platform, bootstrap_tools_arch)
    wheelhouse_manifest_paths = [path for path in checked if path.name == WHEELHOUSE_MANIFEST_NAME]
    wheelhouse_repo_root = root if wheelhouse_source_inputs_exist(root) else None
    if wheelhouse_dir is not None:
        validate_wheelhouse_payload(wheelhouse_dir, wheelhouse_platform, wheelhouse_arch, wheelhouse_repo_root)
    elif wheelhouse_manifest_paths:
        validate_wheelhouse_payload(
            wheelhouse_manifest_paths[0].parent,
            wheelhouse_platform,
            wheelhouse_arch,
            wheelhouse_repo_root,
        )
    return checked


def validate_artifact_path(path: Path) -> None:
    """Validate one matched installer artifact path."""

    if path.is_file():
        if path.stat().st_size <= 0:
            raise RuntimeError(f"installer artifact is empty: {path}")
        return
    if path.is_dir():
        if not any(path.rglob("*")):
            raise RuntimeError(f"installer artifact directory is empty: {path}")
        return
    raise RuntimeError(f"installer artifact is not a file or directory: {path}")


def wheelhouse_source_inputs_exist(root: Path) -> bool:
    """Return whether artifact validation can compare wheelhouse source hashes."""

    return (root / "pyproject.toml").is_file() or (root / "uv.lock").is_file()


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for release workflow validation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=REPO_ROOT,
        help="Root directory used to resolve artifact glob patterns.",
    )
    parser.add_argument(
        "--artifact",
        action="append",
        required=True,
        help="Required artifact glob relative to --root. Can be passed more than once.",
    )
    parser.add_argument(
        "--bootstrap-tools-dir",
        type=Path,
        default=None,
        help="Optional bootstrap-tools directory whose manifest should be validated.",
    )
    parser.add_argument(
        "--bootstrap-tools-platform",
        choices=("windows", "linux", "macos"),
        default=None,
        help="Optional release platform that every bootstrap-tools archive record must target.",
    )
    parser.add_argument(
        "--bootstrap-tools-arch",
        default=None,
        help="Optional release architecture that every bootstrap-tools archive record must target.",
    )
    parser.add_argument(
        "--wheelhouse-dir",
        type=Path,
        default=None,
        help="Optional wheelhouse directory whose manifest should be validated.",
    )
    parser.add_argument(
        "--wheelhouse-platform",
        choices=("windows", "linux", "macos"),
        default=None,
        help="Optional release platform that every wheelhouse record must target.",
    )
    parser.add_argument(
        "--wheelhouse-arch",
        default=None,
        help="Optional release architecture that every wheelhouse record must target.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run installer artifact validation."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        checked = validate_artifacts(
            args.root,
            args.artifact,
            args.bootstrap_tools_dir,
            args.bootstrap_tools_platform,
            args.bootstrap_tools_arch,
            args.wheelhouse_dir,
            args.wheelhouse_platform,
            args.wheelhouse_arch,
        )
    except Exception as exc:
        print(f"[installer-artifacts] error: {exc}", file=sys.stderr)
        return 1
    for path in checked:
        print(f"[installer-artifacts] ok {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
