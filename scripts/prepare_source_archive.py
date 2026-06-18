#!/usr/bin/env python3
"""Prepare an audited source archive for the Tauri bootstrap installer bundle.

The installer uses archives placed under
`apps/bootstrap-installer/src-tauri/source-archive` before it falls back to the
GitHub archive download. This helper is intended for release workflows, not for
the runtime installer path.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from prepare_python_runtime import REPO_ROOT, is_https_url_with_host, name_is_plain_file, sha256_file


DEFAULT_OUTPUT_DIR = REPO_ROOT / "apps" / "bootstrap-installer" / "src-tauri" / "source-archive"
MANIFEST_NAME = "source-archive-manifest.json"
ALLOWED_METADATA = {".gitignore", "README.md", MANIFEST_NAME}


@dataclass(frozen=True)
class PreparedSourceArchive:
    """One source archive with audit metadata for the release manifest."""

    name: str
    url: str
    path: Path
    size_bytes: int
    sha256: str


@dataclass(frozen=True)
class AuditedSourceArchiveSpec:
    """Maintainer-approved source archive input with an expected checksum."""

    name: str
    url: str
    expected_sha256: str


def parse_audited_archive_arg(value: str) -> AuditedSourceArchiveSpec:
    """Parse one explicitly checksummed source archive download mapping."""

    name, separator, remainder = value.partition("=")
    url, checksum_separator, expected_sha256 = remainder.rpartition("=")
    if not separator or not checksum_separator or not name or not url or not expected_sha256:
        raise ValueError("audited source archive must use NAME=HTTPS_URL=SHA256")
    if not name_is_plain_file(name) or not name.endswith(".zip"):
        raise ValueError(f"audited source archive has unsafe name: {name}")
    if not is_https_url_with_host(url):
        raise ValueError("audited source archive URL must be HTTPS")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise ValueError(f"audited source archive has invalid sha256: {name}")
    return AuditedSourceArchiveSpec(name=name, url=url, expected_sha256=expected_sha256.lower())


def download_archive(spec: AuditedSourceArchiveSpec, output_dir: Path, force: bool) -> Path:
    """Download an audited source archive unless a reusable copy already exists."""

    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / spec.name
    if path.exists() and not force:
        if path.is_file() and path.stat().st_size > 0:
            return path
        raise RuntimeError(f"source archive destination exists but is not a non-empty file: {path}")
    with urllib.request.urlopen(spec.url, timeout=120) as response, path.open("wb") as handle:
        shutil.copyfileobj(response, handle)
    if path.stat().st_size <= 0:
        raise RuntimeError(f"downloaded source archive is empty: {path}")
    return path


def prepare_audited_source_archive(
    output_dir: Path,
    audited_archive: str,
    owner: str,
    repo: str,
    archive_ref: str,
    commit: str | None,
    branch: str | None,
    force: bool,
    dry_run: bool,
) -> list[PreparedSourceArchive]:
    """Download one checksummed source archive and write its manifest."""

    spec = parse_audited_archive_arg(audited_archive)
    if dry_run:
        print(f"[source-archive] would download audited {spec.name} <- {spec.url}")
        return []

    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / spec.name
    if force or not path.exists():
        clean_source_archive_dir(output_dir)
    path = download_archive(spec, output_dir, force)
    actual_sha256 = sha256_file(path)
    if actual_sha256.lower() != spec.expected_sha256:
        raise RuntimeError(
            f"audited source archive checksum mismatch for {spec.name}: "
            f"expected {spec.expected_sha256}, got {actual_sha256}"
        )

    record = PreparedSourceArchive(
        name=spec.name,
        url=spec.url,
        path=path,
        size_bytes=path.stat().st_size,
        sha256=actual_sha256,
    )
    manifest_path = write_manifest(output_dir, owner, repo, archive_ref, commit, branch, [record])
    print(f"[source-archive] wrote manifest {manifest_path}")
    return [record]


def clean_source_archive_dir(output_dir: Path) -> None:
    """Remove generated source archive payload while preserving docs and ignore files."""

    if not output_dir.exists():
        return
    for entry in output_dir.iterdir():
        if entry.name in {".gitignore", "README.md"}:
            continue
        if entry.is_dir():
            shutil.rmtree(entry)
        else:
            entry.unlink()


def write_manifest(
    output_dir: Path,
    owner: str,
    repo: str,
    archive_ref: str,
    commit: str | None,
    branch: str | None,
    files: list[PreparedSourceArchive],
) -> Path:
    """Write the source archive manifest consumed by the Rust bootstrapper."""

    if not files:
        raise RuntimeError("source archive manifest has no files")
    payload = {
        "schemaVersion": 1,
        "generatedAtUtc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "owner": owner,
        "repo": repo,
        "archiveRef": archive_ref,
        "commit": commit,
        "branch": branch,
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
    expected_owner: str,
    expected_repo: str,
    expected_archive_ref: str,
    max_archive_bytes: int | None = None,
) -> int:
    """Validate that the source archive directory contains only manifest-owned payloads."""

    count = validate_manifest(
        output_dir,
        expected_owner,
        expected_repo,
        expected_archive_ref,
        max_archive_bytes,
    )
    payload = json.loads((output_dir / MANIFEST_NAME).read_text(encoding="utf-8"))
    expected = {entry["name"] for entry in payload["files"]}
    expected.update(ALLOWED_METADATA)

    for entry in output_dir.iterdir():
        if entry.name not in expected:
            raise RuntimeError(f"unmanifested source archive payload: {entry.name}")
        if not entry.is_file():
            raise RuntimeError(f"source archive payload is not a file: {entry.name}")
    return count


def validate_manifest(
    output_dir: Path,
    expected_owner: str,
    expected_repo: str,
    expected_archive_ref: str,
    max_archive_bytes: int | None = None,
) -> int:
    """Validate that the source archive manifest matches files in the output directory."""

    if max_archive_bytes is not None and max_archive_bytes <= 0:
        raise RuntimeError(f"max source archive bytes must be positive: {max_archive_bytes}")
    manifest_path = output_dir / MANIFEST_NAME
    if not manifest_path.is_file():
        raise RuntimeError(f"missing source archive manifest: {manifest_path}")
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported source archive manifest schema: {payload.get('schemaVersion')}")
    if payload.get("owner") != expected_owner or payload.get("repo") != expected_repo:
        raise RuntimeError("source archive manifest repository does not match expected target")
    if payload.get("archiveRef") != expected_archive_ref:
        raise RuntimeError(f"unexpected source archive ref: {payload.get('archiveRef')}")
    commit = payload.get("commit")
    branch = payload.get("branch")
    if commit is not None and not isinstance(commit, str):
        raise RuntimeError("source archive manifest has invalid commit")
    if branch is not None and not isinstance(branch, str):
        raise RuntimeError("source archive manifest has invalid branch")

    files = payload.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("source archive manifest has no files")
    seen_names: set[str] = set()
    for file in files:
        name = file.get("name")
        if not isinstance(name, str) or not name_is_plain_file(name) or not name.endswith(".zip"):
            raise RuntimeError(f"source archive file has unsafe name: {name}")
        if name in seen_names:
            raise RuntimeError(f"duplicate source archive file: {name}")
        seen_names.add(name)
        url = file.get("url")
        if not isinstance(url, str) or not is_https_url_with_host(url):
            raise RuntimeError(f"source archive file has invalid url: {name}")
        expected_size = file.get("sizeBytes")
        if type(expected_size) is not int or expected_size <= 0:
            raise RuntimeError(f"source archive file has invalid sizeBytes: {name}")
        expected_sha256 = file.get("sha256")
        if not isinstance(expected_sha256, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
            raise RuntimeError(f"source archive file has invalid sha256: {name}")
        path = output_dir / name
        if not path.is_file():
            raise RuntimeError(f"source archive file is missing: {name}")
        actual_size = path.stat().st_size
        if actual_size != expected_size:
            raise RuntimeError(f"source archive file size mismatch: {name}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256.lower():
            raise RuntimeError(f"source archive file checksum mismatch: {name}")
        if max_archive_bytes is not None and actual_size > max_archive_bytes:
            raise RuntimeError(
                f"source archive file {name} size {actual_size} exceeds max source archive bytes "
                f"{max_archive_bytes}"
            )
    return len(files)


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for source archive release automation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--audited-archive", required=False, help="Archive in NAME=HTTPS_URL=SHA256 form.")
    parser.add_argument("--owner", default="NousResearch")
    parser.add_argument("--repo", default="hermes-agent")
    parser.add_argument("--archive-ref", required=True)
    parser.add_argument("--commit", default=None)
    parser.add_argument("--branch", default=None)
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--validate-only", action="store_true")
    parser.add_argument("--max-archive-bytes", type=int, default=None)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the source archive preparation helper."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        if args.validate_only:
            count = validate_payload(
                args.output_dir,
                args.owner,
                args.repo,
                args.archive_ref,
                args.max_archive_bytes,
            )
            print(f"[source-archive] validated {count} file(s) in {args.output_dir}")
            return 0
        if args.audited_archive is None:
            raise RuntimeError("--audited-archive is required unless --validate-only is used")
        prepared = prepare_audited_source_archive(
            args.output_dir,
            args.audited_archive,
            args.owner,
            args.repo,
            args.archive_ref,
            args.commit,
            args.branch,
            args.force,
            args.dry_run,
        )
    except Exception as exc:
        print(f"[source-archive] error: {exc}", file=sys.stderr)
        return 1
    print(f"[source-archive] prepared {len(prepared)} file(s) in {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
