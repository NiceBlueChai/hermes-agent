"""Validate the release gate for promoting bundled Python runtime archives to default."""

from __future__ import annotations

import argparse
import glob
import json
import re
from pathlib import Path


REQUIRED_MARKERS = {
    "current decision": "Default inclusion: not enabled.",
    "fallback retention": "uv python install 3.11",
    "size gate heading": "## Size Gate",
    "per-artifact budget": "--max-artifact-bytes 536870912",
    "total budget": "--max-total-artifact-bytes 536870912",
    "release manifest audit": (
        "actual `python-runtime-manifest.json` size, SHA-256, Python tag, platform, and arch"
    ),
    "signed installer size comparison": "compare the signed installer size with and without the runtime bundle",
    "signed installer platform": "signedInstaller.platform",
    "signed installer release identity": "signedInstaller.release",
    "security gate heading": "## Security-Update Gate",
    "checksum-pinned source": "HTTPS and checksum-pinned",
    "security rebuild policy": "must be rebuilt when the bundled Python patch release receives a security update",
    "release notes runtime source": (
        "identify the Python runtime version and the archive source used for the signed build"
    ),
    "smoke gate heading": "## Windows x64 Smoke Gate",
    "resource self-check": "--self-check",
    "lifecycle self-check": "--self-check-lifecycle",
    "artifact validator": "scripts/validate_installer_artifacts.py",
    "structured release evidence": "python scripts/validate_python_runtime_default_gate.py --evidence",
    "generated release evidence": "--print-evidence",
    "required platform evidence set": "--require-platforms",
}

HEX_SHA256_RE = re.compile(r"[0-9a-fA-F]{64}")
HEX_COMMIT_RE = re.compile(r"[0-9a-fA-F]{40}")
GITHUB_RELEASE_TAG_RE = re.compile(r"^https://github\.com/([^/\s]+)/([^/\s]+)/releases/tag/([^/\s]+)$")
PLATFORM_SIGNATURES = {
    "windows": "authenticode",
    "macos": "developer-id-notarized",
    "linux": "sigstore",
}
DEFAULT_RUNTIME_MANIFEST = Path(
    "apps/bootstrap-installer/src-tauri/python-runtime/python-runtime-manifest.json"
)
DEFAULT_SECURITY_UPDATE_POLICY = (
    "Runtime archive must be rebuilt when the bundled Python patch release receives a security update."
)


def validate_default_gate_doc(path: Path) -> None:
    """Raise RuntimeError when the Python runtime default gate document is incomplete."""
    text = path.read_text(encoding="utf-8")
    missing = [label for label, marker in REQUIRED_MARKERS.items() if marker not in text]
    if missing:
        details = ", ".join(missing)
        raise RuntimeError(f"python runtime default gate is missing required marker(s): {details}")


def require_mapping(value: object, label: str) -> dict:
    """Return a JSON object or raise a release-evidence validation error."""
    if not isinstance(value, dict):
        raise RuntimeError(f"python runtime release evidence field must be an object: {label}")
    return value


def require_non_empty_string(value: object, label: str) -> str:
    """Return a non-empty string release-evidence field."""
    if not isinstance(value, str) or not value.strip():
        raise RuntimeError(f"python runtime release evidence field must be a non-empty string: {label}")
    return value


def require_positive_int(value: object, label: str) -> int:
    """Return a positive integer release-evidence field."""
    if type(value) is not int or value <= 0:
        raise RuntimeError(f"python runtime release evidence field must be a positive integer: {label}")
    return value


def github_release_tag(url: str) -> str | None:
    """Return the GitHub release tag segment when the URL has the expected release shape."""
    match = GITHUB_RELEASE_TAG_RE.fullmatch(url)
    if match is None:
        return None
    return match.group(3)


def github_release_repo(url: str) -> str | None:
    """Return OWNER/REPO when the URL has the expected GitHub release shape."""
    match = GITHUB_RELEASE_TAG_RE.fullmatch(url)
    if match is None:
        return None
    return f"{match.group(1)}/{match.group(2)}"


def validate_release_evidence_payload(payload: object) -> None:
    """Validate structured evidence before the bundled Python runtime can become default."""
    root = require_mapping(payload, "root")
    runtime = require_mapping(root.get("pythonRuntime"), "pythonRuntime")
    installer = require_mapping(root.get("signedInstaller"), "signedInstaller")
    manifest = require_mapping(runtime.get("manifest"), "pythonRuntime.manifest")

    version = require_non_empty_string(runtime.get("version"), "pythonRuntime.version")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[a-zA-Z0-9.+-]*)?", version):
        raise RuntimeError("pythonRuntime.version must identify a Python patch release")
    source_url = require_non_empty_string(runtime.get("sourceUrl"), "pythonRuntime.sourceUrl")
    if not source_url.startswith("https://"):
        raise RuntimeError("pythonRuntime.sourceUrl must be HTTPS")
    archive_sha256 = require_non_empty_string(runtime.get("archiveSha256"), "pythonRuntime.archiveSha256")
    if not HEX_SHA256_RE.fullmatch(archive_sha256):
        raise RuntimeError("pythonRuntime.archiveSha256 must be a SHA-256 hex digest")
    security_policy = require_non_empty_string(
        runtime.get("securityUpdatePolicy"),
        "pythonRuntime.securityUpdatePolicy",
    )
    normalized_policy = security_policy.lower()
    if "security update" not in normalized_policy or "rebuil" not in normalized_policy:
        raise RuntimeError("pythonRuntime.securityUpdatePolicy must require rebuilds after security updates")

    platform = require_non_empty_string(manifest.get("platform"), "pythonRuntime.manifest.platform")
    if platform not in {"windows", "linux", "macos"}:
        raise RuntimeError("pythonRuntime.manifest.platform must be windows, linux, or macos")
    installer_platform = require_non_empty_string(installer.get("platform"), "signedInstaller.platform")
    if installer_platform not in {"windows", "linux", "macos"}:
        raise RuntimeError("signedInstaller.platform must be windows, linux, or macos")
    if installer_platform != platform:
        raise RuntimeError("signedInstaller.platform must match pythonRuntime.manifest.platform")
    release = require_non_empty_string(installer.get("release"), "signedInstaller.release")
    if release != release.strip() or "/" in release:
        raise RuntimeError("signedInstaller.release must be a single GitHub tag segment")
    release_url = require_non_empty_string(installer.get("url"), "signedInstaller.url")
    release_notes = require_non_empty_string(installer.get("releaseNotes"), "signedInstaller.releaseNotes")
    if not release_url.startswith("https://"):
        raise RuntimeError("signedInstaller.url must be HTTPS")
    if not release_notes.startswith("https://"):
        raise RuntimeError("signedInstaller.releaseNotes must be HTTPS")
    if github_release_tag(release_url) != release:
        raise RuntimeError("signedInstaller.url must be a GitHub release tag URL for signedInstaller.release")
    if github_release_tag(release_notes) != release:
        raise RuntimeError("signedInstaller.releaseNotes must be a GitHub release tag URL for signedInstaller.release")
    if github_release_repo(release_notes) != github_release_repo(release_url):
        raise RuntimeError("signedInstaller.releaseNotes must reference the same GitHub repository")
    if release == "vX.Y.Z" or github_release_repo(release_url) == "OWNER/REPO":
        raise RuntimeError("signedInstaller release identity must replace placeholder values")
    commit = require_non_empty_string(installer.get("commit"), "signedInstaller.commit")
    if not HEX_COMMIT_RE.fullmatch(commit):
        raise RuntimeError("signedInstaller.commit must be a 40-character commit SHA")
    signature = require_non_empty_string(installer.get("signature"), "signedInstaller.signature")
    if signature != PLATFORM_SIGNATURES[platform]:
        raise RuntimeError("signedInstaller.signature must match signedInstaller.platform")
    arch = require_non_empty_string(manifest.get("arch"), "pythonRuntime.manifest.arch")
    if arch not in {"x64", "arm64"}:
        raise RuntimeError("pythonRuntime.manifest.arch must be x64 or arm64")
    python_tag = require_non_empty_string(manifest.get("pythonTag"), "pythonRuntime.manifest.pythonTag")
    if not re.fullmatch(r"cp\d+", python_tag):
        raise RuntimeError("pythonRuntime.manifest.pythonTag must be a CPython ABI tag")

    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("pythonRuntime.manifest.files must contain at least one file")
    for index, file_entry in enumerate(files):
        file_payload = require_mapping(file_entry, f"pythonRuntime.manifest.files[{index}]")
        name = require_non_empty_string(file_payload.get("name"), f"pythonRuntime.manifest.files[{index}].name")
        if name != name.strip() or "/" in name or "\\" in name or name in {".", ".."}:
            raise RuntimeError(f"pythonRuntime.manifest.files[{index}].name must be a plain file name")
        file_url = require_non_empty_string(file_payload.get("url"), f"pythonRuntime.manifest.files[{index}].url")
        if not file_url.startswith("https://"):
            raise RuntimeError(f"pythonRuntime.manifest.files[{index}].url must be HTTPS")
        require_positive_int(file_payload.get("sizeBytes"), f"pythonRuntime.manifest.files[{index}].sizeBytes")
        file_sha256 = require_non_empty_string(
            file_payload.get("sha256"),
            f"pythonRuntime.manifest.files[{index}].sha256",
        )
        if not HEX_SHA256_RE.fullmatch(file_sha256):
            raise RuntimeError(f"pythonRuntime.manifest.files[{index}].sha256 must be a SHA-256 hex digest")
    if not any(file_entry.get("url") == source_url for file_entry in files if isinstance(file_entry, dict)):
        raise RuntimeError("pythonRuntime.sourceUrl must match a manifest file URL")
    if not any(file_entry.get("sha256") == archive_sha256 for file_entry in files if isinstance(file_entry, dict)):
        raise RuntimeError("pythonRuntime.archiveSha256 must match a manifest file SHA-256")

    with_runtime = require_positive_int(installer.get("withRuntimeBytes"), "signedInstaller.withRuntimeBytes")
    without_runtime = require_positive_int(
        installer.get("withoutRuntimeBytes"),
        "signedInstaller.withoutRuntimeBytes",
    )
    size_delta = installer.get("sizeDeltaBytes")
    if type(size_delta) is not int:
        raise RuntimeError("signedInstaller.sizeDeltaBytes must be an integer")
    expected_delta = with_runtime - without_runtime
    if size_delta != expected_delta:
        raise RuntimeError(
            "signedInstaller.sizeDeltaBytes must equal withRuntimeBytes minus withoutRuntimeBytes"
        )
    if size_delta <= 0:
        raise RuntimeError("signedInstaller.sizeDeltaBytes must be positive")


def load_release_evidence(path: Path) -> dict:
    """Load and validate one structured release evidence file."""
    payload = json.loads(path.read_text(encoding="utf-8"))
    validate_release_evidence_payload(payload)
    return require_mapping(payload, "root")


def validate_release_evidence(path: Path) -> None:
    """Validate structured release evidence read from a JSON file."""
    load_release_evidence(path)


def validate_release_evidence_files(
    paths: list[Path],
    required_platforms: tuple[str, ...] = (),
) -> None:
    """Validate one or more runtime evidence files and an optional required platform set."""
    payloads = [load_release_evidence(path) for path in paths]
    if not required_platforms:
        return

    required = set(required_platforms)
    unknown_platforms = required - set(PLATFORM_SIGNATURES)
    if not required or unknown_platforms:
        details = ", ".join(sorted(unknown_platforms or required))
        raise RuntimeError(f"unknown required platform(s): {details}")

    installers: dict[str, dict] = {}
    for payload in payloads:
        installer = require_mapping(payload.get("signedInstaller"), "signedInstaller")
        platform = require_non_empty_string(installer.get("platform"), "signedInstaller.platform")
        if platform in installers:
            raise RuntimeError(f"duplicate runtime default evidence for platform: {platform}")
        installers[platform] = installer

    missing = required - set(installers)
    if missing:
        details = ", ".join(sorted(missing))
        raise RuntimeError(f"missing runtime default evidence for platform(s): {details}")

    for field in ("release", "url", "releaseNotes", "commit"):
        values = {installers[platform].get(field) for platform in required}
        if len(values) != 1:
            raise RuntimeError("runtime default evidence must share one signed release and commit")


def build_release_evidence(
    manifest: dict,
    python_version: str,
    security_update_policy: str,
    with_runtime_bytes: int,
    without_runtime_bytes: int,
    release: str,
    release_url: str,
    release_notes: str,
    commit: str,
    signature: str,
) -> dict:
    """Build validated structured evidence from a runtime manifest and installer sizes."""
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        raise RuntimeError("python runtime manifest files must contain at least one archive")
    first_file = require_mapping(files[0], "pythonRuntime.manifest.files[0]")
    evidence = {
        "pythonRuntime": {
            "version": python_version,
            "sourceUrl": first_file.get("url"),
            "archiveSha256": first_file.get("sha256"),
            "securityUpdatePolicy": security_update_policy,
            "manifest": manifest,
        },
        "signedInstaller": {
            "platform": manifest.get("platform"),
            "release": release,
            "url": release_url,
            "releaseNotes": release_notes,
            "commit": commit,
            "signature": signature,
            "withRuntimeBytes": with_runtime_bytes,
            "withoutRuntimeBytes": without_runtime_bytes,
            "sizeDeltaBytes": with_runtime_bytes - without_runtime_bytes,
        },
    }
    validate_release_evidence_payload(evidence)
    return evidence


def resolve_single_artifact(pattern: str) -> Path:
    """Resolve a literal or glob artifact pattern to exactly one file."""
    matches = [Path(match) for match in glob.glob(pattern)]
    if not matches and Path(pattern).is_file():
        matches = [Path(pattern)]
    files = [path for path in matches if path.is_file()]
    if len(files) != 1:
        raise RuntimeError(f"expected exactly one signed installer artifact for pattern: {pattern}")
    return files[0]


def print_release_evidence(args: argparse.Namespace) -> None:
    """Print generated Python runtime default evidence JSON to stdout."""
    if not args.python_version:
        raise RuntimeError("--python-version is required with --print-evidence")
    if args.with_runtime_artifact is None:
        raise RuntimeError("--with-runtime-artifact is required with --print-evidence")
    if args.without_runtime_bytes is None:
        raise RuntimeError("--without-runtime-bytes is required with --print-evidence")
    for attr, flag in (
        ("release", "--release"),
        ("url", "--url"),
        ("release_notes", "--release-notes"),
        ("commit", "--commit"),
        ("signature", "--signature"),
    ):
        if not getattr(args, attr):
            raise RuntimeError(f"{flag} is required with --print-evidence")

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    artifact = resolve_single_artifact(args.with_runtime_artifact)
    evidence = build_release_evidence(
        manifest,
        python_version=args.python_version,
        security_update_policy=args.security_update_policy,
        with_runtime_bytes=artifact.stat().st_size,
        without_runtime_bytes=args.without_runtime_bytes,
        release=args.release,
        release_url=args.url,
        release_notes=args.release_notes,
        commit=args.commit,
        signature=args.signature,
    )
    print(json.dumps(evidence, indent=2, sort_keys=True))


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments for the Python runtime default gate validator."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--doc",
        type=Path,
        default=Path("docs/release/python-runtime-default-gate.md"),
        help="Path to the Python runtime default gate document.",
    )
    parser.add_argument(
        "--print-evidence",
        action="store_true",
        help="Print structured signed-release evidence JSON from the runtime manifest.",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_RUNTIME_MANIFEST,
        help="Path to the Python runtime manifest used by --print-evidence.",
    )
    parser.add_argument(
        "--python-version",
        default="",
        help="Python patch version used by --print-evidence, for example 3.11.9.",
    )
    parser.add_argument(
        "--security-update-policy",
        default=DEFAULT_SECURITY_UPDATE_POLICY,
        help="Security update policy recorded in generated evidence.",
    )
    parser.add_argument(
        "--with-runtime-artifact",
        default=None,
        help="Signed installer artifact path or glob with the bundled Python runtime.",
    )
    parser.add_argument(
        "--without-runtime-bytes",
        type=int,
        default=None,
        help="Signed installer size in bytes from the matching build without the runtime bundle.",
    )
    parser.add_argument(
        "--release",
        default="",
        help="Signed release tag recorded by --print-evidence.",
    )
    parser.add_argument(
        "--url",
        default="",
        help="HTTPS GitHub release tag URL for the signed installer artifact.",
    )
    parser.add_argument(
        "--release-notes",
        default="",
        dest="release_notes",
        help="HTTPS GitHub release tag URL for the published release notes.",
    )
    parser.add_argument(
        "--commit",
        default="",
        help="40-character commit SHA for the signed release artifact.",
    )
    parser.add_argument(
        "--signature",
        default="",
        help="Platform signature type for the signed release artifact.",
    )
    parser.add_argument(
        "--evidence",
        type=Path,
        action="append",
        default=[],
        help="Optional structured signed-release evidence JSON to validate; may be repeated.",
    )
    parser.add_argument(
        "--require-platforms",
        default="",
        help="Comma-separated platform list that repeated --evidence files must cover.",
    )
    return parser.parse_args()


def main() -> int:
    """Validate the configured Python runtime default gate document."""
    args = parse_args()
    validate_default_gate_doc(args.doc)
    if args.print_evidence:
        print_release_evidence(args)
        return 0
    if args.require_platforms and not args.evidence:
        raise RuntimeError("--require-platforms requires --evidence")
    if args.evidence:
        required_platforms = tuple(
            platform.strip() for platform in args.require_platforms.split(",") if platform.strip()
        )
        validate_release_evidence_files(args.evidence, required_platforms)
    print(f"validated Python runtime default gate: {args.doc}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
