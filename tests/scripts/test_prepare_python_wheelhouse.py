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

    def test_build_pip_wheel_command_targets_all_extra_with_constraints(self):
        module = _load_script_module()
        repo_root = Path("repo")
        output_dir = Path("wheelhouse")
        constraints = Path("constraints.txt")

        command = module.build_pip_wheel_command(repo_root, output_dir, "python3", constraints)

        self.assertEqual(
            command,
            [
                "python3",
                "-m",
                "pip",
                "wheel",
                "--wheel-dir",
                str(output_dir),
                "--constraint",
                str(constraints),
                ".[all]",
            ],
        )

    def test_build_locked_constraint_lines_uses_registry_packages_from_uv_lock(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            repo_root = Path(tmp) / "repo"
            repo_root.mkdir()
            (repo_root / "uv.lock").write_text(
                """
version = 1

[[package]]
name = "hermes-agent"
version = "0.16.0"
source = { editable = "." }

[[package]]
name = "Requests"
version = "2.33.0"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "urllib3"
version = "2.7.0"
source = { registry = "https://pypi.org/simple" }
""".lstrip(),
                encoding="utf-8",
            )

            self.assertEqual(
                module.build_locked_constraint_lines(repo_root),
                [
                    "Requests==2.33.0",
                    "urllib3==2.7.0",
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

    def test_manifest_records_and_validates_dependency_source_hashes(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            repo_root = Path(tmp) / "repo"
            output_dir = Path(tmp) / "wheelhouse"
            repo_root.mkdir()
            output_dir.mkdir()
            (repo_root / "pyproject.toml").write_text("[project]\nname = 'demo'\n", encoding="utf-8")
            (repo_root / "uv.lock").write_text("version = 1\n", encoding="utf-8")
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")

            prepared = module.prepared_wheel_record("linux", "x64", "cp311", wheel)
            manifest_path = module.write_manifest(
                output_dir,
                [prepared],
                source_files=module.source_file_records(repo_root),
            )
            payload = json.loads(manifest_path.read_text(encoding="utf-8"))

            self.assertEqual(
                {entry["path"] for entry in payload["sourceFiles"]},
                {"pyproject.toml", "uv.lock"},
            )
            self.assertEqual(module.validate_manifest(output_dir, repo_root=repo_root), 1)

            (repo_root / "uv.lock").write_text("version = 2\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "wheelhouse source hash mismatch"):
                module.validate_manifest(output_dir, repo_root=repo_root)

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

    def test_validate_manifest_rejects_boolean_wheel_size(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / "demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"x")
            (output_dir / "wheelhouse-manifest.json").write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "wheels": [
                            {
                                "arch": "x64",
                                "platform": "linux",
                                "python": "cp311",
                                "name": wheel.name,
                                "sizeBytes": True,
                                "sha256": module.sha256_file(wheel),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "invalid sizeBytes"):
                module.validate_manifest(output_dir)

    def test_validate_manifest_rejects_padded_wheel_name_as_unsafe(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp) / "wheelhouse"
            output_dir.mkdir()
            wheel = output_dir / " demo-0.1-py3-none-any.whl"
            wheel.write_bytes(b"wheel bytes")
            (output_dir / "wheelhouse-manifest.json").write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "wheels": [
                            {
                                "arch": "x64",
                                "platform": "linux",
                                "python": "cp311",
                                "name": wheel.name,
                                "sizeBytes": len(b"wheel bytes"),
                                "sha256": module.sha256_file(wheel),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "unsafe wheel name"):
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
            wheelhouse_upload_sections = [
                section
                for section in text.split("- name: ")
                if "Upload Python wheelhouse" in section and "actions/upload-artifact@" in section
            ]
            self.assertTrue(wheelhouse_upload_sections)
            for section in wheelhouse_upload_sections:
                self.assertIn("if-no-files-found: error", section)


if __name__ == "__main__":
    unittest.main()
