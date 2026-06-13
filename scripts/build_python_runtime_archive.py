#!/usr/bin/env python3
"""Build a uv-managed Python runtime archive for release packaging.

The archive produced here is staged from `UV_PYTHON_INSTALL_DIR` and is meant
to be uploaded by maintainers, then passed to `prepare_python_runtime.py` or the
release workflows as `NAME=HTTPS_URL=SHA256`.
"""

from __future__ import annotations

import argparse
import os
import platform as platform_module
import shutil
import subprocess
import sys
import tarfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from prepare_python_runtime import REPO_ROOT, sha256_file


DEFAULT_OUTPUT_DIR = REPO_ROOT / "dist" / "python-runtime"
DEFAULT_WORK_DIR = REPO_ROOT / ".tmp" / "python-runtime-build"


@dataclass(frozen=True)
class BuiltRuntimeArchive:
    """One locally generated Python runtime archive and its audit metadata."""

    path: Path
    name: str
    size_bytes: int
    sha256: str
    python_path: Path


def archive_name_for_target(platform: str, arch: str) -> str:
    """Return the release archive name for one Python runtime target."""

    if platform == "windows":
        return f"python-runtime-{platform}-{arch}.zip"
    if platform in {"linux", "macos"}:
        return f"python-runtime-{platform}-{arch}.tar.gz"
    raise ValueError(f"unsupported Python runtime platform: {platform}")


def current_host_platform() -> str:
    """Return the release platform label for the current host."""

    if sys.platform.startswith("win"):
        return "windows"
    if sys.platform == "darwin":
        return "macos"
    if sys.platform.startswith("linux"):
        return "linux"
    return sys.platform


def current_host_arch() -> str:
    """Return the release architecture label for the current host."""

    machine = platform_module.machine().lower()
    if machine in {"amd64", "x86_64"}:
        return "x64"
    if machine in {"arm64", "aarch64"}:
        return "arm64"
    if machine in {"x86", "i386", "i686"}:
        return "x86"
    return machine


def validate_local_target_matches_host(platform: str, arch: str) -> None:
    """Reject labels that would misrepresent a locally generated runtime archive."""

    host_platform = current_host_platform()
    host_arch = current_host_arch()
    if platform != host_platform or arch != host_arch:
        raise RuntimeError(
            f"python runtime target {platform}/{arch} does not match host {host_platform}/{host_arch}; "
            "use --allow-target-mismatch only with an audited cross-target builder"
        )


def build_uv_runtime_archive(
    output_dir: Path,
    work_dir: Path,
    platform: str,
    arch: str,
    python_version: str,
    uv: str,
    force: bool,
    allow_target_mismatch: bool = False,
    runner=subprocess.run,
) -> BuiltRuntimeArchive:
    """Install Python with uv in a staging directory and archive the install tree."""

    if not allow_target_mismatch:
        validate_local_target_matches_host(platform, arch)

    archive_name = archive_name_for_target(platform, arch)
    archive_path = output_dir / archive_name
    if archive_path.exists() and not force:
        raise RuntimeError(f"runtime archive already exists: {archive_path}")

    if work_dir.exists():
        if not force:
            raise RuntimeError(f"runtime work directory already exists: {work_dir}")
        shutil.rmtree(work_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    install_dir = work_dir / "python"
    bin_dir = work_dir / "bin"
    cache_dir = work_dir / "uv-cache"
    for path in (install_dir, bin_dir, cache_dir):
        path.mkdir(parents=True, exist_ok=True)

    env = os.environ.copy()
    env.update(
        {
            "UV_PYTHON_INSTALL_DIR": str(install_dir),
            "UV_PYTHON_BIN_DIR": str(bin_dir),
            "UV_CACHE_DIR": str(cache_dir),
        }
    )
    runner([uv, "python", "install", python_version], check=True, env=env)
    found = runner(
        [uv, "python", "find", python_version],
        check=True,
        env=env,
        stdout=subprocess.PIPE,
        text=True,
    )
    python_path = Path(str(found.stdout).strip())
    if not python_path.is_file():
        raise RuntimeError(f"uv did not create a usable Python executable: {python_path}")
    if not install_dir.is_dir() or not any(install_dir.rglob("*")):
        raise RuntimeError(f"uv Python install directory is empty: {install_dir}")

    if archive_path.exists():
        archive_path.unlink()
    if platform == "windows":
        write_zip_from_dir(install_dir, archive_path)
    else:
        write_tar_gz_from_dir(install_dir, archive_path)
    return BuiltRuntimeArchive(
        path=archive_path,
        name=archive_name,
        size_bytes=archive_path.stat().st_size,
        sha256=sha256_file(archive_path),
        python_path=python_path,
    )


def write_zip_from_dir(source_dir: Path, archive_path: Path) -> None:
    """Write a ZIP archive containing files under `source_dir`."""

    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(source_dir.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(source_dir).as_posix())


def write_tar_gz_from_dir(source_dir: Path, archive_path: Path) -> None:
    """Write a tar.gz archive containing files under `source_dir`."""

    with tarfile.open(archive_path, "w:gz") as archive:
        for path in sorted(source_dir.rglob("*")):
            if path.is_file():
                archive.add(path, arcname=path.relative_to(source_dir).as_posix())


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for Python runtime archive generation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--work-dir", type=Path, default=DEFAULT_WORK_DIR)
    parser.add_argument("--platform", choices=("windows", "linux", "macos"), required=True)
    parser.add_argument("--arch", required=True)
    parser.add_argument("--python-version", default="3.11")
    parser.add_argument("--uv", default="uv")
    parser.add_argument("--force", action="store_true")
    parser.add_argument(
        "--allow-target-mismatch",
        action="store_true",
        help="Allow a target label that does not match this host; only use with an audited cross-target builder.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the runtime archive builder."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        built = build_uv_runtime_archive(
            output_dir=args.output_dir,
            work_dir=args.work_dir,
            platform=args.platform,
            arch=args.arch,
            python_version=args.python_version,
            uv=args.uv,
            force=args.force,
            allow_target_mismatch=args.allow_target_mismatch,
        )
    except Exception as exc:
        print(f"[python-runtime-build] error: {exc}", file=sys.stderr)
        return 1
    print(f"[python-runtime-build] wrote {built.path}")
    print(f"[python-runtime-build] sizeBytes={built.size_bytes}")
    print(f"[python-runtime-build] sha256={built.sha256}")
    print(f"[python-runtime-build] workflow-input={built.name}=<HTTPS_URL>={built.sha256}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
