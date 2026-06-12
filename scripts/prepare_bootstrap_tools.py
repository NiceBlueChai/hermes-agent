#!/usr/bin/env python3
"""Prepare portable tool archives for the Tauri bootstrap installer bundle.

The installer can use archives placed under
`apps/bootstrap-installer/src-tauri/bootstrap-tools` before it falls back to
network downloads. This helper is intended for release workflows, not for the
runtime installer path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = REPO_ROOT / "apps" / "bootstrap-installer" / "src-tauri" / "bootstrap-tools"
NODE_MAJOR = 22
USER_AGENT = "Hermes-Setup"
NODE_INDEX_URL = f"https://nodejs.org/dist/latest-v{NODE_MAJOR}.x/"
RIPGREP_VERSION = "15.1.0"
GIT_TAG = "v2.54.0.windows.1"
GIT_VERSION = "2.54.0"
MANIFEST_NAME = "bootstrap-tools-manifest.json"

UV_ARCHIVE_NAMES = {
    "x64": "uv-x86_64-pc-windows-msvc.zip",
    "arm64": "uv-aarch64-pc-windows-msvc.zip",
    "x86": "uv-i686-pc-windows-msvc.zip",
}

UNIX_UV_ARCHIVE_NAMES = {
    ("linux", "x64"): "uv-x86_64-unknown-linux-gnu.tar.gz",
    ("linux", "arm64"): "uv-aarch64-unknown-linux-gnu.tar.gz",
    ("macos", "x64"): "uv-x86_64-apple-darwin.tar.gz",
    ("macos", "arm64"): "uv-aarch64-apple-darwin.tar.gz",
}

GIT_ARCHIVE_NAMES = {
    "x64": "PortableGit-2.54.0-64-bit.7z.exe",
    "arm64": "PortableGit-2.54.0-arm64.7z.exe",
    "x86": "MinGit-2.54.0-32-bit.zip",
}

RIPGREP_ARCHIVE_NAMES = {
    "x64": f"ripgrep-{RIPGREP_VERSION}-x86_64-pc-windows-msvc.zip",
    "arm64": f"ripgrep-{RIPGREP_VERSION}-aarch64-pc-windows-msvc.zip",
    "x86": f"ripgrep-{RIPGREP_VERSION}-i686-pc-windows-msvc.zip",
}

UNIX_RIPGREP_ARCHIVE_NAMES = {
    ("linux", "x64"): f"ripgrep-{RIPGREP_VERSION}-x86_64-unknown-linux-musl.tar.gz",
    ("linux", "arm64"): f"ripgrep-{RIPGREP_VERSION}-aarch64-unknown-linux-gnu.tar.gz",
    ("macos", "x64"): f"ripgrep-{RIPGREP_VERSION}-x86_64-apple-darwin.tar.gz",
    ("macos", "arm64"): f"ripgrep-{RIPGREP_VERSION}-aarch64-apple-darwin.tar.gz",
}


@dataclass(frozen=True)
class ArchiveSpec:
    """One release archive that should be copied into the Tauri resource dir."""

    name: str
    url: str


@dataclass(frozen=True)
class PreparedArchive:
    """One downloaded archive with audit metadata for the release manifest."""

    platform: str
    arch: str
    name: str
    url: str
    path: Path
    size_bytes: int
    sha256: str


def select_latest_node_archive(index_html: str, arch: str, major: int = NODE_MAJOR) -> str:
    """Return the newest Node.js Windows archive name for one architecture."""

    pattern = re.compile(rf"node-v({major})\.(\d+)\.(\d+)-win-{re.escape(arch)}\.zip")
    matches: list[tuple[tuple[int, int, int], str]] = []
    for match in pattern.finditer(index_html):
        version = (int(match.group(1)), int(match.group(2)), int(match.group(3)))
        matches.append((version, match.group(0)))
    if not matches:
        raise ValueError(f"Node.js v{major} Windows {arch} archive not found")
    return max(matches, key=lambda item: item[0])[1]


def select_latest_unix_node_archive(
    index_html: str,
    node_os: str,
    arch: str,
    major: int = NODE_MAJOR,
) -> str:
    """Return the newest Node.js Unix tarball name, preferring gzip over xz."""

    for extension in ("tar.gz", "tar.xz"):
        pattern = re.compile(
            rf"node-v({major})\.(\d+)\.(\d+)-{re.escape(node_os)}-{re.escape(arch)}\.{extension}"
        )
        matches: list[tuple[tuple[int, int, int], str]] = []
        for match in pattern.finditer(index_html):
            version = (int(match.group(1)), int(match.group(2)), int(match.group(3)))
            matches.append((version, match.group(0)))
        if matches:
            return max(matches, key=lambda item: item[0])[1]
    raise ValueError(f"Node.js v{major} {node_os}-{arch} archive not found")


def archive_specs_for_arch(arch: str, node_archive_name: str) -> list[ArchiveSpec]:
    """Build the archive list that matches the Rust installer runtime matrix."""

    if arch not in UV_ARCHIVE_NAMES or arch not in GIT_ARCHIVE_NAMES or arch not in RIPGREP_ARCHIVE_NAMES:
        raise ValueError(f"unsupported Windows architecture: {arch}")
    return [
        ArchiveSpec(
            name=node_archive_name,
            url=f"{NODE_INDEX_URL}{node_archive_name}",
        ),
        ArchiveSpec(
            name=UV_ARCHIVE_NAMES[arch],
            url=f"https://github.com/astral-sh/uv/releases/latest/download/{UV_ARCHIVE_NAMES[arch]}",
        ),
        ArchiveSpec(
            name=RIPGREP_ARCHIVE_NAMES[arch],
            url=(
                "https://github.com/BurntSushi/ripgrep/releases/download/"
                f"{RIPGREP_VERSION}/{RIPGREP_ARCHIVE_NAMES[arch]}"
            ),
        ),
        ArchiveSpec(
            name=GIT_ARCHIVE_NAMES[arch],
            url=f"https://github.com/git-for-windows/git/releases/download/{GIT_TAG}/{GIT_ARCHIVE_NAMES[arch]}",
        ),
    ]


def archive_specs_for_target(
    platform: str,
    arch: str,
    node_archive_name: str | None = None,
) -> list[ArchiveSpec]:
    """Build archive specs for one release platform and architecture."""

    normalized_platform = "macos" if platform == "darwin" else platform
    if normalized_platform == "windows":
        if node_archive_name is None:
            raise ValueError("Windows bootstrap tools require a Node.js archive name")
        return archive_specs_for_arch(arch, node_archive_name)

    archive_name = UNIX_UV_ARCHIVE_NAMES.get((normalized_platform, arch))
    ripgrep_archive_name = UNIX_RIPGREP_ARCHIVE_NAMES.get((normalized_platform, arch))
    if archive_name is None or ripgrep_archive_name is None:
        raise ValueError(f"unsupported Unix uv platform: {normalized_platform}-{arch}")
    if node_archive_name is None:
        raise ValueError("Unix bootstrap tools require a Node.js archive name")
    return [
        ArchiveSpec(
            name=node_archive_name,
            url=f"{NODE_INDEX_URL}{node_archive_name}",
        ),
        ArchiveSpec(
            name=archive_name,
            url=f"https://github.com/astral-sh/uv/releases/latest/download/{archive_name}",
        ),
        ArchiveSpec(
            name=ripgrep_archive_name,
            url=(
                "https://github.com/BurntSushi/ripgrep/releases/download/"
                f"{RIPGREP_VERSION}/{ripgrep_archive_name}"
            ),
        ),
    ]


def fetch_text(url: str) -> str:
    """Fetch a small text resource using the release helper user agent."""

    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read().decode("utf-8")


def download_archive(spec: ArchiveSpec, output_dir: Path, force: bool) -> Path:
    """Download one archive atomically unless a non-empty file already exists."""

    output_dir.mkdir(parents=True, exist_ok=True)
    dest = output_dir / spec.name
    if dest.is_file() and dest.stat().st_size > 0 and not force:
        print(f"[bootstrap-tools] keep {dest}")
        return dest

    tmp = dest.with_name(f"{dest.name}.tmp")
    tmp.unlink(missing_ok=True)
    print(f"[bootstrap-tools] download {spec.url}")
    request = urllib.request.Request(spec.url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=600) as response:
        with tmp.open("wb") as handle:
            shutil.copyfileobj(response, handle)
    if tmp.stat().st_size == 0:
        tmp.unlink(missing_ok=True)
        raise RuntimeError(f"downloaded empty archive: {spec.url}")
    os.replace(tmp, dest)
    return dest


def sha256_file(path: Path) -> str:
    """Return the SHA-256 hex digest for one archive file."""

    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def archive_target_from_name(name: str) -> tuple[str, str] | None:
    """Infer the release platform and architecture from a known archive name."""

    node_suffixes = {
        "-win-x64.zip": ("windows", "x64"),
        "-win-arm64.zip": ("windows", "arm64"),
        "-win-x86.zip": ("windows", "x86"),
        "-linux-x64.tar.gz": ("linux", "x64"),
        "-linux-arm64.tar.gz": ("linux", "arm64"),
        "-linux-x64.tar.xz": ("linux", "x64"),
        "-linux-arm64.tar.xz": ("linux", "arm64"),
        "-darwin-x64.tar.gz": ("macos", "x64"),
        "-darwin-arm64.tar.gz": ("macos", "arm64"),
        "-darwin-x64.tar.xz": ("macos", "x64"),
        "-darwin-arm64.tar.xz": ("macos", "arm64"),
    }
    if name.startswith("node-v"):
        for suffix, target in node_suffixes.items():
            if name.endswith(suffix):
                return target
    known_targets = {
        "uv-x86_64-pc-windows-msvc.zip": ("windows", "x64"),
        "uv-aarch64-pc-windows-msvc.zip": ("windows", "arm64"),
        "uv-i686-pc-windows-msvc.zip": ("windows", "x86"),
        "uv-x86_64-unknown-linux-gnu.tar.gz": ("linux", "x64"),
        "uv-aarch64-unknown-linux-gnu.tar.gz": ("linux", "arm64"),
        "uv-x86_64-apple-darwin.tar.gz": ("macos", "x64"),
        "uv-aarch64-apple-darwin.tar.gz": ("macos", "arm64"),
        f"ripgrep-{RIPGREP_VERSION}-x86_64-pc-windows-msvc.zip": ("windows", "x64"),
        f"ripgrep-{RIPGREP_VERSION}-aarch64-pc-windows-msvc.zip": ("windows", "arm64"),
        f"ripgrep-{RIPGREP_VERSION}-i686-pc-windows-msvc.zip": ("windows", "x86"),
        f"ripgrep-{RIPGREP_VERSION}-x86_64-unknown-linux-musl.tar.gz": ("linux", "x64"),
        f"ripgrep-{RIPGREP_VERSION}-aarch64-unknown-linux-gnu.tar.gz": ("linux", "arm64"),
        f"ripgrep-{RIPGREP_VERSION}-x86_64-apple-darwin.tar.gz": ("macos", "x64"),
        f"ripgrep-{RIPGREP_VERSION}-aarch64-apple-darwin.tar.gz": ("macos", "arm64"),
        "PortableGit-2.54.0-64-bit.7z.exe": ("windows", "x64"),
        "PortableGit-2.54.0-arm64.7z.exe": ("windows", "arm64"),
        "MinGit-2.54.0-32-bit.zip": ("windows", "x86"),
        "ffmpeg-windows-x64.zip": ("windows", "x64"),
        "ffmpeg-windows-arm64.zip": ("windows", "arm64"),
        "ffmpeg-windows-x86.zip": ("windows", "x86"),
        "ffmpeg-linux-x64.tar.gz": ("linux", "x64"),
        "ffmpeg-linux-arm64.tar.gz": ("linux", "arm64"),
        "ffmpeg-macos-x64.tar.gz": ("macos", "x64"),
        "ffmpeg-macos-arm64.tar.gz": ("macos", "arm64"),
        "playwright-browsers-windows-x64.zip": ("windows", "x64"),
        "playwright-browsers-windows-arm64.zip": ("windows", "arm64"),
        "playwright-browsers-windows-x86.zip": ("windows", "x86"),
        "playwright-browsers-linux-x64.tar.gz": ("linux", "x64"),
        "playwright-browsers-linux-arm64.tar.gz": ("linux", "arm64"),
        "playwright-browsers-macos-x64.tar.gz": ("macos", "x64"),
        "playwright-browsers-macos-arm64.tar.gz": ("macos", "arm64"),
    }
    return known_targets.get(name)


def archive_tool_kind_from_name(name: str) -> str | None:
    """Infer which runtime tool one bootstrap archive provides."""

    if name.startswith("node-v"):
        return "node"
    if name.startswith("uv-"):
        return "uv"
    if name.startswith(f"ripgrep-{RIPGREP_VERSION}-"):
        return "ripgrep"
    if name.startswith(("PortableGit-", "MinGit-")):
        return "git"
    if name.startswith("ffmpeg-"):
        return "ffmpeg"
    if name.startswith("playwright-browsers-"):
        return "playwright-browsers"
    return None


def required_tool_kinds_for_target(platform: str, arch: str) -> set[str]:
    """Return runtime tool kinds that must be bundled for one release target."""

    normalized_platform = "macos" if platform == "darwin" else platform
    if normalized_platform == "windows":
        node_archive_name = f"node-v22.0.0-win-{arch}.zip"
    else:
        node_os = "darwin" if normalized_platform == "macos" else normalized_platform
        node_archive_name = f"node-v22.0.0-{node_os}-{arch}.tar.gz"
    specs = archive_specs_for_target(normalized_platform, arch, node_archive_name)
    return {kind for spec in specs if (kind := archive_tool_kind_from_name(spec.name)) is not None}


def prepared_archive_record(platform: str, arch: str, spec: ArchiveSpec, path: Path) -> PreparedArchive:
    """Build manifest metadata for one downloaded archive."""

    return PreparedArchive(
        platform=platform,
        arch=arch,
        name=spec.name,
        url=spec.url,
        path=path,
        size_bytes=path.stat().st_size,
        sha256=sha256_file(path),
    )


def parse_local_archive_arg(value: str) -> tuple[Path, ArchiveSpec, tuple[str, str]]:
    """Parse one maintainer-provided local archive mapping."""

    source_text, separator, url = value.partition("=")
    if not separator or not source_text or not url:
        raise ValueError("local archive must use PATH=HTTPS_URL")
    if not url.startswith("https://"):
        raise ValueError("local archive URL must be HTTPS")
    source = Path(source_text)
    name = source.name
    if Path(name).name != name or name in {".", ".."}:
        raise ValueError(f"local archive has unsafe name: {name}")
    target = archive_target_from_name(name)
    if target is None or archive_tool_kind_from_name(name) is None:
        raise ValueError(f"local archive name is not a recognized bootstrap tool: {name}")
    return source, ArchiveSpec(name=name, url=url), target


def parse_audited_archive_arg(value: str) -> tuple[ArchiveSpec, tuple[str, str], str]:
    """Parse one explicitly checksummed archive download mapping."""

    parts = value.split("=", 2)
    if len(parts) != 3 or not all(parts):
        raise ValueError("audited archive must use NAME=HTTPS_URL=SHA256")
    name, url, expected_sha256 = parts
    if Path(name).name != name or name in {".", ".."}:
        raise ValueError(f"audited archive has unsafe name: {name}")
    if not url.startswith("https://"):
        raise ValueError("audited archive URL must be HTTPS")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise ValueError(f"audited archive has invalid sha256: {name}")
    target = archive_target_from_name(name)
    if target is None or archive_tool_kind_from_name(name) is None:
        raise ValueError(f"audited archive name is not a recognized bootstrap tool: {name}")
    return ArchiveSpec(name=name, url=url), target, expected_sha256.lower()


def prepare_local_archives(output_dir: Path, local_archives: list[str], dry_run: bool) -> list[PreparedArchive]:
    """Copy maintainer-provided local archives into the release resource directory."""

    prepared: list[PreparedArchive] = []
    output_dir.mkdir(parents=True, exist_ok=True)
    for value in local_archives:
        source, spec, (platform, arch) = parse_local_archive_arg(value)
        if dry_run:
            print(f"[bootstrap-tools] would copy {source} as {spec.name} <- {spec.url}")
            continue
        if not source.is_file() or source.stat().st_size <= 0:
            raise RuntimeError(f"local archive is missing or empty: {source}")
        dest = output_dir / spec.name
        if source.resolve() != dest.resolve():
            shutil.copy2(source, dest)
        prepared.append(prepared_archive_record(platform, arch, spec, dest))
    return prepared


def prepare_audited_archives(
    output_dir: Path,
    audited_archives: list[str],
    force: bool,
    dry_run: bool,
) -> list[PreparedArchive]:
    """Download explicitly checksummed optional archives into the release resource directory."""

    prepared: list[PreparedArchive] = []
    output_dir.mkdir(parents=True, exist_ok=True)
    for value in audited_archives:
        spec, (platform, arch), expected_sha256 = parse_audited_archive_arg(value)
        if dry_run:
            print(f"[bootstrap-tools] would download audited {spec.name} <- {spec.url}")
            continue
        path = download_archive(spec, output_dir, force)
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256:
            raise RuntimeError(
                f"audited archive checksum mismatch for {spec.name}: "
                f"expected {expected_sha256}, got {actual_sha256}"
            )
        prepared.append(prepared_archive_record(platform, arch, spec, path))
    return prepared


def write_manifest(output_dir: Path, archives: list[PreparedArchive]) -> Path:
    """Write the bundled tool archive manifest consumed by release reviewers."""

    payload = {
        "schemaVersion": 1,
        "generatedAtUtc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "archives": [
            {
                "arch": archive.arch,
                "platform": archive.platform,
                "name": archive.name,
                "url": archive.url,
                "sizeBytes": archive.size_bytes,
                "sha256": archive.sha256,
            }
            for archive in archives
        ],
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = output_dir / MANIFEST_NAME
    manifest_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest_path


def validate_manifest(
    output_dir: Path,
    expected_platform: str | None = None,
    expected_arch: str | None = None,
) -> int:
    """Validate that the tool manifest matches archives in the output directory."""

    manifest_path = output_dir / MANIFEST_NAME
    if not manifest_path.is_file():
        raise RuntimeError(f"missing bootstrap tools manifest: {manifest_path}")
    payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    if payload.get("schemaVersion") != 1:
        raise RuntimeError(f"unsupported bootstrap tools manifest schema: {payload.get('schemaVersion')}")
    archives = payload.get("archives")
    if not isinstance(archives, list) or not archives:
        raise RuntimeError("bootstrap tools manifest has no archives")

    seen_names: set[str] = set()
    seen_tool_kinds: set[str] = set()
    for archive in archives:
        name = archive.get("name")
        if not isinstance(name, str) or not name:
            raise RuntimeError("bootstrap tools manifest archive is missing name")
        if Path(name).name != name or name in {".", ".."}:
            raise RuntimeError(f"manifest archive has unsafe archive name: {name}")
        if name in seen_names:
            raise RuntimeError(f"duplicate archive in bootstrap tools manifest: {name}")
        seen_names.add(name)
        arch = archive.get("arch")
        if not isinstance(arch, str) or not arch:
            raise RuntimeError(f"manifest archive is missing arch: {name}")
        if expected_arch is not None and arch != expected_arch:
            raise RuntimeError(f"unexpected bootstrap tools arch for {name}: expected {expected_arch}, got {arch}")
        platform = archive.get("platform")
        if platform not in {"windows", "linux", "macos"}:
            raise RuntimeError(f"manifest archive is missing platform: {name}")
        if expected_platform is not None and platform != expected_platform:
            raise RuntimeError(
                f"unexpected bootstrap tools platform for {name}: expected {expected_platform}, got {platform}"
            )
        target = archive_target_from_name(name)
        if target is not None and target != (platform, arch):
            raise RuntimeError(f"manifest archive target mismatch: {name}")
        tool_kind = archive_tool_kind_from_name(name)
        if tool_kind is not None:
            seen_tool_kinds.add(tool_kind)
        url = archive.get("url")
        if not isinstance(url, str) or not url:
            raise RuntimeError(f"manifest archive is missing url: {name}")
        if not url.startswith("https://"):
            raise RuntimeError(f"manifest archive has invalid url: {name}")
        path = output_dir / name
        if not path.is_file():
            raise RuntimeError(f"manifest archive is missing: {path}")
        expected_size = archive.get("sizeBytes")
        if not isinstance(expected_size, int) or expected_size <= 0:
            raise RuntimeError(f"manifest archive has invalid sizeBytes: {name}")
        actual_size = path.stat().st_size
        if actual_size != expected_size:
            raise RuntimeError(
                f"archive size mismatch for {name}: expected {expected_size}, got {actual_size}"
            )
        expected_sha256 = archive.get("sha256")
        if not isinstance(expected_sha256, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
            raise RuntimeError(f"manifest archive has invalid sha256: {name}")
        actual_sha256 = sha256_file(path)
        if actual_sha256.lower() != expected_sha256.lower():
            raise RuntimeError(
                f"archive checksum mismatch for {name}: expected {expected_sha256}, got {actual_sha256}"
            )
    if expected_platform is not None and expected_arch is not None:
        missing_kinds = sorted(
            required_tool_kinds_for_target(expected_platform, expected_arch) - seen_tool_kinds
        )
        if missing_kinds:
            raise RuntimeError(f"missing required bootstrap tool archive: {', '.join(missing_kinds)}")
    return len(archives)


def prepare_archives(
    output_dir: Path,
    arches: list[str],
    force: bool,
    dry_run: bool,
    platform: str = "windows",
    local_archives: list[str] | None = None,
    audited_archives: list[str] | None = None,
) -> list[PreparedArchive]:
    """Resolve and optionally download all archives for the requested architectures."""

    normalized_platform = "macos" if platform == "darwin" else platform
    index_html = fetch_text(NODE_INDEX_URL)
    downloaded: list[PreparedArchive] = []
    for arch in arches:
        if normalized_platform == "windows":
            node_archive = select_latest_node_archive(index_html, arch)
        else:
            node_os = "darwin" if normalized_platform == "macos" else normalized_platform
            node_archive = select_latest_unix_node_archive(index_html, node_os, arch)
        for spec in archive_specs_for_target(normalized_platform, arch, node_archive):
            if dry_run:
                print(f"[bootstrap-tools] would download {spec.name} <- {spec.url}")
            else:
                path = download_archive(spec, output_dir, force)
                downloaded.append(prepared_archive_record(normalized_platform, arch, spec, path))
    downloaded.extend(prepare_local_archives(output_dir, local_archives or [], dry_run))
    downloaded.extend(prepare_audited_archives(output_dir, audited_archives or [], force, dry_run))
    if downloaded:
        manifest_path = write_manifest(output_dir, downloaded)
        print(f"[bootstrap-tools] wrote manifest {manifest_path}")
    return downloaded


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line options for release automation."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--platform",
        choices=("windows", "linux", "macos", "darwin"),
        default="windows",
        help="Release platform to bundle tools for. Defaults to windows.",
    )
    parser.add_argument(
        "--arch",
        action="append",
        choices=sorted(set(UV_ARCHIVE_NAMES) | {"x64", "arm64"}),
        default=None,
        help="Architecture to bundle. Can be passed more than once. Defaults to x64.",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help="Directory copied by tauri.conf.json bundle.resources.",
    )
    parser.add_argument("--force", action="store_true", help="Re-download archives that already exist.")
    parser.add_argument("--dry-run", action="store_true", help="Print the planned archive URLs without downloading.")
    parser.add_argument("--validate-only", action="store_true", help="Validate an existing bootstrap tools manifest.")
    parser.add_argument(
        "--local-archive",
        action="append",
        default=None,
        help="Copy an audited local archive into the manifest, in PATH=HTTPS_URL form.",
    )
    parser.add_argument(
        "--audited-archive",
        action="append",
        default=None,
        help="Download an explicitly checksummed archive into the manifest, in NAME=HTTPS_URL=SHA256 form.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the archive preparation helper."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    arches = args.arch or ["x64"]
    try:
        if args.validate_only:
            count = validate_manifest(args.output_dir, args.platform, arches[0] if len(arches) == 1 else None)
            print(f"[bootstrap-tools] validated {count} archive(s) in {args.output_dir}")
            return 0
        prepared = prepare_archives(
            args.output_dir,
            arches,
            args.force,
            args.dry_run,
            args.platform,
            args.local_archive,
            args.audited_archive,
        )
    except Exception as exc:
        print(f"[bootstrap-tools] error: {exc}", file=sys.stderr)
        return 1
    print(f"[bootstrap-tools] prepared {len(prepared)} archive(s) in {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
