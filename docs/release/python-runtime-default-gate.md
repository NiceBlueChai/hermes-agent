<!--
文件意图：记录 Python runtime bundle 从可选资源提升为默认发布内容前必须满足的大小、安全和烟测条件。
-->

# Python Runtime Default Gate

This note keeps the bundled Python runtime as an explicit release decision, not an automatic default.

## Current Decision

- Status: optional audited resource.
- Default inclusion: not enabled.
- First candidate: Windows x64 only, because that is the highest-impact installer dependency reduction.
- Fallbacks: system Python discovery and `uv python install 3.11` remain available for direct installs and one release
  after the bundled runtime ships by default.

## Size Gate

- The Windows runtime archive must pass `scripts/validate_installer_artifacts.py` with
  `--max-artifact-bytes 536870912` and `--max-total-artifact-bytes 536870912`.
- The release PR must include the actual `python-runtime-manifest.json` size, SHA-256, Python tag, platform, and arch.
- The release PR must compare the signed installer size with and without the runtime bundle.
- If the installer plus runtime exceeds the budget above, keep the runtime optional and do not promote it to default.

## Security-Update Gate

- The runtime archive source must be HTTPS and checksum-pinned in `python-runtime-manifest.json`.
- The runtime must be rebuilt when the bundled Python patch release receives a security update.
- Release notes must identify the Python runtime version and the archive source used for the signed build.
- If the runtime source cannot be audited or patched promptly, keep the runtime optional.

## Windows x64 Smoke Gate

Before default inclusion, CI must run the signed Windows `Hermes-Setup.exe` with:

- `--self-check` for bootstrap tools, wheelhouse, and Python runtime resources.
- `--self-check-lifecycle` with bootstrap tools, wheelhouse, and Python runtime resource arguments together.
- `scripts/validate_installer_artifacts.py` with both `--wheelhouse-dir` and `--python-runtime-dir`.

The smoke evidence must prove the runtime archive is manifest-owned, checksum-validated, platform-specific, and aligned
with the wheelhouse Python tag.
