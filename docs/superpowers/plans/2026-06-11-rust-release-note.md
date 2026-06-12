<!--
    Release note draft for Rust-backed install and packaging changes.
-->

# Rust Install Manager Release Note

Hermes desktop release packages now include a small Rust install manager binary and a native-first Tauri bootstrapper.

User-visible behavior:

- Lite uninstall can remove the managed Hermes runtime checkout, source-built desktop artifacts, desktop shortcuts,
  Linux desktop entries, and Electron `userData` even when the Python environment is broken or missing.
- Fresh desktop/bootstrap-installer installs record manager metadata under `HERMES_HOME/manager/installed-files.json`,
  including managed runtime/tool/cache directories and the staged bootstrap installer.
- The Tauri bootstrap installer now handles repository archive installs and archive-based updates, install state
  probing, bootstrap marker creation, install-method stamping, config templates, PATH/profile setup, Windows shortcuts,
  and native runtime setup before falling back to direct install scripts.
- Native-first setup covers managed `uv`, Node.js, Python 3.11 probing, venv creation, Python dependency fallback
  tiers, npm/Playwright/TUI dependencies, desktop packaging recovery, platform SDK recovery, ripgrep, common Unix Git
  acquisition, and common Unix `ffmpeg` package-manager recovery.
- Release packages can bundle reviewed Node.js, `uv`, Git for Windows, and ripgrep archives under `bootstrap-tools/`.
  The installer validates their manifest schema, HTTPS URLs, target architecture labels, size, and SHA-256 before use.
- Native bootstrap diagnostics now preserve npm and Unix package-manager failure output, including permission hints for
  Hermes-managed npm cache and `node_modules` paths.
- Release staging writes a checksummed bundled manifest beside the Rust manager binary and retains a
  `bootstrap-tools-manifest.json` artifact for packaged runtime archives.

Compatibility and fallback:

- Existing Python, PowerShell, shell, and Electron fallback paths remain available for one release cycle and for direct
  `install.ps1` / `install.sh` invocation.
- User config, sessions, memories, logs, skills, and other data under `HERMES_HOME` are preserved unless the user chooses
  a full uninstall path.
- Script fallback is still used for unsupported platforms, denied or interactive privilege escalation, unrecognized
  package managers/distributions, failed package-manager recovery, and cases where both native pip and uv targeted SDK
  installs fail.
- `ffmpeg`, Python wheels, Playwright browser downloads, and Electron caches are not bundled by default; they remain
  download or package-manager work unless the release-size and security-update tradeoff is explicitly accepted later.

Operational note:

- Release builds require Rust/Cargo on the build machine so the manager and Tauri bootstrapper can be compiled.
- End users do not need Rust installed; published packages include the required Rust binaries and reviewed bootstrap
  resource manifests.
