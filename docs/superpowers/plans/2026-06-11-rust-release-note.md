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
- Release validation now also rejects unknown bootstrap-tool archive names, so every bundled payload must be one of the
  registered runtime tools or optional cache archives before it can ship.
- The installer applies the same known-archive-name rule at runtime before trusting bundled bootstrap-tool archives or
  their manifest-sourced checksum metadata.
- Rust bootstrap archive resolution now requires a complete manifest record, including `sizeBytes`, before trusting
  bundled files or cache checksum metadata, matching the stricter script recovery path.
- Bundled Node.js archive selection now also ignores unmanifested files, so stray resource-directory archives cannot
  influence which Node runtime the installer plans to use.
- Release manifest validation now rejects boolean `sizeBytes` values for bootstrap-tool archives and Python wheels.
- Optional reviewed ffmpeg archives can now use `ffmpeg-<platform>-<arch>` names under `bootstrap-tools/`; native
  setup installs them before package-manager recovery when present and manifest-verified.
- Python TTS/STT and WhatsApp voice conversion can consume that managed ffmpeg binary from `$HERMES_HOME/bin` while
  keeping PATH-based ffmpeg as the compatibility fallback.
- Release maintainers can add such archives with `prepare_bootstrap_tools.py --local-archive PATH=HTTPS_URL`, which
  copies the local file and records the reviewed URL, target platform, architecture, size, and SHA-256 in the manifest.
- Release workflows can alternatively use `prepare_bootstrap_tools.py --audited-archive NAME=HTTPS_URL=SHA256` to
  download a maintainer-selected archive only when its checksum matches before writing it to the manifest.
- The Windows, Linux, and macOS installer workflows expose optional audited-archive inputs for manual release builds,
  enabling reviewed ffmpeg payloads without making ffmpeg a required bundled dependency.
- Native bootstrap diagnostics now preserve npm and Unix package-manager failure output, including permission hints for
  Hermes-managed npm cache and `node_modules` paths.
- Bootstrap diagnostics now report why every stage that can still invoke `install.ps1` or `install.sh` may do so,
  making native-first, probe-then-script, interactive, and unported fallback paths explicit in logs.
- Bootstrap diagnostics now also report the total number of script-fallback-capable stages separately from pure
  script-only stages, so release review can track shell dependency reduction more accurately.
- Interactive post-install stages that the Rust bootstrapper already skips for the GUI flow are no longer counted as
  script fallback stages in the bootstrap plan summary.
- If Node dependency or desktop build setup falls back to the install scripts, the Rust bootstrapper now passes its
  bundled bootstrap-tools path through so script recovery can restore npm-cache, Playwright browser, and Electron cache
  archives before network downloads.
- Script recovery only extracts those bundled cache archives after their `bootstrap-tools-manifest.json` size and
  SHA-256 entries match, so a stale or tampered release payload falls back to the existing network path.
- The built installer's self-check now also verifies that the Tauri bundle config still includes `bootstrap-tools/`
  and `wheelhouse/`, catching accidental resource removal in release smoke tests.
- Release npm-cache archive preparation reuses an existing populated workflow npm cache before falling back to
  `npm ci`, reducing release packaging time and avoidable registry traffic.
- Native Unix PATH setup now updates every existing shell profile relevant to the user's shell, so fresh installs are
  less likely to need shell-script fallback or manual profile edits before `hermes` is visible.
- Direct shell and PowerShell installs now keep uv and pip caches under `HERMES_HOME`, matching the packaged bootstrap
  cleanup model and reducing global cache pollution.
- Native and script-fallback desktop packaging now direct Electron download/build caches into
  `HERMES_HOME/electron-cache`, so repair and lite uninstall can remove that installer-owned cache without touching
  unrelated user-wide Electron caches.
- Python dependency setup now has a local wheelhouse entry point: if a release package supplies
  `resources/wheelhouse/`, native bootstrap tries it with `--no-index` before falling back to the existing `uv.lock`
  and PyPI tiers.
- Installer release workflows now generate a Python wheelhouse with `pip wheel .[all]`, write
  `wheelhouse-manifest.json`, validate every wheel's platform, architecture, Python tag, size, and SHA-256, and upload
  the retained wheel payload for release review.
- Linux and macOS installer workflows now upload installer binaries, bootstrap tool payloads, and Python wheelhouse
  payloads as separate artifacts, matching Windows and making bundled dependency review explicit.
- Native platform SDK recovery and script fallback now receive the Rust bootstrapper's bundled wheelhouse path, so SDK
  recovery can still avoid PyPI when the release package contains valid wheels.
- The built installer's no-UI self-check now verifies the bundled wheelhouse manifest and rejects missing, mismatched,
  or unmanifested wheel payloads during release smoke tests.
- At install time, a wheelhouse with a manifest is used only when every listed wheel still matches its recorded size and
  SHA-256; manifestless local wheelhouses remain supported for development fallback.
- Release staging writes a checksummed bundled manifest beside the Rust manager binary and retains a
  `bootstrap-tools-manifest.json` artifact for packaged runtime archives. Installer workflows now run a no-UI binary
  self-check against the just-built setup executable, embedded install scripts, commit pin, and bootstrap-tools manifest.
- Release artifact validation also checks directory-style artifacts such as macOS `.app` bundles and rejects empty
  placeholder bundles or bundles whose files are all zero bytes before upload.
- The no-UI lifecycle self-check can now include bootstrap-tools and wheelhouse validation in the same JSON report, and
  release installer workflows pass those resource arguments before uploading packaged artifacts.

Compatibility and fallback:

- Existing Python, PowerShell, shell, and Electron fallback paths remain available for one release cycle and for direct
  `install.ps1` / `install.sh` invocation.
- User config, sessions, memories, logs, skills, and other data under `HERMES_HOME` are preserved unless the user chooses
  a full uninstall path.
- Script fallback is still used for unsupported platforms, denied or interactive privilege escalation, unrecognized
  package managers/distributions, failed package-manager recovery, and cases where both native pip and uv targeted SDK
  installs fail.
- `ffmpeg` and Playwright browser downloads when no system browser is available are not bundled by default; ffmpeg now
  has an optional audited archive path, while automatic bundling still waits for an explicit release-size and
  security-update decision.

Operational note:

- Release builds require Rust/Cargo on the build machine so the manager and Tauri bootstrapper can be compiled.
- End users do not need Rust installed; published packages include the required Rust binaries and reviewed bootstrap
  resource manifests.
