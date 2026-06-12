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


if __name__ == "__main__":
    unittest.main()
