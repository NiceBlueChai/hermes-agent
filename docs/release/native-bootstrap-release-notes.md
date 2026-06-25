<!--
文件意图：记录 Rust 原生 bootstrap 和自包含安装切换在正式发布说明中必须保留的用户可见说明。
-->

# Native Bootstrap Release Notes Draft

This draft is the release-note text for the first signed release that enables the native packaged bootstrap path.
Do not publish this behavior as the default until `docs/release/fallback-burn-down.json` records signed Windows, macOS,
and Linux evidence for `desktop-bootstrap-script-fallback`, `installer-source-archive-download-fallback`, and
`installer-python-runtime-download-fallback`.

## Install Behavior Change

- Packaged desktop installers can complete the non-interactive bootstrap through the bundled Rust native manager when
  the signed release evidence gate reports `canRunFullBootstrap=true`.
- The native path uses manifest-owned packaged resources for the source archive, bootstrap tools, wheelhouse, and
  Python runtime where those resources are present and validated.
- `install.ps1` and `install.sh` remain supported for direct source installs and one-release recovery fallback.
- If a packaged resource is missing, stale, checksum-mismatched, or built for the wrong platform or architecture, the
  installer keeps the existing recovery behavior instead of deleting the script fallback path.

## Bundled Python Runtime

- The packaged Python runtime is checksum-pinned by `python-runtime-manifest.json` and must match the platform,
  architecture, and wheelhouse Python tag validated by CI.
- Release notes for the signed build must state the Python patch version, archive source, archive SHA-256, and signed
  installer size delta.
- The runtime must be rebuilt when its bundled Python patch release receives a security update.

## Repair And Uninstall

- Repair clean removes only managed runtime, cache, tool, and staged updater resources created by the native path.
- Lite uninstall preserves user config, `.env`, sessions, skills, memories, logs, and secrets.
- Full uninstall remains an explicit destructive action and must reject unsafe paths outside owned Hermes roots.

## Release Evidence Required Before Defaulting

- Signed Windows packaged smoke reports `canRunFullBootstrap=true`.
- Signed macOS packaged smoke reports `canRunFullBootstrap=true`.
- Signed Linux packaged smoke reports `canRunFullBootstrap=true`.
- Packaged smoke covers the native bridge, repair/uninstall cleanup, and fallback registry validation on all three
  platforms.
- Fallback burn-down evidence is complete for the retained desktop bootstrap, source archive download, and Python
  runtime download script fallbacks.
