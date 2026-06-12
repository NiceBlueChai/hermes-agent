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
- Existing install metadata is refreshed with newly introduced managed runtime paths, so lite uninstall and repair
  cleanup can remove caches added by later installer versions.
- The Tauri bootstrap installer now handles repository archive installs and archive-based updates, install state
  probing, bootstrap marker creation, install-method stamping, config templates, PATH/profile setup, Windows shortcuts,
  and native runtime setup before falling back to direct install scripts.
- Native-first setup covers managed `uv`, Node.js, Python 3.11 probing, venv creation, Python dependency fallback
  tiers, npm/Playwright/TUI dependencies, desktop packaging recovery, platform SDK recovery, ripgrep, common Unix Git
  acquisition, and common Unix `ffmpeg` package-manager recovery.
- Browser setup now reuses an existing Chrome/Chromium/Edge installation when available by writing
  `AGENT_BROWSER_EXECUTABLE_PATH`, avoiding an unnecessary Playwright Chromium download without disabling browser tools.
- Browser setup now recognizes common Brave and Microsoft Edge install locations/commands across Windows, macOS, and
  Linux before falling back to Playwright Chromium.
- Direct `install.ps1` and `install.sh` paths use the same broader browser detection, preserving the optimization when
  the Rust bootstrapper falls back or users run the scripts directly.
- Release packages can bundle reviewed Node.js, `uv`, Git for Windows, and ripgrep archives under `bootstrap-tools/`.
  The installer validates their manifest schema, HTTPS URLs, target platform and architecture labels, size, and SHA-256
  before use; release validation rejects platform or architecture labels that do not match the archive name or the
  installer platform being uploaded.
- Native bootstrap diagnostics now preserve npm and Unix package-manager failure output, including permission hints for
  Hermes-managed npm cache and `node_modules` paths.
- Native and script-fallback desktop packaging now direct Electron download/build caches into
  `HERMES_HOME/electron-cache`, so repair and lite uninstall can remove that installer-owned cache without touching
  unrelated user-wide Electron caches.
- Python dependency setup now has a local wheelhouse entry point: if a release package supplies
  `resources/wheelhouse/`, native bootstrap tries it with `--no-index` before falling back to the existing `uv.lock`
  and PyPI tiers.
- Installer release workflows now generate a Python wheelhouse with `pip wheel .[all]`, write
  `wheelhouse-manifest.json`, validate every wheel's platform, architecture, Python tag, size, and SHA-256, and upload
  the retained wheel payload for release review.
- The built installer's no-UI self-check now verifies the bundled wheelhouse manifest and rejects missing, mismatched,
  or unmanifested wheel payloads during release smoke tests.
- At install time, a wheelhouse with a manifest is used only when every listed wheel still matches its recorded size and
  SHA-256; manifestless local wheelhouses remain supported for development fallback.
- Release staging writes a checksummed bundled manifest beside the Rust manager binary and retains a
  `bootstrap-tools-manifest.json` artifact for packaged runtime archives. Installer workflows now run a no-UI binary
  self-check against the just-built setup executable, embedded install scripts, commit pin, and bootstrap-tools manifest.

Compatibility and fallback:

- Existing Python, PowerShell, shell, and Electron fallback paths remain available for one release cycle and for direct
  `install.ps1` / `install.sh` invocation.
- User config, sessions, memories, logs, skills, and other data under `HERMES_HOME` are preserved unless the user chooses
  a full uninstall path.
- Script fallback is still used for unsupported platforms, denied or interactive privilege escalation, unrecognized
  package managers/distributions, failed package-manager recovery, and cases where both native pip and uv targeted SDK
  installs fail.
- `ffmpeg` and Playwright browser downloads when no system browser is available are not bundled by default; they remain
  download or package-manager work unless the release-size and security-update tradeoff is explicitly accepted later.

Operational note:

- Release builds require Rust/Cargo on the build machine so the manager and Tauri bootstrapper can be compiled.
- End users do not need Rust installed; published packages include the required Rust binaries and reviewed bootstrap
  resource manifests.
