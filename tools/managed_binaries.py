#!/usr/bin/env python3
"""Resolve binaries installed into Hermes-managed runtime directories."""

from __future__ import annotations

import os
import shutil
from pathlib import Path
from typing import Optional


def find_hermes_managed_binary(binary_name: str) -> Optional[str]:
    """Return a binary from ``HERMES_HOME/bin`` when it exists and is executable."""

    try:
        from hermes_constants import get_hermes_home

        hermes_home = get_hermes_home()
    except Exception:
        return None

    for candidate_name in _managed_binary_names(binary_name):
        candidate = hermes_home / "bin" / candidate_name
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
    return None


def find_binary_on_path_or_hermes_home(binary_name: str) -> Optional[str]:
    """Return a binary from PATH first, then from ``HERMES_HOME/bin``."""

    return shutil.which(binary_name) or find_hermes_managed_binary(binary_name)


def _managed_binary_names(binary_name: str) -> tuple[str, ...]:
    """Return platform-specific binary names used by Hermes-managed tools."""

    if os.name == "nt" and not Path(binary_name).suffix:
        return (f"{binary_name}.exe", binary_name)
    return (binary_name,)
