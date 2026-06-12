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
        self.assertIn("Get-FileHash", text)


if __name__ == "__main__":
    unittest.main()
