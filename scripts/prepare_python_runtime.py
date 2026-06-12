#!/usr/bin/env python3
"""Prepare an audited Python runtime archive for the Tauri bootstrap installer bundle.

The installer will eventually use archives placed under
`apps/bootstrap-installer/src-tauri/python-runtime` before it falls back to
`uv python install`. This helper is intended for release workflows, not for
the runtime installer path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "apps" / "bootstrap-installer" / "src-tauri" / "python-runtime"
MANIFEST_NAME = "python-runtime-manifest.json"
ALLOWED_METADATA = {".gitignore", "README.md", MANIFEST_NAME}


@dataclass(frozen=True)
class PreparedRuntimeFile:
    """One Python runtime archive with audit metadata for the release manifest."""

    platform: str
    arch: str
    python_tag: str
    name: str
    url: str
    path: Path
    size_bytes: int
    sha256: str


@dataclass(frozen=True)
class AuditedRuntimeArchiveSpec:
    """Maintainer-approved Python runtime archive input with an expected checksum."""

    name: str
    url: str
    expected_sha256: str


def sha256_file(path: Path) -> str:
    """Return the SHA-256 digest for one file."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def name_is_plain_file(name: str) -> bool:
    """Return whether a manifest path is a direct, unpadded file name."""

    return bool(name.strip()) and name == name.strip() and Path(name).name == name and name not in {".", ".."}


def prepared_runtime_record(
    platform: str,
    arch: str,
    python_tag: str,
    source_url: str,
    path: Path,
) -> PreparedRuntimeFile:
    """Build manifest metadata for one Python runtime archive."""

    return PreparedRuntimeFile(
        platform=platform,
        arch=arch,
        python_tag=python_tag,
        name=path.name,
        url=source_url,
        path=path,
        size_bytes=path.stat().st_size,
        sha256=sha256_file(path),
    )


def prepare_runtime_archive(
    output_dir: Path,
    archive_path: Path,
    platform: str,
    arch: str,
    python_tag: str,
    source_url: str,
    force: bool,
    dry_run: bool,
) -> list[PreparedRuntimeFile]:
    """Copy one audited Python runtime archive and write its manifest."""

    if not archive_path.is_file():
        raise RuntimeError(f"missing Python runtime archive: {archive_path}")
    if not name_is_plain_file(archive_path.name):
        raise RuntimeError(f"unsafe Python runtime archive name: {archive_path.name}")
    if not source_url.startswith("https://"):
        raise RuntimeError(f"python runtime archive has invalid url: {archive_path.name}")

    output_dir.mkdir(parents=True, exist_ok=True)
    dest = output_dir / archive_path.name
    if dry_run:
        print(f"[python-runtime] would copy {archive_path} as {dest.name}")
        return []
    if dest.exists() and not force:
        if dest.is_file() and dest.stat().st_size > 0:
            record = prepared_runtime_record(platform, arch, python_tag, source_url, dest)
            write_manifest(output_dir, [record])
            return [record]
        raise RuntimeError(f"python runtime destination exists but is not a non-empty file: {dest}")

    clean_runtime_dir(output_dir)
    shutil.copy2(archive_path, dest)
    record = prepared_runtime_record(platform, arch, python_tag, source_url, dest)
    manifest_path = write_manifest(output_dir, [record])
    print(f"[python-runtime] wrote manifest {manifest_path}")
    return [record]


def parse_audited_archive_arg(value: str) -> AuditedRuntimeArchiveSpec:
    """Parse one explicitly checksummed Python runtime archive download mapping."""

    name, separator, remainder = value.partition("=")
    url, checksum_separator, expected_sha256 = remainder.rpartition("=")
    if not separator or not checksum_separator or not name or not url or not expected_sha256:
        raise ValueError("audited runtime archive must use NAME=HTTPS_URL=SHA256")
    if not name_is_plain_file(name):
        raise ValueError(f"audited runtime archive has unsafe name: {name}")
    if not url.startswith("https://"):
        raise ValueError("audited runtime archive URL must be HTTPS")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise ValueError(f"audited runtime archive has invalid sha256: {name}")
    return AuditedRuntimeArchiveSpec(name=name, url=url, expected_sha256=expected_sha256.lower())


def download_archive(spec: AuditedRuntimeArchiveSpec, output_dir: Path, force: bool) -> Path:
    """Download an audited Python runtime archive unless a reusable copy already exists."""

    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / spec.name
    if path.exists() and not force:
        if path.is_file() and path.stat().st_size > 0:
            return path
        raise RuntimeError(f"python runtime destination exists but is not a non-empty file: {path}")
    with urllib.request.urlopen(spec.url, timeout=120) as response, path.open("wb") as handle:
        shutil.copyfileobj(response, handle)
    if path.stat().st_size <= 0:
        raise RuntimeError(f"downloaded python runtime archive is empty: {path}")
    return path


def prepare_audited_runtime_archive(
    output_dir: Path,
    audited_archive: str,
    platform: str,
    arch: str,
    python_tag: str,
    force: bool,
    dry_run: bool,
) -> list[PreparedRuntimeFile]:
    """Download one checksummed Python runtime archive and write its manifest."""

    spec = parse_audited_archive_arg(audited_archive)
    if dry_run:
        print(f"[python-runtime] would download audited {spec.name} <- {spec.url}")
        return []

    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / spec.name
    if force or not path.exists():
        clean_runtime_dir(output_dir)
    path = download_archive(spec, output_dir, force)
    actual_sha256 = sha256_file(path)
    if actual_sha256.lower() != spec.expected_sha256:
        raise RuntimeError(
            f"audited runtime archive checksum mismatch for {spec.name}: "
            f"expected {spec.expected_sha256}, got {actual_sha256}"
        )

    record = prepared_runtime_record(platform, arch, python_tag, spec.url, path)
    manifest_path = write_manifest(output_dir, [record])
    print(f"[python-runtime] wrote manifest {manifest_path}")
    return [record]


def clean_runtime_dir(output_dir: Path) -> None:
    """Remove generated Python runtime payload while preserving docs and ignore files."""

    if not output_dir.exists():
        return
    for entry in output_dir.iterdir():
        if entry.name in {".gitignore", "README.md"}:
            continue
        if entry.is_dir():
            shutil.rmtree(entry)
        else:
            entry.unlink()


def write_manifest(output_dir: Path, files: list[PreparedRuntimeFile]) -> Path:
    """Write the Python runtime manifest consumed by release reviewers."""

    if not files:
        raise RuntimeError("python runtime manifest has no files")
    platforms = {file.platform for file in files}
    arches = {file.arch for file in files}
    python_tags = {file.python_tag for file in files}
    if len(platforms) != 1 or len(arches) != 1 or len(python_tags) != 1:
        raise RuntimeError("python runtime manifest files must share one platform, arch, and python tag")
    payload = {
        "schemaVersion": 1,
        "generatedAtUtc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "platform": files[0].platform,
        "arch": files[0].arch,
        "pythonTag": files[0].python_tag,
        "files": [
            {
                "name": file.name,
                "url": file.url,
                "sizeBytes": file.size_bytes,
                "sha256": file.sha256,
            }
            for file in files
        ],
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = output_dir / MANIFEST_NAME
    manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest_path


def validate_payload(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate that the Python runtime directory contains only manifest-owned payloads."""

    count = validate_manifest(output_dir, expected_platform, expected_arch)
    payload = json.loads((output_dir / MANIFEST_NAME).read_text(encoding="utf-8"))
    expected = {entry["name"] for entry in payload["files"]}
    expected.update(ALLOWED_METADATA)

    for entry in output_dir.iterdir():
        if entry.name not in expected:
            raise RuntimeError(f"unmanifested python runtime payload: {entry.name}")
        if not entry.is_file():
            raise RuntimeError(f"python runtime payload is not a file: {entry.name}")
    return count


def validate_manifest(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate that the Python runtime manifest matches files in the output directory."""

    manifest_path = output_dir / MANIFEST_NAME
    if not manifest_path.is_file():
        raise RuntimeError(f"missing python runtime manifest: {manifest_path}")
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported python runtime manifest schema: {payload.get('schemaVersion')}")
    platform = payload.get("platform")
    if platform not in {"windows", "linux", "macos"}:
        raise RuntimeError("python runtime manifest is missing platform")
    if expected_platform is not None and platform != expected_platform:
        raise RuntimeError(f"unexpected python runtime platform: expected {expected_platform}, got {platform}")
    arch = payload.get("arch")
    if not isinstance(arch, str) or not arch:
        raise RuntimeError("python runtime manifest is missing arch")
    if expected_arch is not None and arch != expected_arch:
        raise RuntimeError(f"unexpected python runtime arch: expected {expected_arch}, got {arch}")
    python_tag = payload.get("pythonTag")
    if not isinstance(python_tag, str) or not python_tag.startswith("cp") or len(python_tag) <= 2:
        raise RuntimeError("python runtime manifest has invalid pythonTag")

    files = payload.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("python runtime manifest has no files")
    seen_names: set[str] = set()
    for file in files:
        name = file.get("name")
        if not isinstance(name, str) or not name_is_plain_file(name):
            raise RuntimeError(f"python runtime file has unsafe name: {name}")
        if name in seen_names:
            raise RuntimeError(f"duplicate python runtime file: {name}")
        seen_names.add(name)
        url = file.get("url")
        if not isinstance(url, str) or not url.startswith("https://"):
            raise RuntimeError(f"python runtime file has invalid url: {name}")
        expected_size = file.get("sizeBytes")
        if type(expected_size) is not int or expected_size <= 0:
            raise RuntimeError(f"python runtime file has invalid sizeBytes: {name}")
        expected_sha256 = file.get("sha256")
        if not isinstance(expected_sha256, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
            raise RuntimeError(f"python runtime file has invalid sha256: {name}")
        path = output_dir / name
        if not path.is_file():
            raise RuntimeError(f"python runtime file is missing: {name}")
        actual_size = path.stat().st_size
        if actual_size != expected_size:
            raise RuntimeError(f"python runtime file size mismatch: {name}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256.lower():
            raise RuntimeError(f"python runtime file checksum mismatch: {name}")
    return len(files)


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for Python runtime release automation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--archive", type=Path, default=None, help="Audited Python runtime archive to bundle.")
    parser.add_argument(
        "--audited-archive",
        default=None,
        help="Download an audited Python runtime archive, in NAME=HTTPS_URL=SHA256 form.",
    )
    parser.add_argument("--source-url", default=None, help="HTTPS source URL recorded for the runtime archive.")
    parser.add_argument("--platform", choices=("windows", "linux", "macos"), required=True)
    parser.add_argument("--arch", required=True)
    parser.add_argument("--python-tag", required=True, help="Python ABI tag such as cp311.")
    parser.add_argument("--force", action="store_true", help="Replace existing generated runtime payload.")
    parser.add_argument("--dry-run", action="store_true", help="Print actions without copying files.")
    parser.add_argument("--validate-only", action="store_true", help="Validate an existing runtime manifest.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the Python runtime preparation helper."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.validate_only:
            count = validate_payload(args.output_dir, args.platform, args.arch)
            print(f"[python-runtime] validated {count} file(s) in {args.output_dir}")
            return 0
        if args.audited_archive is not None:
            if args.archive is not None or args.source_url:
                raise RuntimeError("--audited-archive cannot be combined with --archive or --source-url")
            prepared = prepare_audited_runtime_archive(
                args.output_dir,
                args.audited_archive,
                args.platform,
                args.arch,
                args.python_tag,
                args.force,
                args.dry_run,
            )
            print(f"[python-runtime] prepared {len(prepared)} file(s) in {args.output_dir}")
            return 0
        if args.archive is None:
            raise RuntimeError("--archive or --audited-archive is required unless --validate-only is used")
        if not args.source_url:
            raise RuntimeError("--source-url is required unless --validate-only is used")
        prepared = prepare_runtime_archive(
            args.output_dir,
            args.archive,
            args.platform,
            args.arch,
            args.python_tag,
            args.source_url,
            args.force,
            args.dry_run,
        )
    except Exception as exc:
        print(f"[python-runtime] error: {exc}", file=sys.stderr)
        return 1
    print(f"[python-runtime] prepared {len(prepared)} file(s) in {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
