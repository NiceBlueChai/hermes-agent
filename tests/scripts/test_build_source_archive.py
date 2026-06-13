"""Tests for the git-backed source archive builder."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "build_source_archive.py"
    spec = importlib.util.spec_from_file_location("_build_source_archive", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class BuildSourceArchiveTests(unittest.TestCase):
    """Validate source archive generation without invoking real git."""

    def test_build_git_source_archive_creates_zip_from_requested_ref(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            calls = []

            def fake_runner(command, cwd, check):
                calls.append((command, cwd, check))
                output_arg = next(arg for arg in command if arg.startswith("--output="))
                prefix_arg = next(arg for arg in command if arg.startswith("--prefix="))
                output = Path(output_arg.removeprefix("--output="))
                prefix = prefix_arg.removeprefix("--prefix=")
                with zipfile.ZipFile(output, "w") as archive:
                    archive.writestr(f"{prefix}README.md", b"source")

            built = module.build_git_source_archive(
                output_dir=root / "dist",
                archive_ref="abcdef123",
                git="git",
                force=False,
                runner=fake_runner,
            )

            self.assertEqual(built.name, "hermes-agent-abcdef123.zip")
            self.assertEqual(built.sha256, module.sha256_file(built.path))
            self.assertEqual(calls[0][0][-1], "abcdef123")
            with zipfile.ZipFile(built.path) as archive:
                self.assertEqual(archive.read("hermes-agent-abcdef123/README.md"), b"source")

    def test_build_git_source_archive_rejects_existing_output_without_force(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output_dir = root / "dist"
            output_dir.mkdir()
            (output_dir / "hermes-agent-main.zip").write_bytes(b"old")

            with self.assertRaisesRegex(RuntimeError, "already exists"):
                module.build_git_source_archive(
                    output_dir=output_dir,
                    archive_ref="main",
                    git="git",
                    force=False,
                )

    def test_safe_archive_ref_rejects_blank_refs(self):
        module = _load_script_module()

        with self.assertRaisesRegex(ValueError, "empty"):
            module.safe_archive_ref("///")


if __name__ == "__main__":
    unittest.main()
