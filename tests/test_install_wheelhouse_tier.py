"""Regression tests for direct installer wheelhouse fallback parity."""

import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
INSTALL_SH = REPO_ROOT / "scripts" / "install.sh"
INSTALL_PS1 = REPO_ROOT / "scripts" / "install.ps1"


def _read(path: Path) -> str:
    """Read installer script text for static protocol checks."""
    return path.read_text(encoding="utf-8")


class InstallWheelhouseTierTests(unittest.TestCase):
    """Validate direct installer static wheelhouse tier wiring."""

    def test_install_sh_prefers_valid_local_wheelhouse_before_uv_lock(self) -> None:
        """install.sh should use a valid local wheelhouse before network dependency tiers."""
        text = _read(INSTALL_SH)

        wheelhouse_pos = text.index("install_local_wheelhouse_tier")
        uv_lock_pos = text.index('if [ -f "uv.lock" ]')

        self.assertLess(wheelhouse_pos, uv_lock_pos)
        self.assertIn("--no-index --find-links", text)
        self.assertIn("wheelhouse-manifest.json", text)
        self.assertIn('wheel.get("platform") != expected_platform', text)
        self.assertIn('wheel.get("arch") != expected_arch', text)
        self.assertIn('payload.get("sourceFiles")', text)
        self.assertIn("wheelhouse source sha256 mismatch", text)
        self.assertIn("hashlib.sha256", text)

    def test_install_ps1_prefers_valid_local_wheelhouse_before_uv_lock(self) -> None:
        """install.ps1 should use a valid local wheelhouse before network dependency tiers."""
        text = _read(INSTALL_PS1)

        wheelhouse_pos = text.index("Install-LocalWheelhouseTier")
        uv_lock_pos = text.index('(Test-Path "uv.lock")')

        self.assertLess(wheelhouse_pos, uv_lock_pos)
        self.assertIn("--no-index", text)
        self.assertIn("--find-links", text)
        self.assertIn("wheelhouse-manifest.json", text)
        self.assertIn('platform -ne "windows"', text)
        self.assertIn("arch -ne $expectedArch", text)
        self.assertIn("$payload.sourceFiles", text)
        self.assertIn("source sha256 mismatch", text)
        self.assertIn("Get-FileHash", text)

    def test_install_sh_platform_sdks_try_wheelhouse_before_network_pip(self) -> None:
        """install.sh platform SDK recovery should try wheelhouse before network pip."""
        text = _read(INSTALL_SH)
        function_pos = text.index("install_platform_sdks()")
        wheelhouse_pos = text.index("wheelhouse_arg", function_pos)
        fallback_pos = text.index('"pip", "install", spec', function_pos)
        function_text = text[function_pos:]

        self.assertLess(wheelhouse_pos, fallback_pos)
        self.assertIn('"--no-index"', function_text)
        self.assertIn('"--find-links"', function_text)
        self.assertIn("str(wheelhouse)", function_text)

    def test_install_ps1_platform_sdks_try_wheelhouse_before_network_pip(self) -> None:
        """install.ps1 platform SDK recovery should try wheelhouse before network pip."""
        text = _read(INSTALL_PS1)
        function_pos = text.index("function Install-PlatformSdks")
        wheelhouse_pos = text.index("Test-LocalWheelhouseManifest", function_pos)
        fallback_pos = text.index("-m pip install $sdk.Spec", function_pos)

        self.assertLess(wheelhouse_pos, fallback_pos)
        self.assertIn("--no-index", text[function_pos:])
        self.assertIn("--find-links", text[function_pos:])


if __name__ == "__main__":
    unittest.main()
