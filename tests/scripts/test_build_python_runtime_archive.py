"""Tests for the uv-backed Python runtime archive builder."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "build_python_runtime_archive.py"
    spec = importlib.util.spec_from_file_location("_build_python_runtime_archive", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class BuildPythonRuntimeArchiveTests(unittest.TestCase):
    """Validate Python runtime archive generation without invoking real uv."""

    def test_build_uv_runtime_archive_creates_windows_zip_from_staged_python(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            calls = []

            def fake_runner(command, check, env, stdout=None, text=False):
                calls.append((command, stdout, text))
                install_dir = Path(env["UV_PYTHON_INSTALL_DIR"])
                python = install_dir / "cpython-3.11" / "python.exe"
                if command[1:3] == ["python", "install"]:
                    python.parent.mkdir(parents=True)
                    python.write_bytes(b"python runtime")
                    return subprocess.CompletedProcess(command, 0)
                return subprocess.CompletedProcess(command, 0, stdout=f"{python}\n")

            built = module.build_uv_runtime_archive(
                output_dir=root / "dist",
                work_dir=root / "work",
                platform="windows",
                arch="x64",
                python_version="3.11",
                uv="uv",
                force=False,
                runner=fake_runner,
            )

            self.assertEqual(built.name, "python-runtime-windows-x64.zip")
            self.assertEqual(built.sha256, module.sha256_file(built.path))
            self.assertEqual(calls[0][0], ["uv", "python", "install", "3.11"])
            self.assertEqual(calls[1][0], ["uv", "python", "find", "3.11"])
            with zipfile.ZipFile(built.path) as archive:
                self.assertEqual(archive.read("cpython-3.11/python.exe"), b"python runtime")

    def test_build_uv_runtime_archive_creates_unix_tar_gz(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            def fake_runner(command, check, env, stdout=None, text=False):
                install_dir = Path(env["UV_PYTHON_INSTALL_DIR"])
                python = install_dir / "cpython-3.11" / "bin" / "python"
                if command[1:3] == ["python", "install"]:
                    python.parent.mkdir(parents=True)
                    python.write_bytes(b"python runtime")
                    return subprocess.CompletedProcess(command, 0)
                return subprocess.CompletedProcess(command, 0, stdout=f"{python}\n")

            built = module.build_uv_runtime_archive(
                output_dir=root / "dist",
                work_dir=root / "work",
                platform="linux",
                arch="x64",
                python_version="3.11",
                uv="uv",
                force=False,
                runner=fake_runner,
            )

            self.assertEqual(built.name, "python-runtime-linux-x64.tar.gz")
            with tarfile.open(built.path, "r:gz") as archive:
                member = archive.extractfile("cpython-3.11/bin/python")
                assert member is not None
                self.assertEqual(member.read(), b"python runtime")

    def test_build_uv_runtime_archive_rejects_existing_output_without_force(self):
        module = _load_script_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output_dir = root / "dist"
            output_dir.mkdir()
            (output_dir / "python-runtime-windows-x64.zip").write_bytes(b"old")

            with self.assertRaisesRegex(RuntimeError, "already exists"):
                module.build_uv_runtime_archive(
                    output_dir=output_dir,
                    work_dir=root / "work",
                    platform="windows",
                    arch="x64",
                    python_version="3.11",
                    uv="uv",
                    force=False,
                )


if __name__ == "__main__":
    unittest.main()
