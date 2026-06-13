"""Tests for the fallback burn-down release registry validator."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "validate_fallback_burn_down.py"
REGISTRY = REPO_ROOT / "docs" / "release" / "fallback-burn-down.json"


def run_validator(registry: Path, repo_root: Path) -> subprocess.CompletedProcess[str]:
    """Run the fallback burn-down validator and capture text output."""
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--registry",
            str(registry),
            "--repo-root",
            str(repo_root),
        ],
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


class ValidateFallbackBurnDownTests(unittest.TestCase):
    """Covers the machine-readable fallback removal evidence registry."""

    def test_current_registry_is_valid(self) -> None:
        """The checked-in fallback burn-down registry should match source markers."""
        result = run_validator(REGISTRY, REPO_ROOT)

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("validated", result.stdout)

    def test_missing_marker_fails(self) -> None:
        """A registry entry must point at a marker that exists in its source file."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            source.write_text("// source without marker\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "missing-marker",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": "HERMES-FALLBACK-BURN-DOWN: missing-marker",
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "evidence": [],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("marker not found", result.stderr)

    def test_duplicate_ids_fail(self) -> None:
        """Fallback ids are stable handles and must be unique."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: duplicate-id"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            entry = {
                "id": "duplicate-id",
                "owner": "installer",
                "file": "src/entry.js",
                "marker": marker,
                "fallback": "Fallback description.",
                "removalGate": "Release evidence gate.",
                "evidence": [],
            }
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps({"schemaVersion": 1, "entries": [entry, dict(entry)]}),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate id", result.stderr)

    def test_unsafe_file_path_fails(self) -> None:
        """Registry file paths must stay inside the repository root."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "unsafe-path",
                                "owner": "installer",
                                "file": "../outside.js",
                                "marker": "HERMES-FALLBACK-BURN-DOWN: unsafe-path",
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "evidence": [],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsafe file path", result.stderr)


if __name__ == "__main__":
    unittest.main()
