"""Tests for the bootstrap tool archive preparation helper."""

from __future__ import annotations

import importlib.util
import json
import sys
import unittest
from pathlib import Path


def _load_script_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "prepare_bootstrap_tools.py"
    spec = importlib.util.spec_from_file_location("_prepare_bootstrap_tools", script_path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class PrepareBootstrapToolsTests(unittest.TestCase):
    """Validate archive naming logic used by the release preparation helper."""

    def test_select_latest_node_archive_filters_major_and_arch(self):
        module = _load_script_module()
        html = """
            <a href="node-v22.18.0-win-x64.zip">node-v22.18.0-win-x64.zip</a>
            <a href="node-v22.19.1-win-arm64.zip">node-v22.19.1-win-arm64.zip</a>
            <a href="node-v22.19.0-win-x64.zip">node-v22.19.0-win-x64.zip</a>
            <a href="node-v21.99.0-win-x64.zip">node-v21.99.0-win-x64.zip</a>
        """

        self.assertEqual(
            module.select_latest_node_archive(html, "x64"),
            "node-v22.19.0-win-x64.zip",
        )

    def test_select_latest_unix_node_archive_prefers_gz(self):
        module = _load_script_module()
        html = """
            <a href="node-v22.18.0-linux-x64.tar.gz">node-v22.18.0-linux-x64.tar.gz</a>
            <a href="node-v22.19.1-linux-arm64.tar.xz">node-v22.19.1-linux-arm64.tar.xz</a>
            <a href="node-v22.19.2-linux-x64.tar.gz">node-v22.19.2-linux-x64.tar.gz</a>
            <a href="node-v22.19.1-linux-x64.tar.xz">node-v22.19.1-linux-x64.tar.xz</a>
            <a href="node-v21.99.0-linux-x64.tar.xz">node-v21.99.0-linux-x64.tar.xz</a>
        """

        self.assertEqual(
            module.select_latest_unix_node_archive(html, "linux", "x64"),
            "node-v22.19.2-linux-x64.tar.gz",
        )

    def test_archive_specs_match_installer_runtime_assets(self):
        module = _load_script_module()

        x64_specs = module.archive_specs_for_arch("x64", "node-v22.19.0-win-x64.zip")
        x64_names = [spec.name for spec in x64_specs]

        self.assertEqual(
            x64_names,
            [
                "node-v22.19.0-win-x64.zip",
                "uv-x86_64-pc-windows-msvc.zip",
                "ripgrep-15.1.0-x86_64-pc-windows-msvc.zip",
                "PortableGit-2.54.0-64-bit.7z.exe",
            ],
        )
        self.assertTrue(x64_specs[0].url.endswith("/latest-v22.x/node-v22.19.0-win-x64.zip"))
        self.assertTrue(
            x64_specs[2].url.endswith("/15.1.0/ripgrep-15.1.0-x86_64-pc-windows-msvc.zip")
        )
        self.assertTrue(
            x64_specs[3].url.endswith("/v2.54.0.windows.1/PortableGit-2.54.0-64-bit.7z.exe")
        )

        arm64_specs = module.archive_specs_for_arch("arm64", "node-v22.19.0-win-arm64.zip")
        self.assertEqual(
            [spec.name for spec in arm64_specs],
            [
                "node-v22.19.0-win-arm64.zip",
                "uv-aarch64-pc-windows-msvc.zip",
                "ripgrep-15.1.0-aarch64-pc-windows-msvc.zip",
                "PortableGit-2.54.0-arm64.7z.exe",
            ],
        )

    def test_archive_specs_reject_unknown_architecture(self):
        module = _load_script_module()

        with self.assertRaisesRegex(ValueError, "unsupported Windows architecture"):
            module.archive_specs_for_arch("mips", "node-v22.19.0-win-mips.zip")

    def test_unix_archive_specs_match_uv_runtime_assets(self):
        module = _load_script_module()

        linux_specs = module.archive_specs_for_target(
            "linux",
            "x64",
            "node-v22.19.1-linux-x64.tar.gz",
        )
        self.assertEqual(
            [spec.name for spec in linux_specs],
            [
                "node-v22.19.1-linux-x64.tar.gz",
                "uv-x86_64-unknown-linux-gnu.tar.gz",
                "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz",
            ],
        )
        self.assertTrue(
            linux_specs[1].url.endswith("/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz")
        )
        self.assertTrue(
            linux_specs[2].url.endswith("/15.1.0/ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz")
        )

        mac_specs = module.archive_specs_for_target(
            "macos",
            "arm64",
            "node-v22.19.1-darwin-arm64.tar.gz",
        )
        self.assertEqual(
            [spec.name for spec in mac_specs],
            [
                "node-v22.19.1-darwin-arm64.tar.gz",
                "uv-aarch64-apple-darwin.tar.gz",
                "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz",
            ],
        )

        with self.assertRaisesRegex(ValueError, "unsupported Unix uv platform"):
            module.archive_specs_for_target("linux", "x86", "node-v22.19.1-linux-x86.tar.gz")

    def test_required_tool_kinds_match_release_targets(self):
        module = _load_script_module()

        self.assertEqual(
            module.required_tool_kinds_for_target("windows", "x64"),
            {"git", "node", "ripgrep", "uv"},
        )
        self.assertEqual(
            module.required_tool_kinds_for_target("linux", "x64"),
            {"node", "ripgrep", "uv"},
        )
        self.assertEqual(
            module.required_tool_kinds_for_target("macos", "arm64"),
            {"node", "ripgrep", "uv"},
        )

    def test_ffmpeg_archive_names_have_target_metadata_without_being_required(self):
        module = _load_script_module()

        self.assertEqual(
            module.archive_target_from_name("ffmpeg-windows-x64.zip"),
            ("windows", "x64"),
        )
        self.assertEqual(
            module.archive_target_from_name("ffmpeg-linux-arm64.tar.gz"),
            ("linux", "arm64"),
        )
        self.assertEqual(module.archive_tool_kind_from_name("ffmpeg-macos-x64.tar.gz"), "ffmpeg")
        self.assertNotIn("ffmpeg", module.required_tool_kinds_for_target("macos", "x64"))

    def test_prepare_local_archive_copies_optional_ffmpeg_into_manifest(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-local-archive-test")
        source_dir = root / "source"
        output_dir = root / "bootstrap-tools"
        source_dir.mkdir(parents=True, exist_ok=True)
        output_dir.mkdir(parents=True, exist_ok=True)
        source = source_dir / "ffmpeg-windows-x64.zip"
        source.write_bytes(b"ffmpeg archive")

        try:
            prepared = module.prepare_local_archives(
                output_dir,
                [f"{source}=https://example.invalid/ffmpeg-windows-x64.zip"],
                dry_run=False,
            )
            module.write_manifest(output_dir, prepared)

            self.assertEqual(len(prepared), 1)
            self.assertEqual(prepared[0].platform, "windows")
            self.assertEqual(prepared[0].arch, "x64")
            self.assertEqual(prepared[0].name, "ffmpeg-windows-x64.zip")
            self.assertEqual(prepared[0].url, "https://example.invalid/ffmpeg-windows-x64.zip")
            self.assertEqual((output_dir / "ffmpeg-windows-x64.zip").read_bytes(), b"ffmpeg archive")
            self.assertEqual(module.validate_manifest(output_dir), 1)
        finally:
            for entry in output_dir.glob("*"):
                entry.unlink()
            for entry in source_dir.glob("*"):
                entry.unlink()
            output_dir.rmdir()
            source_dir.rmdir()
            root.rmdir()

    def test_manifest_records_archive_platform_size_and_sha256(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")

        try:
            spec = module.ArchiveSpec(
                name="uv-x86_64-pc-windows-msvc.zip",
                url="https://example.invalid/uv.zip",
            )
            prepared = module.prepared_archive_record("windows", "x64", spec, archive)
            manifest_path = module.write_manifest(output_dir, [prepared])
            payload = json.loads(manifest_path.read_text(encoding="utf-8"))

            self.assertEqual(payload["schemaVersion"], 1)
            self.assertEqual(payload["archives"][0]["platform"], "windows")
            self.assertEqual(payload["archives"][0]["arch"], "x64")
            self.assertEqual(payload["archives"][0]["name"], "uv-x86_64-pc-windows-msvc.zip")
            self.assertEqual(payload["archives"][0]["sizeBytes"], len(b"uv archive"))
            self.assertEqual(
                payload["archives"][0]["sha256"],
                "ba8cad66b72bd2f5aabb165b4b2c0a935637a8f629025bbfa1caf739f6706ed5",
            )
        finally:
            if archive.exists():
                archive.unlink()
            manifest = output_dir / "bootstrap-tools-manifest.json"
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_checksum_mismatch(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-validate-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")

        try:
            spec = module.ArchiveSpec(
                name="uv-x86_64-pc-windows-msvc.zip",
                url="https://example.invalid/uv.zip",
            )
            prepared = module.prepared_archive_record("windows", "x64", spec, archive)
            module.write_manifest(output_dir, [prepared])

            self.assertEqual(module.validate_manifest(output_dir), 1)

            archive.write_bytes(b"badarchive")
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            manifest = output_dir / "bootstrap-tools-manifest.json"
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_without_url(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-url-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "x64",
                            "platform": "windows",
                            "name": "uv-x86_64-pc-windows-msvc.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "missing url"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_with_insecure_url(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-url-scheme-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "x64",
                            "platform": "windows",
                            "name": "uv-x86_64-pc-windows-msvc.zip",
                            "url": "http://example.invalid/uv.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "invalid url"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_without_arch(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-arch-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "platform": "windows",
                            "name": "uv-x86_64-pc-windows-msvc.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "missing arch"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_without_platform(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-platform-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "x64",
                            "name": "uv-x86_64-pc-windows-msvc.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "missing platform"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_archive_target_mismatch(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-target-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "x64",
                            "platform": "linux",
                            "name": "uv-x86_64-pc-windows-msvc.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "target mismatch"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_requires_all_release_tool_kinds(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-required-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)

        try:
            prepared = []
            for spec in module.archive_specs_for_arch("x64", "node-v22.19.0-win-x64.zip")[:-1]:
                archive = output_dir / spec.name
                archive.write_bytes(spec.name.encode("utf-8"))
                prepared.append(module.prepared_archive_record("windows", "x64", spec, archive))
            module.write_manifest(output_dir, prepared)

            with self.assertRaisesRegex(RuntimeError, "missing required bootstrap tool archive: git"):
                module.validate_manifest(output_dir, expected_platform="windows", expected_arch="x64")
        finally:
            for entry in output_dir.iterdir():
                entry.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_expected_arch_mismatch(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-arch-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-aarch64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "arm64",
                            "platform": "windows",
                            "name": "uv-aarch64-pc-windows-msvc.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "unexpected bootstrap tools arch"):
                module.validate_manifest(output_dir, expected_platform="windows", expected_arch="x64")
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_duplicate_archive_names(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-duplicate-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = output_dir / "uv-x86_64-pc-windows-msvc.zip"
        archive.write_bytes(b"uv archive")
        archive_record = {
            "arch": "x64",
            "platform": "windows",
            "name": "uv-x86_64-pc-windows-msvc.zip",
            "url": "https://example.invalid/uv.zip",
            "sizeBytes": len(b"uv archive"),
            "sha256": module.sha256_file(archive),
        }
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [archive_record, dict(archive_record)],
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        try:
            with self.assertRaisesRegex(RuntimeError, "duplicate archive"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_validate_manifest_rejects_unsafe_archive_name(self):
        module = _load_script_module()
        root = Path("tmp-bootstrap-tools-unsafe-name-test")
        output_dir = root / "bootstrap-tools"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive = root / "uv.zip"
        archive.write_bytes(b"uv archive")
        manifest = output_dir / "bootstrap-tools-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schemaVersion": 1,
                    "archives": [
                        {
                            "arch": "x64",
                            "platform": "windows",
                            "name": "../uv.zip",
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

        try:
            with self.assertRaisesRegex(RuntimeError, "unsafe archive name"):
                module.validate_manifest(output_dir)
        finally:
            if archive.exists():
                archive.unlink()
            if manifest.exists():
                manifest.unlink()
            output_dir.rmdir()
            root.rmdir()

    def test_installer_workflows_upload_bootstrap_tools_manifest(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow_path = repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        windows_workflow = windows_workflow_path.read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")
        manifest_path = "apps/bootstrap-installer/src-tauri/bootstrap-tools/bootstrap-tools-manifest.json"
        archive_globs = [
            "apps/bootstrap-installer/src-tauri/bootstrap-tools/*.zip",
            "apps/bootstrap-installer/src-tauri/bootstrap-tools/*.tar.gz",
            "apps/bootstrap-installer/src-tauri/bootstrap-tools/*.7z.exe",
        ]

        self.assertIn(manifest_path, windows_workflow)
        self.assertIn(manifest_path, unix_workflow)
        self.assertIn(archive_globs[0], windows_workflow)
        self.assertIn(archive_globs[2], windows_workflow)
        self.assertIn(archive_globs[1], unix_workflow)
        self.assertNotIn(
            "apps/bootstrap-installer/src-tauri/bootstrap-tools/*",
            [line.strip() for line in windows_workflow.splitlines()],
        )
        self.assertNotIn(
            "apps/bootstrap-installer/src-tauri/bootstrap-tools/*",
            [line.strip() for line in unix_workflow.splitlines()],
        )
        self.assertIn("python scripts/prepare_bootstrap_tools.py", windows_workflow)
        self.assertIn("python scripts/prepare_bootstrap_tools.py", unix_workflow)
        self.assertIn("--validate-only", windows_workflow)
        self.assertIn("--validate-only", unix_workflow)
        self.assertIn("--bootstrap-tools-platform windows", windows_workflow)
        self.assertIn("--bootstrap-tools-platform ${{ matrix.platform }}", unix_workflow)
        self.assertIn("--bootstrap-tools-arch x64", windows_workflow)
        self.assertIn("--bootstrap-tools-arch ${{ runner.arch", unix_workflow)
        upload_sections = [
            section
            for section in windows_workflow.split("\n      - name: ")
            if "actions/upload-artifact@" in section
        ]
        self.assertGreaterEqual(len(upload_sections), 3)
        for section in upload_sections:
            self.assertIn("if-no-files-found: error", section)

    def test_installer_workflows_pin_bootstrap_builds_to_current_commit(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")
        pin = "HERMES_BUILD_PIN_COMMIT: ${{ github.sha }}"

        self.assertIn(pin, windows_workflow)
        self.assertIn(pin, unix_workflow)

    def test_installer_workflows_run_built_binary_self_check(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("Hermes-Setup.exe --self-check", windows_workflow)
        self.assertIn("Hermes-Setup --self-check", unix_workflow)
        self.assertIn("--self-check-expect-commit \"${{ github.sha }}\"", windows_workflow)
        self.assertIn("--self-check-expect-commit \"${{ github.sha }}\"", unix_workflow)
        self.assertIn(
            "--self-check-bootstrap-tools apps/bootstrap-installer/src-tauri/bootstrap-tools",
            windows_workflow,
        )
        self.assertIn(
            "--self-check-bootstrap-tools apps/bootstrap-installer/src-tauri/bootstrap-tools",
            unix_workflow,
        )
        self.assertIn("--self-check-bootstrap-tools-arch x64", windows_workflow)
        self.assertIn("--self-check-bootstrap-tools-arch ${{ runner.arch", unix_workflow)
        self.assertIn(
            "--self-check-wheelhouse apps/bootstrap-installer/src-tauri/wheelhouse",
            windows_workflow,
        )
        self.assertIn(
            "--self-check-wheelhouse apps/bootstrap-installer/src-tauri/wheelhouse",
            unix_workflow,
        )
        self.assertIn("--self-check-wheelhouse-arch x64", windows_workflow)
        self.assertIn("--self-check-wheelhouse-arch ${{ runner.arch", unix_workflow)
        self.assertIn("Smoke built installer lifecycle", windows_workflow)
        self.assertIn("Smoke built installer lifecycle", unix_workflow)
        self.assertIn("Hermes-Setup.exe --self-check-lifecycle", windows_workflow)
        self.assertIn("Hermes-Setup --self-check-lifecycle", unix_workflow)
        self.assertGreater(
            windows_workflow.index("- name: Smoke built installer binary"),
            windows_workflow.index("- name: Sign Hermes-Setup.exe with Azure Artifact Signing"),
        )
        self.assertGreater(
            windows_workflow.index("- name: Smoke built installer lifecycle"),
            windows_workflow.index("- name: Smoke built installer binary"),
        )
        self.assertGreater(
            unix_workflow.index("- name: Smoke built installer lifecycle"),
            unix_workflow.index("- name: Smoke built installer binary"),
        )

    def test_lifecycle_workflow_smokes_bootstrap_release_binary(self):
        repo_root = Path(__file__).resolve().parents[2]
        workflow = (repo_root / ".github" / "workflows" / "tests.yml").read_text(encoding="utf-8")

        self.assertIn("Build bootstrap release binary", workflow)
        self.assertIn("cargo build --release --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml", workflow)
        self.assertIn("Run bootstrap release binary self-check", workflow)
        self.assertIn("--self-check-expect-commit \"${{ github.sha }}\"", workflow)
        self.assertIn("Run bootstrap release binary lifecycle self-check", workflow)
        self.assertIn("--self-check-lifecycle", workflow)
        self.assertGreater(
            workflow.index("- name: Run bootstrap release binary self-check"),
            workflow.index("- name: Build bootstrap release binary"),
        )
        self.assertGreater(
            workflow.index("- name: Run bootstrap release binary lifecycle self-check"),
            workflow.index("- name: Build bootstrap release binary"),
        )


if __name__ == "__main__":
    unittest.main()
