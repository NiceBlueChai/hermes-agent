<!--
    Roadmap for reducing Hermes Agent installation dependencies through incremental Rust components.
-->

# Rust Migration Roadmap

## Goal

Reduce install-time dependency complexity and make install, repair, update, and uninstall faster and more reliable,
while preserving every existing user-facing feature and keeping a safe fallback during each transition.

## Non-Negotiable Constraints

- Do not remove existing Python, Electron, shell, or installer functionality until the Rust replacement has parity.
- Do not require Rust for normal end users.
- Release packages may require Rust during CI/release build, but the built artifacts must include the needed binaries.
- Every phase must be independently shippable and reversible.
- User data under Hermes home must be preserved unless the user explicitly chooses full uninstall.
- Windows path, lock, symlink, junction, and current-directory deletion hazards must be tested before expanding scope.

## Highest-Level Rust Plan

The target is not "rewrite Hermes in Rust." The target is a smaller and more predictable release package:
users should download one installer, the installer should bring the platform-specific tools it owns, and shell/Python
scripts should become recovery paths instead of the normal first-run path.

**Target package shape:**
- Rust bootstrap installer owns install orchestration, repository archive refresh, PATH/profile changes, shortcuts,
  install metadata, repair cleanup, and uninstall planning.
- Release package bundles `hermes-manager`, pinned install scripts, a checksummed bootstrap resource manifest, and
  platform-specific tool archives that create real install-time savings.
- Bundled tool archives start with Node.js and `uv`, then expand only when the size/security trade-off is clear.
- Python application features stay Python. Rust should install, update, verify, and clean them rather than rewriting
  fast-moving agent logic.
- Electron renderer features stay TypeScript. Rust should only handle the native install/update shell around them.
- Git should not be required for normal fresh installs or archive-created updates. It remains available as fallback for
  source checkouts and advanced contributor workflows.
- Shell scripts remain supported for direct CLI installs and one-release fallback, but desktop bootstrap should prefer
  native Rust stages wherever parity exists.

**Dependency reduction order:**
1. Remove Git from fresh packaged installs and archive-based updates.
2. Bundle and install Node.js and `uv` from Rust before any shell script can download them.
3. Create Python 3.11, the virtual environment, and locked Python dependencies through Rust-invoked `uv`.
4. Install npm, Playwright, TUI, and desktop dependencies through Rust first, while preserving platform recovery
   fallbacks for Linux system libraries and Electron mirrors.
5. Move PATH/profile, shortcuts, install stamps, metadata, repair, and uninstall into `hermes-manager`.
6. Add release manifests and smoke tests so every bundled binary has an owner, checksum, and update path.
7. Only after installer parity, evaluate larger Rust runtime candidates with measurable dependency or reliability wins.

**Execution ladder from the current branch:**
1. Finish the normal packaged-installer path first: repository archive, Git recovery, Node, `uv`, Python, venv,
   locked Python dependencies, npm dependencies, Playwright browser download, desktop build, config, PATH, shortcuts,
   metadata, and completion stages should all be Rust-native or native-first.
2. Move recovery branches one at a time after the normal path is native-first. The current priority order is Unix
   `ffmpeg` package-manager recovery, Linux Playwright system-library recovery, Electron/npm cache and mirror recovery,
   remaining privileged `chrome-sandbox` repair, and platform SDK fallback recovery.
3. Keep direct `install.ps1` and `install.sh` supported for one release cycle, but make packaged desktop/bootstrap
   paths reach them only for unsupported platforms, denied privileges, mirror/package-manager failures, or explicitly
   interactive recovery.
4. Expand bundled resources only when they clearly reduce first-run work. Node, `uv`, Git for Windows, and ripgrep are
   appropriate bootstrap-owned tools; `ffmpeg`, Python wheels, Playwright browsers, and Electron caches need explicit
   size, patch cadence, and security-update justification before bundling.
5. Promote the release artifact smoke from local archive lifecycle tests to real packaged installer tests before
   removing any fallback. A stage is considered "highest" only when the packaged artifact includes the needed binary or
   native recovery path, verifies it, records ownership metadata, and can uninstall/repair it without Python.

**Never-rust-first areas for this roadmap:**
- Model/provider orchestration, conversation state, plugin/skill execution, and fast-changing agent features.
- Messaging gateway platform behavior unless a low-level helper has a stable API and a clear dependency payoff.
- Web UI or Electron renderer code.

## Phase 0: Safety and Inventory

**Purpose:** Know exactly what can be moved before moving it.

**Work:**
- Inventory install-time dependencies from `scripts/install.ps1`, `scripts/install.sh`, desktop bootstrap, and
  bootstrap installer.
- Classify dependencies into:
  - runtime-required features,
  - install-only tooling,
  - release-package build tooling,
  - optional platform integrations.
- Record which features require Python packages, Node packages, uv, git, PowerShell, bash, or platform tools.

**Exit Criteria:**
- A dependency matrix exists.
- Each dependency has an owner path and a planned Rust replacement or explicit reason to keep it.

## Phase 1: Rust Install Manager Foundation

**Status:** Implemented on this branch.

**Purpose:** Add a small, testable Rust binary that safely owns Hermes-managed runtime paths.

**Work:**
- Add `apps/hermes-manager`.
- Implement path resolution, bundled manifest validation, installed-files manifest, lite uninstall, repair cleanup,
  and safety checks.
- Package manager into desktop resources when present.
- Let desktop lite uninstall use manager without reducing existing Python cleanup behavior.
- Add CI for Rust manager and desktop platform tests.

**Exit Criteria:**
- `cargo test --manifest-path apps/hermes-manager/Cargo.toml` passes.
- `npm --workspace apps/desktop run test:desktop:platforms` passes.
- Desktop lite uninstall preserves Python parity and can still clean when the venv is missing.

## Phase 2: Release Build Closure

**Status:** Implemented on this branch.

**Purpose:** Ensure published desktop packages always include the Rust manager.

**Work:**
- Add a desktop release-only script to compile `apps/hermes-manager` with Cargo.
- Wire `pack`, `dist`, `dist:win`, `dist:mac`, and `dist:linux` to build the manager before staging.
- Keep normal `npm run build` fallback-friendly so local desktop development does not require Rust.
- Fail release packaging clearly if a target manager binary cannot be produced.

**Exit Criteria:**
- Release build scripts produce and stage `hermes-manager(.exe)` deterministically.
- CI checks manager build logic.
- Staging never ships stale manager binaries.

**Implemented:**
- `build:release`, `pack`, and every platform `dist:*` target build `apps/hermes-manager` before packaging.
- Release builds set `HERMES_DESKTOP_REQUIRE_MANAGER=1`, so staging fails clearly if the target manager binary is
  missing instead of silently shipping the Python-only fallback.
- Plain `npm run build` remains fallback-friendly for desktop development when Rust is not installed.

## Phase 3: Install Metadata Integration

**Status:** Implemented on this branch.

**Purpose:** Make the Rust manager aware of what the installer actually created.

**Work:**
- Call `hermes-manager install-metadata` from successful desktop bootstrap and bootstrap installer paths.
- Extend metadata beyond the default `hermes-agent` runtime directory only when ownership is clear.
- Keep shell/Python install scripts as the source of truth until Rust metadata has enough coverage.
- Add repair behavior for missing or old metadata.

**Exit Criteria:**
- Fresh desktop installs write `$HERMES_HOME/manager/installed-files.json`.
- Lite uninstall works with and without metadata.
- Metadata never includes user config, sessions, `.env`, logs, or other user data.

## Phase 4: Rust Bootstrap Orchestrator

**Status:** In progress on this branch.

**Purpose:** Replace shell-driven orchestration with a self-contained Rust bootstrapper while still delegating complex
language-specific setup where needed.

**Work:**
- Add Rust commands for:
  - download with checksum,
  - archive extraction,
  - atomic file replacement,
  - PATH and environment probing,
  - install-state reporting.
- Make desktop/bootstrap installer call Rust orchestration first.
- Keep `install.ps1` and `install.sh` as fallback for stages not yet ported.

**Implemented so far:**
- Bootstrap installer emits a Rust-side install state and stage plan report before running script-backed stages.
- Downloaded installer artifacts use a Rust helper for HTTP download, optional SHA-256 verification, and atomic cache
  writes.
- Rust ZIP extraction exists with path traversal protection for future repository/archive fallback replacement.
- Rust repository archive fallback primitives can build GitHub ZIP URLs, strip the archive's single top-level directory,
  and refuse to overwrite an existing install root.
- Bootstrap installer can use the Rust repository archive path for Windows fresh installs, while existing install roots
  still fall back to the script-backed Git update path.
- Windows fresh installs now defer the script-backed `git` stage while the Rust repository archive path is available.
  If the archive path fails before creating the install root, the bootstrapper installs Git through the existing script
  stage and then falls back to the script-backed repository stage.
- Archive-created checkouts write `.hermes-source.json` with the GitHub archive owner, repo, ref, branch/commit,
  cached archive path, and best-effort Git initialization status, giving the update path an explicit source marker for
  future no-Git refresh support.
- The Tauri updater now detects archive-created checkouts that are missing `.git` and logs the need for Git checkout
  preparation before handing off to the existing `hermes update` flow.
- When an archive-created checkout without `.git` is updated, the Tauri updater can run the existing installer Git
  stage on demand, initialize the checkout as a Git repository, fetch the recorded archive ref, and reset to it before
  handing off to `hermes update`.
- For Windows archive-created checkouts, the Tauri updater now skips Git checkout preparation, refreshes the repository
  from a GitHub ZIP archive natively in Rust, and then calls `hermes update --finalize-only` so Python only refreshes
  dependencies, caches, and generated assets.
- Archive-created checkout updates now use the same native ZIP refresh path on Windows, Linux, and macOS, so those
  installs do not require Git just to refresh source files before dependency finalization.
- Fresh bootstrap repository stages now try the native Rust GitHub archive path before falling back to script-backed
  clone behavior on every platform. On Unix bootstrap runs, `install.sh` receives an internal archive signal so the
  prerequisites stage does not install or require Git when Rust will fetch the source archive.
- Fresh bootstrap runs now defer the separate Git stage on Windows, Linux, and macOS while the Rust repository archive
  path is available, so new archive-based installs do not install Git unless archive fallback or later update recovery
  actually needs it.
- Repository archive fresh installs now default to `main` when neither a commit nor branch pin is available, keeping
  detached development builds on the no-Git archive path instead of failing into script-backed clone recovery.
- Rust ZIP extraction now rejects symlink entries as well as path traversal entries before materializing repository
  archive contents.
- Rust stage planning now reports native-first, probe-only, and script-only coverage counts so later bootstrap work can
  target the remaining script-owned dependency stages explicitly.
- Script-only stage planning now records a reason for each remaining script-owned stage and includes those reasons in
  the bootstrap orchestrator summary, distinguishing post-install UI stages from unported script fallbacks.
- Bootstrap stage manifests are now generated in Rust for both Windows and Unix scripts, so setup no longer starts
  PowerShell or bash just to discover the stage list.
- Non-interactive post-install stages that require user input are now skipped in Rust with the same successful skipped
  result shape, so GUI bootstrap no longer starts shell processes for those no-op stages.
- Windows bootstrap tool stages for `uv`, `git`, `node`, and `system-packages` now use Rust preflight checks to skip
  the script process when the required tools are already available, while preserving script fallback when anything is
  missing or the detected Node.js version is too old for the desktop build.
- Windows `system-packages` now installs ripgrep natively from a bundled or cached release ZIP before falling back to
  PowerShell for ffmpeg/package-manager recovery, reducing one common package-manager dependency without dropping TTS
  voice-message support.
- Windows `system-packages` now also attempts ffmpeg recovery through Rust-planned winget, Chocolatey, and Scoop
  commands before using PowerShell fallback, matching the direct script package-manager order.
- Unix `system-packages` now mirrors that native-first ripgrep path with pinned Linux/macOS tarballs, then falls back to
  `install.sh` for ffmpeg/package-manager recovery so voice-message support remains intact.
- Unix Node, uv, and ripgrep `.tar.gz` extraction now run through Rust instead of spawning system `tar`; Unix Node
  release preparation prefers `.tar.gz` over `.tar.xz` so packaged native setup no longer assumes system `tar`/`xz`.
- Unix PATH/profile setup now includes both the Hermes virtualenv command directory and `$HERMES_HOME/bin`, so Rust
  installed tools such as `uv` and `rg` remain available after bootstrap instead of only inside the installer process.
- Unix PATH/profile setup now detects fish shells and writes Hermes-managed `fish_add_path` entries to
  `~/.config/fish/config.fish` instead of POSIX `export PATH=...` syntax.
- Unix PATH setup now writes a managed `hermes` launcher that clears `PYTHONPATH`/`PYTHONHOME` before delegating to the
  venv entry point; user-scoped Rust bootstraps keep `$HERMES_HOME/bin/hermes`, while Linux root/FHS bootstraps now use
  `/usr/local/bin/hermes` and shared `/usr/local/share/uv` Python runtime paths.
- Windows `uv` now has a Rust native-first GitHub release ZIP path for x64, ARM64, and x86, installing `uv.exe` into
  `$HERMES_HOME/bin` and preserving the PowerShell astral installer as fallback for download, extraction, or version
  check failures.
- Windows `git` now has a Rust native-first path for the same pinned Git for Windows release used by `install.ps1`,
  downloading PortableGit or 32-bit MinGit into `$HERMES_HOME/git`, updating current/User PATH entries, and persisting
  `HERMES_GIT_BASH_PATH` when Bash is available; the PowerShell stage remains fallback for download/extraction/PATH
  failures.
- Unix `git` now has a Rust native-first availability/acquisition path: existing usable Git skips the shell stage, and
  missing Git can be installed through Rust-planned `apt-get`, `dnf`, `pacman`, Homebrew, or Termux `pkg` commands before
  falling back to `install.sh` for unsupported distros, macOS CLT prompts, or package-manager failures.
- Windows `node` now has a Rust native-first portable ZIP path for Node.js v22, including official index resolution,
  ZIP download/extraction into `$HERMES_HOME/node`, current-process PATH update, and User PATH persistence; the
  PowerShell stage remains fallback for download, extraction, PATH, or version-verification failures.
- Unix `prerequisites` now runs a Rust native-first Node.js v22 tarball preflight before handing off to `install.sh`.
  Successful preflight installs `$HERMES_HOME/node`, updates the bootstrap process PATH, and creates Node/npm/npx
  symlinks, so the script can skip its own Node curl/tar path while still owning uv, Python, Git, system package, and
  network fallback behavior.
- Unix Node preflight now prefers matching bundled Node tarballs from `bootstrap-tools/` before downloading from
  nodejs.org, matching the Windows bundled archive behavior for packaged installers.
- Unix Node native install failures now retain the script fallback path for Linux and macOS packaged bootstraps,
  preserving one-release recovery behavior while the Rust path remains native-first.
- Rust stage planning now classifies Node as native-first on Windows, Linux, and macOS packaged targets, so bootstrap
  summaries match the actual execution path instead of under-reporting Unix Node coverage.
- Unix bootstrap manifests now expose Node.js as a separate native-first stage after `uv`, and Rust passes an internal
  skip signal so the prerequisites shell fallback does not repeat the managed Node.js check/install path.
- Unix `uv` now has a Rust native-first GitHub release tarball path for Linux and macOS x64/arm64, installing `uv` and
  `uvx` into `$HERMES_HOME/bin` while preserving `install.sh` fallback for unsupported platforms, Termux, download,
  extraction, or version-check failures.
- Unix bootstrap manifests now expose `uv` as a separate native-first stage before `prerequisites`, and Rust passes an
  internal skip signal so the prerequisites shell fallback does not repeat the managed `uv` install.
- Windows `python` now uses a Rust `uv python find 3.11` preflight to skip the PowerShell stage when the required
  runtime is already available, while preserving script fallback so missing Python can still be installed by uv.
- Unix bootstrap manifests now expose Python 3.11 as a separate native-first stage after Node.js, and Rust passes an
  internal skip signal so the prerequisites shell fallback does not repeat the Python check/install path.
- Unix bootstrap manifests now expose `system-packages` as a separate probe-then-script stage after Python, so Rust can
  skip the shell process when `rg` and `ffmpeg` are already available and preserve shell package-manager fallback.
- Normal Unix bootstrap manifests no longer include the legacy aggregate `prerequisites` stage after its tool and
  system-package responsibilities were split into explicit stages; `install.sh --stage prerequisites` remains
  available for compatibility.
- `python` now also runs native-first installation through Rust by invoking `uv python install 3.11`, with script
  fallback preserved if uv fails to install or locate the runtime.
- Rust native Python runtime, venv, and dependency stages now set `UV_CACHE_DIR=$HERMES_HOME/uv-cache`,
  `UV_PYTHON_INSTALL_DIR=$HERMES_HOME/python`, and `UV_PYTHON_BIN_DIR=$HERMES_HOME/bin`, keeping uv-managed Python
  and uv's install cache under Hermes-managed repair/uninstall roots instead of the user's global uv cache.
- Native messaging-platform SDK recovery now sets `PIP_CACHE_DIR=$HERMES_HOME/pip-cache` when installing missing SDKs
  into the Hermes venv, keeping pip's recovery cache out of the user's global pip cache.
- Native messaging-platform SDK recovery now also tries the repository-local `resources/wheelhouse/` before network
  pip/uv recovery when the wheelhouse manifest is valid for the current platform and architecture.
- Native messaging-platform SDK recovery now ignores disabled token values such as `false`, `0`, `no`, `off`,
  `none`, and `null`, avoiding unnecessary SDK installs when a platform is explicitly disabled.
- Native npm, Playwright, TUI, and desktop build commands now set `npm_config_cache=$HERMES_HOME/npm-cache`, keeping
  npm's install cache under Hermes-managed repair/uninstall roots instead of the user's global npm cache.
- Native Playwright Chromium install now sets `PLAYWRIGHT_BROWSERS_PATH=$HERMES_HOME/playwright-browsers`, and the
  browser tool uses the same managed path by default when no explicit Playwright browser path is configured.
- Native Playwright Chromium install now mirrors the Linux shell recovery for apt-family distributions by using
  Playwright's `--with-deps` path when root or non-interactive sudo is available, and mirrors the Arch-family recovery
  by installing the same pacman system libraries before the browser download.
- Native Playwright Chromium install now also plans the same Fedora/RHEL-family and openSUSE/SLES system-library
  packages that the shell script previously only printed as manual recovery hints.
- Native Playwright Chromium install now treats Linux package-manager dependency failures as recoverable, records the
  failed commands, and still attempts the browser install before handing the stage to script fallback.
- Native Playwright Chromium install can now consume optional manifest-verified
  `playwright-browsers-<platform>-<arch>` archives from `bootstrap-tools/` before running
  `npx playwright install chromium`, so release packages can pre-seed the managed browser cache while keeping the
  existing system-browser and Playwright download fallbacks.
- `prepare_bootstrap_tools.py --bundle-playwright-browsers` now installs Playwright Chromium into a temporary managed
  cache and archives it as `playwright-browsers-<platform>-<arch>`, and the Windows/Linux/macOS installer workflows
  enable that path by default so release artifacts can avoid the browser download during user installation.
- Native desktop packaging can now consume optional manifest-verified `electron-cache-<platform>-<arch>` archives from
  `bootstrap-tools/` before running `npm run pack`, so release packages can pre-seed Electron's managed cache without
  removing the existing Electron download and mirror fallback path.
- `prepare_bootstrap_tools.py --bundle-electron-cache` now downloads the pinned desktop Electron zip into a temporary
  managed cache, archives it as `electron-cache-<platform>-<arch>`, and the Windows/Linux/macOS installer workflows
  enable that path by default so desktop packaging avoids another install-time Electron download when possible.
- Script fallback for `node-deps` and desktop npm stages now receives the same managed npm cache and Playwright browser
  path environment where applicable, so native fallback does not spill browser/runtime caches back into global user
  locations.
- Native `node-deps` can now consume optional manifest-verified `npm-cache-<platform>-<arch>` archives from
  `bootstrap-tools/` before running npm commands, so release packages can pre-seed the Hermes-managed npm cache while
  keeping normal npm registry fallback.
- npm cache archive preparation now reuses the current workflow npm cache when it already contains package content,
  avoiding a second full `npm ci` during release packaging while keeping the registry-backed fallback.
- Script fallback for Python, venv, dependency, and platform-SDK stages now receives the same managed uv and pip cache
  environment as the native Rust path, keeping retry/recovery installs under Hermes-owned runtime directories.
- `venv` now runs native-first through Rust by invoking `uv venv venv --python 3.11` in the checkout, with script
  fallback preserved if native venv creation fails.
- Python dependency installation now has a Rust native-first lockfile path using `uv sync --extra all --locked` with
  `UV_PROJECT_ENVIRONMENT` pinned to `venv`, while the script keeps all PyPI fallback tiers.
- Python dependency installation now keeps the script's PyPI fallback order in Rust: hash-verified `uv sync`, then
  `uv pip install -e .[all]`, then an `[all]` minus known-broken extra tier parsed from `pyproject.toml`, then
  core-only `uv pip install -e .`.
- `node-deps` now uses a Rust no-op skip when npm is unavailable on every platform, matching the existing script
  behavior without starting PowerShell or bash for a stage that can only skip.
- Windows `node-deps` now has a Rust native-first path for root npm dependencies, Playwright Chromium, and TUI npm
  dependencies, while preserving the PowerShell stage as fallback for missing `npx` or failed npm/Playwright commands.
- Rust npm command failures now capture npm output and add a managed-cache permission diagnostic for EACCES/EPERM-style
  failures, pointing users at Hermes-owned npm cache and `node_modules` paths instead of relying on script-only hints.
- Native `node-deps` now records optional TUI npm install failures without invoking script fallback for a path that the
  install scripts already treat as warning-only.
- macOS `node-deps` now uses the same Rust native-first npm/Playwright/TUI dependency path as Windows, while Linux
  now uses the same native-first path; Linux keeps script fallback for failed npm/Playwright commands and for RPM,
  zypper, or unknown distribution Playwright system-library recovery.
- Rust native `node-deps` now mirrors the script browser optimization: if a system Chrome/Chromium browser is already
  available, it writes `AGENT_BROWSER_EXECUTABLE_PATH` to `$HERMES_HOME/.env` and skips the Playwright Chromium download.
- System browser probing now also recognizes common Brave and Microsoft Edge install locations/commands across
  Windows, macOS, and Linux, reducing avoidable Playwright Chromium downloads on Chromium-family browser machines.
- Direct `install.ps1` and `install.sh` browser probing now share the same broader Brave/Edge coverage, so script
  fallback and direct installs preserve the same download-saving behavior.
- Python TTS and STT runtime paths now resolve `ffmpeg` from PATH first and then from `$HERMES_HOME/bin`, so an
  installer-managed or bundled ffmpeg remains usable before a fresh shell picks up PATH changes.
- The WhatsApp bridge voice-conversion path now uses the same `$HERMES_HOME/bin/ffmpeg` fallback before falling back to
  PATH lookup, so managed ffmpeg archives also cover WhatsApp voice replies without requiring a separate user install.
- `desktop` now uses a Rust no-op skip when `apps/desktop/package.json` is absent, matching the existing script
  behavior without starting PowerShell or bash for a stage that can only skip.
- Windows `desktop` now has a Rust native-first build path for workspace npm install and `npm run pack`, verifies the
  produced `Hermes.exe`, and still creates shortcuts through the Rust manager; the PowerShell stage remains fallback
  for dependency/build failures so its cache purge and Electron mirror recovery are preserved.
- macOS `desktop` now uses the same Rust native-first workspace npm install and `npm run pack` path as Windows, verifies
  the produced `Hermes.app`, and preserves shell fallback for dependency or build recovery.
- Linux `desktop` now uses the same Rust native-first workspace npm install and `npm run pack` path, verifies the
  produced unpacked app, and configures Electron's `chrome-sandbox` helper while preserving script fallback for build
  recovery or privileged sandbox setup failures.
- Native desktop pack now retries once with the same public Electron mirror fallback as the install scripts when the
  default Electron download path fails and the user has not pinned `ELECTRON_MIRROR`.
- Native desktop pack now clears cached `electron-*.zip` downloads and stale `release/*-unpacked` output after the
  first pack failure, then retries once before using the mirror fallback.
- Linux `chrome-sandbox` repair now checks the current effective UID through Rust instead of spawning `id -u`, removing
  another external command assumption from the native desktop stage.
- When the Linux bootstrap process is already root, `chrome-sandbox` owner/mode repair now uses libc/Rust filesystem
  calls instead of spawning `chown` and `chmod`; non-root repair now requires non-interactive sudo and runs `sudo -n`
  so GUI bootstrap cannot block on a password prompt.
- Windows `platform-sdks` now skips natively when `.env` has no configured messaging platform tokens, and runs
  native-first SDK import checks plus targeted `pip install` recovery when tokens are present, while preserving script
  fallback if the native recovery path fails.
- Unix bootstrap manifests now expose the same `platform-sdks` stage after config preparation, so Linux and macOS GUI
  installs also skip SDK work when no messaging platform tokens are configured and run native-first targeted SDK
  recovery when tokens are present.
- Platform SDK recovery now tries `python -m pip install` first and falls back inside Rust to
  `uv pip install --python <venv-python>` with Hermes-owned caches before delegating to scripts.
- Unix `system-packages` now installs missing `ffmpeg` through Rust-planned package-manager commands on common
  Linux/macOS/Termux targets after the native ripgrep archive path runs, preserving shell fallback for unsupported
  distributions, denied privileges, and package-manager failures.
- Native Windows/Linux/macOS `system-packages` can now consume optional manifest-verified `ffmpeg-<platform>-<arch>`
  archives from `bootstrap-tools/` before package-manager recovery, so a later release can bundle ffmpeg without
  changing the installer control flow or dropping existing fallback behavior.
- `prepare_bootstrap_tools.py --local-archive PATH=HTTPS_URL` now lets release maintainers copy an explicitly audited
  local ffmpeg archive into `bootstrap-tools/` and manifest it with URL, platform, architecture, size, and SHA-256
  metadata, without adding an automatic upstream ffmpeg download source.
- `prepare_bootstrap_tools.py --audited-archive NAME=HTTPS_URL=SHA256` now lets release workflows download an
  explicitly checksummed optional archive, verify it before manifesting, and keep ffmpeg bundling source selection in
  release policy rather than installer code.
- Windows, Linux, and macOS installer workflows now expose optional audited-archive dispatch inputs, so an admin can
  produce a more self-contained package with reviewed ffmpeg or Playwright browser-cache payloads without changing
  workflow YAML for each release.
- Linux distro detection now uses both `ID` and `ID_LIKE` from `/etc/os-release`, so derivative distributions can use
  the native apt/dnf/pacman/zypper recovery paths for Git, ffmpeg, and Playwright system libraries.
- Native Unix Git and package-manager recovery failures now capture stdout/stderr in the Rust error message, so GUI
  bootstrap logs expose apt/dnf/pacman/zypper/pkg/brew failure details before any script fallback.
- `bootstrap-marker` now runs as a native Rust stage in the Tauri bootstrapper on Windows and Unix manifests.
- `config-templates` and the Unix `config` stage now run as native Rust stages while preserving Python
  `tools/skills_sync.py` when available and retaining the existing bundled-skill copy fallback.
- Bootstrap-installer Rust tests now include a local archive lifecycle smoke that exercises fresh archive extraction,
  archive refresh, manager metadata recording, repair cleanup, and lite uninstall without network access.
- CI runs bootstrap-installer Rust unit tests in addition to the manager and desktop platform tests, and a dedicated
  Windows/Linux/macOS installer lifecycle smoke matrix runs the manager and archive lifecycle smoke tests on each OS.
- The same lifecycle smoke matrix now builds the bootstrap installer release binary and runs `Hermes-Setup --self-check`
  against it, extending coverage from library tests toward the actual release executable entry point.
- The bootstrap installer release binary now also exposes `--self-check-lifecycle`, running the local archive refresh,
  repair-clean, and lite-uninstall smoke through the actual executable in the Windows/Linux/macOS lifecycle matrix.
- Windows, Linux, and macOS release installer workflows now also run the built `Hermes-Setup --self-check-lifecycle`
  before artifact validation and upload, so manually triggered packaged builds exercise the same lifecycle smoke.

**Still script-backed:**
- Recovery tiers remain script-backed for failure cases that still need package-manager or mirror-specific handling:
  Linux distributions without a recognized `ID` or `ID_LIKE`, interactive or denied Linux `chrome-sandbox` privilege
  escalation, failed Unix package-manager recovery, and messaging-platform SDK recovery if both native pip and uv
  targeted installs fail.
- Fresh archive installs and archive updates do not require Git. Unix Git acquisition now has a native-first package
  manager path for common Linux/macOS/Termux setups, while unsupported distro handling, macOS CLT dialog recovery, and
  package-manager failures remain shell fallbacks.
- Remaining platform shell/profile edge cases that are not covered by the current Rust path-stage helpers, plus direct
  `install.ps1` / `install.sh` invocation paths that intentionally stay supported for one release cycle.

**Exit Criteria:**
- First-launch desktop bootstrap can complete the platform/file-management stages without shell scripts.
- Existing install scripts still work independently.
- Failures report actionable errors and leave repairable state.

## Phase 5: Platform Integration in Rust

**Status:** Foundation in progress on this branch.

**Purpose:** Move fragile platform-specific install/uninstall operations out of ad hoc scripts.

**Work:**
- Windows:
  - PATH mutation,
  - Start Menu/Desktop shortcuts,
  - portable Git ownership checks,
  - process-lock-aware cleanup.
- macOS/Linux:
  - symlink creation/removal,
  - shell profile PATH hints,
  - app bundle/AppImage cleanup.
- Add dry-run and machine-readable JSON output for desktop UI.

**Implemented so far:**
- `hermes-manager uninstall-lite` and `repair-clean` support dry-run planning without deleting files.
- Cleanup commands can emit machine-readable JSON via `--json`, while keeping the existing text output for desktop
  cleanup script compatibility.
- `hermes-manager plan-path` computes side-effect-free PATH updates and Unix shell profile hints for future desktop UI
  and OS-specific apply commands.
- `hermes-manager write-profile-hint` can idempotently write a managed Hermes PATH block to an explicitly provided
  shell profile file, with dry-run and JSON output support.
- `hermes-manager write-user-path` can dry-run or write the current user's Windows `Path` registry value, using the
  registry as the default source of truth and broadcasting an environment-change notification after apply.
- Bootstrap installer now runs the Windows `path` stage natively, preserving user `Path` and `HERMES_HOME` setup.
- Bootstrap installer now runs the Unix `path` stage natively for shell profile PATH setup, writing an idempotent
  Hermes-managed profile block through the Rust manager and refreshing the bootstrap process PATH. Linux root/FHS
  bootstraps now resolve the checkout to `/usr/local/lib/hermes-agent`, write the launcher to `/usr/local/bin/hermes`,
  and keep native/script fallback Python runtime paths under `/usr/local/share/uv` for non-root command usability.
- The native Unix `path` stage now writes the same managed PATH block to every existing shell profile that the selected
  shell normally uses, such as `.bashrc` plus `.profile`, instead of only touching one profile file.
- The Rust manager writes fish-compatible managed profile blocks when the selected Unix profile is `config.fish`, while
  keeping POSIX export blocks for bash/zsh-compatible profile files.
- The Rust manager can create an idempotent Unix `hermes` launcher in the managed tool bin directory, so the bootstrap
  path stage no longer depends only on adding the venv's script directory to shell profiles.
- Bootstrap installer now runs the Unix `complete` stage natively for the install-method stamp, preserving the existing
  `git` value so status, dashboard, and update recommendations remain compatible with archive-created checkouts.
- `hermes-manager plan-shortcuts` reports Start Menu and Desktop `.lnk` targets for the packaged Windows desktop app,
  including working directory and icon location, without mutating user state.
- `hermes-manager write-shortcuts` can dry-run or create those `.lnk` files through the built-in Windows shortcut COM
  API, keeping shortcut setup in the Rust-managed command surface.
- Tauri bootstrap now passes an internal `-SkipDesktopShortcuts` flag for Windows desktop stages and creates Start
  Menu/Desktop shortcuts through the Rust manager after the desktop build succeeds, while direct `install.ps1`
  invocations keep the legacy PowerShell fallback.
- Windows desktop lite uninstall now asks `hermes-manager uninstall-lite --shortcuts` to remove Rust-managed Start
  Menu/Desktop shortcuts. The manager only removes planned `.lnk` files whose shortcut target still points at the
  packaged Hermes desktop executable.
- `hermes-manager install-metadata` now records existing Hermes-managed runtime directories such as `bin`, `uv-cache`,
  `pip-cache`, `npm-cache`, `playwright-browsers`, `node`, `python`, `git`, and `bootstrap-cache` in addition to the
  source checkout, and records the staged `hermes-setup(.exe)` updater when present. Lite uninstall accepts only those
  runtime roots/files while continuing to reject user config and data paths.
- `hermes-manager install-metadata` now updates an existing installed-files manifest with newly introduced managed
  runtime paths, so upgraded installs can pick up newer cleanup ownership without requiring a reinstall.
- `hermes-manager install-metadata`, `uninstall-lite`, and `repair-clean` now also treat
  `$HERMES_HOME/gateway-service` as a managed runtime artifact, matching the Python uninstall cleanup list.
- `hermes-manager install-metadata`, `uninstall-lite`, and `repair-clean` now also treat
  `$HERMES_HOME/desktop-build-stamp.json` as a managed runtime file, so source-built desktop rebuild state no longer
  depends on Python GUI uninstall cleanup.
- `hermes-manager uninstall-gui-build` can now dry-run or remove source-built desktop GUI artifacts under the checkout
  (`dist`, `release`, desktop/workspace `node_modules`, and the build stamp) while preserving the Python agent and user
  config/data.
- The desktop cleanup script now invokes packaged `hermes-manager uninstall-gui-build` for GUI-only uninstall when the
  manager is available, while keeping Python GUI uninstall for packaged app parity.
- `hermes-manager uninstall-gui-build --user-data` now resolves the Electron desktop `userData` directory with
  platform-native rules and removes it as part of GUI-only desktop cleanup, while preserving `$HERMES_HOME` config and
  sessions.
- `hermes-manager uninstall-gui-build --desktop-entries` now removes the Linux `hermes.desktop` / `Hermes.desktop`
  launcher entries from the standard XDG applications directory.
- Packaged GUI-only cleanup can now continue with the Rust manager when the Python venv is missing, as long as the
  running app bundle/install directory is resolvable.
- Packaged GUI-only cleanup now skips the Python uninstaller entirely when the Rust manager and deferred app bundle
  removal cover the full cleanup path; Python remains a fallback when manager/app path resolution is unavailable.
- `hermes-manager repair-clean` now removes the same Hermes-managed runtime roots as repairable install state, so
  broken managed Node/Python/uv/pip/Git/bootstrap-cache directories and staged updater binaries are recreated by the
  next bootstrap while user config and data stay intact.
- `hermes-manager uninstall-lite` and `repair-clean` now also plan and remove Unix `node`, `npm`, and `npx` symlinks
  only when those links still point into `$HERMES_HOME/node`, preserving user-managed Node installations.
- `hermes-manager uninstall-lite` and `repair-clean` now also plan and remove legacy Unix `hermes` wrapper scripts
  from managed command-link directories, but only when the wrapper content still identifies a Hermes launcher.
- `hermes-manager uninstall-lite` now also plans and removes Hermes-managed shell profile PATH blocks from common Unix
  shell config files, while leaving user-authored PATH lines outside the managed block untouched.
- `hermes-manager uninstall-lite` now also plans and removes current-user Windows `HERMES_HOME` and
  `HERMES_GIT_BASH_PATH` environment variables when their values still belong to the active Hermes home.
- `hermes-manager uninstall-lite` now also plans and removes current-user Windows `Path` entries that point under the
  active Hermes home, including managed Git, Node, `bin`, `venv`, and source-checkout entries.
- `hermes-manager uninstall-lite` now accepts manifest-recorded Unix FHS checkout roots under
  `/usr/local/lib/hermes-agent`, so Linux root installs can remove the system checkout while still rejecting arbitrary
  paths outside the active Hermes home.
- `hermes-manager` now has a CLI smoke test that runs `install-metadata`, `uninstall-lite`, and `repair-clean` against
  an isolated Hermes home, proving the command surface preserves user config while cleaning every current
  Hermes-managed runtime directory and staged installer file.
- The cross-platform lifecycle smoke matrix now builds the `hermes-manager` release binary and reruns the same
  install/repair/uninstall smoke through that binary, moving lifecycle coverage from test harness binaries toward
  release-packaged executables.

**Exit Criteria:**
- Rust manager can perform platform cleanup with parity to Python/shell uninstall.
- Existing Python/shell uninstall remains fallback for one release cycle.

## Phase 6: Dependency Bundle Strategy

**Status:** Foundation in progress on this branch.

**Purpose:** Reduce user install-time downloads and dependency setup.

**Work:**
- Decide which artifacts belong in release packages:
  - manager binary,
  - pinned install scripts,
  - checksummed runtime manifests,
  - optional prebuilt helper tools.
- Avoid bundling large or frequently patched dependencies unless there is a clear user-install win.
- For Python/uv dependencies, prefer reproducible cache or wheelhouse strategy before attempting a Rust rewrite of
  Python functionality.

**Exit Criteria:**
- Installer downloads fewer moving pieces.
- Release manifest documents every bundled binary and checksum.
- Security update path remains clear.

**Implemented so far:**
- Desktop release staging writes `build/hermes-manager/bundled-manifest.json` beside the packaged Rust manager.
- The manifest records schema version, Hermes desktop version, source commit, manager resource path, and SHA-256.
- `hermes-manager doctor --manifest <path>` validates the generated manifest successfully in a local staging smoke.
- Commit-pinned bootstrap installers now compile `scripts/install.ps1` and `scripts/install.sh` into the Rust binary,
  so the first run does not need to download the orchestration script from GitHub.
- Windows/Linux/macOS installer release workflows now build Tauri installers with `HERMES_BUILD_PIN_COMMIT` set to the
  workflow SHA, so manually triggered release artifacts use that commit-pinned embedded script path.
- The bootstrap installer binary now has a no-UI `--self-check` mode, and release workflows run the just-built
  `Hermes-Setup` executable to verify its embedded script resources and commit pin before signing or uploading.
- Windows release workflow runs that smoke after Azure signing, so the final raw exe artifact is checked after signing
  has modified it; Linux/macOS run the same smoke immediately after the unsigned Tauri build.
- The same binary smoke now validates the packaged `bootstrap-tools/` manifest directory, including archive
  existence, size, SHA-256, and unmanifested-payload rejection before installer artifacts are uploaded.
- Rust binary self-check also enforces bootstrap-tool audit fields (`arch`, HTTPS `url`, unique archive names, and
  SHA-256 shape), matching the release helper's manifest review gate before upload.
- Branch-following bootstrap builds still resolve install scripts from GitHub raw, preserving HEAD-tracking behavior
  for development and non-immutable builds.
- Bootstrap logs now include an embedded install-script resource summary with size and SHA-256 prefix for diagnostics
  and future release manifest integration.
- Desktop release staging writes install-script metadata into `embedded_resources`, so release manifests document
  embedded script names, sizes, and SHA-256 values without treating them as standalone files.
- Tauri bootstrap installer bundles a `bootstrap-tools/` resource directory and the Windows native Node, uv, Git, and
  ripgrep runtime stages prefer matching bundled archives before falling back to the download cache.
- Windows installer release workflow prepares x64 Node v22, uv, pinned ripgrep, and pinned Git archives before Tauri
  packaging, then writes `bootstrap-tools-manifest.json` with archive URL, size, and SHA-256 metadata for review.
- Windows installer release preparation now includes the pinned ripgrep ZIP used by the native `system-packages`
  stage, so packaged installers can provide fast file search without first invoking winget/choco/scoop.
- Linux and macOS release preparation now include pinned ripgrep tarballs used by the Unix native `system-packages`
  stage, reducing first-run apt/brew work while keeping ffmpeg outside the bundled payload.
- The same release preparation helper now supports `--platform linux|macos` for x64/arm64 Node and uv tarballs, matching
  the Unix Rust Node/uv installer asset matrix when future macOS/Linux installer packaging wires in bundled tools.
- A manual Unix installer workflow now builds Linux and macOS Tauri setup artifacts with matching bundled Node, `uv`, and
  ripgrep archives and uploads the generated `bootstrap-tools-manifest.json` alongside the installer artifacts.
- Runtime bootstrap archive resolution now reads `bootstrap-tools-manifest.json` when present and only uses a bundled
  Node/uv/ripgrep/Git archive if the manifest record exists and its SHA-256 matches the file on disk; otherwise the
  installer falls back to the managed download cache path with the same expected checksum when available.
- Bundled Node.js archive selection now treats the bootstrap-tools manifest as the source of truth when it exists, so
  stray or stale files in the resource directory cannot override the release-reviewed archive list.
- Bootstrap-tools manifest parsing now enforces schema version 1 before trusting archive checksum records, leaving
  future manifest schema changes on the safe download-cache fallback path until the Rust reader is updated.
- Bundled archive validation now also honors manifest `sizeBytes` when present, so truncated or partially copied
  release resources fall back to the managed cache path instead of being extracted.
- Windows installer builds now upload `bootstrap-tools-manifest.json` as a release artifact, matching the Unix
  installer workflow so every packaged bootstrap tool archive has a retained checksum record for review; Windows
  installer, raw exe, and manifest artifact uploads now fail the workflow if any expected file is missing.
- The bootstrap tool preparation helper now has a validate-only mode, and Windows/Linux/macOS installer workflows run it
  after bundling so release builds fail before packaging if any manifest archive is missing, truncated, or hash-mismatched.
- Installer workflows now run a local release artifact validator before upload, requiring the expected Windows, Linux,
  or macOS installer files plus the retained `bootstrap-tools-manifest.json` to exist and pass manifest validation.
- The release artifact validator now accepts non-empty directory artifacts as well as non-empty files, so macOS `.app`
  bundles in the Unix installer workflow are checked without weakening the empty-artifact gate.
- Installer workflows now retain the generated bootstrap-tool archive payloads as artifacts alongside the packaged
  installers, so release review can compare the actual bundled Node/uv/ripgrep/Git archives against the manifest.
- The release artifact validator now rejects unmanifested runtime payloads in `bootstrap-tools/`, making the retained
  manifest the complete allow-list for bundled archives while allowing repository metadata files such as README/.gitignore.
- Bootstrap-tools release validation now requires each target platform/architecture manifest to include every expected
  runtime tool kind: Node, uv, ripgrep, and Git on Windows; Node, uv, and ripgrep on Linux/macOS.
- The no-UI installer self-check now enforces the same required bootstrap-tool kind set when release workflows pass a
  target platform and architecture, so a packaged binary fails before upload if Node, uv, ripgrep, or Windows Git is
  missing from `bootstrap-tools/`.
- Native and script-fallback desktop packaging now set Electron download/build caches to
  `HERMES_HOME/electron-cache`, and `hermes-manager` treats that directory as installer-owned runtime state for repair
  and lite uninstall.
- Release packaging now bundles the pinned Electron zip as an optional `electron-cache-<platform>-<arch>` archive and
  the Rust desktop stage extracts it into `HERMES_HOME/electron-cache` before invoking electron-builder.
- Release packaging now bundles a locked-workspace npm cache as an optional `npm-cache-<platform>-<arch>` archive and
  the Rust `node-deps` stage extracts it into `HERMES_HOME/npm-cache` before root, TUI, Playwright, or desktop npm
  commands try their normal `--prefer-offline` install path.
- Python dependency setup now detects `resources/wheelhouse/` in the installed checkout and tries an offline
  `uv pip install --no-index --find-links` tier before the existing `uv.lock` and PyPI fallback tiers.
- Installer workflows now prepare and validate a Tauri-bundled Python wheelhouse, including a retained
  `wheelhouse-manifest.json` with platform, architecture, Python tag, size, and SHA-256 for every wheel.
- Python wheelhouse manifests now also record dependency input hashes for `pyproject.toml` and `uv.lock`, and the
  release validate-only and final artifact-validation paths reject stale wheelhouse payloads when those inputs no longer
  match the current checkout.
- Python wheelhouse preparation now derives a temporary pip constraints file from registry packages pinned in `uv.lock`,
  so release wheel builds stay lock-constrained without requiring an extra `uv export` tool on the CI runner.
- The no-UI installer self-check now accepts `--self-check-wheelhouse` and validates the bundled wheelhouse manifest,
  every wheel checksum, and unmanifested payloads before release artifacts are accepted.
- The no-UI installer self-check now also requires wheelhouse `sourceFiles` audit metadata with safe source names and
  valid SHA-256 values, so packaged installers cannot silently omit dependency-input provenance.
- Runtime Python dependency planning now trusts a local wheelhouse manifest when present and skips the offline
  wheelhouse tier if any listed wheel size or SHA-256 does not match, while preserving manifestless dev wheelhouses.
- Runtime Python dependency planning now also requires wheelhouse manifest platform and architecture labels to match
  the current installer target before trying the offline tier.
- Runtime Python dependency planning and direct install scripts now also validate wheelhouse `sourceFiles` hashes against
  the local `pyproject.toml`/`uv.lock` inputs before trusting a manifested offline wheelhouse.
- Release installer self-check and artifact validators now accept expected `bootstrap-tools` and wheelhouse platform
  and architecture labels, rejecting mismatched runtime archive or Python wheelhouse payloads before upload.
- Directory-style release artifacts such as macOS `.app` bundles must now contain at least one non-empty file, so CI
  rejects empty placeholder bundles before upload instead of relying on artifact upload shape alone.
- Direct `install.ps1` and `install.sh` Python dependency stages now also prefer a repository-local
  `resources/wheelhouse/` offline tier when its manifest matches the current target platform, architecture, wheel size,
  and SHA-256, while falling back to the existing `uv.lock` and PyPI tiers on any mismatch or install failure.
- Direct `install.ps1` and `install.sh` platform SDK recovery now also tries the same validated local wheelhouse before
  network pip, reducing follow-up SDK downloads for token-enabled messaging platforms without removing pip fallback.
- Direct `install.ps1 --ensure browser` and `install.sh --ensure browser` now use Hermes-owned npm and browser cache
  directories, matching the native bootstrap cache layout for post-install browser dependency repair.
- Direct `install.ps1` and `install.sh` full Node/Desktop install paths now also use Hermes-owned npm,
  Playwright-browser, and Electron cache directories, keeping direct installs and script fallback cleanup-compatible with
  the Rust bootstrap layout.
- TTS and STT ffmpeg callers now fall back to `$HERMES_HOME/bin/ffmpeg(.exe)` after PATH lookup, aligning runtime media
  conversion with the installer-managed binary location used by bundled or native ffmpeg recovery.
- Native and direct Node/Desktop npm install paths now pass `--prefer-offline --no-audit --fund=false`, preferring the
  Hermes-managed npm cache and avoiding audit/funding network calls without removing normal registry fallback.
- Native and direct root Node dependency installs now try `npm ci` when `package-lock.json` is present, falling back to
  `npm install` only when the lockfile path cannot complete.
- The validate-only gate now also requires every archive record to retain its download URL, keeping the packaged
  runtime archive update path auditable alongside size and SHA-256.
- Archive URLs in the retained bootstrap-tools manifest must be HTTPS, so release review cannot accidentally accept an
  insecure update source for a packaged runtime archive.
- Runtime bootstrap archive selection now also requires each manifest archive URL to be HTTPS before trusting a bundled
  payload, preserving the release audit contract even if a local manifest is hand-edited.
- The same gate requires every archive record to retain its target architecture label, preserving review visibility for
  mixed Windows/Linux/macOS bootstrap-tool bundles.
- Bootstrap-tools manifests now also retain an explicit target platform per archive, and both the release helper and
  Rust binary self-check reject archive records that omit it.
- Runtime bootstrap archive selection now rejects bundled archives whose manifest platform or architecture label does
  not match the selected archive name, keeping packaged payload identity checks aligned with release validation.
- Release helper validation and the Rust binary self-check now perform the same archive-name target check, so CI rejects
  mismatched bootstrap-tool platform or architecture labels before upload.
- Installer artifact validation now receives the expected release platform from the Windows/Linux/macOS workflows and
  rejects `bootstrap-tools` manifests whose archive records target a different platform.
- Duplicate archive names are rejected by the validate-only gate, keeping each bundled runtime archive's ownership,
  checksum, and update source unambiguous.
- Manifest archive names must be plain file names with no path separators or parent traversal, matching the runtime
  expectation that bootstrap tool resources live directly inside `bootstrap-tools/`.
- The Rust bootstrap manifest reader applies the same plain-file-name rule at runtime, so a malformed release resource
  cannot make the installer trust a parent-directory archive record even if CI validation was bypassed.

## Phase 7: Larger Runtime Rust Candidates

**Purpose:** Only after install is stable, consider deeper Rust replacements.

**Candidate Areas:**
- File/archive/download helpers currently duplicated across shell, Python, and Electron.
- Local process supervision and health checks.
- Gateway-adjacent low-level utilities if they are dependency-heavy and have stable APIs.

**Do Not Move Yet:**
- Model/provider orchestration.
- Plugin/skill runtime.
- Web UI/Electron renderer.
- Fast-changing Python features where Rust would slow product iteration.

**Exit Criteria:**
- Each candidate has a parity test suite and measurable dependency or reliability benefit.

## Execution Order

1. Finish native-first runtime setup parity: make every platform's `uv`, Node, Python, venv, Python deps, npm deps, and
   desktop stages either native-first or explicitly script-only with a recorded reason.
2. Keep release bundle validation current as Linux/macOS and Windows installer workflows add or remove
   `bootstrap-tools/` archives.
3. Expand `hermes-manager` ownership only when new bootstrap-owned runtime roots or files are introduced; current
   metadata covers the checkout, managed tool/runtime/cache directories, bootstrap cache, and staged updater binary.
4. Extend the current Windows/Linux/macOS lifecycle smoke from local archive/manager paths to real packaged binaries.
5. Reduce shell usage in desktop bootstrap until scripts are only fallback or direct-install entry points.
6. After one release with native bootstrap enabled, evaluate larger Rust runtime candidates from Phase 7 using measured
   install-time dependency reduction, not rewrite preference.

## Review Gates

Every phase needs:

- A focused implementation plan.
- Unit tests for safety boundaries.
- At least one end-to-end install/update/uninstall smoke path.
- A fallback path for one release.
- A short release note explaining changed install behavior.

