"""Tests for direct installer scripts using Hermes-owned dependency caches."""

from __future__ import annotations

import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class InstallScriptManagedCachesTests(unittest.TestCase):
    """Validate direct installer scripts use Hermes-owned dependency caches."""

    def test_install_sh_exports_managed_python_caches(self):
        """Direct Unix installs should not spill uv or pip caches into user-global directories."""

        script = (REPO_ROOT / "scripts" / "install.sh").read_text(encoding="utf-8")

        self.assertIn('export UV_CACHE_DIR="${UV_CACHE_DIR:-$HERMES_HOME/uv-cache}"', script)
        self.assertIn('export PIP_CACHE_DIR="${PIP_CACHE_DIR:-$HERMES_HOME/pip-cache}"', script)
        self.assertIn("${HERMES_BUNDLED_WHEELHOUSE_DIR:-}", script)
        self.assertIn('local_wheelhouse_dir "$INSTALL_DIR/resources/wheelhouse"', script)
        self.assertGreaterEqual(script.count('local_wheelhouse_dir "$INSTALL_DIR/resources/wheelhouse"'), 2)
        self.assertIn("${HERMES_BUNDLED_BOOTSTRAP_TOOLS_DIR:-}", script)
        self.assertIn("bootstrap-tools-manifest.json", script)
        self.assertIn("hashlib.sha256", script)
        self.assertIn("member.isfile()", script)
        self.assertIn("member.isdir()", script)
        self.assertIn("restore_bundled_npm_cache_if_available", script)
        self.assertIn("restore_bundled_playwright_browsers_if_available", script)
        self.assertIn("restore_bundled_electron_cache_if_available", script)

    def test_install_ps1_exports_managed_python_caches(self):
        """Direct Windows installs should not spill uv or pip caches into user-global directories."""

        script = (REPO_ROOT / "scripts" / "install.ps1").read_text(encoding="utf-8")

        self.assertIn('$env:UV_CACHE_DIR = Join-Path $HermesHome "uv-cache"', script)
        self.assertIn('$env:PIP_CACHE_DIR = Join-Path $HermesHome "pip-cache"', script)
        self.assertIn("$env:HERMES_BUNDLED_WHEELHOUSE_DIR", script)
        self.assertIn('Get-LocalWheelhouseDir -FallbackDir $fallbackWheelhouseDir', script)
        self.assertGreaterEqual(script.count("Get-LocalWheelhouseDir -FallbackDir"), 2)
        self.assertIn("$env:HERMES_BUNDLED_BOOTSTRAP_TOOLS_DIR", script)
        self.assertIn("bootstrap-tools-manifest.json", script)
        self.assertIn("Get-FileHash", script)
        self.assertIn("Test-ZipArchiveMembersSafe", script)
        self.assertIn("[System.IO.Compression.ZipFile]::OpenRead", script)
        self.assertIn("Restore-BundledNpmCacheIfAvailable", script)
        self.assertIn("Restore-BundledPlaywrightBrowsersIfAvailable", script)
        self.assertIn("Restore-BundledElectronCacheIfAvailable", script)
