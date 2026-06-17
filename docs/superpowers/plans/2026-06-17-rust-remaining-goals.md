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
completion standard. A later unsigned fork run
`https://github.com/NiceBlueChai/hermes-agent/actions/runs/27683435614` also passed on head
`7e148815877498e3ae17951878ed60700ef68f41`, after `canRunFullBootstrap` became evidence-gated and signed mode gained
an Azure signing configuration preflight. Missing signing secrets and variables now fail before installer build. The
Python runtime default-inclusion note is now validated by `scripts/validate_python_runtime_default_gate.py`, and both
Windows and Unix installer workflows run that gate before packaging proceeds past runtime validation. Latest unsigned
fork run `https://github.com/NiceBlueChai/hermes-agent/actions/runs/27684717379` passed on head
`e64c1a3b8882baefd08031bb378740a4b54f86f9`, including the Python runtime default gate, runtime resource smoke,
lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact upload, and source
artifact validation.

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
python -m unittest tests.scripts.test_prepare_python_runtime
python scripts/validate_python_runtime_default_gate.py
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
- [Build Windows Installer run 27682605513](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27682605513)
  completed successfully on head `aed8471f30ab002aeaafde94aba071329f05107e`, revalidating Linux and macOS packaged
  runtime smoke after `canRunFullBootstrap` moved behind the release evidence gate.
- [Build Windows Installer run 27684717355](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27684717355)
  completed successfully on head `e64c1a3b8882baefd08031bb378740a4b54f86f9`, revalidating Linux and macOS packaged
  runtime smoke with the Python runtime default gate active in both Unix matrix jobs.

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

**Status:** In progress. `hermes-manager bootstrap-capabilities` now derives `canRunFullBootstrap` from the
`desktop-bootstrap-script-fallback` evidence in `docs/release/fallback-burn-down.json` instead of a hard-coded release
flag. The checked-in registry still has no signed release evidence, so the reported value remains `false` until
Windows, macOS, and Linux evidence each cover every required check. Release-note draft text is recorded in
`docs/release/native-bootstrap-release-notes.md` and must be reconciled with the actual signed release artifacts before
publication. When the gate eventually reports `true`, the desktop bootstrap runner now uses the native manager manifest
directly and no longer requires `install.ps1` or `install.sh` just to discover the first-launch stage list. Fallback
burn-down evidence must now carry `signed: true`; `scripts/validate_fallback_burn_down.py` rejects unsigned evidence and
`hermes-manager bootstrap-capabilities` ignores unsigned evidence when computing `canRunFullBootstrap`. The validator
also requires each evidence item to carry a 40-character release commit SHA and supports
`--require-complete desktop-bootstrap-script-fallback` so the final fallback removal can fail fast until all signed
platform evidence is present. Use `--print-template desktop-bootstrap-script-fallback` after the signed release smoke to
generate the missing evidence skeleton instead of hand-authoring the JSON shape. Use
`--add-evidence desktop-bootstrap-script-fallback --platform <platform> --release <tag> --url <https-release-url>
--release-notes <https-release-notes-url> --commit <40-char-sha> --all-required-checks` to record complete signed release
evidence for one platform without hand-listing checks; use repeated `--check <check>` only for deliberate partial
evidence. The command forces `signed: true`, validates the platform and checks against the registry contract, and merges
checks for the same release artifact. Complete evidence is artifact-scoped: for each platform, one signed `release` +
`url` + `commit` group must cover every required check, so multiple partial release artifacts cannot be combined to
unlock `canRunFullBootstrap` or hide missing checks from `--print-template`. Any evidence that claims `release-notes`
must also carry a HTTPS `releaseNotes` URL so the final gate points at the exact published notes. The release URL and
release-notes URL must both include the claimed `release` tag, preventing evidence for one tag from unlocking another.
Complete Windows, macOS, and Linux evidence must also share the same `release` tag and `commit` SHA, so platform
evidence from different signed releases cannot be stitched together to unlock the final gate. Evidence `url` and
`releaseNotes` values must point at GitHub release tag pages for the same repository, matching the generated
`--print-template` shape. Generated placeholders such as `OWNER/REPO` and `vX.Y.Z` must be replaced before evidence can
be recorded or used by `canRunFullBootstrap`. The manager-side gate also requires the GitHub path to be exactly
`owner/repo/releases/tag/<tag>`, so a repository subpath that merely ends with `/releases/tag/<tag>` cannot unlock the
native bootstrap gate.

**Evidence:**

- Local manager and desktop tests prove the new gate remains `false` with the checked-in empty evidence registry and
  becomes `true` only when Windows, macOS, and Linux evidence cover every required check.
- [Unsigned Windows smoke run 27683435614](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27683435614)
  passed on head `7e148815877498e3ae17951878ed60700ef68f41`, including built binary smoke, Python runtime resource
  smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, and runtime artifact upload.
- In that unsigned run, `Validate Azure signing configuration` was skipped as expected before `Setup Node.js`; signed
  mode runs the same preflight before the expensive installer build.
- [Unix smoke run 27682605513](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27682605513) passed on head
  `aed8471f30ab002aeaafde94aba071329f05107e`, including Linux and macOS packaged runtime lifecycle smoke and fallback
  burn-down validation.
- These runs are unsigned fork smoke and do not satisfy the signed Windows/macOS/Linux release evidence requirement.
- Signed Windows workflow mode now preflights `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID`,
  `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT_NAME`, and `AZURE_SIGNING_CERTIFICATE_PROFILE` before Azure login,
  so missing release-signing configuration is reported directly before the expensive installer build.
- Desktop runner tests prove a full-native gated bootstrap can complete from the native manifest even when no installer
  script or build stamp is available.
- [Unsigned Windows smoke run 27684717379](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27684717379)
  passed on head `e64c1a3b8882baefd08031bb378740a4b54f86f9`, including the Python runtime default gate before
  packaging, built binary smoke, Python runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback
  burn-down validation, runtime artifact upload, and source artifact validation.
- [Unsigned Windows smoke run 27685640529](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27685640529)
  passed on head `d0722920b49e906a939d5614880ff9ee06920d7c`, revalidating fallback burn-down validation after unsigned
  evidence was forbidden from unlocking `canRunFullBootstrap`.
- [Unsigned Windows smoke run 27686664164](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27686664164)
  passed on head `102f95e84a662de7aba94bb6ab8c0f8c9ed565e4`, revalidating fallback burn-down validation after release
  evidence was required to include an exact 40-character commit SHA.
- [Unsigned Windows smoke run 27687996094](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27687996094)
  passed on head `d5889597e2279e897e4726d9da9141d28f387b60`, revalidating fallback burn-down validation after the
  signed evidence recording command was added.
- [Unsigned Windows smoke run 27689132412](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27689132412)
  passed on head `4bd5125544ca2022163b6ca3edda9d0e95c8f4c4`, revalidating fallback burn-down validation after complete
  evidence was made artifact-scoped.
- [Unsigned Windows smoke run 27690130287](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27690130287)
  passed on head `aa06646c6f71d1d9919fa278fcfa2157e91c99b1`, revalidating fallback burn-down validation after
  `--print-template` was made artifact-scoped.
- [Unix smoke run 27684717355](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27684717355) passed on head
  `e64c1a3b8882baefd08031bb378740a4b54f86f9`, including Linux and macOS packaged runtime lifecycle smoke with the
  Python runtime default gate active.
- [Unix smoke run 27685640571](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27685640571) passed on head
  `d0722920b49e906a939d5614880ff9ee06920d7c`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  signed-evidence-only gate was added.
- [Unix smoke run 27686664189](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27686664189) passed on head
  `102f95e84a662de7aba94bb6ab8c0f8c9ed565e4`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  exact-commit evidence gate was added.
- [Unix smoke run 27687996129](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27687996129) passed on head
  `d5889597e2279e897e4726d9da9141d28f387b60`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  signed evidence recording command was added.
- [Unix smoke run 27689132371](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27689132371) passed on head
  `4bd5125544ca2022163b6ca3edda9d0e95c8f4c4`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  complete evidence was made artifact-scoped.
- [Unix smoke run 27690130266](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27690130266) passed on head
  `aa06646c6f71d1d9919fa278fcfa2157e91c99b1`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--print-template` was made artifact-scoped.
- [Unsigned Windows smoke run 27691194492](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27691194492)
  passed on head `7e699c9d2c007771e39dbaa4b7c1635e6de2e1c8`, revalidating fallback burn-down validation after
  `--all-required-checks` was added.
- [Unix smoke run 27691194903](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27691194903) passed on head
  `7e699c9d2c007771e39dbaa4b7c1635e6de2e1c8`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--all-required-checks` was added.
- [Unsigned Windows smoke run 27692430261](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27692430261)
  passed on head `ba0285c66272b38ca72114c2979d8f1166807231`, revalidating fallback burn-down validation after
  `releaseNotes` evidence links became required.
- [Unix smoke run 27692430316](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27692430316) passed on head
  `ba0285c66272b38ca72114c2979d8f1166807231`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `releaseNotes` evidence links became required.
- [Unsigned Windows smoke run 27693530474](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27693530474)
  passed on head `a10eaf2f3801c8c1ded4b51e491fd9f9a6e0b6d7`, revalidating fallback burn-down validation after
  evidence URLs were required to include the claimed release tag.
- [Unix smoke run 27693530293](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27693530293) passed on head
  `a10eaf2f3801c8c1ded4b51e491fd9f9a6e0b6d7`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  evidence URLs were required to include the claimed release tag.
- [Unsigned Windows smoke run 27694624971](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27694624971)
  passed on head `1569b001732f45a88b9a45bea6a3b0ecf2bb0bf8`, revalidating fallback burn-down validation after
  `--add-evidence` began failing fast when `release-notes` needs `--release-notes`.
- [Unix smoke run 27694625053](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27694625053) passed on head
  `1569b001732f45a88b9a45bea6a3b0ecf2bb0bf8`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--add-evidence` began failing fast when `release-notes` needs `--release-notes`.
- [Unsigned Windows smoke run 27695881787](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27695881787)
  passed on head `3fffdf23c14d95ab8d4f7abf8cab105e955329d6`, revalidating fallback burn-down validation after
  conflicting release-note evidence was rejected.
- [Unix smoke run 27695881688](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27695881688) passed on head
  `3fffdf23c14d95ab8d4f7abf8cab105e955329d6`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  conflicting release-note evidence was rejected.
- [Unsigned Windows smoke run 27697652532](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27697652532)
  passed on head `19c20b0c68c2449db14218442f1bcebbc13e7de7`, revalidating fallback burn-down validation after
  cross-platform evidence was required to share one release tag and commit.
- [Unix smoke run 27697655389](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27697655389) passed on head
  `19c20b0c68c2449db14218442f1bcebbc13e7de7`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  cross-platform evidence was required to share one release tag and commit.
- [Unsigned Windows smoke run 27698742941](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27698742941)
  passed on head `cd7e6cfdf6e2bbeed3a85250955e64144b00ffea`, revalidating fallback burn-down validation after
  mixed-release `--print-template` output was corrected.
- [Unix smoke run 27698746550](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27698746550) passed on head
  `cd7e6cfdf6e2bbeed3a85250955e64144b00ffea`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  mixed-release `--print-template` output was corrected.
- [Unsigned Windows smoke run 27700219258](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27700219258)
  passed on head `945ab9d251837a00f9f5e01b9665d387c11336bd`, revalidating fallback burn-down validation after
  release evidence links were limited to GitHub release tag pages.
- [Unix smoke run 27700222263](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27700222263) passed on head
  `945ab9d251837a00f9f5e01b9665d387c11336bd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  release evidence links were limited to GitHub release tag pages.
- [Unsigned Windows smoke run 27701219816](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27701219816)
  passed on head `2b388bdbb1bcb155945de1e19015cafc4ee754b1`, revalidating fallback burn-down validation after
  release-note evidence was required to point at the same GitHub repository as the release artifact.
- [Unix smoke run 27701222464](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27701222464) passed on head
  `2b388bdbb1bcb155945de1e19015cafc4ee754b1`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  release-note evidence was required to point at the same GitHub repository as the release artifact.
- [Unsigned Windows smoke run 27703353433](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27703353433)
  passed on head `13b45f8d32c38f71f4cfd5f8491c82758121d6d7`, revalidating fallback burn-down validation after
  generated `OWNER/REPO` and `vX.Y.Z` placeholders were rejected as real release evidence.
- [Unix smoke run 27703355862](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27703355862) passed on head
  `13b45f8d32c38f71f4cfd5f8491c82758121d6d7`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  generated `OWNER/REPO` and `vX.Y.Z` placeholders were rejected as real release evidence.
- Local validator and manager tests now prove unsigned fallback burn-down evidence cannot unlock
  `canRunFullBootstrap`; release entries must explicitly set `signed: true`.
- Local validator and manager tests now reject split evidence where a platform's required checks are spread across
  multiple signed release artifacts; every platform needs one complete signed artifact group.
- Local validator tests now prove `--print-template` reports a missing platform evidence skeleton when checks are split
  across multiple signed release artifacts.
- Local validator tests now prove `--add-evidence --all-required-checks` records every required check for a platform
  without hand-listing them.
- Local validator and manager tests now reject `release-notes` evidence without a HTTPS `releaseNotes` URL, and
  `--print-template` includes the field for signed release operators.
- Local validator and manager tests now reject evidence whose release URL or release-notes URL points at a different tag
  than the claimed `release` value.
- Local validator tests now fail fast when `--add-evidence --all-required-checks` would record `release-notes` without
  an explicit `--release-notes` URL, so signed release operators do not need to infer the missing flag from the generic
  registry validation error.
- Local validator and manager tests now reject one signed release artifact carrying conflicting `releaseNotes` URLs, so
  the final `canRunFullBootstrap` gate points at one unambiguous published release-note target.
- Local validator and manager tests now reject cross-platform evidence assembled from different signed release tags or
  commits, so the final gate requires one shared signed release across Windows, macOS, and Linux.
- Local validator tests now prove `--print-template` does not hide mixed-release platform evidence; when every platform
  is complete but no shared release and commit exists, it still prints replacement skeletons for all platforms.
- Local validator and manager tests now reject release evidence whose `url` or `releaseNotes` is not a GitHub release
  tag URL, so arbitrary HTTPS pages cannot unlock fallback removal.
- Local validator and manager tests now reject release-note links that point at a different GitHub repository than the
  signed artifact URL.
- Local validator and manager tests now reject generated `OWNER/REPO` and `vX.Y.Z` placeholders as real release
  evidence, keeping `--print-template` output from being recorded verbatim.
- Local manager tests now reject GitHub release URLs with extra repository path segments before `/releases/tag/<tag>`,
  keeping the Rust `canRunFullBootstrap` gate aligned with the stricter Python fallback burn-down validator.

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
python scripts/validate_fallback_burn_down.py --print-template desktop-bootstrap-script-fallback
python -m unittest tests.scripts.test_validate_fallback_burn_down
python scripts/validate_fallback_burn_down.py --require-complete desktop-bootstrap-script-fallback
cargo test --manifest-path apps/hermes-manager/Cargo.toml full_bootstrap -- --nocapture
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
