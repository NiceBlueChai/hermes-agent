"""Tests for the Python runtime release preparation helper."""

from __future__ import annotations

import importlib.util
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


if __name__ == "__main__":
    unittest.main()
