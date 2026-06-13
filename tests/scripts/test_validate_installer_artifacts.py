"""Tests for installer release artifact validation."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "validate_installer_artifacts.py"
    spec = importlib.util.spec_from_file_location("_validate_installer_artifacts", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class ValidateInstallerArtifactsTests(unittest.TestCase):
    """Validate release workflow artifact checks before upload."""

    def test_validate_artifacts_rejects_missing_required_glob(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            with self.assertRaisesRegex(RuntimeError, "missing installer artifact"):
                module.validate_artifacts(root, ["target/release/Hermes-Setup.exe"])

    def test_validate_artifacts_accepts_files_and_bootstrap_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exe = root / "target" / "release" / "Hermes-Setup.exe"
            nsis = root / "target" / "release" / "bundle" / "nsis" / "Hermes Setup.exe"
            tools = root / "bootstrap-tools"
            archive = tools / "uv-x86_64-pc-windows-msvc.zip"
            manifest = tools / "bootstrap-tools-manifest.json"
            exe.parent.mkdir(parents=True)
            nsis.parent.mkdir(parents=True)
            tools.mkdir(parents=True)
            exe.write_bytes(b"setup exe")
            nsis.write_bytes(b"nsis exe")
            archive.write_bytes(b"uv archive")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "x64",
                                "platform": "windows",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": len(b"uv archive"),
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            checked = module.validate_artifacts(
                root,
                [
                    "target/release/Hermes-Setup.exe",
                    "target/release/bundle/nsis/*.exe",
                    "bootstrap-tools/bootstrap-tools-manifest.json",
                ],
                bootstrap_tools_dir=tools,
            )

            self.assertEqual(
                checked,
                [
                    exe,
                    nsis,
                    manifest,
                ],
            )

    def test_validate_artifacts_accepts_non_empty_directory_artifact(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            app_binary = app_bundle / "Contents" / "MacOS" / "Hermes"
            app_binary.parent.mkdir(parents=True)
            app_binary.write_bytes(b"app binary")

            checked = module.validate_artifacts(
                root,
                [
                    "target/release/bundle/macos/*.app",
                ],
            )

            self.assertEqual(checked, [app_bundle])

    def test_validate_artifacts_rejects_macos_app_without_executable(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            resources = app_bundle / "Contents" / "Resources"
            resources.mkdir(parents=True)
            (resources / "marker.txt").write_text("resource", encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "missing macOS app executable"):
                module.validate_artifacts(
                    root,
                    [
                        "target/release/bundle/macos/*.app",
                    ],
                )

    def test_validate_artifacts_rejects_file_above_size_gate(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exe = root / "target" / "release" / "Hermes-Setup.exe"
            exe.parent.mkdir(parents=True)
            exe.write_bytes(b"123456")

            with self.assertRaisesRegex(RuntimeError, "exceeds max artifact bytes"):
                module.validate_artifacts(
                    root,
                    ["target/release/Hermes-Setup.exe"],
                    max_artifact_bytes=5,
                )

    def test_validate_artifacts_rejects_directory_above_size_gate(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            app_binary = app_bundle / "Contents" / "MacOS" / "Hermes"
            resource = app_bundle / "Contents" / "Resources" / "payload.bin"
            app_binary.parent.mkdir(parents=True)
            resource.parent.mkdir(parents=True)
            app_binary.write_bytes(b"123")
            resource.write_bytes(b"456")

            with self.assertRaisesRegex(RuntimeError, "exceeds max artifact bytes"):
                module.validate_artifacts(
                    root,
                    ["target/release/bundle/macos/*.app"],
                    max_artifact_bytes=5,
                )

    def test_validate_artifacts_rejects_total_above_size_gate(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            first = root / "target" / "one.bin"
            second = root / "target" / "two.bin"
            first.parent.mkdir(parents=True)
            first.write_bytes(b"123")
            second.write_bytes(b"456")

            with self.assertRaisesRegex(RuntimeError, "exceeds max total artifact bytes"):
                module.validate_artifacts(
                    root,
                    ["target/*.bin"],
                    max_total_artifact_bytes=5,
                )

    def test_validate_artifacts_size_gate_includes_payload_directories(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = root / "bootstrap-tools"
            archive = tools / "uv-x86_64-pc-windows-msvc.zip"
            manifest = tools / "bootstrap-tools-manifest.json"
            tools.mkdir(parents=True)
            archive.write_bytes(b"x" * 4096)
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "x64",
                                "platform": "windows",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": 4096,
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "exceeds max artifact bytes"):
                module.validate_artifacts(
                    root,
                    ["bootstrap-tools/bootstrap-tools-manifest.json"],
                    bootstrap_tools_dir=tools,
                    max_artifact_bytes=1024,
                )

    def test_validate_artifacts_rejects_empty_directory_artifact(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            app_bundle.mkdir(parents=True)

            with self.assertRaisesRegex(RuntimeError, "installer artifact directory is empty"):
                module.validate_artifacts(
                    root,
                    [
                        "target/release/bundle/macos/*.app",
                    ],
                )

    def test_validate_artifacts_rejects_directory_artifact_with_only_empty_dirs(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            (app_bundle / "Contents" / "MacOS").mkdir(parents=True)

            with self.assertRaisesRegex(RuntimeError, "installer artifact directory has no files"):
                module.validate_artifacts(
                    root,
                    [
                        "target/release/bundle/macos/*.app",
                    ],
                )

    def test_validate_artifacts_rejects_directory_artifact_with_only_empty_files(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            app_bundle = root / "target" / "release" / "bundle" / "macos" / "Hermes.app"
            app_binary = app_bundle / "Contents" / "MacOS" / "Hermes"
            app_binary.parent.mkdir(parents=True)
            app_binary.write_bytes(b"")

            with self.assertRaisesRegex(RuntimeError, "installer artifact directory has no non-empty files"):
                module.validate_artifacts(
                    root,
                    [
                        "target/release/bundle/macos/*.app",
                    ],
                )

    def test_validate_artifacts_allows_bootstrap_readme_and_gitignore(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = root / "bootstrap-tools"
            archive = tools / "uv-x86_64-pc-windows-msvc.zip"
            manifest = tools / "bootstrap-tools-manifest.json"
            tools.mkdir(parents=True)
            archive.write_bytes(b"uv archive")
            (tools / "README.md").write_text("Bootstrap tool docs\n", encoding="utf-8")
            (tools / ".gitignore").write_text("*\n!.gitignore\n!README.md\n", encoding="utf-8")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "x64",
                                "platform": "windows",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": len(b"uv archive"),
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            checked = module.validate_artifacts(
                root,
                [
                    "bootstrap-tools/bootstrap-tools-manifest.json",
                ],
                bootstrap_tools_dir=tools,
            )

            self.assertEqual(checked, [manifest])

    def test_validate_artifacts_rejects_unmanifested_bootstrap_payload(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = root / "bootstrap-tools"
            archive = tools / "uv-x86_64-pc-windows-msvc.zip"
            rogue = tools / "rogue.zip"
            manifest = tools / "bootstrap-tools-manifest.json"
            tools.mkdir(parents=True)
            archive.write_bytes(b"uv archive")
            rogue.write_bytes(b"unexpected payload")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "x64",
                                "platform": "windows",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": len(b"uv archive"),
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "unmanifested bootstrap tool payload"):
                module.validate_artifacts(
                    root,
                    [
                        "bootstrap-tools/bootstrap-tools-manifest.json",
                    ],
                    bootstrap_tools_dir=tools,
                )

    def test_validate_artifacts_rejects_wrong_bootstrap_tools_platform(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = root / "bootstrap-tools"
            archive = tools / "uv-x86_64-unknown-linux-gnu.tar.gz"
            manifest = tools / "bootstrap-tools-manifest.json"
            tools.mkdir(parents=True)
            archive.write_bytes(b"uv archive")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "x64",
                                "platform": "linux",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": len(b"uv archive"),
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "unexpected bootstrap tools platform"):
                module.validate_artifacts(
                    root,
                    [
                        "bootstrap-tools/bootstrap-tools-manifest.json",
                    ],
                    bootstrap_tools_dir=tools,
                    bootstrap_tools_platform="windows",
                )

    def test_validate_artifacts_rejects_wrong_bootstrap_tools_arch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tools = root / "bootstrap-tools"
            archive = tools / "uv-aarch64-pc-windows-msvc.zip"
            manifest = tools / "bootstrap-tools-manifest.json"
            tools.mkdir(parents=True)
            archive.write_bytes(b"uv archive")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "archives": [
                            {
                                "arch": "arm64",
                                "platform": "windows",
                                "name": archive.name,
                                "url": "https://example.invalid/uv.zip",
                                "sizeBytes": len(b"uv archive"),
                                "sha256": module.sha256_file(archive),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "unexpected bootstrap tools arch"):
                module.validate_artifacts(
                    root,
                    [
                        "bootstrap-tools/bootstrap-tools-manifest.json",
                    ],
                    bootstrap_tools_dir=tools,
                    bootstrap_tools_platform="windows",
                    bootstrap_tools_arch="x64",
                )

    def test_validate_artifacts_accepts_wheelhouse_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            wheelhouse = root / "wheelhouse"
            wheel = wheelhouse / "demo-0.1-py3-none-any.whl"
            manifest = wheelhouse / "wheelhouse-manifest.json"
            wheelhouse.mkdir(parents=True)
            wheel.write_bytes(b"wheel bytes")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "wheels": [
                            {
                                "arch": "x64",
                                "platform": "windows",
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

            checked = module.validate_artifacts(
                root,
                [
                    "wheelhouse/wheelhouse-manifest.json",
                ],
                wheelhouse_dir=wheelhouse,
                wheelhouse_platform="windows",
            )

            self.assertEqual(checked, [manifest])

    def test_validate_artifacts_rejects_stale_wheelhouse_sources(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            wheelhouse = root / "wheelhouse"
            wheel = wheelhouse / "demo-0.1-py3-none-any.whl"
            manifest = wheelhouse / "wheelhouse-manifest.json"
            wheelhouse.mkdir(parents=True)
            wheel.write_bytes(b"wheel bytes")
            (root / "pyproject.toml").write_text("[project]\nname = 'demo'\n", encoding="utf-8")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "sourceFiles": [
                            {
                                "path": "pyproject.toml",
                                "sha256": "0" * 64,
                            }
                        ],
                        "wheels": [
                            {
                                "arch": "x64",
                                "platform": "windows",
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

            with self.assertRaisesRegex(RuntimeError, "wheelhouse source hash mismatch"):
                module.validate_artifacts(
                    root,
                    [
                        "wheelhouse/wheelhouse-manifest.json",
                    ],
                    wheelhouse_dir=wheelhouse,
                    wheelhouse_platform="windows",
                    wheelhouse_arch="x64",
                )

    def test_validate_artifacts_rejects_unmanifested_wheelhouse_payload(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            wheelhouse = root / "wheelhouse"
            wheel = wheelhouse / "demo-0.1-py3-none-any.whl"
            rogue = wheelhouse / "rogue-0.1-py3-none-any.whl"
            manifest = wheelhouse / "wheelhouse-manifest.json"
            wheelhouse.mkdir(parents=True)
            wheel.write_bytes(b"wheel bytes")
            rogue.write_bytes(b"rogue")
            manifest.write_text(
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

            with self.assertRaisesRegex(RuntimeError, "unmanifested wheelhouse payload"):
                module.validate_artifacts(
                    root,
                    [
                        "wheelhouse/wheelhouse-manifest.json",
                    ],
                    wheelhouse_dir=wheelhouse,
                    wheelhouse_platform="linux",
                )

    def test_validate_artifacts_rejects_wheelhouse_arch_mismatch(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            wheelhouse = root / "wheelhouse"
            wheel = wheelhouse / "demo-0.1-py3-none-any.whl"
            manifest = wheelhouse / "wheelhouse-manifest.json"
            wheelhouse.mkdir(parents=True)
            wheel.write_bytes(b"wheel bytes")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "wheels": [
                            {
                                "arch": "arm64",
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

            with self.assertRaisesRegex(RuntimeError, "unexpected wheelhouse arch"):
                module.validate_artifacts(
                    root,
                    [
                        "wheelhouse/wheelhouse-manifest.json",
                    ],
                    wheelhouse_dir=wheelhouse,
                    wheelhouse_platform="linux",
                    wheelhouse_arch="x64",
                )

    def test_validate_artifacts_accepts_python_runtime_manifest(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = root / "python-runtime"
            python = runtime / "python.exe"
            manifest = runtime / "python-runtime-manifest.json"
            runtime.mkdir(parents=True)
            python.write_bytes(b"python runtime")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "platform": "windows",
                        "arch": "x64",
                        "pythonTag": "cp311",
                        "files": [
                            {
                                "name": python.name,
                                "url": "https://example.invalid/python-runtime.zip",
                                "sizeBytes": len(b"python runtime"),
                                "sha256": module.sha256_file(python),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            checked = module.validate_artifacts(
                root,
                ["python-runtime/python-runtime-manifest.json"],
                python_runtime_dir=runtime,
                python_runtime_platform="windows",
                python_runtime_arch="x64",
            )

            self.assertEqual(checked, [manifest])

    def test_validate_artifacts_rejects_unmanifested_python_runtime_payload(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            runtime = root / "python-runtime"
            python = runtime / "python.exe"
            rogue = runtime / "rogue.dll"
            manifest = runtime / "python-runtime-manifest.json"
            runtime.mkdir(parents=True)
            python.write_bytes(b"python runtime")
            rogue.write_bytes(b"rogue")
            manifest.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "platform": "windows",
                        "arch": "x64",
                        "pythonTag": "cp311",
                        "files": [
                            {
                                "name": python.name,
                                "url": "https://example.invalid/python-runtime.zip",
                                "sizeBytes": len(b"python runtime"),
                                "sha256": module.sha256_file(python),
                            }
                        ],
                    },
                    indent=2,
                )
                + "\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(RuntimeError, "unmanifested python runtime payload"):
                module.validate_artifacts(
                    root,
                    ["python-runtime/python-runtime-manifest.json"],
                    python_runtime_dir=runtime,
                    python_runtime_platform="windows",
                    python_runtime_arch="x64",
                )

    def test_installer_workflows_enforce_artifact_size_gates(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("--max-artifact-bytes 2147483648", windows_workflow)
        self.assertIn("--max-total-artifact-bytes 3221225472", windows_workflow)
        self.assertIn("--max-artifact-bytes 536870912", windows_workflow)

        self.assertIn("--max-artifact-bytes 2147483648", unix_workflow)
        self.assertIn("--max-total-artifact-bytes 3221225472", unix_workflow)
        self.assertIn("--max-artifact-bytes 536870912", unix_workflow)


if __name__ == "__main__":
    unittest.main()
