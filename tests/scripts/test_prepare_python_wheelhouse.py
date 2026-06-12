"""Tests for the Python wheelhouse release preparation helper."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "prepare_python_wheelhouse.py"
    spec = importlib.util.spec_from_file_location("_prepare_python_wheelhouse", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class PreparePythonWheelhouseTests(unittest.TestCase):
    """Validate wheelhouse manifest generation and release checks."""

    def test_build_pip_wheel_command_targets_all_extra(self):
        module = _load_script_module()
        repo_root = Path("repo")
        output_dir = Path("wheelhouse")

        command = module.build_pip_wheel_command(repo_root, output_dir, "python3")

        self.assertEqual(
            command,
            [
                "python3",
                "-m",
                "pip",
                "wheel",
                "--wheel-dir",
                str(output_dir),
                ".[all]",
            ],
        )

    def test_manifest_records_wheel_platform_arch_python_size_and_sha256(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")

            prepared = module.prepared_wheel_record("windows", "x64", "cp311", wheel)
            manifest_path = module.write_manifest(output_dir, [prepared])
            payload = json.loads(manifest_path.read_text(encoding="utf-8"))

            self.assertEqual(payload["schemaVersion"], 1)
            self.assertEqual(payload["wheels"][0]["platform"], "windows")
            self.assertEqual(payload["wheels"][0]["arch"], "x64")
            self.assertEqual(payload["wheels"][0]["python"], "cp311")
            self.assertEqual(payload["wheels"][0]["name"], wheel.name)
            self.assertEqual(payload["wheels"][0]["sizeBytes"], len(b"wheel bytes"))
            self.assertEqual(payload["wheels"][0]["sha256"], module.sha256_file(wheel))

    def test_validate_manifest_rejects_checksum_mismatch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")
            prepared = module.prepared_wheel_record("linux", "x64", "cp311", wheel)
            module.write_manifest(output_dir, [prepared])

            self.assertEqual(module.validate_manifest(output_dir), 1)

            wheel.write_bytes(b"WHEEL BYTES")
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                module.validate_manifest(output_dir)

    def test_validate_manifest_rejects_expected_arch_mismatch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")
            prepared = module.prepared_wheel_record("linux", "arm64", "cp311", wheel)
            module.write_manifest(output_dir, [prepared])

            with self.assertRaisesRegex(RuntimeError, "unexpected wheelhouse arch"):
                module.validate_manifest(output_dir, expected_platform="linux", expected_arch="x64")

    def test_validate_payload_rejects_unmanifested_wheel(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            rogue = output_dir / "rogue-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")
            rogue.write_bytes(b"rogue")
            prepared = module.prepared_wheel_record("macos", "arm64", "cp311", wheel)
            module.write_manifest(output_dir, [prepared])

            with self.assertRaisesRegex(RuntimeError, "unmanifested wheelhouse payload"):
                module.validate_payload(output_dir)

    def test_installer_workflows_prepare_validate_and_upload_wheelhouse(self):
        repo_root = Path(__file__).resolve().parents[2]
        workflow_paths = [
            repo_root / ".github" / "workflows" / "build-windows-installer.yml",
            repo_root / ".github" / "workflows" / "build-unix-installers.yml",
        ]

        for workflow_path in workflow_paths:
            text = workflow_path.read_text(encoding="utf-8")
            self.assertIn("scripts/prepare_python_wheelhouse.py", text)
            self.assertIn("--validate-only", text)
            self.assertIn("--arch", text)
            self.assertIn("wheelhouse/wheelhouse-manifest.json", text)
            self.assertIn("wheelhouse/*.whl", text)


if __name__ == "__main__":
    unittest.main()
