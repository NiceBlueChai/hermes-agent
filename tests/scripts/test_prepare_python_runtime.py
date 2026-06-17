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


def _load_default_gate_module():
    repo_root = Path(__file__).resolve().parents[2]
    script_path = repo_root / "scripts" / "validate_python_runtime_default_gate.py"
    spec = importlib.util.spec_from_file_location("_validate_python_runtime_default_gate", script_path)
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
        self.assertIn("unix-smoke-only:", windows_workflow)
        self.assertIn("if: ${{ !inputs['unix-smoke-only'] }}", windows_workflow)
        self.assertIn("Unix packaged runtime smoke", windows_workflow)
        self.assertIn("os: ubuntu-latest", windows_workflow)
        self.assertIn("os: macos-latest", windows_workflow)
        self.assertIn("Smoke Unix packaged runtime lifecycle", windows_workflow)
        self.assertIn("find apps/bootstrap-installer/src-tauri/target/release/bundle \\", windows_workflow)
        self.assertIn("-path '*/Contents/MacOS/*' -type f -print -quit", windows_workflow)
        self.assertIn('resource_root="${GITHUB_WORKSPACE}/apps/bootstrap-installer/src-tauri"', windows_workflow)
        self.assertIn('--self-check-bootstrap-tools "${resource_root}/bootstrap-tools"', windows_workflow)
        self.assertIn('--self-check-wheelhouse "${resource_root}/wheelhouse"', windows_workflow)
        self.assertIn('--self-check-python-runtime "${resource_root}/python-runtime"', windows_workflow)

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

    def test_python_runtime_default_gate_is_validated_in_release_workflows(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")
        module = _load_default_gate_module()

        module.validate_default_gate_doc(repo_root / "docs" / "release" / "python-runtime-default-gate.md")
        self.assertIn("python scripts/validate_python_runtime_default_gate.py", windows_workflow)
        self.assertIn("python scripts/validate_python_runtime_default_gate.py", unix_workflow)

    def test_signed_release_workflows_upload_python_runtime_default_evidence(self):
        repo_root = Path(__file__).resolve().parents[2]
        windows_workflow = (
            repo_root / ".github" / "workflows" / "build-windows-installer.yml"
        ).read_text(encoding="utf-8")
        unix_workflow = (
            repo_root / ".github" / "workflows" / "build-unix-installers.yml"
        ).read_text(encoding="utf-8")

        self.assertIn("Print signed Python runtime default evidence", windows_workflow)
        self.assertIn("Upload signed Python runtime default evidence", windows_workflow)
        self.assertIn("python-runtime-default-evidence-windows.json", windows_workflow)
        self.assertIn("--print-evidence", windows_workflow)
        self.assertIn("Print signed Python runtime default evidence", unix_workflow)
        self.assertIn("Upload signed Python runtime default evidence", unix_workflow)
        self.assertIn("python-runtime-default-evidence-${{ matrix.platform }}.json", unix_workflow)
        self.assertIn("--print-evidence", unix_workflow)

    def test_python_runtime_default_gate_rejects_missing_release_decision_details(self):
        module = _load_default_gate_module()
        repo_root = Path(__file__).resolve().parents[2]
        doc_text = (repo_root / "docs" / "release" / "python-runtime-default-gate.md").read_text(encoding="utf-8")
        mutations = {
            "release manifest audit": doc_text.replace(
                "actual `python-runtime-manifest.json` size, SHA-256, Python tag, platform, and arch",
                "runtime manifest summary",
            ),
            "signed installer size comparison": doc_text.replace(
                "compare the signed installer size with and without the runtime bundle",
                "record installer size notes",
            ),
            "security rebuild policy": doc_text.replace(
                "must be rebuilt when the bundled Python patch release receives a security update",
                "must be reviewed when the bundled Python patch release receives a security update",
            ),
            "release notes runtime source": doc_text.replace(
                "identify the Python runtime version and the archive source used for the signed build",
                "mention the bundled runtime",
            ),
        }

        with tempfile.TemporaryDirectory() as tmp:
            for expected_label, mutated_text in mutations.items():
                doc_path = Path(tmp) / f"{expected_label.replace(' ', '-')}.md"
                doc_path.write_text(mutated_text, encoding="utf-8")

                with self.subTest(expected_label=expected_label):
                    with self.assertRaisesRegex(RuntimeError, expected_label):
                        module.validate_default_gate_doc(doc_path)

    def test_python_runtime_default_gate_validates_structured_release_evidence(self):
        module = _load_default_gate_module()
        evidence = {
            "pythonRuntime": {
                "version": "3.11.9",
                "sourceUrl": "https://example.invalid/python-runtime-windows-x64.zip",
                "archiveSha256": "a" * 64,
                "securityUpdatePolicy": (
                    "Runtime archive must be rebuilt when the bundled Python patch release "
                    "receives a security update."
                ),
                "manifest": {
                    "platform": "windows",
                    "arch": "x64",
                    "pythonTag": "cp311",
                    "files": [
                        {
                            "name": "python-runtime-windows-x64.zip",
                            "url": "https://example.invalid/python-runtime-windows-x64.zip",
                            "sizeBytes": 100,
                            "sha256": "a" * 64,
                        }
                    ],
                },
            },
            "signedInstaller": {
                "withRuntimeBytes": 400,
                "withoutRuntimeBytes": 250,
                "sizeDeltaBytes": 150,
            },
        }

        with tempfile.TemporaryDirectory() as tmp:
            evidence_path = Path(tmp) / "evidence.json"
            evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

            module.validate_release_evidence(evidence_path)

    def test_python_runtime_default_gate_builds_structured_release_evidence_from_manifest(self):
        module = _load_default_gate_module()
        manifest = {
            "schemaVersion": 1,
            "platform": "windows",
            "arch": "x64",
            "pythonTag": "cp311",
            "files": [
                {
                    "name": "python-runtime-windows-x64.zip",
                    "url": "https://example.invalid/python-runtime-windows-x64.zip",
                    "sizeBytes": 100,
                    "sha256": "a" * 64,
                }
            ],
        }

        with tempfile.TemporaryDirectory() as tmp:
            evidence = module.build_release_evidence(
                manifest,
                python_version="3.11.9",
                security_update_policy=(
                    "Runtime archive must be rebuilt when the bundled Python patch release receives a security update."
                ),
                with_runtime_bytes=400,
                without_runtime_bytes=250,
            )
            evidence_path = Path(tmp) / "runtime-evidence.json"
            evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

            self.assertEqual(evidence["pythonRuntime"]["sourceUrl"], manifest["files"][0]["url"])
            self.assertEqual(evidence["pythonRuntime"]["archiveSha256"], manifest["files"][0]["sha256"])
            self.assertEqual(evidence["signedInstaller"]["sizeDeltaBytes"], 150)
            module.validate_release_evidence(evidence_path)

    def test_python_runtime_default_gate_rejects_incomplete_structured_release_evidence(self):
        module = _load_default_gate_module()
        evidence = {
            "pythonRuntime": {
                "version": "3.11.9",
                "sourceUrl": "https://example.invalid/python-runtime-windows-x64.zip",
                "archiveSha256": "a" * 64,
                "securityUpdatePolicy": "Rebuild promptly after Python patch security updates.",
                "manifest": {
                    "platform": "windows",
                    "arch": "x64",
                    "pythonTag": "cp311",
                    "files": [
                        {
                            "name": "python-runtime-windows-x64.zip",
                            "url": "https://example.invalid/python-runtime-windows-x64.zip",
                            "sizeBytes": 100,
                            "sha256": "a" * 64,
                        }
                    ],
                },
            },
            "signedInstaller": {
                "withRuntimeBytes": 400,
                "withoutRuntimeBytes": 250,
                "sizeDeltaBytes": 149,
            },
        }

        with tempfile.TemporaryDirectory() as tmp:
            evidence_path = Path(tmp) / "evidence.json"
            evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "sizeDeltaBytes"):
                module.validate_release_evidence(evidence_path)

    def test_python_runtime_default_gate_rejects_non_positive_runtime_size_delta(self):
        module = _load_default_gate_module()
        evidence = {
            "pythonRuntime": {
                "version": "3.11.9",
                "sourceUrl": "https://example.invalid/python-runtime-windows-x64.zip",
                "archiveSha256": "a" * 64,
                "securityUpdatePolicy": "Rebuild promptly after Python patch security updates.",
                "manifest": {
                    "platform": "windows",
                    "arch": "x64",
                    "pythonTag": "cp311",
                    "files": [
                        {
                            "name": "python-runtime-windows-x64.zip",
                            "url": "https://example.invalid/python-runtime-windows-x64.zip",
                            "sizeBytes": 100,
                            "sha256": "a" * 64,
                        }
                    ],
                },
            },
            "signedInstaller": {
                "withRuntimeBytes": 250,
                "withoutRuntimeBytes": 250,
                "sizeDeltaBytes": 0,
            },
        }

        with tempfile.TemporaryDirectory() as tmp:
            evidence_path = Path(tmp) / "evidence.json"
            evidence_path.write_text(json.dumps(evidence), encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "sizeDeltaBytes must be positive"):
                module.validate_release_evidence(evidence_path)


if __name__ == "__main__":
    unittest.main()
