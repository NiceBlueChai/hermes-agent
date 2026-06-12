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
PYTHON_RUNTIME_MANIFEST_NAME = "python-runtime-manifest.json"
ALLOWED_PYTHON_RUNTIME_METADATA = {".gitignore", "README.md", PYTHON_RUNTIME_MANIFEST_NAME}


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
    python_runtime_manifest_paths = [
        path for path in checked if path.name == PYTHON_RUNTIME_MANIFEST_NAME
    ]
    if python_runtime_dir is not None:
        validate_python_runtime_payload(
            python_runtime_dir,
            python_runtime_platform,
            python_runtime_arch,
        )
    elif python_runtime_manifest_paths:
        validate_python_runtime_payload(
            python_runtime_manifest_paths[0].parent,
            python_runtime_platform,
            python_runtime_arch,
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
        if not any(child.is_file() for child in path.rglob("*")):
            raise RuntimeError(f"installer artifact directory has no files: {path}")
        if not any(child.is_file() and child.stat().st_size > 0 for child in path.rglob("*")):
            raise RuntimeError(f"installer artifact directory has no non-empty files: {path}")
        if path.suffix == ".app":
            validate_macos_app_artifact(path)
        return
    raise RuntimeError(f"installer artifact is not a file or directory: {path}")


def validate_macos_app_artifact(path: Path) -> None:
    """Validate the executable entry point inside a macOS app bundle."""

    executable = path / "Contents" / "MacOS" / "Hermes"
    if not executable.is_file() or executable.stat().st_size <= 0:
        raise RuntimeError(f"missing macOS app executable: {executable}")


def wheelhouse_source_inputs_exist(root: Path) -> bool:
    """Return whether artifact validation can compare wheelhouse source hashes."""

    return (root / "pyproject.toml").is_file() or (root / "uv.lock").is_file()


def validate_python_runtime_payload(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate that the Python runtime directory contains only manifest-owned payloads."""

    file_count = validate_python_runtime_manifest(output_dir, expected_platform, expected_arch)
    payload = json.loads((output_dir / PYTHON_RUNTIME_MANIFEST_NAME).read_text(encoding="utf-8"))
    expected = {entry["name"] for entry in payload["files"]}
    expected.update(ALLOWED_PYTHON_RUNTIME_METADATA)

    for entry in output_dir.iterdir():
        if entry.name not in expected:
            raise RuntimeError(f"unmanifested python runtime payload: {entry.name}")
        if not entry.is_file():
            raise RuntimeError(f"python runtime payload is not a file: {entry.name}")
    return file_count


def validate_python_runtime_manifest(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate the Python runtime manifest and each listed runtime file."""

    manifest_path = output_dir / PYTHON_RUNTIME_MANIFEST_NAME
    if not manifest_path.is_file():
        raise RuntimeError(f"missing python runtime manifest: {manifest_path}")
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported python runtime manifest schema: {payload.get('schemaVersion')}")
    if expected_platform is not None and payload.get("platform") != expected_platform:
        raise RuntimeError(
            f"unexpected python runtime platform: expected {expected_platform}, got {payload.get('platform')}"
        )
    if expected_arch is not None and payload.get("arch") != expected_arch:
        raise RuntimeError(f"unexpected python runtime arch: expected {expected_arch}, got {payload.get('arch')}")
    if not isinstance(payload.get("pythonTag"), str) or not payload["pythonTag"].strip():
        raise RuntimeError("python runtime manifest has no pythonTag")

    files = payload.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("python runtime manifest has no files")

    seen: set[str] = set()
    for file_record in files:
        name = file_record.get("name")
        if not isinstance(name, str) or Path(name).name != name or name.strip() != name:
            raise RuntimeError("python runtime file has unsafe name")
        if name in seen:
            raise RuntimeError(f"duplicate python runtime file: {name}")
        seen.add(name)

        size = file_record.get("sizeBytes")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
            raise RuntimeError(f"python runtime file has invalid size: {name}")
        sha256 = file_record.get("sha256")
        if (
            not isinstance(sha256, str)
            or len(sha256) != 64
            or not all(ch in "0123456789abcdefABCDEF" for ch in sha256)
        ):
            raise RuntimeError(f"python runtime file has invalid sha256: {name}")

        path = output_dir / name
        if not path.is_file():
            raise RuntimeError(f"python runtime file is missing: {name}")
        if path.stat().st_size != size:
            raise RuntimeError(f"python runtime file size mismatch: {name}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != sha256.lower():
            raise RuntimeError(f"python runtime file hash mismatch: {name}")
    return len(files)


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
        )
    except Exception as exc:
        print(f"[installer-artifacts] error: {exc}", file=sys.stderr)
        return 1
    for path in checked:
        print(f"[installer-artifacts] ok {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
