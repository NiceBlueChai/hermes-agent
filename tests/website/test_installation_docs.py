"""Regression tests for Rust-backed installer documentation."""

from __future__ import annotations

import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class InstallationDocsTest(unittest.TestCase):
    """Validate that user-facing installer docs describe the native release path."""

    def _read(self, *parts: str) -> str:
        return (REPO_ROOT.joinpath(*parts)).read_text(encoding="utf-8")

    def test_english_installation_doc_explains_packaged_release_behavior(self) -> None:
        """The public install guide should mention bundled resources and script fallback."""
        text = self._read("website", "docs", "getting-started", "installation.md")

        self.assertIn("Rust-backed bootstrapper", text)
        self.assertIn("Python wheelhouse", text)
        self.assertIn("before network downloads", text)
        self.assertIn("install.sh", text)
        self.assertIn("install.ps1", text)

    def test_chinese_installation_doc_explains_packaged_release_behavior(self) -> None:
        """The localized install guide should mirror the same release behavior."""
        text = self._read(
            "website",
            "i18n",
            "zh-Hans",
            "docusaurus-plugin-content-docs",
            "current",
            "getting-started",
            "installation.md",
        )

        self.assertIn("Rust 原生引导器", text)
        self.assertIn("Python wheelhouse", text)
        self.assertIn("优先于网络下载", text)
        self.assertIn("install.sh", text)
        self.assertIn("install.ps1", text)

    def test_windows_guides_do_not_claim_desktop_only_calls_install_ps1(self) -> None:
        """Desktop docs should describe script fallback, not script-only bootstrap."""
        docs = [
            self._read("website", "docs", "user-guide", "windows-native.md"),
            self._read(
                "website",
                "i18n",
                "zh-Hans",
                "docusaurus-plugin-content-docs",
                "current",
                "user-guide",
                "windows-native.md",
            ),
        ]

        for text in docs:
            self.assertIn("Rust", text)
            self.assertIn("wheelhouse", text)
            self.assertNotIn("calls `install.ps1` under the hood", text)
            self.assertNotIn("后台调用 `install.ps1`", text)


if __name__ == "__main__":
    unittest.main()
