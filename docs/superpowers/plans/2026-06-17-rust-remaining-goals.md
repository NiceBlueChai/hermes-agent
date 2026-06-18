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
artifact validation. Local tests now prove the default gate rejects release notes that omit the manifest audit fields,
the signed installer size comparison, the Python runtime archive source, or the Python security-update rebuild policy.
The same gate now accepts optional structured release evidence JSON and validates the actual Python runtime manifest,
archive source URL, archive SHA-256, security-update rebuild policy, signed installer sizes, and signed installer size
delta before default inclusion can be claimed. Signed Windows, Linux, and macOS workflow paths now generate that JSON
with `--print-evidence` from the runtime manifest, the signed installer artifact size, and a required without-runtime
baseline size input, then upload it as a signed-only runtime default evidence artifact.

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
`cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml update -- --nocapture`. Direct script
fallback installs now only treat Git as available when the existing `git` command also passes `git --version`, so a
broken system Git no longer blocks the self-contained managed Git recovery path.

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
checks for the same release artifact, but refuses to silently rewrite an existing `releaseNotes` URL for that artifact.
Complete evidence is artifact-scoped: for each platform, one signed `release` + `url` + `commit` group must cover every
required check, so multiple partial release artifacts cannot be combined to unlock `canRunFullBootstrap` or hide missing
checks from `--print-template`. Any evidence that claims `release-notes` must also carry a HTTPS `releaseNotes` URL so
the final gate points at the exact published notes. The release URL and release-notes URL must both include the claimed
`release` tag, preventing evidence for one tag from unlocking another.
Complete Windows, macOS, and Linux evidence must also share the same `release` tag and `commit` SHA, so platform
evidence from different signed releases cannot be stitched together to unlock the final gate. Evidence `url` and
`releaseNotes` values must point at GitHub release tag pages for the same repository, matching the generated
`--print-template` shape. Generated placeholders such as `OWNER/REPO` and `vX.Y.Z` must be replaced before evidence can
be recorded or used by `canRunFullBootstrap`. The manager-side gate also requires the GitHub path to be exactly
`owner/repo/releases/tag/<tag>` with non-empty, whitespace-free owner, repository, and tag segments, so malformed paths
or repository subpaths that merely end with `/releases/tag/<tag>` cannot unlock the native bootstrap gate. It also
rejects query or fragment suffixes on release tag URLs instead of accepting the stripped path.
The manager-side gate now also rejects malformed evidence contracts instead of relying on map/set deduplication:
duplicate or unknown required platforms, empty, invalid, or duplicate required checks, and undeclared or duplicate
evidence checks cannot unlock `canRunFullBootstrap`.
The Python fallback burn-down validator now rejects whitespace, backslashes, query/fragment delimiters inside GitHub
release tag URL segments, and query/fragment URL suffixes, matching the Rust manager parser before signed evidence can
be recorded. Release workflows now put step-level timeouts on Linux Tauri dependency installation and bootstrap tool
archive bundling, and the Linux
dependency install uses apt retries, download timeouts, and `--no-install-recommends`, so transient apt, download, or
cache hangs fail quickly instead of consuming the full release job timeout.
The validator now also requires the `desktop-bootstrap-script-fallback` entry to declare Windows, macOS, and Linux
required evidence before printing the full-bootstrap template, preventing incomplete signed evidence skeletons.
Full-bootstrap signed evidence must now carry a platform signature type: `authenticode` for Windows,
`developer-id-notarized` for macOS, and `sigstore` for Linux. The Python validator rejects missing or mismatched
signature types, `--add-evidence` and `--print-evidence-item` fail fast when full-bootstrap evidence omits
`--signature`, and the Rust `canRunFullBootstrap` gate ignores evidence without the expected platform signature.
The Unix release paths now have a signed path instead of being unsigned-only: the standalone Unix workflow and the fork
dispatchable Windows workflow's Unix smoke job both use `unsigned-smoke-only=true` to preserve fork smoke coverage, while
signed mode validates Apple signing configuration, exports Tauri's Apple signing/notarization environment only for signed
macOS jobs, verifies notarized macOS artifacts with `codesign`, `spctl`, and `stapler`, and signs Linux AppImages with
Sigstore before uploading the Sigstore bundles. Signed Windows, macOS, and Linux workflow paths now print the exact
`validate_fallback_burn_down.py --add-evidence` command with the required platform signature type after artifact
validation and before upload, so release operators can record evidence without hand-authoring the registry shape. Signed
workflow mode now requires explicit `release-tag` and `release-notes-url` inputs before signing proceeds, rejects
release tags containing whitespace, `/`, `\`, `?`, or `#`, requires the release notes URL to equal the current
repository's GitHub `releases/tag/<tag>` page, and uses those inputs in the printed evidence command instead of
placeholder release metadata.
The signed paths now also write that command into a `.release-evidence/fallback-burn-down-*` file, generate a validated
machine-readable evidence JSON with `--print-evidence-item`, and upload both as signed-only artifacts, so the final
release evidence handoff does not depend on scraping workflow logs or hand-authoring JSON. The signed release operator
runbook is recorded in `docs/release/native-bootstrap-signed-release-runbook.md`.

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
- Local Python runtime tests now require `validate_python_runtime_default_gate.py --evidence` to validate structured
  signed-release evidence for the runtime manifest, archive source, SHA-256, security-update rebuild policy, signed
  installer sizes, and exact size delta.
- Local Python runtime tests now require `validate_python_runtime_default_gate.py --print-evidence` to build structured
  signed-release evidence from `python-runtime-manifest.json` and installer sizes, and require signed Windows, Linux,
  and macOS workflow paths to upload `python-runtime-default-evidence-*.json` artifacts.
- Local Python runtime tests now reject non-positive signed installer runtime size deltas, so default promotion evidence
  must prove the bundled runtime actually increases the measured signed artifact size.
- Local Python runtime tests now require structured runtime default evidence to record `signedInstaller.platform` and
  reject signed installer evidence whose platform does not match the runtime manifest platform.
- Local Python runtime tests now require structured runtime default evidence to carry release tag, release URL, release
  notes URL, commit SHA, and platform signature type, and reject release-note links from a different GitHub repository.
- Local Python runtime tests now reject `OWNER/REPO` and `vX.Y.Z` placeholders in structured runtime default evidence,
  keeping generated release-evidence templates from being accepted as real runtime default evidence.
- Local Python runtime tests now validate repeated `--evidence` files with
  `--require-platforms windows,macos,linux`, so runtime default promotion can prove all packaged desktop platforms came
  from one signed release and commit.
- Local Python runtime tests now reject `--require-platforms` without any `--evidence` file, preventing release
  operators from accidentally running only the documentation gate while skipping platform evidence validation.
- Local Python runtime tests now reject multiple `--evidence` files without `--require-platforms`, preventing
  multi-platform runtime evidence from being validated as unrelated single-platform files.
- Local Python runtime tests now reject duplicate required platforms in runtime default evidence validation, aligning the
  runtime gate with the fallback burn-down validator's required-platform contract.
- Local Python runtime tests now reject multi-platform runtime default evidence whose required platforms do not share
  the same Python runtime version.
- Local Python runtime tests now reject runtime default evidence whose manifest Python tag does not match
  `pythonRuntime.version`, preventing a Python archive from being promoted with the wrong CPython ABI tag.
- Local Python runtime tests now reject runtime default evidence whose runtime source or manifest file URL lacks an HTTPS
  host, preventing placeholder URL strings from satisfying the archive provenance gate.
- Local Python runtime tests now reject runtime default evidence whose manifest schema version is not `1`, keeping the
  signed-release gate aligned with the checked-in runtime manifest contract.
- Local Python runtime tests now reject runtime default evidence whose source URL and archive SHA-256 match different
  manifest files, keeping archive provenance tied to one manifest-owned file record.
- Local Python runtime tests now reject evidence for platforms outside `--require-platforms`, so multi-platform runtime
  default validation cannot silently ignore unrelated release artifacts.
- Local Python runtime tests now reject audited runtime archive inputs and generated runtime manifests whose archive URL
  lacks an HTTPS host, aligning manifest generation with the runtime default evidence gate.
- Local bootstrap tool tests now reject audited/local bootstrap archive inputs and generated bootstrap manifests whose
  archive URL lacks an HTTPS host, aligning tool bundle provenance with the runtime archive gate.
- Local workflow tests now require signed runtime archive metadata preflights to reject `NAME=https:///...=SHA256`
  inputs before packaging starts, keeping workflow dispatch validation aligned with the archive validators.
- Local source archive tests now reject audited source archive inputs and generated source archive manifests whose archive
  URL lacks an HTTPS host, aligning the optional source snapshot provenance gate with runtime and bootstrap archives.
- [Unsigned Windows smoke run 27739136009](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27739136009)
  passed on head `23d87bcbf269c8a4a654085832928d55924b903c`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after release tag validation began rejecting backslashes.
- [Unix smoke run 27739136044](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27739136044) passed on head
  `23d87bcbf269c8a4a654085832928d55924b903c`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  release tag validation began rejecting backslashes.
- [Unsigned Windows smoke run 27738121419](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27738121419)
  passed on head `44d6c7b75a97f9d65c4d83c8dec2484d8b6f513c`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after optional source archive validation began requiring archive URLs
  to include HTTPS hosts.
- [Unix smoke run 27738121314](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27738121314) passed on head
  `44d6c7b75a97f9d65c4d83c8dec2484d8b6f513c`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  optional source archive validation began requiring archive URLs to include HTTPS hosts.
- [Unsigned Windows smoke run 27737605924](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27737605924)
  passed on head `f52b0b2f12ac24fbec6b937972886d3460c07ffd`, revalidating Windows workflow dispatch, build,
  Python runtime default gate, runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down
  validation, runtime artifact upload, and Python runtime artifact validation after signed runtime archive metadata
  preflights began rejecting HTTPS URLs without hosts.
- [Unix smoke run 27737605937](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27737605937) passed on head
  `f52b0b2f12ac24fbec6b937972886d3460c07ffd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed runtime archive metadata preflights began rejecting HTTPS URLs without hosts.
- [Unsigned Windows smoke run 27737010222](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27737010222)
  passed on head `a50caf2c07cbff906fb94606efc6f85592a96e21`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after bootstrap tool archive input and manifest validation began
  requiring archive URLs to include HTTPS hosts.
- [Unix smoke run 27737010239](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27737010239) passed on head
  `a50caf2c07cbff906fb94606efc6f85592a96e21`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  bootstrap tool archive input and manifest validation began requiring archive URLs to include HTTPS hosts.
- [Unsigned Windows smoke run 27736466116](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27736466116)
  passed on head `08af161d34e20287aa89c99a631809ac9d2a05bd`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime archive input and manifest validation began requiring
  archive URLs to include HTTPS hosts.
- [Unix smoke run 27736466102](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27736466102) passed on head
  `08af161d34e20287aa89c99a631809ac9d2a05bd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime archive input and manifest validation began requiring archive URLs to include HTTPS hosts.
- [Unsigned Windows smoke run 27735880942](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27735880942)
  passed on head `823cca140dacd2c3ff675c99182ec125c4676bc0`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime default evidence began rejecting platforms outside
  `--require-platforms`.
- [Unix smoke run 27735880932](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27735880932) passed on head
  `823cca140dacd2c3ff675c99182ec125c4676bc0`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime default evidence began rejecting platforms outside `--require-platforms`.
- [Unsigned Windows smoke run 27735423103](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27735423103)
  passed on head `ec2e2a9b85f11aac5deb9e59c3752c589c6886fd`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime default evidence began requiring source URL and archive
  SHA-256 to match the same manifest file.
- [Unix smoke run 27735423119](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27735423119) passed on head
  `ec2e2a9b85f11aac5deb9e59c3752c589c6886fd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime default evidence began requiring source URL and archive SHA-256 to match the same manifest file.
- [Unsigned Windows smoke run 27734904335](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27734904335)
  passed on head `02c25c7ce0595bc0b6e578572e58f157f5c4b974`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime default evidence began requiring manifest
  `schemaVersion: 1`.
- [Unix smoke run 27734904345](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27734904345) passed on head
  `02c25c7ce0595bc0b6e578572e58f157f5c4b974`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime default evidence began requiring manifest `schemaVersion: 1`.
- [Unsigned Windows smoke run 27734391124](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27734391124)
  passed on head `eca2c9c100fd9b3814bb513bcdd052de71e08156`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime default evidence began requiring runtime source and
  manifest file URLs to be HTTPS URLs with hosts.
- [Unix smoke run 27734391070](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27734391070) passed on head
  `eca2c9c100fd9b3814bb513bcdd052de71e08156`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime default evidence began requiring runtime source and manifest file URLs to be HTTPS URLs with hosts.
- [Unsigned Windows smoke run 27733900664](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27733900664)
  passed on head `4054ba8df6ad9144235c9c1222617a6f381add06`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after runtime default evidence began requiring the manifest Python tag
  to match `pythonRuntime.version`.
- [Unix smoke run 27733900646](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27733900646) passed on head
  `4054ba8df6ad9144235c9c1222617a6f381add06`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  runtime default evidence began requiring the manifest Python tag to match `pythonRuntime.version`.
- [Unsigned Windows smoke run 27733238764](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27733238764)
  passed on head `081829ea0259e21e425d3696853485628e38ef20`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after multi-platform runtime default evidence began requiring one
  Python runtime version.
- [Unix smoke run 27733238752](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27733238752) passed on head
  `081829ea0259e21e425d3696853485628e38ef20`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  multi-platform runtime default evidence began requiring one Python runtime version.
- [Unsigned Windows smoke run 27732652113](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27732652113)
  passed on head `9585043433c96ce4eb88142a1e0c08b5e2e37bb1`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after duplicate runtime default required platforms were rejected.
- [Unix smoke run 27732652075](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27732652075) passed on head
  `9585043433c96ce4eb88142a1e0c08b5e2e37bb1`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  duplicate runtime default required platforms were rejected.
- [Unsigned Windows smoke run 27732210386](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27732210386)
  passed on head `85310ff19ec1ac34a6e4f6a31cbb5ef84f4ccde2`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after multiple `--evidence` files began requiring
  `--require-platforms`.
- [Unix smoke run 27732210399](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27732210399) passed on head
  `85310ff19ec1ac34a6e4f6a31cbb5ef84f4ccde2`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  multiple `--evidence` files began requiring `--require-platforms`.
- [Unsigned Windows smoke run 27731639049](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27731639049)
  passed on head `4c8dfcc47cc1d9e786837b25748dfe326f7ca1bd`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after `--require-platforms` was made invalid without evidence files.
- [Unix smoke run 27731639118](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27731639118) passed on head
  `4c8dfcc47cc1d9e786837b25748dfe326f7ca1bd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--require-platforms` was made invalid without evidence files.
- [Unsigned Windows smoke run 27731042152](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27731042152)
  passed on head `75838e76e84a438fe95b5f2cc59f115b683c1136`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after `--require-platforms` multi-evidence validation was added.
- [Unix smoke run 27731042223](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27731042223) passed on head
  `75838e76e84a438fe95b5f2cc59f115b683c1136`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--require-platforms` multi-evidence validation was added.
- [Unsigned Windows smoke run 27730366737](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27730366737)
  passed on head `e6681e73e8f66a6b3d88b8f9177a8040797a1ef7`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after structured runtime default evidence began rejecting release
  identity placeholders.
- [Unix smoke run 27730366792](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27730366792) passed on head
  `e6681e73e8f66a6b3d88b8f9177a8040797a1ef7`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  structured runtime default evidence began rejecting release identity placeholders.
- Local Python runtime tests now require signed Windows, Linux, and macOS workflow paths to fail preflight unless the
  Python runtime archive is supplied as an audited `NAME=HTTPS_URL=SHA256` input. Unsigned smoke remains allowed to
  build temporary runtime archives in Actions, but those unsigned archives cannot satisfy the signed release evidence
  gate.
- Local Python runtime tests now require signed workflow preflight to reject malformed runtime archive inputs before the
  build starts; archive inputs must use HTTPS and a 64-character SHA-256 in `NAME=HTTPS_URL=SHA256` form.
- A local CLI smoke generated runtime default evidence from a temporary manifest and artifact, parsed it with
  `python -m json.tool`, and validated it again with `validate_python_runtime_default_gate.py --evidence`.
- [Unsigned Windows smoke run 27729840045](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27729840045)
  passed on head `187d4f1f7fba7099893d6c4663e2fd0dab8540b2`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after structured runtime default evidence began carrying release tag,
  release URL, release notes URL, commit SHA, and platform signature type.
- [Unix smoke run 27729840028](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27729840028) passed on head
  `187d4f1f7fba7099893d6c4663e2fd0dab8540b2`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  structured runtime default evidence began carrying signed release identity fields.
- [Unsigned Windows smoke run 27728649935](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27728649935)
  failed on head `d796a0c59815a39ece7457e2c273a2d4cfdc2e03` after the Python runtime default gate and lifecycle
  smokes passed; the later unsigned artifact validation step exited with Windows process code `-1073741502` without a
  Python traceback, so the same-head retry was used to test reproducibility.
- [Unsigned Windows smoke run 27729111070](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27729111070)
  passed on head `d796a0c59815a39ece7457e2c273a2d4cfdc2e03`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after structured runtime default evidence began requiring
  `signedInstaller.platform`.
- [Unix smoke run 27728649918](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27728649918) passed on head
  `d796a0c59815a39ece7457e2c273a2d4cfdc2e03`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  structured runtime default evidence began requiring `signedInstaller.platform`.
- [Unsigned Windows smoke run 27725143579](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27725143579)
  passed on head `589c42ccdabf96c3d594bf117de46d4fa5849991`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, runtime lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime
  artifact upload, and Python runtime artifact validation after signed runtime default evidence JSON generation/upload
  was added. The new signed-only runtime evidence steps were parsed by GitHub and skipped in unsigned smoke mode.
- [Unix smoke run 27725151117](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27725151117) passed on head
  `589c42ccdabf96c3d594bf117de46d4fa5849991`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed runtime default evidence JSON generation/upload was added. The new signed-only runtime evidence steps were
  parsed by GitHub and skipped in unsigned smoke mode.
- [Unsigned Windows smoke run 27726203165](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27726203165)
  passed on head `bd616b8914ef8348dce0406a47aa492cfcaefd5d`, revalidating Windows build, Python runtime default
  gate, runtime resource smoke, runtime lifecycle smoke, unsigned artifact validation, fallback burn-down validation,
  runtime artifact upload, and Python runtime artifact validation after signed workflow preflight began requiring an
  audited runtime archive input.
- [Unix smoke run 27726203189](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27726203189) passed on head
  `bd616b8914ef8348dce0406a47aa492cfcaefd5d`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed workflow preflight began requiring audited runtime archive inputs. The new signed-only preflight checks were
  parsed by GitHub and skipped in unsigned smoke mode.
- [Unsigned Windows smoke run 27722259009](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27722259009)
  passed on head `d973e23bbf6e032ba61f2c7bd6f6f9f2fba9488b`, revalidating the Python runtime default gate on a Windows
  runner after structured signed-release evidence validation was added, then passing built binary smoke, Python runtime
  resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact upload,
  and Python runtime artifact validation.
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
- [Unsigned Windows smoke run 27704425534](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27704425534)
  passed on head `07e057f9c0f0f6296a0f60e5d0656d25c95bb15c`, revalidating fallback burn-down validation after the
  Rust manager release URL parser was tightened to reject extra repository path segments.
- [Unix smoke run 27704425496](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27704425496) passed on head
  `07e057f9c0f0f6296a0f60e5d0656d25c95bb15c`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  Rust manager release URL parser was tightened to reject extra repository path segments.
- [Unsigned Windows smoke run 27705409987](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27705409987)
  passed on head `7bc50ee62b19e888e5cdad9fe57f1fa83dbb6de6`, revalidating fallback burn-down validation after the
  Rust manager release URL parser was tightened to reject empty owner, repository, and tag path segments.
- [Unix smoke run 27705410037](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27705410037) passed on head
  `7bc50ee62b19e888e5cdad9fe57f1fa83dbb6de6`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  Rust manager release URL parser was tightened to reject empty owner, repository, and tag path segments.
- [Unsigned Windows smoke run 27706430230](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27706430230)
  passed on head `cf124c8cdd4ebe6b8fac721d82625793d54d41b2`, revalidating fallback burn-down validation after the
  Rust manager release URL parser was tightened to reject whitespace in owner, repository, and tag path segments.
- [Unix smoke run 27706430156](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27706430156) passed on head
  `cf124c8cdd4ebe6b8fac721d82625793d54d41b2`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  Rust manager release URL parser was tightened to reject whitespace in owner, repository, and tag path segments.
- [Unsigned Windows smoke run 27708129591](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27708129591)
  passed on head `b5263f0631b1e79837e168dc5be5ba17f9181913`, revalidating fallback burn-down validation after
  `--add-evidence` was changed to reject `releaseNotes` rewrites for an existing signed release artifact.
- [Unix smoke run 27708129552](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27708129552) passed on head
  `b5263f0631b1e79837e168dc5be5ba17f9181913`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  `--add-evidence` was changed to reject `releaseNotes` rewrites for an existing signed release artifact.
- [Unsigned Windows smoke run 27709380798](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27709380798)
  passed on head `33c840ae9e02b5136c554c1102fcd6836a81ef15`, revalidating fallback burn-down validation after
  the Rust manager required/evidence contract checks were aligned with the Python validator.
- [Unix smoke run 27710885918](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27710885918) passed on head
  `33c840ae9e02b5136c554c1102fcd6836a81ef15`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  the Rust manager required/evidence contract checks were aligned with the Python validator.
- [Unix smoke run 27711748457](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27711748457) passed on head
  `02925563af1af67ea6dfa777c51034ccfdfe9462`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  the Python fallback burn-down validator was tightened to reject whitespace inside GitHub release tag URL segments.
- Windows unsigned smoke runs
  [27711748319](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27711748319) and
  [27712784209](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27712784209) were cancelled after repeatedly
  stalling in the bootstrap tool archive bundling step on head `02925563af1af67ea6dfa777c51034ccfdfe9462`; the release
  workflows now cap that step with `timeout-minutes: 10`.
- [Unsigned Windows smoke run 27713192682](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27713192682)
  passed on head `ebeef5c5f24e6a11a10fe4839a7a0100b199fcc3`, revalidating fallback burn-down validation and Windows
  packaged runtime lifecycle smoke after bootstrap tool archive bundling gained a step-level timeout.
- [Unix smoke run 27713192658](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27713192658) passed on head
  `ebeef5c5f24e6a11a10fe4839a7a0100b199fcc3`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  bootstrap tool archive bundling gained a step-level timeout in release workflows.
- [Unsigned Windows smoke run 27714218350](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27714218350)
  passed on head `a2eef1e38ead29fac0108ddf5203c9ef25e6768e`, revalidating fallback burn-down validation and Windows
  packaged runtime lifecycle smoke after the full-bootstrap required platform contract became mandatory before template
  generation.
- [Unix smoke run 27714218659](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27714218659) passed on head
  `a2eef1e38ead29fac0108ddf5203c9ef25e6768e`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  same validator hardening.
- [Unix smoke run 27716761713](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27716761713) failed on head
  `84c06efdd39b1eaa1a042b64dccb25337b7708ae` when the Linux Tauri dependency install hit the new 10-minute step
  timeout, proving the timeout prevented a full-job hang but was too aggressive for slow hosted-runner apt mirrors.
- [Unsigned Windows smoke run 27717579449](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27717579449)
  passed on head `8ea556a5fc4399c559473c1494cab2bb2acb70ee`, revalidating fallback burn-down validation and Windows
  packaged runtime lifecycle smoke after Linux Tauri dependency installation gained apt retry options and a 20-minute
  step timeout.
- [Unix smoke run 27717579490](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27717579490) passed on head
  `8ea556a5fc4399c559473c1494cab2bb2acb70ee`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  same apt retry and timeout hardening.
- [Unix smoke run 27726801272](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27726801272) failed on head
  `13ed8d349ac62486f4d71a9c9673646152e108906` when the Linux Tauri dependency install was still making progress
  through package setup but hit the 20-minute step timeout, proving the timeout still needed more hosted-runner margin.
- [Unsigned Windows smoke run 27728046087](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27728046087)
  passed on head `a088e63471edce7eed7f0b4afe0e10a1117c4357`, revalidating fallback burn-down validation and Windows
  packaged runtime lifecycle smoke after Linux Tauri dependency installation was widened to a 30-minute step timeout.
- [Unix smoke run 27728046068](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27728046068) passed on head
  `a088e63471edce7eed7f0b4afe0e10a1117c4357`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  Linux dependency install timeout was widened to 30 minutes; the Linux job completed successfully instead of timing out
  during apt package setup.
- Local workflow tests now require both Unix release entry points to support signed macOS and Linux release artifacts:
  `unsigned-smoke-only` keeps fork smoke unsigned, signed macOS jobs require Apple signing/notarization configuration and
  notarization verification, and signed Linux jobs use Sigstore and upload `*.sigstore.json` bundles.
- [Unix smoke run 27718705486](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27718705486) passed on head
  `84a3deacb542c9fed3de3ffbb17e219fbe6d7735`, revalidating Linux and macOS packaged runtime lifecycle smoke through the
  fork-dispatchable Unix job after the signed Unix release path and `unsigned-smoke-only` skip mode were added.
- [Unix smoke run 27719815041](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27719815041) passed on head
  `84a770afb3f47eea24357106cf36349f2029fb1c`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed fallback evidence command output was added. The new command-output step was parsed by GitHub and skipped in
  unsigned smoke mode, preserving the release evidence boundary.
- [Unix smoke run 27720615008](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27720615008) passed on head
  `b2446ccf276c4cbbbf325f32073974c0e9a099bd`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed workflow mode began requiring release evidence metadata inputs. The new metadata preflight was parsed by
  GitHub and skipped in unsigned smoke mode.
- [Unix smoke run 27721403348](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27721403348) passed on head
  `29d90933dfbd9ea7295d323eef16a3a480ec62da`, revalidating Linux and macOS packaged runtime lifecycle smoke after the
  signed metadata preflight was tightened to require a single release tag segment and the exact current-repository
  GitHub `releases/tag/<tag>` URL. The tightened preflight was parsed by GitHub and skipped in unsigned smoke mode.
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
- Local validator and manager tests now reject cross-platform evidence assembled from different GitHub repositories, so
  a fork or upstream release cannot be stitched together with another repository's artifacts.
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
- Local manager tests now reject GitHub release URLs with empty owner, repository, or tag path segments before they can
  be counted as signed release evidence.
- Local manager tests now reject GitHub release URLs whose owner, repository, or tag path segments contain whitespace,
  matching the Python fallback burn-down validator's GitHub release URL shape.
- Local validator tests now reject `--add-evidence` updates that would rewrite the `releaseNotes` URL for an existing
  signed release artifact, keeping recorded release notes immutable once attached.
- Local manager tests now reject malformed required/evidence contracts that the Python validator would reject, keeping
  the Rust `canRunFullBootstrap` gate aligned when required platforms or check lists are duplicated or invalid.
- Local validator and manager tests now reject GitHub release tag URLs whose tag segment contains whitespace or `\`,
  matching the Rust manager release URL parser before signed release evidence can be recorded.
- Local Python runtime tests now reject structured runtime default evidence whose GitHub release tag URL treats `?` or
  `#` as part of the tag segment, and local workflow tests require signed release metadata preflights to reject those
  delimiters in `release-tag`.
- Local Rust manager tests now reject fallback burn-down release URLs with query or fragment suffixes, keeping
  `canRunFullBootstrap` aligned with the stricter Python release evidence validator.
- Local fallback burn-down validator tests now reject query or fragment suffixes on release evidence and release-note
  URLs before those links can be recorded in `docs/release/fallback-burn-down.json`.
- The signed release operator runbook now requires `--require-complete` verification for all three retained fallback
  entries, not only `desktop-bootstrap-script-fallback`, before any script fallback is removed.
- Local fallback burn-down validator tests now reject optional signature fields that do not match the evidence platform,
  so installer fallback evidence cannot claim the wrong signed-artifact proof type.
- [Unsigned Windows smoke run 27742360725](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27742360725)
  passed on head `07100fcb89b5820bd65ea1a85a674adfe7148bb8`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, source archive validation, and Python runtime artifact validation after fallback evidence signature type
  validation was tightened.
- [Unix smoke run 27742360791](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27742360791) passed on head
  `07100fcb89b5820bd65ea1a85a674adfe7148bb8`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  fallback evidence signature type validation was tightened.
- [Unsigned Windows smoke run 27743309024](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27743309024)
  passed on head `67f4f7d30f08f0000c2b9c7a7490c55438a573fe`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, source archive validation, and Python runtime artifact validation after cross-repository release evidence was
  rejected.
- [Unix smoke run 27743308971](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27743308971) passed on head
  `67f4f7d30f08f0000c2b9c7a7490c55438a573fe`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  cross-repository release evidence was rejected.
- [Unsigned Windows smoke run 27744551562](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27744551562)
  passed on head `dd5d6a4948a5a70a02c18ad77d94ab5d8e29c7ea`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after direct installer scripts began requiring existing Git commands to
  pass `git --version` before skipping managed Git recovery.
- [Unix smoke run 27744551533](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27744551533) passed on head
  `dd5d6a4948a5a70a02c18ad77d94ab5d8e29c7ea`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  direct installer scripts began requiring existing Git commands to pass `git --version` before skipping managed Git
  recovery.
- [Unsigned Windows smoke run 27741586603](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27741586603)
  passed on head `09ee22560ee052e478903df7ea26f88232f65517`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, source archive validation, and Python runtime artifact validation after release evidence URL parsing was
  tightened across the Rust manager and Python fallback validator.
- [Unix smoke run 27741592661](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27741592661) passed on head
  `09ee22560ee052e478903df7ea26f88232f65517`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  release evidence URL parsing was tightened across the Rust manager and Python fallback validator.
- [Unsigned Windows smoke run 27740524669](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27740524669)
  passed on head `1708abbbe66856ffc7558d978e08e473634094cc`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after release tag validation began rejecting `?` and `#`.
- [Unix smoke run 27740524657](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27740524657) passed on head
  `1708abbbe66856ffc7558d978e08e473634094cc`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  release tag validation began rejecting `?` and `#`.
- Local workflow tests now require bootstrap tool archive bundling steps to carry `timeout-minutes: 10`, preventing
  release smoke from hanging the full job when external archive/cache preparation stalls.
- Local workflow tests now require Linux Tauri dependency installation steps to carry `timeout-minutes: 30`,
  `--no-install-recommends`, and apt retry options, preventing apt-level stalls from consuming the full Unix release
  smoke job timeout while keeping enough time for slow hosted-runner mirrors.
- Local validator tests now reject full-bootstrap evidence templates when `requiredEvidence` omits any supported release
  platform, preventing incomplete signed evidence skeletons from being generated.
- Local validator and manager tests now reject full-bootstrap signed evidence without the expected platform signature
  type, and `--print-template desktop-bootstrap-script-fallback` emits the required `signature` values for release
  operators.
- Local validator tests now make `--add-evidence desktop-bootstrap-script-fallback` fail fast when release operators omit
  `--signature`, preventing a later generic registry error.
- Local workflow tests now require signed installer workflows to print the fallback burn-down evidence command for
  Windows `authenticode`, macOS `developer-id-notarized`, and Linux `sigstore` after release artifact verification.
- Local workflow tests now require signed installer workflows to validate `release-tag` and `release-notes-url` inputs,
  reject release tags with whitespace, `/`, `\`, `?`, or `#`, require the exact current-repository GitHub
  `releases/tag/<tag>` URL, use those values in the evidence command, and avoid printing placeholder release metadata.
- Local workflow tests now require signed installer workflows to upload the fallback burn-down evidence command as a
  signed-only `.release-evidence/fallback-burn-down-*` artifact for Windows, macOS, and Linux release paths.
- Local validator tests now prove `--print-evidence-item` emits a validated signed evidence JSON object without
  mutating the checked-in fallback registry, and local workflow tests require signed installer workflows to upload that
  JSON beside the command artifact.
- The signed release operator runbook now records required workflow inputs, signed Windows/Unix dispatch commands,
  evidence artifacts to collect, the desktop bootstrap evidence gate for `canRunFullBootstrap=true`, and the retained
  fallback evidence gates that must pass before script fallback removal.
- [Unsigned Windows smoke run 27739907686](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27739907686)
  passed on head `ac758470aeb56df37c381c20609323ab658cd1a7`, revalidating Windows build, Python runtime default gate,
  runtime resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact
  upload, and Python runtime artifact validation after full-bootstrap evidence recording began failing fast when
  `--signature` is omitted.
- [Unix smoke run 27739907676](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27739907676) passed on head
  `ac758470aeb56df37c381c20609323ab658cd1a7`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  full-bootstrap evidence recording began failing fast when `--signature` is omitted.
- [Unix smoke run 27723006956](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27723006956) passed on head
  `823268a8ebee9cdbab0d3b7f14a7bedc3c8eef20`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  signed fallback evidence command artifacts were added. The new upload step was parsed by GitHub and skipped in
  unsigned smoke mode.
- [Unsigned Windows smoke run 27723448105](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27723448105)
  passed on head `823268a8ebee9cdbab0d3b7f14a7bedc3c8eef20`, revalidating Windows built binary smoke, Python runtime
  resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact upload,
  and Python runtime artifact validation after the signed fallback evidence command artifact step was added.
- [Unix smoke run 27724315888](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27724315888) passed on head
  `e30fcf2acbc48105e03a8734cbba6d776f02e372`, revalidating Linux and macOS packaged runtime lifecycle smoke after
  the signed fallback evidence JSON artifact was added. The JSON generation/upload step was parsed by GitHub and
  skipped in unsigned smoke mode.
- [Unsigned Windows smoke run 27724317636](https://github.com/NiceBlueChai/hermes-agent/actions/runs/27724317636)
  passed on head `e30fcf2acbc48105e03a8734cbba6d776f02e372`, revalidating Windows built binary smoke, Python runtime
  resource smoke, lifecycle smoke, unsigned artifact validation, fallback burn-down validation, runtime artifact upload,
  and Python runtime artifact validation after the signed fallback evidence JSON artifact was added. The JSON
  generation/upload step was parsed by GitHub and skipped in unsigned smoke mode.

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
