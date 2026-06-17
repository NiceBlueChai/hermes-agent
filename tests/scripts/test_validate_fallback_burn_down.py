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


def run_validator(
    registry: Path,
    repo_root: Path,
    *extra_args: str,
) -> subprocess.CompletedProcess[str]:
    """Run the fallback burn-down validator and capture text output."""
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--registry",
            str(registry),
            "--repo-root",
            str(repo_root),
            *extra_args,
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
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
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
                "requiredEvidence": [
                    {
                        "platform": "windows",
                        "checks": ["packaged-native-bridge-smoke"],
                    }
                ],
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

    def test_missing_required_evidence_fails(self) -> None:
        """Every retained fallback must declare the release evidence needed for removal."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: missing-required-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "missing-required-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
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
        self.assertIn("requiredEvidence", result.stderr)

    def test_evidence_must_match_required_platform(self) -> None:
        """Evidence entries must reference a declared platform."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: bad-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "bad-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "linux",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/release",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("undeclared evidence platform", result.stderr)

    def test_evidence_must_match_required_checks(self) -> None:
        """Evidence entries must only claim declared checks for that platform."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: bad-evidence-check"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "bad-evidence-check",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/release",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["unreviewed-check"],
                                    }
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("undeclared evidence check", result.stderr)

    def test_evidence_must_be_marked_signed(self) -> None:
        """Fallback removal evidence must come from signed release artifacts."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: unsigned-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "unsigned-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/release",
                                        "commit": "a" * 40,
                                        "signed": False,
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evidence signed must be true", result.stderr)

    def test_evidence_must_include_release_commit(self) -> None:
        """Fallback removal evidence must name the exact signed release commit."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: missing-commit"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "missing-commit",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/release",
                                        "signed": True,
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evidence commit must be a 40-character git SHA", result.stderr)

    def test_require_complete_passes_when_signed_evidence_covers_all_checks(self) -> None:
        """Release fallback removal can require complete signed evidence for one entry."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: complete-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            checks = ["can-run-full-bootstrap", "release-notes"]
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "complete-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": platform, "checks": checks}
                                    for platform in ("windows", "macos", "linux")
                                ],
                                "evidence": [
                                    {
                                        "platform": platform,
                                        "release": "v1.0.0",
                                        "url": f"https://example.invalid/releases/v1.0.0/{platform}",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": checks,
                                    }
                                    for platform in ("windows", "macos", "linux")
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--require-complete", "complete-evidence")

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("complete evidence: complete-evidence", result.stdout)

    def test_require_complete_fails_when_a_required_platform_is_missing(self) -> None:
        """Release fallback removal should fail if signed evidence is incomplete."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: incomplete-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            checks = ["can-run-full-bootstrap", "release-notes"]
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "incomplete-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": platform, "checks": checks}
                                    for platform in ("windows", "macos", "linux")
                                ],
                                "evidence": [
                                    {
                                        "platform": platform,
                                        "release": "v1.0.0",
                                        "url": f"https://example.invalid/releases/v1.0.0/{platform}",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": checks,
                                    }
                                    for platform in ("windows", "macos")
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--require-complete", "incomplete-evidence")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing complete evidence for linux", result.stderr)

    def test_print_template_outputs_missing_signed_evidence_skeleton(self) -> None:
        """Release operators can generate a complete evidence skeleton for one fallback."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: template-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "template-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["can-run-full-bootstrap", "release-notes"],
                                    }
                                ],
                                "evidence": [],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--print-template", "template-evidence")

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        template = json.loads(result.stdout)
        self.assertEqual(template["entryId"], "template-evidence")
        self.assertEqual(
            template["evidence"],
            [
                {
                    "platform": "windows",
                    "release": "vX.Y.Z",
                    "url": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "commit": "<40-character-git-sha>",
                    "signed": True,
                    "checks": ["can-run-full-bootstrap", "release-notes"],
                }
            ],
        )

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
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["packaged-native-bridge-smoke"],
                                    }
                                ],
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
