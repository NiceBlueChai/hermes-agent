"""Validate the release gate for promoting bundled Python runtime archives to default."""

from __future__ import annotations

import argparse
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
}


def validate_default_gate_doc(path: Path) -> None:
    """Raise RuntimeError when the Python runtime default gate document is incomplete."""
    text = path.read_text(encoding="utf-8")
    missing = [label for label, marker in REQUIRED_MARKERS.items() if marker not in text]
    if missing:
        details = ", ".join(missing)
        raise RuntimeError(f"python runtime default gate is missing required marker(s): {details}")


def parse_args() -> argparse.Namespace:
    """Parse command-line arguments for the Python runtime default gate validator."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--doc",
        type=Path,
        default=Path("docs/release/python-runtime-default-gate.md"),
        help="Path to the Python runtime default gate document.",
    )
    return parser.parse_args()


def main() -> int:
    """Validate the configured Python runtime default gate document."""
    args = parse_args()
    validate_default_gate_doc(args.doc)
    print(f"validated Python runtime default gate: {args.doc}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
