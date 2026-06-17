<!--
文件意图：记录 Rust 自包含发布路线的剩余可执行目标、完成标准和验证命令，便于 goal 模式逐项推进。
-->

# Rust Remaining Goals Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the Rust-backed self-contained release path without deleting features or removing recovery fallbacks
before release evidence exists.

**Architecture:** Keep Python and Electron as the feature layers. Move only installer, packaged bootstrap, resource
validation, repair, update, and uninstall boundaries into Rust-backed native paths with shell scripts retained for direct
installs and one-release recovery.

**Tech Stack:** Rust/Tauri bootstrap installer, `apps/hermes-manager`, Electron bootstrap runner, Python release
validators, GitHub Actions packaging workflows, PowerShell/POSIX installer fallbacks.

---

## Current Status

- Branch: `docs/rust-self-contained-release`
- Fork remote: `git@github.com:NiceBlueChai/hermes-agent.git`
- Latest pushed checkpoint before this plan: `4835d2705 feat(manager): 平台SDK复用检出wheelhouse`
- `canRunFullBootstrap` must remain `false` until signed Windows, macOS, and Linux packaged release smoke proves full
  native bootstrap parity.
- No script fallback may be removed until `docs/release/fallback-burn-down.json` has matching release evidence.

## Goal 1: Document Remaining Goals In Repository

**Status:** Complete on branch. Added by `85ea59e73 docs(rust): 记录剩余目标验收标准`.

**Completion standard:**

- This file exists and lists all remaining release goals with concrete completion standards.
- The master plan remains the authoritative design reference:
  `docs/superpowers/plans/2026-06-13-rust-highest-path-master-plan.md`.

**Verification:**

```powershell
git diff --check
```

## Goal 2: Lock Windows Release Artifact Validation Order

**Status:** Complete on branch. Verified with
`python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts`.

**Completion standard:**

- `.github/workflows/build-windows-installer.yml` signs the raw `Hermes-Setup.exe` before smoke.
- The signed raw exe runs `--self-check` and `--self-check-lifecycle` before artifact upload.
- NSIS output under `target/release/bundle/nsis/*.exe` is validated before upload.
- `tests/scripts/test_prepare_bootstrap_tools.py` fails if signing, raw smoke, lifecycle smoke, validation, and upload
  are reordered unsafely.
- Do not pretend NSIS supports direct no-UI app flags unless the installer demonstrably forwards them.

**Verification:**

```powershell
python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
git diff --check
```

## Goal 3: Enforce Zero Unreasoned Script Stages In Packaged Bootstrap

**Status:** Complete on branch. Verified with
`cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml build_stage_plan -- --nocapture`,
`cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml fallback_scripts_use -- --nocapture`, and
`python -m unittest tests.scripts.test_prepare_bootstrap_tools`.

**Completion standard:**

- Every packaged bootstrap stage is classified as one of:
  `native`, `probe-only`, `native-first-with-script-fallback:<reason>`, or `direct-install-script:<reason>`.
- Packaged GUI bootstrap summaries report zero pure script-only stages.
- Any script fallback carries a concrete reason string and receives the same bundled resource paths as the native path.
- Interactive stages such as `configure` and `gateway` remain Rust-handled skips in GUI bootstrap.

**Verification:**

```powershell
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml build_stage_plan -- --nocapture
python -m unittest tests.scripts.test_prepare_bootstrap_tools
git diff --check
```

## Goal 4: Prove Windows Bundled Python Runtime As Default Candidate

**Status:** In progress. Windows workflow now builds a Python runtime archive by default, keeps external audited archive
override support, has a Python runtime lifecycle smoke, and `--self-check-lifecycle` extracts a manifest-owned runtime
archive into a temporary directory. Fork run
`https://github.com/NiceBlueChai/hermes-agent/actions/runs/27672497098` proved bootstrap tool bundle/validate,
wheelhouse bundle/validate, Python runtime bundle/validate, and installer build. It failed at Azure OIDC login because
the fork signing environment did not provide the Azure client and tenant values, so signed smoke evidence is still
missing. Unsigned fork run `https://github.com/NiceBlueChai/hermes-agent/actions/runs/27674401930` passed build,
bootstrap tool bundle/validate, wheelhouse bundle/validate, Python runtime bundle/validate, runtime resource smoke,
runtime lifecycle smoke, unsigned artifact validation, fallback registry validation, and runtime artifact upload.
That `unsigned-smoke-only` mode skips signing and installer binary uploads and does not satisfy the signed-release
completion standard.

**Completion standard:**

- Windows x64 packaged smoke proves bundled Python runtime extraction, wheelhouse use, venv creation, and dependency
  install work together.
- Runtime manifest is platform-specific, checksummed, manifest-owned, and validated by `--self-check`.
- Runtime extraction rejects traversal, symlinks, and non-regular archive entries.
- System Python and `uv python install` remain direct-install or recovery fallbacks for at least one release.
- Release size and security-update notes are written before enabling the runtime bundle by default.

**Verification:**

```powershell
python -m unittest tests.scripts.test_validate_installer_artifacts tests.scripts.test_prepare_bootstrap_tools
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml self_check_validates -- --nocapture
git diff --check
```

## Goal 5: Extend Bundled Python Runtime Proof To macOS And Linux

**Status:** Complete on branch for unsigned fork smoke. The dispatchable Windows workflow now has a
`unix-smoke-only` path that skips Windows signing/build, builds Linux/macOS runtime archives by default, validates
runtime manifests, runs built-binary and packaged AppImage/.app lifecycle smoke, validates artifacts, validates the
fallback burn-down registry, and uploads installer/runtime artifacts. Signed release evidence is still deferred to
Goal 9.

**Completion standard:**

- macOS `.app` packaged smoke validates the `CFBundleExecutable` entry point, runtime manifest, runtime extraction,
  wheelhouse Python tag, and lifecycle self-check.
- Linux AppImage packaged smoke validates the same runtime and wheelhouse path.
- Runtime fallback behavior remains intact on all three supported desktop platforms.

**Evidence:**

- [Build Windows Installer run 27681501026](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27681501026)
  completed successfully on head `326ca5e45fffd5f8ffe3e920bd98629bece0d97c`.
- `Unix packaged runtime smoke (linux)` passed: bootstrap tools bundle/validate, wheelhouse bundle/validate, Python
  runtime bundle/validate, built binary smoke, built lifecycle smoke, packaged AppImage lifecycle smoke, Linux artifact
  validation, fallback burn-down validation, and artifact uploads.
- `Unix packaged runtime smoke (macos)` passed: bootstrap tools bundle/validate, wheelhouse bundle/validate, Python
  runtime bundle/validate, built binary smoke, built lifecycle smoke, packaged `.app` lifecycle smoke, macOS artifact
  validation using `CFBundleExecutable`, fallback burn-down validation, and artifact uploads.

**Verification:**

```powershell
python -m unittest tests.scripts.test_prepare_python_runtime tests.scripts.test_build_python_runtime_archive tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml symlink -- --nocapture
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml python_runtime -- --nocapture
git diff --check
```

## Goal 6: Finish No-Git Normal Packaged Install And Update Proof

**Status:** Complete on branch. Verified with
`cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml archive -- --nocapture` and
`cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml update -- --nocapture`.

**Completion standard:**

- Fresh packaged installs prefer manifest-verified source archive or bundled source snapshot on Windows, macOS, and
  Linux.
- Archive-created updates refresh source through Rust and call `hermes update --finalize-only`.
- Git preparation is not entered before dependency finalization for supported archive-created installs.
- Git clone/update remains available for direct script installs and recovery.

**Verification:**

```powershell
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml archive -- --nocapture
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml update -- --nocapture
git diff --check
```

## Goal 7: Close Rust Repair And Uninstall Parity Gaps

**Status:** Complete on branch. Verified with
`cargo test --manifest-path apps/hermes-manager/Cargo.toml -- --nocapture` and
`node --test apps/desktop/electron/desktop-uninstall.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs`.

**Completion standard:**

- Lite uninstall preserves user config, `.env`, sessions, skills, memories, logs, and secrets.
- Repair-clean removes only managed runtime, cache, tools, and staged updater resources.
- Desktop uninstall prefers `hermes-manager` for lite mode and falls back to Python uninstall on failure.
- If full uninstall moves into Rust, it requires explicit confirmation and still rejects unsafe paths outside owned
  roots.

**Verification:**

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml -- --nocapture
node --test apps/desktop/electron/desktop-uninstall.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs
git diff --check
```

## Goal 8: Add Size Gates For Any New Default Bundle

**Status:** Complete on branch. Verified with
`python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts`.

**Completion standard:**

- Any newly defaulted bundle has manifest owner, checksum, platform, architecture, validator coverage, and stale/missing
  fallback behavior.
- Decision-gated bundles such as ffmpeg, platform SDK wheels, voice/STT/TTS, and extra browser automation resources have
  release-size and security-update notes before default inclusion.
- CI fails if bundle size exceeds the documented budget.

**Verification:**

```powershell
python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
git diff --check
```

## Goal 9: Flip canRunFullBootstrap Only With Release Evidence

**Completion standard:**

- One signed Windows release reports `canRunFullBootstrap=true`.
- One signed macOS release reports `canRunFullBootstrap=true`.
- One signed Linux release reports `canRunFullBootstrap=true`.
- Packaged smoke covers the native bridge on all three platforms.
- Repair and uninstall clean resources created by the native path.
- Release notes document the install behavior change.

**Verification:**

```powershell
python scripts/validate_fallback_burn_down.py
node --test apps/desktop/electron/bootstrap-runner.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs
cargo test --manifest-path apps/hermes-manager/Cargo.toml -- --nocapture
git diff --check
```

## Goal 10: Burn Down Release Fallbacks

**Status:** Complete for the current branch state. No fallback has been removed; the retained fallback registry validates.
Verified with `python -m unittest tests.scripts.test_validate_fallback_burn_down` and
`python scripts/validate_fallback_burn_down.py`.

**Completion standard:**

- Each removed fallback has release evidence recorded in `docs/release/fallback-burn-down.json`.
- `scripts/validate_fallback_burn_down.py` confirms the entry, marker, required checks, evidence platform, release URL,
  and check names.
- Direct `install.ps1` and `install.sh` still cover source installs where applicable.
- Removing fallback does not delete user-visible functionality.

**Verification:**

```powershell
python -m unittest tests.scripts.test_validate_fallback_burn_down
python scripts/validate_fallback_burn_down.py
git diff --check
```

## Goal 11: Defer Or Approve Larger Rust Candidates

**Status:** Complete on branch. Candidate decisions are recorded in
`docs/release/rust-candidate-decisions.md`.

**Completion standard:**

- Each deeper Rust candidate has a short candidate note covering exact parity tests, measurable dependency or reliability
  gain, prompt-cache impact, and model-tool footprint impact.
- Fast-changing agent logic, provider logic, gateway behavior, and plugin execution stay out of Rust unless a candidate
  note proves the migration is worth the maintenance cost.

**Verification:**

```powershell
git diff --check
```
