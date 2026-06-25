"""Tests for resolving installer-managed runtime binaries."""

from __future__ import annotations

import os
import stat
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class ManagedBinariesTests(unittest.TestCase):
    """Verify runtime tools can find binaries staged under HERMES_HOME/bin."""

    def test_path_binary_takes_precedence_over_managed_binary(self):
        from tools.managed_binaries import find_binary_on_path_or_hermes_home

        with patch("tools.managed_binaries.shutil.which", return_value="/usr/bin/ffmpeg"):
            with patch.dict(os.environ, {"HERMES_HOME": "/tmp/hermes"}, clear=False):
                self.assertEqual(find_binary_on_path_or_hermes_home("ffmpeg"), "/usr/bin/ffmpeg")

    def test_managed_ffmpeg_is_used_when_path_is_missing(self):
        from tools.managed_binaries import find_binary_on_path_or_hermes_home

        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            binary_name = "ffmpeg.exe" if os.name == "nt" else "ffmpeg"
            binary = home / "bin" / binary_name
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fake ffmpeg")
            binary.chmod(binary.stat().st_mode | stat.S_IXUSR)

            with patch("tools.managed_binaries.shutil.which", return_value=None):
                with patch.dict(os.environ, {"HERMES_HOME": str(home)}, clear=False):
                    self.assertEqual(find_binary_on_path_or_hermes_home("ffmpeg"), str(binary))

    def test_tts_ffmpeg_lookup_uses_managed_binary(self):
        import tools.tts_tool as tts_tool

        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            binary = _write_managed_binary(home, "ffmpeg")

            with patch("tools.managed_binaries.shutil.which", return_value=None):
                with patch.dict(os.environ, {"HERMES_HOME": str(home)}, clear=False):
                    self.assertEqual(tts_tool._find_ffmpeg_binary(), str(binary))

    def test_transcription_ffmpeg_lookup_uses_managed_binary(self):
        import tools.transcription_tools as transcription_tools

        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            binary = _write_managed_binary(home, "ffmpeg")

            with patch("tools.managed_binaries.shutil.which", return_value=None):
                with patch.dict(os.environ, {"HERMES_HOME": str(home)}, clear=False):
                    self.assertEqual(transcription_tools._find_ffmpeg_binary(), str(binary))


def _write_managed_binary(home: Path, binary_name: str) -> Path:
    """Create one executable managed binary in the test Hermes home."""

    candidate_name = f"{binary_name}.exe" if os.name == "nt" else binary_name
    binary = home / "bin" / candidate_name
    binary.parent.mkdir(parents=True)
    binary.write_bytes(b"fake binary")
    binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
    return binary


if __name__ == "__main__":
    unittest.main()
