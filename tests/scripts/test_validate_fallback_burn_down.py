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
VALID_RELEASE_BASE = "https://github.com/NiceBlueChai/hermes-agent/releases/tag"
VALID_RELEASE_V1_URL = f"{VALID_RELEASE_BASE}/v1.0.0"
VALID_RELEASE_V1_1_URL = f"{VALID_RELEASE_BASE}/v1.0.1"
VALID_RELEASE_V2_URL = f"{VALID_RELEASE_BASE}/v2.0.0"


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
                                        "url": VALID_RELEASE_V1_URL,
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
                                        "url": VALID_RELEASE_V1_URL,
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

    def test_signed_full_bootstrap_evidence_requires_signature_type(self) -> None:
        """Full-bootstrap release evidence must name the platform signing proof."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: desktop-bootstrap-script-fallback"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "desktop-bootstrap-script-fallback",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {
                                        "platform": "windows",
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                    {
                                        "platform": "macos",
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                    {
                                        "platform": "linux",
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                ],
                                "evidence": [
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": VALID_RELEASE_V1_URL,
                                        "commit": "a" * 40,
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

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evidence signature must be", result.stderr)

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
                                        "url": VALID_RELEASE_V1_URL,
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
                                        "url": VALID_RELEASE_V1_URL,
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
                                        "url": VALID_RELEASE_V2_URL,
                                        "releaseNotes": VALID_RELEASE_V2_URL,
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

    def test_evidence_url_must_be_github_release_tag_url(self) -> None:
        """Release evidence must point at a GitHub release tag page."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: non-github-release-url"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "non-github-release-url",
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
                                        "releaseNotes": VALID_RELEASE_V1_URL,
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
        self.assertIn("evidence url must be a GitHub release tag URL", result.stderr)

    def test_evidence_url_rejects_release_tag_with_whitespace(self) -> None:
        """Release evidence must not accept whitespace inside the GitHub tag segment."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: release-tag-whitespace"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "release-tag-whitespace",
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
                                        "release": "v1.0.0 beta",
                                        "url": f"{VALID_RELEASE_BASE}/v1.0.0 beta",
                                        "releaseNotes": f"{VALID_RELEASE_BASE}/v1.0.0 beta",
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
        self.assertIn("evidence url must be a GitHub release tag URL", result.stderr)

    def test_evidence_url_rejects_release_tag_with_backslash(self) -> None:
        """Release evidence must not accept backslashes inside the GitHub tag segment."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: release-tag-backslash"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            release = "v1.0.0\\evil"
            url = f"{VALID_RELEASE_BASE}/{release}"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "release-tag-backslash",
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
                                        "release": release,
                                        "url": url,
                                        "releaseNotes": url,
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
        self.assertIn("evidence url must be a GitHub release tag URL", result.stderr)

    def test_evidence_url_rejects_query_or_fragment_suffix(self) -> None:
        """Release evidence URLs must be exact GitHub release tag URLs."""
        for suffix in ("?download=true", "#notes"):
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                source = root / "src" / "entry.js"
                source.parent.mkdir(parents=True)
                marker = "HERMES-FALLBACK-BURN-DOWN: release-tag-query-fragment"
                source.write_text(f"// {marker}\n", encoding="utf-8")
                registry = root / "fallback.json"
                registry.write_text(
                    json.dumps(
                        {
                            "schemaVersion": 1,
                            "entries": [
                                {
                                    "id": "release-tag-query-fragment",
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
                                            "url": f"{VALID_RELEASE_V1_URL}{suffix}",
                                            "releaseNotes": VALID_RELEASE_V1_URL,
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
            self.assertIn("evidence url must be a GitHub release tag URL", result.stderr)

    def test_release_notes_url_must_be_github_release_tag_url(self) -> None:
        """Release-note evidence must point at a GitHub release tag page."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: non-github-release-notes-url"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "non-github-release-notes-url",
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
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": "https://example.invalid/releases/tag/v1.0.0",
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
        self.assertIn("evidence releaseNotes must be a GitHub release tag URL", result.stderr)

    def test_github_release_url_rejects_tag_subpath(self) -> None:
        """Release evidence must point at the tag page, not a nested path below it."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: release-tag-subpath"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "release-tag-subpath",
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
                                        "url": f"{VALID_RELEASE_V1_URL}/extra",
                                        "releaseNotes": VALID_RELEASE_V1_URL,
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
        self.assertIn("evidence url must be a GitHub release tag URL", result.stderr)

    def test_release_notes_url_must_match_release_repo(self) -> None:
        """Release-note evidence must point at the same GitHub repository as the artifact."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: release-notes-wrong-repo"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "release-notes-wrong-repo",
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
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": "https://github.com/OTHER/REPO/releases/tag/v1.0.0",
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
        self.assertIn("evidence releaseNotes must reference the same GitHub repository", result.stderr)

    def test_evidence_url_rejects_template_repository_placeholder(self) -> None:
        """Release evidence must not use the generated OWNER/REPO placeholder."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: placeholder-repo"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "placeholder-repo",
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
                                        "url": "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
                                        "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/v1.0.0",
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
        self.assertIn("evidence url must replace OWNER/REPO placeholder", result.stderr)

    def test_evidence_rejects_template_release_placeholder(self) -> None:
        """Release evidence must not use the generated vX.Y.Z placeholder."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: placeholder-release"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "placeholder-release",
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
                                        "release": "vX.Y.Z",
                                        "url": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
                                        "releaseNotes": "https://github.com/OWNER/REPO/releases/tag/vX.Y.Z",
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
        self.assertIn("evidence release must replace vX.Y.Z placeholder", result.stderr)

    def test_same_release_artifact_rejects_release_notes_fragment(self) -> None:
        """One signed release artifact must point at an exact release note URL."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: conflicting-release-notes"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "conflicting-release-notes",
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
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": VALID_RELEASE_V1_URL,
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["release-notes"],
                                    },
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.0",
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": f"{VALID_RELEASE_V1_URL}#notes",
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                ],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evidence releaseNotes must be a GitHub release tag URL", result.stderr)

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
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": VALID_RELEASE_V1_URL,
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
                                        "url": VALID_RELEASE_V1_URL,
                                        "releaseNotes": VALID_RELEASE_V1_URL,
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

    def test_require_complete_rejects_platforms_from_different_releases(self) -> None:
        """All required platforms must be proven by one shared signed release."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: mixed-release-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            checks = ["can-run-full-bootstrap", "release-notes"]
            evidence = []
            for platform, release, commit in (
                ("windows", "v1.0.0", "a" * 40),
                ("macos", "v1.0.1", "b" * 40),
                ("linux", "v1.0.1", "b" * 40),
            ):
                evidence.append(
                    {
                        "platform": platform,
                        "release": release,
                        "url": f"{VALID_RELEASE_BASE}/{release}",
                        "releaseNotes": f"{VALID_RELEASE_BASE}/{release}",
                        "commit": commit,
                        "signed": True,
                        "checks": checks,
                    }
                )
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "mixed-release-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": platform, "checks": checks}
                                    for platform in ("windows", "macos", "linux")
                                ],
                                "evidence": evidence,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--require-complete", "mixed-release-evidence")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing shared complete signed release evidence", result.stderr)

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
                                        "url": VALID_RELEASE_V1_URL,
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
                                        "url": VALID_RELEASE_V1_1_URL,
                                        "releaseNotes": VALID_RELEASE_V1_1_URL,
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
                                        "url": VALID_RELEASE_V1_URL,
                                        "commit": "a" * 40,
                                        "signed": True,
                                        "checks": ["can-run-full-bootstrap"],
                                    },
                                    {
                                        "platform": "windows",
                                        "release": "v1.0.1",
                                        "url": VALID_RELEASE_V1_1_URL,
                                        "releaseNotes": VALID_RELEASE_V1_1_URL,
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

    def test_print_template_treats_mixed_platform_releases_as_incomplete(self) -> None:
        """Template generation should require one shared release across platforms."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: mixed-template-evidence"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            checks = ["can-run-full-bootstrap", "release-notes"]
            evidence = []
            for platform, release, commit in (
                ("windows", "v1.0.0", "a" * 40),
                ("macos", "v1.0.1", "b" * 40),
                ("linux", "v1.0.1", "b" * 40),
            ):
                evidence.append(
                    {
                        "platform": platform,
                        "release": release,
                        "url": f"{VALID_RELEASE_BASE}/{release}",
                        "releaseNotes": f"{VALID_RELEASE_BASE}/{release}",
                        "commit": commit,
                        "signed": True,
                        "checks": checks,
                    }
                )
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "mixed-template-evidence",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": platform, "checks": checks}
                                    for platform in ("windows", "macos", "linux")
                                ],
                                "evidence": evidence,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            result = run_validator(registry, root, "--print-template", "mixed-template-evidence")

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        template = json.loads(result.stdout)
        self.assertEqual(
            [item["platform"] for item in template["evidence"]],
            ["windows", "macos", "linux"],
        )

    def test_print_template_rejects_full_bootstrap_missing_required_platform(self) -> None:
        """Full bootstrap templates must not hide a missing signed release platform."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: desktop-bootstrap-script-fallback"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            checks = ["can-run-full-bootstrap", "release-notes"]
            registry = root / "fallback.json"
            registry.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "entries": [
                            {
                                "id": "desktop-bootstrap-script-fallback",
                                "owner": "desktop",
                                "file": "src/entry.js",
                                "marker": marker,
                                "fallback": "Fallback description.",
                                "removalGate": "Release evidence gate.",
                                "requiredEvidence": [
                                    {"platform": "windows", "checks": checks},
                                    {"platform": "macos", "checks": checks},
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
                "--print-template",
                "desktop-bootstrap-script-fallback",
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing requiredEvidence platform: linux", result.stderr)

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
                VALID_RELEASE_V1_URL,
                "--release-notes",
                VALID_RELEASE_V1_URL,
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
                    "url": VALID_RELEASE_V1_URL,
                    "releaseNotes": VALID_RELEASE_V1_URL,
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
                VALID_RELEASE_V1_URL,
                "--release-notes",
                VALID_RELEASE_V1_URL,
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

    def test_print_evidence_item_outputs_signed_release_json_without_mutating_registry(self) -> None:
        """Signed release workflows can upload machine-readable evidence without editing the registry."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: print-evidence-item"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            original_payload = {
                "schemaVersion": 1,
                "entries": [
                    {
                        "id": "print-evidence-item",
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
            registry.write_text(json.dumps(original_payload), encoding="utf-8")

            result = run_validator(
                registry,
                root,
                "--print-evidence-item",
                "print-evidence-item",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                VALID_RELEASE_V1_URL,
                "--release-notes",
                VALID_RELEASE_V1_URL,
                "--commit",
                "e" * 40,
                "--all-required-checks",
            )

            printed = json.loads(result.stdout)
            payload_after = json.loads(registry.read_text(encoding="utf-8"))

        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertEqual(payload_after, original_payload)
        self.assertEqual(printed["entryId"], "print-evidence-item")
        self.assertEqual(
            printed["evidence"],
            [
                {
                    "platform": "linux",
                    "release": "v1.0.0",
                    "url": VALID_RELEASE_V1_URL,
                    "releaseNotes": VALID_RELEASE_V1_URL,
                    "commit": "e" * 40,
                    "signed": True,
                    "checks": [
                        "can-run-full-bootstrap",
                        "packaged-native-bridge-smoke",
                        "release-notes",
                    ],
                }
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
                VALID_RELEASE_V1_URL,
                "--commit",
                "e" * 40,
                "--all-required-checks",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--add-evidence requires --release-notes", result.stderr)
        self.assertEqual(payload, original)

    def test_add_full_bootstrap_evidence_requires_signature_without_mutating_registry(self) -> None:
        """Full-bootstrap evidence recording should fail fast when signature type is missing."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: desktop-bootstrap-script-fallback"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            original = {
                "schemaVersion": 1,
                "entries": [
                    {
                        "id": "desktop-bootstrap-script-fallback",
                        "owner": "desktop",
                        "file": "src/entry.js",
                        "marker": marker,
                        "fallback": "Fallback description.",
                        "removalGate": "Release evidence gate.",
                        "requiredEvidence": [
                            {"platform": "windows", "checks": ["can-run-full-bootstrap"]},
                            {"platform": "macos", "checks": ["can-run-full-bootstrap"]},
                            {"platform": "linux", "checks": ["can-run-full-bootstrap"]},
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
                "desktop-bootstrap-script-fallback",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                VALID_RELEASE_V1_URL,
                "--commit",
                "e" * 40,
                "--all-required-checks",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--add-evidence requires --signature", result.stderr)
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
            url = VALID_RELEASE_V1_URL
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

    def test_add_evidence_rejects_release_notes_query_without_mutating_registry(self) -> None:
        """Repeated signed evidence updates must still use exact release note URLs."""
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "src" / "entry.js"
            source.parent.mkdir(parents=True)
            marker = "HERMES-FALLBACK-BURN-DOWN: conflicting-add-release-notes"
            source.write_text(f"// {marker}\n", encoding="utf-8")
            registry = root / "fallback.json"
            original = {
                "schemaVersion": 1,
                "entries": [
                    {
                        "id": "conflicting-add-release-notes",
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
                                "url": VALID_RELEASE_V1_URL,
                                "releaseNotes": VALID_RELEASE_V1_URL,
                                "commit": "b" * 40,
                                "signed": True,
                                "checks": ["can-run-full-bootstrap"],
                            }
                        ],
                    }
                ],
            }
            registry.write_text(json.dumps(original), encoding="utf-8")

            result = run_validator(
                registry,
                root,
                "--add-evidence",
                "conflicting-add-release-notes",
                "--platform",
                "linux",
                "--release",
                "v1.0.0",
                "--url",
                VALID_RELEASE_V1_URL,
                "--release-notes",
                f"{VALID_RELEASE_V1_URL}?notes=changed",
                "--commit",
                "b" * 40,
                "--check",
                "release-notes",
            )

            payload = json.loads(registry.read_text(encoding="utf-8"))

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("evidence releaseNotes must be a GitHub release tag URL", result.stderr)
        self.assertEqual(payload, original)

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
                VALID_RELEASE_V1_URL,
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
