#!/usr/bin/env python3
"""Build a repository source archive for release packaging.

The archive produced here is intended to be uploaded by maintainers, then
passed to `prepare_source_archive.py` or the release workflows as
`NAME=HTTPS_URL=SHA256`.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from prepare_python_runtime import REPO_ROOT, sha256_file


DEFAULT_OUTPUT_DIR = REPO_ROOT / "dist" / "source-archive"


@dataclass(frozen=True)
class BuiltSourceArchive:
    """One locally generated source archive and its audit metadata."""

    path: Path
    name: str
    size_bytes: int
    sha256: str
    archive_ref: str


def safe_archive_ref(value: str) -> str:
    """Return a filesystem-safe archive reference segment."""

    safe = re.sub(r"[^A-Za-z0-9._-]+", "-", value.strip())
    safe = safe.strip(".-")
    if not safe:
        raise ValueError("source archive ref is empty")
    return safe[:80]


def build_git_source_archive(
    output_dir: Path,
    archive_ref: str,
    git: str,
    force: bool,
    runner=subprocess.run,
) -> BuiltSourceArchive:
    """Create a zip source archive from a git ref."""

    safe_ref = safe_archive_ref(archive_ref)
    archive_name = f"hermes-agent-{safe_ref}.zip"
    archive_path = output_dir / archive_name
    if archive_path.exists() and not force:
        raise RuntimeError(f"source archive already exists: {archive_path}")

    output_dir.mkdir(parents=True, exist_ok=True)
    if archive_path.exists():
        archive_path.unlink()

    prefix = f"hermes-agent-{safe_ref}/"
    runner(
        [
            git,
            "archive",
            "--format=zip",
            f"--output={archive_path}",
            f"--prefix={prefix}",
            archive_ref,
        ],
        cwd=REPO_ROOT,
        check=True,
    )
    if not archive_path.is_file() or archive_path.stat().st_size <= 0:
        raise RuntimeError(f"git archive did not create a non-empty file: {archive_path}")
    return BuiltSourceArchive(
        path=archive_path,
        name=archive_name,
        size_bytes=archive_path.stat().st_size,
        sha256=sha256_file(archive_path),
        archive_ref=archive_ref,
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for source archive generation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--archive-ref", required=True, help="Git commit or branch to archive.")
    parser.add_argument("--git", default="git")
    parser.add_argument("--force", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the source archive builder."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        built = build_git_source_archive(
            output_dir=args.output_dir,
            archive_ref=args.archive_ref,
            git=args.git,
            force=args.force,
        )
    except Exception as exc:
        print(f"[source-archive-build] error: {exc}", file=sys.stderr)
        return 1
    print(f"[source-archive-build] wrote {built.path}")
    print(f"[source-archive-build] archiveRef={built.archive_ref}")
    print(f"[source-archive-build] sizeBytes={built.size_bytes}")
    print(f"[source-archive-build] sha256={built.sha256}")
    print(f"[source-archive-build] workflow-input={built.name}=<HTTPS_URL>={built.sha256}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
