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
from prepare_python_runtime import (
    MANIFEST_NAME as PYTHON_RUNTIME_MANIFEST_NAME,
    validate_payload as validate_python_runtime_payload,
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
    python_runtime_dir: Path | None = None,
    python_runtime_platform: str | None = None,
    python_runtime_arch: str | None = None,
    max_artifact_bytes: int | None = None,
    max_total_artifact_bytes: int | None = None,
) -> list[Path]:
    """Return matched artifact paths after enforcing non-empty required globs."""

    checked: list[Path] = []
    budget_paths: list[Path] = []
    validated_wheelhouse_dir = None
    validated_python_runtime_dir = None
    for pattern in patterns:
        matches = sorted(root.glob(pattern))
        if not matches:
            raise RuntimeError(f"missing installer artifact for pattern: {pattern}")
        for path in matches:
            validate_artifact_path(path)
            if max_artifact_bytes is not None:
                validate_artifact_size_gate(path, max_artifact_bytes)
            checked.append(path)
            budget_paths.append(path)

    manifest_paths = [path for path in checked if path.name == MANIFEST_NAME]
    if bootstrap_tools_dir is not None:
        validate_bootstrap_tools_payload(bootstrap_tools_dir, bootstrap_tools_platform, bootstrap_tools_arch)
        if checked_contains_payload_dir(checked, bootstrap_tools_dir, MANIFEST_NAME):
            budget_paths.append(bootstrap_tools_dir)
    elif manifest_paths:
        validate_bootstrap_tools_payload(manifest_paths[0].parent, bootstrap_tools_platform, bootstrap_tools_arch)
        budget_paths.append(manifest_paths[0].parent)
    wheelhouse_manifest_paths = [path for path in checked if path.name == WHEELHOUSE_MANIFEST_NAME]
    wheelhouse_repo_root = root if wheelhouse_source_inputs_exist(root) else None
    if wheelhouse_dir is not None:
        validate_wheelhouse_payload(wheelhouse_dir, wheelhouse_platform, wheelhouse_arch, wheelhouse_repo_root)
        validated_wheelhouse_dir = wheelhouse_dir
        if checked_contains_payload_dir(checked, wheelhouse_dir, WHEELHOUSE_MANIFEST_NAME):
            budget_paths.append(wheelhouse_dir)
    elif wheelhouse_manifest_paths:
        validate_wheelhouse_payload(
            wheelhouse_manifest_paths[0].parent,
            wheelhouse_platform,
            wheelhouse_arch,
            wheelhouse_repo_root,
        )
        validated_wheelhouse_dir = wheelhouse_manifest_paths[0].parent
        budget_paths.append(wheelhouse_manifest_paths[0].parent)
    python_runtime_manifest_paths = [
        path for path in checked if path.name == PYTHON_RUNTIME_MANIFEST_NAME
    ]
    if python_runtime_dir is not None:
        validate_python_runtime_payload(
            python_runtime_dir,
            python_runtime_platform,
            python_runtime_arch,
        )
        validated_python_runtime_dir = python_runtime_dir
        if checked_contains_payload_dir(checked, python_runtime_dir, PYTHON_RUNTIME_MANIFEST_NAME):
            budget_paths.append(python_runtime_dir)
    elif python_runtime_manifest_paths:
        validate_python_runtime_payload(
            python_runtime_manifest_paths[0].parent,
            python_runtime_platform,
            python_runtime_arch,
        )
        validated_python_runtime_dir = python_runtime_manifest_paths[0].parent
        budget_paths.append(python_runtime_manifest_paths[0].parent)
    if validated_wheelhouse_dir is not None and validated_python_runtime_dir is not None:
        validate_python_runtime_matches_wheelhouse(
            validated_wheelhouse_dir,
            validated_python_runtime_dir,
        )
    if max_artifact_bytes is not None:
        for path in budget_paths:
            validate_artifact_size_gate(path, max_artifact_bytes)
    if max_total_artifact_bytes is not None:
        total_bytes = sum(artifact_size_bytes(path) for path in budget_paths)
        if total_bytes > max_total_artifact_bytes:
            raise RuntimeError(
                f"installer artifacts total size {total_bytes} exceeds max total artifact bytes "
                f"{max_total_artifact_bytes}"
            )
    return checked


def checked_contains_payload_dir(checked: list[Path], payload_dir: Path, manifest_name: str) -> bool:
    """Return whether matched artifacts explicitly include a payload directory or its manifest."""

    payload_dir = payload_dir.resolve()
    manifest_path = payload_dir / manifest_name
    for path in checked:
        resolved = path.resolve()
        if resolved == payload_dir or resolved == manifest_path or resolved.parent == payload_dir:
            return True
    return False


def validate_python_runtime_matches_wheelhouse(wheelhouse_dir: Path, python_runtime_dir: Path) -> None:
    """Validate that bundled wheels and the bundled runtime target the same Python ABI."""

    wheelhouse_manifest = json.loads((wheelhouse_dir / WHEELHOUSE_MANIFEST_NAME).read_text(encoding="utf-8"))
    runtime_manifest = json.loads((python_runtime_dir / PYTHON_RUNTIME_MANIFEST_NAME).read_text(encoding="utf-8"))
    wheel_tags = {
        wheel.get("python")
        for wheel in wheelhouse_manifest.get("wheels", [])
        if isinstance(wheel.get("python"), str)
    }
    if len(wheel_tags) != 1:
        raise RuntimeError("wheelhouse manifest must contain exactly one Python tag")
    wheel_tag = next(iter(wheel_tags))
    runtime_tag = runtime_manifest.get("pythonTag")
    if wheel_tag != runtime_tag:
        raise RuntimeError(
            f"python runtime tag mismatch: wheelhouse uses {wheel_tag}, runtime uses {runtime_tag}"
        )


def validate_artifact_path(path: Path) -> None:
    """Validate one matched installer artifact path."""

    if path.is_file():
        if path.stat().st_size <= 0:
            raise RuntimeError(f"installer artifact is empty: {path}")
        return
    if path.is_dir():
        if not any(path.rglob("*")):
            raise RuntimeError(f"installer artifact directory is empty: {path}")
        if not any(child.is_file() for child in path.rglob("*")):
            raise RuntimeError(f"installer artifact directory has no files: {path}")
        if not any(child.is_file() and child.stat().st_size > 0 for child in path.rglob("*")):
            raise RuntimeError(f"installer artifact directory has no non-empty files: {path}")
        if path.suffix == ".app":
            validate_macos_app_artifact(path)
        return
    raise RuntimeError(f"installer artifact is not a file or directory: {path}")


def validate_artifact_size_gate(path: Path, max_bytes: int) -> None:
    """Validate that one artifact stays within the configured byte budget."""

    if max_bytes <= 0:
        raise RuntimeError(f"max artifact bytes must be positive: {max_bytes}")
    size_bytes = artifact_size_bytes(path)
    if size_bytes > max_bytes:
        raise RuntimeError(
            f"installer artifact size {size_bytes} exceeds max artifact bytes {max_bytes}: {path}"
        )


def artifact_size_bytes(path: Path) -> int:
    """Return the recursive file size for one artifact path."""

    if path.is_file():
        return path.stat().st_size
    if path.is_dir():
        return sum(child.stat().st_size for child in path.rglob("*") if child.is_file())
    raise RuntimeError(f"installer artifact is not a file or directory: {path}")


def validate_macos_app_artifact(path: Path) -> None:
    """Validate the executable entry point inside a macOS app bundle."""

    executable = path / "Contents" / "MacOS" / "Hermes"
    if not executable.is_file() or executable.stat().st_size <= 0:
        raise RuntimeError(f"missing macOS app executable: {executable}")


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
    parser.add_argument(
        "--python-runtime-dir",
        type=Path,
        default=None,
        help="Optional Python runtime directory whose manifest should be validated.",
    )
    parser.add_argument(
        "--python-runtime-platform",
        choices=("windows", "linux", "macos"),
        default=None,
        help="Optional release platform that the Python runtime manifest must target.",
    )
    parser.add_argument(
        "--python-runtime-arch",
        default=None,
        help="Optional release architecture that the Python runtime manifest must target.",
    )
    parser.add_argument(
        "--max-artifact-bytes",
        type=int,
        default=None,
        help="Optional per-artifact recursive size budget in bytes.",
    )
    parser.add_argument(
        "--max-total-artifact-bytes",
        type=int,
        default=None,
        help="Optional total recursive size budget across matched artifacts in bytes.",
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
            args.python_runtime_dir,
            args.python_runtime_platform,
            args.python_runtime_arch,
            args.max_artifact_bytes,
            args.max_total_artifact_bytes,
        )
    except Exception as exc:
        print(f"[installer-artifacts] error: {exc}", file=sys.stderr)
        return 1
    for path in checked:
        print(f"[installer-artifacts] ok {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
