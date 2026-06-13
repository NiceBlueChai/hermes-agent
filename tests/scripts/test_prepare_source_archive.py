"""Tests for preparing audited source archives for the installer bundle."""

from __future__ import annotations

import importlib.util
import io
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "prepare_source_archive.py"
    spec = importlib.util.spec_from_file_location("_prepare_source_archive", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class PrepareSourceArchiveTests(unittest.TestCase):
    """Validate source archive auditing and manifest generation."""

    def test_prepare_audited_source_archive_downloads_and_writes_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "source-archive"
            archive_bytes = _zip_bytes("hermes-agent-abcdef123/README.md", b"source")
            sha256 = _sha256_bytes(module, archive_bytes)

            def fake_urlopen(url, timeout):
                self.assertEqual(url, "https://example.invalid/hermes-agent-abcdef123.zip")
                self.assertEqual(timeout, 120)
                return _FakeResponse(archive_bytes)

            with mock.patch.object(module.urllib.request, "urlopen", fake_urlopen):
                prepared = module.prepare_audited_source_archive(
                    output_dir=output_dir,
                    audited_archive=(
                        "hermes-agent-abcdef123.zip="
                        f"https://example.invalid/hermes-agent-abcdef123.zip={sha256}"
                    ),
                    owner="NousResearch",
                    repo="hermes-agent",
                    archive_ref="abcdef123",
                    commit="abcdef123",
                    branch="main",
                    force=True,
                    dry_run=False,
                )

            self.assertEqual(len(prepared), 1)
            payload = json.loads((output_dir / "source-archive-manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(payload["archiveRef"], "abcdef123")
            self.assertEqual(payload["commit"], "abcdef123")
            self.assertEqual(payload["branch"], "main")
            self.assertEqual(payload["files"][0]["sha256"], sha256)
            self.assertEqual(module.validate_payload(output_dir, "NousResearch", "hermes-agent", "abcdef123"), 1)

    def test_prepare_audited_source_archive_rejects_checksum_mismatch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "source-archive"
            archive_bytes = _zip_bytes("hermes-agent-abcdef123/README.md", b"source")

            with mock.patch.object(module.urllib.request, "urlopen", lambda url, timeout: _FakeResponse(archive_bytes)):
                with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                    module.prepare_audited_source_archive(
                        output_dir=output_dir,
                        audited_archive=f"hermes-agent-abcdef123.zip=https://example.invalid/a.zip={'0' * 64}",
                        owner="NousResearch",
                        repo="hermes-agent",
                        archive_ref="abcdef123",
                        commit="abcdef123",
                        branch="main",
                        force=True,
                        dry_run=False,
                    )

    def test_parse_audited_archive_rejects_unsafe_inputs(self):
        module = _load_script_module()
        sha256 = "a" * 64

        with self.assertRaisesRegex(ValueError, "HTTPS"):
            module.parse_audited_archive_arg(f"source.zip=http://example.invalid/source.zip={sha256}")
        with self.assertRaisesRegex(ValueError, "unsafe"):
            module.parse_audited_archive_arg(f"../source.zip=https://example.invalid/source.zip={sha256}")
        with self.assertRaisesRegex(ValueError, "invalid sha256"):
            module.parse_audited_archive_arg("source.zip=https://example.invalid/source.zip=bad")

    def test_installer_workflows_accept_optional_audited_source_archive(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("source-archive:", windows_workflow)
        self.assertIn("HERMES_SOURCE_ARCHIVE: ${{ inputs['source-archive'] }}", windows_workflow)
        self.assertIn("scripts/prepare_source_archive.py", windows_workflow)
        self.assertIn("--audited-archive \"$env:HERMES_SOURCE_ARCHIVE\"", windows_workflow)
        self.assertIn("apps/bootstrap-installer/src-tauri/source-archive/*", windows_workflow)

        self.assertIn("linux-source-archive:", unix_workflow)
        self.assertIn("macos-source-archive:", unix_workflow)
        self.assertIn("HERMES_LINUX_SOURCE_ARCHIVE: ${{ inputs['linux-source-archive'] }}", unix_workflow)
        self.assertIn("HERMES_MACOS_SOURCE_ARCHIVE: ${{ inputs['macos-source-archive'] }}", unix_workflow)
        self.assertIn("--audited-archive \"${HERMES_SOURCE_ARCHIVE}\"", unix_workflow)
        self.assertIn("scripts/prepare_source_archive.py", unix_workflow)
        self.assertIn("apps/bootstrap-installer/src-tauri/source-archive/*", unix_workflow)


def _zip_bytes(name: str, content: bytes) -> bytes:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "archive.zip"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr(name, content)
        return path.read_bytes()


def _sha256_bytes(module, data: bytes) -> str:
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "archive.zip"
        path.write_bytes(data)
        return module.sha256_file(path)


class _FakeResponse(io.BytesIO):
    """Context-manager byte stream used to fake urllib responses."""

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
        return None


if __name__ == "__main__":
    unittest.main()
