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
                                        "url": "https://example.invalid/releases/tag/v1.0.0",
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
                                        "url": "https://example.invalid/releases/tag/v1.0.0",
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
                                        "url": "https://example.invalid/releases/tag/v1.0.0",
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

    def test_release_notes_check_requires_release_notes_url(self) -> None:
        """Release-note evidence must link to the exact published release notes."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: missing-release-notes-url"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "missing-release-notes-url",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["release-notes"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/releases/tag/v1.0.0",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["release-notes"],
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
        self.assertIn("releaseNotes must be HTTPS", result.stderr)

    def test_evidence_urls_must_reference_release_tag(self) -> None:
        """Release evidence URLs must point at the same release tag they claim."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: mismatched-release-url"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "mismatched-release-url",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["release-notes"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/releases/tag/v2.0.0",
                                        "releaseNotes": "https://example.invalid/releases/tag/v2.0.0",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["release-notes"],
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
        self.assertIn("evidence url must include release tag: v1.0.0", result.stderr)

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
                                        "releaseNotes": f"https://example.invalid/releases/v1.0.0/{platform}",
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
                                        "releaseNotes": f"https://example.invalid/releases/v1.0.0/{platform}",
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

    def test_require_complete_rejects_platform_checks_split_across_releases(self) -> None:
        """Each platform must have one signed release artifact covering all checks."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: split-release-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            required_checks = ["can-run-full-bootstrap", "release-notes"]
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "split-release-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": platform, "checks": required_checks}
                                    for platform in ("windows", "macos", "linux")
                                ],
                                "evidence": [
                                    {
                                        "platform": platform,
                                        "release": "v1.0.0",
                                        "url": f"https://example.invalid/releases/v1.0.0/{platform}",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["can-run-full-bootstrap"],
                                    }
                                    for platform in ("windows", "macos", "linux")
                                ]
                                + [
                                    {
                                        "platform": platform,
                                        "release": "v1.0.1",
                                        "url": f"https://example.invalid/releases/v1.0.1/{platform}",
                                        "releaseNotes": f"https://example.invalid/releases/v1.0.1/{platform}",
                                        "commit": "b" * 40,
                                        "signed": True,
                                        "checks": ["release-notes"],
                                    }
                                    for platform in ("windows", "macos", "linux")
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--require-complete", "split-release-evidence")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing complete evidence for windows", result.stderr)

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
                    "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "commit": "<40-character-git-sha>",
                    "signed": True,
                    "checks": ["can-run-full-bootstrap", "release-notes"],
                }
            ],
        )

    def test_print_template_treats_split_release_evidence_as_incomplete(self) -> None:
        """Template generation should not let split release evidence hide missing checks."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: split-template-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "split-template-evidence",
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
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": "https://example.invalid/releases/v1.0.0/windows",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.1",
                                        "url": "https://example.invalid/releases/v1.0.1/windows",
                                        "releaseNotes": "https://example.invalid/releases/v1.0.1/windows",
                                        "commit": "b" * 40,
                                        "signed": True,
                                        "checks": ["release-notes"],
                                    },
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--print-template", "split-template-evidence")

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        template = json.loads(result.stdout)
        self.assertEqual(
            template["evidence"],
            [
                {
                    "platform": "windows",
                    "release": "vX.Y.Z",
                    "url": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                    "commit": "<40-character-git-sha>",
                    "signed": True,
                    "checks": ["can-run-full-bootstrap", "release-notes"],
                }
            ],
        )

    def test_add_evidence_appends_signed_release_evidence(self) -> None:
        """Release operators can record signed evidence without hand-editing JSON."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: add-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "add-evidence",
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

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "add-evidence",
                "--platform",
                "windows",
                "--release",
                "v1.0.0",
                "--url",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--release-notes",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--commit",
                "a" * 40,
                "--check",
                "can-run-full-bootstrap",
                "--check",
                "release-notes",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("added evidence: add-evidence windows", result.stdout)
        self.assertEqual(
            payload["entries"][0]["evidence"],
            [
                {
                    "platform": "windows",
                    "release": "v1.0.0",
                    "url": "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                    "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                    "commit": "a" * 40,
                    "signed": True,
                    "checks": ["can-run-full-bootstrap", "release-notes"],
                }
            ],
        )

    def test_add_evidence_can_record_all_required_checks_for_platform(self) -> None:
        """Release operators can avoid hand-listing every required platform check."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: add-all-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "add-all-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "linux",
                                        "checks": [
                                            "can-run-full-bootstrap",
                                            "packaged-native-bridge-smoke",
                                            "release-notes",
                                        ],
                                    }
                                ],
                                "evidence": [],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "add-all-evidence",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--release-notes",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--commit",
                "d" * 40,
                "--all-required-checks",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(
            payload["entries"][0]["evidence"][0]["checks"],
            [
                "can-run-full-bootstrap",
                "packaged-native-bridge-smoke",
                "release-notes",
            ],
        )

    def test_add_evidence_requires_release_notes_for_all_required_checks(self) -> None:
        """Release operators get an immediate error when release notes would be recorded."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: missing-add-release-notes"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            original = {
                "schemaVersion": 1,
                "entries": [
                    {
                        "id": "missing-add-release-notes",
                        "owner": "desktop",
                        "file": "src/entry.js",
                        "marker": marker,
                        "fallback": "Fallback description.",
                        "removalGate": "Release evidence gate.",
                        "requiredEvidence": [
                            {
                                "platform": "linux",
                                "checks": ["can-run-full-bootstrap", "release-notes"],
                            }
                        ],
                        "evidence": [],
                    }
                ],
            }
            registry.write_text(json.dumps(original), encoding="utf-8")

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "missing-add-release-notes",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--commit",
                "e" * 40,
                "--all-required-checks",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--add-evidence requires --release-notes", result.stderr)
        self.assertEqual(payload, original)

    def test_add_evidence_merges_existing_matching_release_evidence(self) -> None:
        """Repeated signed evidence updates should merge checks for the same release."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: merge-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            url = "https://github.com/OWNER/REPO/releases/tag/v1.0.0"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "merge-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "linux",
                                        "checks": ["can-run-full-bootstrap", "release-notes"],
                                    }
                                ],
                                "evidence": [
                                    {
                                        "platform": "linux",
                                        "release": "v1.0.0",
                                        "url": url,
                                        "commit": "b" * 40,
                                        "signed": True,
                                        "checks": ["can-run-full-bootstrap"],
                                    }
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "merge-evidence",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                url,
                "--release-notes",
                url,
                "--commit",
                "b" * 40,
                "--check",
                "release-notes",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(len(payload["entries"][0]["evidence"]), 1)
        self.assertEqual(
            payload["entries"][0]["evidence"][0]["checks"],
            ["can-run-full-bootstrap", "release-notes"],
        )

    def test_add_evidence_rejects_undeclared_check_without_mutating_registry(self) -> None:
        """Evidence recording should fail before writing undeclared checks."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: reject-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            original = {
                "schemaVersion": 1,
                "entries": [
                    {
                        "id": "reject-evidence",
                        "owner": "desktop",
                        "file": "src/entry.js",
                        "marker": marker,
                        "fallback": "Fallback description.",
                        "removalGate": "Release evidence gate.",
                        "requiredEvidence": [
                            {
                                "platform": "macos",
                                "checks": ["can-run-full-bootstrap"],
                            }
                        ],
                        "evidence": [],
                    }
                ],
            }
            registry.write_text(json.dumps(original), encoding="utf-8")

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "reject-evidence",
                "--platform",
                "macos",
                "--release",
                "v1.0.0",
                "--url",
                "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                "--commit",
                "c" * 40,
                "--check",
                "unknown-check",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("undeclared evidence check", result.stderr)
        self.assertEqual(payload, original)

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
