#!/usr/bin/env python3
"""Prepare Python wheels for the Tauri bootstrap installer bundle.

The installer can use wheels placed under
`apps/bootstrap-installer/src-tauri/wheelhouse` before it falls back to
`uv.lock` and online PyPI resolution. This helper is intended for release
workflows, not for the runtime installer path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised on Python 3.10 CI runners.
    from pip._vendor import tomli as tomllib


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "apps" / "bootstrap-installer" / "src-tauri" / "wheelhouse"
MANIFEST_NAME = "wheelhouse-manifest.json"
ALLOWED_METADATA = {".gitignore", "README.md", MANIFEST_NAME}
LOCKED_CONSTRAINTS_NAME = "wheelhouse-constraints.txt"


@dataclass(frozen=True)
class PreparedWheel:
    """One wheel with audit metadata for the release manifest."""

    platform: str
    arch: str
    python: str
    name: str
    path: Path
    size_bytes: int
    sha256: str


@dataclass(frozen=True)
class SourceFileRecord:
    """One dependency input file used to prepare the release wheelhouse."""

    path: str
    sha256: str


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


def source_file_records(repo_root: Path) -> list[SourceFileRecord]:
    """Return dependency input file hashes recorded in the wheelhouse manifest."""

    records = []
    for relative in ("pyproject.toml", "uv.lock"):
        path = repo_root / relative
        if path.is_file():
            records.append(SourceFileRecord(path=relative, sha256=sha256_file(path)))
    if not records:
        raise RuntimeError(f"no dependency source files found under {repo_root}")
    return records


def python_tag_from_executable(python: str) -> str:
    """Return a compact Python tag such as cp311 for the selected interpreter."""

    output = subprocess.check_output(
        [
            python,
            "-c",
            "import sys; print(f'cp{sys.version_info.major}{sys.version_info.minor}')",
        ],
        text=True,
    )
    return output.strip()


def build_locked_constraint_lines(repo_root: Path) -> list[str]:
    """Return pip constraints derived from registry packages pinned in uv.lock."""

    lock_path = repo_root / "uv.lock"
    if not lock_path.is_file():
        return []
    payload = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    constraints: dict[str, str] = {}
    for package in payload.get("package", []):
        source = package.get("source") or {}
        if "registry" not in source:
            continue
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        key = canonical_package_name(name)
        line = f"{name}=={version}"
        existing = constraints.get(key)
        if existing is not None and existing != line:
            raise RuntimeError(f"uv.lock contains conflicting versions for {name}")
        constraints[key] = line
    return [constraints[key] for key in sorted(constraints)]


def canonical_package_name(name: str) -> str:
    """Normalize a Python package name for duplicate detection."""

    return name.lower().replace("_", "-")


def write_locked_constraints(path: Path, constraints: list[str]) -> None:
    """Write a temporary pip constraints file for lock-driven wheel builds."""

    path.write_text("\n".join(constraints) + "\n", encoding="utf-8")


def build_pip_wheel_command(
    repo_root: Path,
    output_dir: Path,
    python: str,
    constraints_path: Path | None = None,
) -> list[str]:
    """Build the pip command used to materialize the release wheelhouse."""

    command = [
        python,
        "-m",
        "pip",
        "wheel",
        "--wheel-dir",
        str(output_dir),
    ]
    if constraints_path is not None:
        command.extend(["--constraint", str(constraints_path)])
    command.append(".[all]")
    return command


def prepare_wheelhouse(
    repo_root: Path,
    output_dir: Path,
    platform: str,
    arch: str,
    python: str,
    force: bool,
    dry_run: bool,
) -> list[PreparedWheel]:
    """Build wheels and return manifest records for the generated payload."""

    output_dir.mkdir(parents=True, exist_ok=True)
    if force:
        clean_wheelhouse(output_dir)
    constraints = build_locked_constraint_lines(repo_root)
    if dry_run:
        constraints_path = output_dir / LOCKED_CONSTRAINTS_NAME if constraints else None
        command = build_pip_wheel_command(repo_root, output_dir, python, constraints_path)
        print("[wheelhouse] would run " + " ".join(command))
        return []
    with tempfile.TemporaryDirectory(prefix="hermes-wheelhouse-") as tmp:
        constraints_path = None
        if constraints:
            constraints_path = Path(tmp) / LOCKED_CONSTRAINTS_NAME
            write_locked_constraints(constraints_path, constraints)
        command = build_pip_wheel_command(repo_root, output_dir, python, constraints_path)
        subprocess.run(command, cwd=repo_root, check=True)
    python_tag = python_tag_from_executable(python)
    wheels = sorted(output_dir.glob("*.whl"))
    if not wheels:
        raise RuntimeError(f"no wheels were generated in {output_dir}")
    records = [prepared_wheel_record(platform, arch, python_tag, wheel) for wheel in wheels]
    manifest_path = write_manifest(output_dir, records, source_files=source_file_records(repo_root))
    print(f"[wheelhouse] wrote manifest {manifest_path}")
    return records


def clean_wheelhouse(output_dir: Path) -> None:
    """Remove generated wheelhouse payload while preserving docs and ignore files."""

    if not output_dir.exists():
        return
    for entry in output_dir.iterdir():
        if entry.name in {".gitignore", "README.md"}:
            continue
        if entry.is_dir():
            shutil.rmtree(entry)
        else:
            entry.unlink()


def prepared_wheel_record(platform: str, arch: str, python: str, path: Path) -> PreparedWheel:
    """Build manifest metadata for one wheel."""

    return PreparedWheel(
        platform=platform,
        arch=arch,
        python=python,
        name=path.name,
        path=path,
        size_bytes=path.stat().st_size,
        sha256=sha256_file(path),
    )


def write_manifest(
    output_dir: Path,
    wheels: list[PreparedWheel],
    source_files: list[SourceFileRecord] | None = None,
) -> Path:
    """Write the wheelhouse manifest consumed by release reviewers."""

    payload = {
        "schemaVersion": 1,
        "generatedAtUtc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "sourceFiles": [
            {
                "path": source.path,
                "sha256": source.sha256,
            }
            for source in (source_files or [])
        ],
        "wheels": [
            {
                "arch": wheel.arch,
                "platform": wheel.platform,
                "python": wheel.python,
                "name": wheel.name,
                "sizeBytes": wheel.size_bytes,
                "sha256": wheel.sha256,
            }
            for wheel in wheels
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
    repo_root: Path | None = None,
) -> int:
    """Validate the manifest and reject unmanifested wheel payloads."""

    count = validate_manifest(output_dir, expected_platform, expected_arch, repo_root)
    payload = json.loads((output_dir / MANIFEST_NAME).read_text(encoding="utf-8"))
    expected = {wheel["name"] for wheel in payload["wheels"]}
    expected.update(ALLOWED_METADATA)
    for entry in output_dir.iterdir():
        if entry.name not in expected:
            raise RuntimeError(f"unmanifested wheelhouse payload: {entry.name}")
        if not entry.is_file():
            raise RuntimeError(f"wheelhouse payload is not a file: {entry.name}")
    return count


def validate_manifest(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
    repo_root: Path | None = None,
) -> int:
    """Validate that the wheelhouse manifest matches wheels in the output directory."""

    manifest_path = output_dir / MANIFEST_NAME
    if not manifest_path.is_file():
        raise RuntimeError(f"missing wheelhouse manifest: {manifest_path}")
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported wheelhouse manifest schema: {payload.get('schemaVersion')}")
    wheels = payload.get("wheels")
    if not isinstance(wheels, list) or not wheels:
        raise RuntimeError("wheelhouse manifest has no wheels")
    if repo_root is not None:
        validate_source_files(payload, repo_root)

    seen_names: set[str] = set()
    for wheel in wheels:
        name = wheel.get("name")
        if not isinstance(name, str) or not name.endswith(".whl"):
            raise RuntimeError("wheelhouse manifest wheel is missing .whl name")
        if not name_is_plain_file(name):
            raise RuntimeError(f"manifest wheel has unsafe wheel name: {name}")
        if name in seen_names:
            raise RuntimeError(f"duplicate wheel in wheelhouse manifest: {name}")
        seen_names.add(name)
        platform = wheel.get("platform")
        if platform not in {"windows", "linux", "macos"}:
            raise RuntimeError(f"manifest wheel is missing platform: {name}")
        if expected_platform is not None and platform != expected_platform:
            raise RuntimeError(
                f"unexpected wheelhouse platform for {name}: expected {expected_platform}, got {platform}"
            )
        arch = wheel.get("arch")
        if not isinstance(arch, str) or not arch:
            raise RuntimeError(f"manifest wheel is missing arch: {name}")
        if expected_arch is not None and arch != expected_arch:
            raise RuntimeError(f"unexpected wheelhouse arch for {name}: expected {expected_arch}, got {arch}")
        python = wheel.get("python")
        if not isinstance(python, str) or not python.startswith("cp"):
            raise RuntimeError(f"manifest wheel has invalid python tag: {name}")
        path = output_dir / name
        if not path.is_file():
            raise RuntimeError(f"manifest wheel is missing: {path}")
        expected_size = wheel.get("sizeBytes")
        if type(expected_size) is not int or expected_size <= 0:
            raise RuntimeError(f"manifest wheel has invalid sizeBytes: {name}")
        actual_size = path.stat().st_size
        if actual_size != expected_size:
            raise RuntimeError(f"wheel size mismatch for {name}: expected {expected_size}, got {actual_size}")
        expected_sha256 = wheel.get("sha256")
        if not isinstance(expected_sha256, str) or len(expected_sha256) != 64:
            raise RuntimeError(f"manifest wheel has invalid sha256: {name}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256.lower():
            raise RuntimeError(
                f"wheel checksum mismatch for {name}: expected {expected_sha256}, got {actual_sha256}"
            )
    return len(wheels)


def validate_source_files(payload: dict, repo_root: Path) -> None:
    """Validate that wheelhouse source hashes match the current dependency files."""

    source_files = payload.get("sourceFiles")
    if not isinstance(source_files, list) or not source_files:
        raise RuntimeError("wheelhouse manifest has no sourceFiles")
    seen_paths: set[str] = set()
    for source in source_files:
        relative = source.get("path")
        if not isinstance(relative, str) or not name_is_plain_file(relative):
            raise RuntimeError("wheelhouse source file has unsafe path")
        if relative in seen_paths:
            raise RuntimeError(f"duplicate wheelhouse source file: {relative}")
        seen_paths.add(relative)
        expected_sha256 = source.get("sha256")
        if not isinstance(expected_sha256, str) or len(expected_sha256) != 64:
            raise RuntimeError(f"wheelhouse source file has invalid sha256: {relative}")
        path = repo_root / relative
        if not path.is_file():
            raise RuntimeError(f"wheelhouse source file is missing: {relative}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256.lower():
            raise RuntimeError(
                f"wheelhouse source hash mismatch for {relative}: "
                f"expected {expected_sha256}, got {actual_sha256}"
            )


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for wheelhouse release automation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("windows", "linux", "macos"), default=sys.platform)
    parser.add_argument("--arch", default=os.environ.get("PROCESSOR_ARCHITECTURE", "x64").lower())
    parser.add_argument("--python", default=sys.executable, help="Python interpreter used to run pip wheel.")
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--force", action="store_true", help="Clear existing generated wheelhouse payload first.")
    parser.add_argument("--dry-run", action="store_true", help="Print the pip wheel command without running it.")
    parser.add_argument("--validate-only", action="store_true", help="Validate an existing wheelhouse manifest.")
    return parser.parse_args(argv)


def normalized_platform(value: str) -> str:
    """Map Python and workflow platform names to manifest platform labels."""

    if value in {"win32", "cygwin"}:
        return "windows"
    if value == "darwin":
        return "macos"
    return value


def main(argv: list[str] | None = None) -> int:
    """Run the wheelhouse preparation helper."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    platform = normalized_platform(args.platform)
    try:
        if args.validate_only:
            count = validate_payload(args.output_dir, platform, args.arch, args.repo_root)
            print(f"[wheelhouse] validated {count} wheel(s) in {args.output_dir}")
            return 0
        prepared = prepare_wheelhouse(
            args.repo_root,
            args.output_dir,
            platform,
            args.arch,
            args.python,
            args.force,
            args.dry_run,
        )
    except Exception as exc:
        print(f"[wheelhouse] error: {exc}", file=sys.stderr)
        return 1
    print(f"[wheelhouse] prepared {len(prepared)} wheel(s) in {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
