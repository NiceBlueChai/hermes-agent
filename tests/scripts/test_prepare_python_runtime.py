"""Tests for the Python runtime release preparation helper."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "prepare_python_runtime.py"
    spec = importlib.util.spec_from_file_location("_prepare_python_runtime", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class PreparePythonRuntimeTests(unittest.TestCase):
    """Validate Python runtime manifest generation and release checks."""

    def test_prepare_runtime_archive_copies_archive_and_writes_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "python-runtime.zip"
            output_dir = root / "python-runtime"
            source.write_bytes(b"runtime archive")

            records = module.prepare_runtime_archive(
                output_dir=output_dir,
                archive_path=source,
                platform="windows",
                arch="x64",
                python_tag="cp311",
                source_url="https://example.invalid/python-runtime.zip",
                force=False,
                dry_run=False,
            )

            manifest_path = output_dir / "python-runtime-manifest.json"
            payload = json.loads(manifest_path.read_text(encoding="utf-8"))
            copied = output_dir / source.name
            self.assertEqual(len(records), 1)
            self.assertEqual(copied.read_bytes(), b"runtime archive")
            self.assertEqual(payload["schemaVersion"], 1)
            self.assertEqual(payload["platform"], "windows")
            self.assertEqual(payload["arch"], "x64")
            self.assertEqual(payload["pythonTag"], "cp311")
            self.assertEqual(payload["files"][0]["name"], source.name)
            self.assertEqual(payload["files"][0]["url"], "https://example.invalid/python-runtime.zip")
            self.assertEqual(payload["files"][0]["sizeBytes"], len(b"runtime archive"))
            self.assertEqual(payload["files"][0]["sha256"], module.sha256_file(copied))
            self.assertEqual(module.validate_payload(output_dir, "windows", "x64"), 1)

    def test_validate_payload_rejects_unmanifested_runtime_archive(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "python-runtime"
            output_dir.mkdir()
            archive = output_dir / "python-runtime.zip"
            rogue = output_dir / "rogue.zip"
            archive.write_bytes(b"runtime archive")
            rogue.write_bytes(b"rogue")
            record = module.prepared_runtime_record(
                platform="linux",
                arch="x64",
                python_tag="cp311",
                source_url="https://example.invalid/python-runtime.zip",
                path=archive,
            )
            module.write_manifest(output_dir, [record])

            with self.assertRaisesRegex(RuntimeError, "unmanifested python runtime payload"):
                module.validate_payload(output_dir, "linux", "x64")

    def test_validate_manifest_rejects_non_https_source_url(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "python-runtime"
            output_dir.mkdir()
            archive = output_dir / "python-runtime.zip"
            archive.write_bytes(b"runtime archive")
            record = module.prepared_runtime_record(
                platform="macos",
                arch="arm64",
                python_tag="cp311",
                source_url="http://example.invalid/python-runtime.zip",
                path=archive,
            )
            module.write_manifest(output_dir, [record])

            with self.assertRaisesRegex(RuntimeError, "invalid url"):
                module.validate_payload(output_dir, "macos", "arm64")

    def test_parse_audited_archive_rejects_bad_shape_or_non_https_url(self):
        module = _load_script_module()
        sha256 = "a" * 64

        spec = module.parse_audited_archive_arg(
            f"python-runtime-linux-x64.tar.gz=https://example.invalid/r.tar.gz={sha256}"
        )

        self.assertEqual(spec.name, "python-runtime-linux-x64.tar.gz")
        self.assertEqual(spec.url, "https://example.invalid/r.tar.gz")
        self.assertEqual(spec.expected_sha256, sha256)
        with self.assertRaisesRegex(ValueError, "NAME=HTTPS_URL=SHA256"):
            module.parse_audited_archive_arg("python-runtime.zip=https://example.invalid/runtime.zip")
        with self.assertRaisesRegex(ValueError, "HTTPS"):
            module.parse_audited_archive_arg(f"python-runtime.zip=http://example.invalid/runtime.zip={sha256}")
        with self.assertRaisesRegex(ValueError, "invalid sha256"):
            module.parse_audited_archive_arg("python-runtime.zip=https://example.invalid/runtime.zip=bad")

    def test_prepare_audited_runtime_archive_downloads_and_writes_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "python-runtime"
            runtime_bytes = b"downloaded runtime"
            sha256 = hashlib.sha256(runtime_bytes).hexdigest()
            calls = []
            original_download = module.download_archive

            def fake_download(spec, target_dir, force):
                calls.append((spec.name, spec.url, target_dir, force))
                path = target_dir / spec.name
                path.write_bytes(runtime_bytes)
                return path

            module.download_archive = fake_download
            try:
                records = module.prepare_audited_runtime_archive(
                    output_dir=output_dir,
                    audited_archive=f"python-runtime.zip=https://example.invalid/python-runtime.zip={sha256}",
                    platform="linux",
                    arch="x64",
                    python_tag="cp311",
                    force=True,
                    dry_run=False,
                )
            finally:
                module.download_archive = original_download

            payload = json.loads((output_dir / "python-runtime-manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(len(records), 1)
            self.assertEqual(
                calls,
                [("python-runtime.zip", "https://example.invalid/python-runtime.zip", output_dir, True)],
            )
            self.assertEqual((output_dir / "python-runtime.zip").read_bytes(), runtime_bytes)
            self.assertEqual(payload["files"][0]["sha256"], sha256)
            self.assertEqual(payload["files"][0]["url"], "https://example.invalid/python-runtime.zip")
            self.assertEqual(module.validate_payload(output_dir, "linux", "x64"), 1)

    def test_prepare_audited_runtime_archive_rejects_checksum_mismatch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "python-runtime"
            original_download = module.download_archive

            def fake_download(spec, target_dir, force):
                path = target_dir / spec.name
                path.write_bytes(b"unexpected runtime")
                return path

            module.download_archive = fake_download
            try:
                with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                    module.prepare_audited_runtime_archive(
                        output_dir=output_dir,
                        audited_archive=f"python-runtime.zip=https://example.invalid/python-runtime.zip={'0' * 64}",
                        platform="windows",
                        arch="x64",
                        python_tag="cp311",
                        force=True,
                        dry_run=False,
                    )
            finally:
                module.download_archive = original_download

    def test_installer_workflows_accept_optional_audited_python_runtime(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")
        windows_workflow_flat = " ".join(windows_workflow.split())
        unix_workflow_flat = " ".join(unix_workflow.split())

        self.assertIn("python-runtime-archive:", windows_workflow)
        self.assertIn("HERMES_PYTHON_RUNTIME_ARCHIVE: ${{ inputs['python-runtime-archive'] }}", windows_workflow)
        self.assertIn("astral-sh/setup-uv@", windows_workflow)
        self.assertIn("scripts/build_python_runtime_archive.py", windows_workflow)
        self.assertIn("dist/python-runtime/python-runtime-windows-x64.zip", windows_workflow)
        self.assertIn('@("--audited-archive", "$env:HERMES_PYTHON_RUNTIME_ARCHIVE")', windows_workflow)
        self.assertIn('@("--archive", "dist/python-runtime/python-runtime-windows-x64.zip")', windows_workflow)
        self.assertIn("scripts/prepare_python_runtime.py", windows_workflow)
        self.assertIn("--validate-only --platform windows --arch x64 --python-tag cp311", windows_workflow_flat)
        self.assertIn(
            "--self-check-python-runtime apps/bootstrap-installer/src-tauri/python-runtime",
            windows_workflow,
        )
        self.assertIn("--wheelhouse-dir apps/bootstrap-installer/src-tauri/wheelhouse", windows_workflow)
        self.assertIn("--wheelhouse-platform windows", windows_workflow)
        self.assertIn("--python-runtime-dir apps/bootstrap-installer/src-tauri/python-runtime", windows_workflow)
        self.assertIn("apps/bootstrap-installer/src-tauri/python-runtime/*", windows_workflow)

        self.assertIn("linux-python-runtime-archive:", unix_workflow)
        self.assertIn("macos-python-runtime-archive:", unix_workflow)
        self.assertIn(
            "HERMES_LINUX_PYTHON_RUNTIME_ARCHIVE: ${{ inputs['linux-python-runtime-archive'] }}",
            unix_workflow,
        )
        self.assertIn(
            "HERMES_MACOS_PYTHON_RUNTIME_ARCHIVE: ${{ inputs['macos-python-runtime-archive'] }}",
            unix_workflow,
        )
        self.assertIn("astral-sh/setup-uv@", unix_workflow)
        self.assertIn("scripts/build_python_runtime_archive.py", unix_workflow)
        self.assertIn("python-runtime-${{ matrix.platform }}-${runtime_arch}", unix_workflow)
        self.assertIn('runtime_archive="${runtime_archive}.${extension}"', unix_workflow)
        self.assertIn("--audited-archive \"${HERMES_PYTHON_RUNTIME_ARCHIVE}\"", unix_workflow)
        self.assertIn("scripts/prepare_python_runtime.py", unix_workflow)
        self.assertIn(
            "--validate-only --platform ${{ matrix.platform }} --arch "
            "${{ runner.arch == 'ARM64' && 'arm64' || 'x64' }} --python-tag cp311",
            unix_workflow_flat,
        )
        self.assertNotIn("inputs['linux-python-runtime-archive'] != ''", unix_workflow)
        self.assertNotIn("inputs['macos-python-runtime-archive'] != ''", unix_workflow)
        self.assertIn("--self-check-python-runtime apps/bootstrap-installer/src-tauri/python-runtime", unix_workflow)
        self.assertIn("--wheelhouse-dir apps/bootstrap-installer/src-tauri/wheelhouse", unix_workflow)
        self.assertIn("--wheelhouse-platform ${{ matrix.platform }}", unix_workflow)
        self.assertIn("--python-runtime-dir apps/bootstrap-installer/src-tauri/python-runtime", unix_workflow)
        self.assertIn("apps/bootstrap-installer/src-tauri/python-runtime/*", unix_workflow)


if __name__ == "__main__":
    unittest.main()
