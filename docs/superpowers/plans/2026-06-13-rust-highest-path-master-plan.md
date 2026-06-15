<!--
文件意图：定义 Hermes Agent Rust 化与自包含发布路线的总控计划，约束后续切片按依赖减少收益逐步推进。
-->

# Rust Highest Path Master Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive Hermes Agent toward the highest practical Rust boundary: packaged releases install, update, repair, and
uninstall through native Rust-managed resources with no user-visible dependency loss and no feature deletion.

**Architecture:** Keep Python as the agent/runtime feature layer and TypeScript/Electron as the desktop UI layer.
Move the release boundary into Rust: resource manifests, bundled tool selection, source archive refresh, dependency
preflight, managed caches, lifecycle smoke, repair, and uninstall. Shell and Python installers remain direct-install or
one-release recovery paths until a packaged release proves native parity on Windows, macOS, and Linux.

**Tech Stack:** Rust/Tauri bootstrap installer, `apps/hermes-manager`, Python release validation scripts, PowerShell and
POSIX installer fallbacks, GitHub Actions Windows/Linux/macOS packaging workflows, Electron desktop resources.

---

## Scope Control

This is the master plan, not a single coding slice. Each phase below produces one or more focused implementation plans
or small direct commits. A phase is only complete when the release artifact path is tested, not merely when local unit
tests pass.

The plan intentionally does not rewrite:

- agent conversation orchestration;
- provider adapters;
- gateway platform adapters;
- skills and plugin execution;
- Electron renderer UI;
- fast-changing model/tool schema generation.

Those areas remain Python or TypeScript unless a later measured candidate proves a dependency, reliability, or packaging
win that outweighs the migration cost.

## Current Baseline

The branch already contains the important foundation:

- `apps/hermes-manager` owns safe managed-path cleanup, repair, lifecycle smoke, and installed metadata.
- The Tauri bootstrap installer embeds pinned install scripts and supports no-UI `--self-check` and
  `--self-check-lifecycle`.
- Native archive fresh install and update paths avoid Git for archive-created checkouts across Windows, Linux, and
  macOS.
- Release workflows bundle and validate bootstrap tools: Node.js, `uv`, ripgrep, and Windows Git.
- Release workflows build and validate Python wheelhouse resources with platform, architecture, source hash, and wheel
  checksum metadata.
- Runtime and direct script paths prefer validated local wheelhouse, npm cache, Electron cache, and Playwright browser
  cache before network fallback.
- Installer validators reject malformed manifests, unmanifested payloads, unsafe archive entries, blank names, path
  traversal, special tar entries, and empty artifacts.
- Unix packaged artifacts now smoke the Linux AppImage and macOS `.app` executable with real bundled resources.

This baseline means the next work should close release-quality gaps, not start a broad rewrite.

## Highest-Path Definition

The highest practical Rust boundary is reached when a normal packaged install has this shape:

1. User downloads one platform installer.
2. Installer contains all resources needed for core CLI and desktop bootstrap: Rust manager, pinned scripts,
   source snapshot or archive path, Node.js, `uv`, Python dependency wheelhouse, ripgrep, managed caches, and audited
   optional bootstrap archives.
3. First launch does not require user-preinstalled Python, `uv`, Git, Node.js, npm, or ripgrep.
4. Network is only required for user-selected optional features, explicit updates, or fallbacks after bundled resources
   fail validation.
5. Rust owns normal-path install/update/repair/uninstall orchestration.
6. Shell scripts remain available for direct CLI installs and one-release recovery, but packaged desktop/bootstrap
   normal path does not enter unreasoned script stages.
7. Every bundled file has a manifest owner, checksum, target platform, target architecture, and validator coverage.
8. Lite uninstall removes only Hermes-managed runtime resources and preserves user config, sessions, skills, memories,
   logs, and secrets.
9. Full uninstall requires explicit confirmation and still respects path ownership boundaries.
10. A release fallback is removed only after at least one packaged release proves native parity on all supported OSes.

## Phase 1: Release Confidence Closure

**Purpose:** Make packaged artifacts prove they contain and can use the resources they claim before upload.

**Files:**

- Modify: `.github/workflows/build-windows-installer.yml`
- Modify: `.github/workflows/build-unix-installers.yml`
- Modify: `scripts/validate_installer_artifacts.py`
- Modify: `tests/scripts/test_prepare_bootstrap_tools.py`
- Modify: `tests/scripts/test_validate_installer_artifacts.py`
- Modify: `apps/bootstrap-installer/src-tauri/src/lib.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`

**Required gates:**

- Windows raw signed `Hermes-Setup.exe` runs `--self-check` and `--self-check-lifecycle` after signing.
- Windows NSIS package is at least validated as a non-empty signed packaged artifact with retained manifest resources.
- Linux AppImage runs `--self-check` and `--self-check-lifecycle` through the packaged executable.
- macOS `.app/Contents/MacOS/Hermes` runs `--self-check` and `--self-check-lifecycle` through the packaged executable.
- Artifact validators reject platform-mismatched bootstrap-tools and wheelhouse manifests.
- Workflow tests fail if validation happens after upload or if packaged smoke silently regresses to only local helper
  paths.

**Immediate next slice:**

- [ ] **Step 1: Audit Windows packaged artifact smoke**

  Run:

  ```powershell
  rg -n "Smoke built installer|self-check-lifecycle|bundle/nsis|Hermes-Setup.exe|Validate installer artifacts|Upload" `
    .github/workflows/build-windows-installer.yml tests/scripts/test_prepare_bootstrap_tools.py
  ```

  Expected: identify whether the workflow currently proves only the raw signed exe or also the NSIS packaged artifact.

- [ ] **Step 2: Write the failing workflow-structure test**

  Add or tighten a test in `tests/scripts/test_prepare_bootstrap_tools.py` so Windows installer workflow ordering is
  locked:

  ```python
  def test_windows_installer_workflow_validates_signed_outputs_before_upload() -> None:
      workflow = _read_workflow(".github/workflows/build-windows-installer.yml")
      signing = workflow.index("name: Sign Hermes installer")
      raw_smoke = workflow.index("name: Smoke built installer binary")
      lifecycle = workflow.index("name: Smoke built installer lifecycle")
      validation = workflow.index("name: Validate installer artifacts")
      upload = workflow.index("name: Upload installer artifacts")

      assert signing < raw_smoke < lifecycle < validation < upload
      assert "target/release/Hermes-Setup.exe --self-check" in workflow
      assert "target/release/Hermes-Setup.exe --self-check-lifecycle" in workflow
      assert 'target/release/bundle/nsis/*.exe' in workflow
  ```

  Run:

  ```powershell
  python -m unittest tests.scripts.test_prepare_bootstrap_tools
  ```

  Expected before implementation: fail if the workflow does not preserve this exact gate.

- [ ] **Step 3: Update the Windows workflow only if the test exposes a real gap**

  Keep NSIS handling conservative. Do not add a fake `--self-check` invocation against the NSIS installer unless the
  installer demonstrably forwards CLI flags to the app binary. If NSIS cannot run no-UI app self-checks safely, keep the
  real smoke on the signed raw exe and require NSIS presence/signature/artifact validation before upload.

- [ ] **Step 4: Verify and commit**

  Run:

  ```powershell
  python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
  git diff --check
  ```

  Commit:

  ```powershell
  git add .github/workflows/build-windows-installer.yml tests/scripts/test_prepare_bootstrap_tools.py
  git commit -m "ci(installer): 锁定 Windows 发布产物校验顺序"
  ```

## Phase 2: Normal-Path Shell Budget To Zero

**Purpose:** Packaged desktop/bootstrap normal path should not start PowerShell or bash for stages Rust can already
classify, skip, probe, or execute natively.

**Files:**

- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/install_script.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/bootstrap.rs`
- Modify: `tests/scripts/test_prepare_bootstrap_tools.py`
- Modify: `scripts/install.ps1`
- Modify: `scripts/install.sh`

**Required gates:**

- Every bootstrap stage is one of `native`, `probe-only`, `native-first-with-script-fallback:<reason>`, or
  `direct-install-script:<reason>`.
- Packaged GUI bootstrap summaries report zero pure script-only stages.
- Any script fallback has a concrete reason string and receives the same bundled resource paths as Rust.
- Interactive stages such as configure/setup/gateway remain Rust-handled skips in GUI bootstrap.

**Implementation slices:**

1. Add tests that fail on a script-capable packaged stage without a reason.
2. Port the highest-frequency remaining script fallback into Rust first.
3. Keep shell behavior feature-complete for direct `install.ps1` and `install.sh`.
4. Commit each migrated stage separately with a lifecycle or stage-plan test.

**Verification command:**

```powershell
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml build_stage_plan -- --nocapture
python -m unittest tests.scripts.test_prepare_bootstrap_tools
```

## Phase 3: Bundled Python Runtime Decision

**Purpose:** Remove user-visible Python dependency from packaged installs without pretending Hermes core is a Rust app.

**Files:**

- Create or modify: `scripts/prepare_python_runtime.py`
- Modify: `.github/workflows/build-windows-installer.yml`
- Modify: `.github/workflows/build-unix-installers.yml`
- Modify: `apps/bootstrap-installer/src-tauri/src/bootstrap.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/lib.rs`
- Modify: `scripts/validate_installer_artifacts.py`
- Test: `tests/scripts/test_validate_installer_artifacts.py`
- Test: `tests/scripts/test_prepare_bootstrap_tools.py`

**Required gates:**

- Bundled Python runtime is platform-specific, checksummed, and manifest-owned.
- Runtime extraction is regular-file/directory-only with path traversal and symlink rejection.
- Runtime is tried before system Python in packaged installs.
- System Python remains direct-install fallback for one release.
- Wheelhouse Python tag matches the bundled runtime tag.
- `--self-check` validates runtime manifest shape without launching the GUI.

**Implementation slices:**

1. Document and test the Python runtime manifest schema. Done in branch.
2. Prepare one Windows x64 runtime bundle first because it addresses the user's most likely installer pain.
3. Add Rust runtime selection and extraction with tests. Done in branch.
4. Extend Linux/macOS after Windows passes packaged smoke.
5. Add release-size notes before enabling the runtime bundle by default.

**Current branch status:**

- `scripts/prepare_python_runtime.py` can create and validate an audited runtime manifest from local or
  `NAME=HTTPS_URL=SHA256` inputs.
- Windows, Linux, and macOS installer workflows accept optional audited Python runtime archives and validate them before
  upload.
- The Tauri bundle declares `python-runtime/` as a resource directory, and self-check fails if that resource contract is
  removed.
- The Rust Python stage prefers a manifest-verified bundled runtime archive, validates extraction through
  `uv python find 3.11`, and falls back to `uv python install 3.11` without deleting the legacy path.
- `scripts/build_python_runtime_archive.py` now rejects host/target label mismatches by default so a locally generated
  uv runtime cannot be uploaded with the wrong platform or architecture metadata.
- Release artifact validation now checks that the bundled runtime `pythonTag` matches the wheelhouse Python tag whenever
  both resources are present.
- The Rust runtime extraction path has a dedicated unsafe ZIP-entry regression test in addition to shared archive
  extraction safety coverage.
- The runtime bundle is still decision-gated for default release inclusion. The next runtime work is packaged smoke and
  release-size/security-update review, not another runtime schema rewrite.

**Verification command:**

```powershell
python -m unittest tests.scripts.test_validate_installer_artifacts tests.scripts.test_prepare_bootstrap_tools
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml self_check_validates -- --nocapture
```

## Phase 4: Source Snapshot And No-Git Update Finalization

**Purpose:** Make Git unnecessary for normal packaged fresh install and archive-created update paths.

**Files:**

- Modify: `apps/bootstrap-installer/src-tauri/src/repo_archive.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/update.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/bootstrap.rs`
- Modify: `.github/workflows/build-windows-installer.yml`
- Modify: `.github/workflows/build-unix-installers.yml`
- Add: `scripts/build_source_archive.py`
- Add: `scripts/prepare_source_archive.py`
- Modify: `scripts/install.ps1`
- Modify: `scripts/install.sh`
- Test: `apps/bootstrap-installer/src-tauri/src/repo_archive.rs`
- Test: `apps/bootstrap-installer/src-tauri/src/update.rs`
- Test: `tests/scripts/test_build_source_archive.py`
- Test: `tests/scripts/test_prepare_source_archive.py`

**Required gates:**

- Fresh packaged installs prefer native source archive or bundled source snapshot. **Done for manifest-verified Tauri
  `source-archive/` resources; falls back to GitHub archive download.**
- Archive-created updates refresh source through Rust and call `hermes update --finalize-only`. **Done for Windows,
  Linux, and macOS targets; update refresh now prefers the installer commit pin and bundled source archive.**
- Git preparation is not entered before dependency finalization for archive-created installs. **Done for supported
  desktop targets; unsupported targets still fall back to Git preparation.**
- Script Git clone/update remains available for direct installs and recovery.

**Verification command:**

```powershell
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml archive -- --nocapture
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml update -- --nocapture
```

## Phase 5: Managed Resource Ownership And Uninstall Parity

**Purpose:** Make install, repair, lite uninstall, and full uninstall faster and safer through explicit ownership.

**Files:**

- Modify: `apps/hermes-manager/src/commands.rs`
- Modify: `apps/hermes-manager/src/installed_manifest.rs`
- Modify: `apps/hermes-manager/src/ownership.rs`
- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`
- Modify: `apps/desktop/electron/desktop-uninstall.cjs`
- Test: Rust manager tests
- Test: desktop uninstall tests

**Required gates:**

- Managed roots include only installer-owned runtime, cache, tools, and staged updater paths. **Verified by
  `apps/hermes-manager` path and ownership tests.**
- Lite uninstall preserves user config, `.env`, sessions, skills, memories, and logs. **Verified by manager unit and
  CLI smoke tests.**
- Full uninstall requires explicit confirmation and still rejects paths outside Hermes home. **Left in Python
  uninstall path; Rust manager covers lite/gui cleanup only.**
- Repair-clean can remove corrupted managed Node/Python/uv/Git/cache roots and allow next launch to restore them.
  **Verified by `repair-clean` unit and CLI smoke tests.**
- Desktop uninstall prefers `hermes-manager` for lite mode and falls back to Python uninstall on failure. **Verified
  by desktop uninstall tests.**

**Verification command:**

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
node --test apps/desktop/electron/desktop-uninstall.test.cjs
```

## Phase 6: Optional Dependency Bundles With Size Gates

**Purpose:** Reduce install-time downloads without bloating every release with dependencies many users never use.

**Allowed default bundles:**

- Node.js
- `uv`
- ripgrep
- Windows Git only while Git recovery remains needed
- Python runtime after Phase 3 approval
- Core Python wheelhouse
- npm cache
- Electron cache
- Playwright browser cache if release-size review accepts it

**Decision-gated bundles:**

- ffmpeg
- platform SDK wheels for messaging providers
- voice/STT/TTS heavy dependencies
- browser automation dependencies beyond Playwright cache
- model-provider native helpers

**Required gate before adding a decision-gated bundle:**

- manifest schema support;
- validator support; **Done for release artifacts, bundled payload directories, Python runtime, and source archive.**
- runtime trust-boundary support;
- release-size and security-update note; **Partially enforced with CI byte budgets for installer artifacts,
  bundled payload directories, Python runtime archives, and source archives.**
- fallback behavior if the bundled archive is stale, missing, or fails checksum.

## Phase 7: Desktop Bootstrap Native Bridge

**Purpose:** Make desktop first launch call one native manager/bootstrap bridge for environment preparation instead of
duplicating inference across JavaScript and Rust.

**Files:**

- Modify: `apps/desktop/electron/bootstrap-runner.cjs`
- Modify: `apps/desktop/electron/bootstrap-platform.cjs`
- Modify: `apps/desktop/electron/main.cjs`
- Modify: `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`
- Modify: `apps/hermes-manager/src/commands.rs`
- Test: `apps/desktop/electron/bootstrap-runner.test.cjs`
- Test: `apps/desktop/electron/bootstrap-platform.test.cjs`

**Required gates:**

- JavaScript resolves the packaged native bridge path and delegates environment preparation to it. **Started:
  bootstrap runner now owns packaged `hermes-manager bootstrap-stage install-metadata` after successful first-launch
  install.**
- JavaScript still owns UI progress, IPC, and desktop-specific presentation.
- Native bridge returns structured stage events and failure categories. **Started: `hermes-manager bootstrap-capabilities`
  exposes a schema-versioned JSON capability probe; `hermes-manager bootstrap-manifest` reports the native bridge
  manifest and is probed by the desktop runner; `hermes-manager bootstrap-stage install-metadata` returns
  script-compatible stage JSON and is now used by the desktop runner after first-launch bootstrap. Desktop runner now
  dispatches manifest-matched native stages through the bridge, and `bootstrap-marker` is the first real script manifest
  stage handled natively. Windows `path` is also handled natively, including user PATH and `HERMES_HOME` writes.
  Windows `config-templates` is handled natively for directory setup, `.env`, `config.yaml`, `SOUL.md`, and bundled
  skills fallback copy. Windows `uv`, `git`, `python`, `node`, `system-packages`, `node-deps`, `repository`, `venv`,
  `dependencies`, `desktop`, and `platform-sdks` now have native-first execution paths that use packaged resources where
  available and return structured fallback categories when parity cannot be proven. Windows non-interactive `configure`
  and `gateway` stages now short-circuit through native skip results instead of launching PowerShell only to no-op.
  Desktop keeps script fallback when full native bootstrap is not available or the native stage returns a known fallback
  category.**
- Python/shell fallback remains available if native bridge is missing or exits with a known fallback code.

**Current branch status:**

- The desktop runner probes `hermes-manager bootstrap-manifest` and dispatches matching script stages through
  `hermes-manager bootstrap-stage` before invoking PowerShell.
- Windows `uv` installs from bundled `bootstrap-tools` archives after manifest SHA-256 verification.
- Windows `git` preserves local/PATH Git priority, then installs bundled PortableGit/MinGit only when Git is missing.
- Windows `python` preserves local Python 3.11 priority, then uses managed/PATH `uv python find/install 3.11`.
- Windows `node` installs a bundled portable Node.js archive, verifies the Node floor, and preserves npm.
- Windows `system-packages` restores bundled ripgrep and ffmpeg into the managed bin directory.
- Windows `node-deps` runs npm for root browser-tool dependencies and `ui-tui`, restores bundled npm cache and
  Playwright browser cache, and falls back if browser-engine parity cannot be proven.
- Windows `repository` can clone the official HTTPS repository with managed or PATH Git, detach to a pinned commit, and
  verify HEAD; existing checkout update/repair still falls back to the script.
- Windows `platform-sdks` verifies configured messaging SDK imports in the venv and attempts targeted pip recovery
  before returning a fallback category.
- Windows `path`, `config-templates`, `bootstrap-marker`, `install-metadata`, `configure`, and `gateway` are native
  handled, native probed, or native skipped with structured stage output.
- Windows `venv` is now a real native stage: it uses managed or PATH `uv`, creates `venv` with Python 3.11, verifies the
  venv Python shim, and returns a script fallback category for missing or failed prerequisites.
- Windows `dependencies` is now a real native stage: it uses the managed venv, prefers a bundled wheelhouse when
  present, falls back through locked `uv sync` and editable install tiers, and verifies baseline imports before
  declaring success.
- The desktop runner passes the active install root and packaged `bootstrap-tools/` and `wheelhouse/` resource paths into
  native stages, so the bridge can use the same local resources as the script fallback.
- `canRunFullBootstrap` must stay false until packaged artifact smoke proves the native path end-to-end and Phase 8
  records which script fallbacks can be burned down after one release.
- `configure` and `gateway` remain user-input stages. In GUI bootstrap they should stay native skipped unless the user
  explicitly starts an interactive setup flow.

**Verification command:**

```powershell
node --test apps/desktop/electron/bootstrap-runner.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs
cargo test --manifest-path apps/hermes-manager/Cargo.toml
```

## Phase 8: One-Release Fallback Burn-Down

**Purpose:** Remove duplicated fallback branches only after release evidence exists.

**Removal rule:**

Do not delete a fallback in the same phase that introduces its Rust replacement. A fallback can be removed only when:

1. Windows, macOS, and Linux packaged releases have shipped with the Rust path enabled.
2. CI has packaged artifact smoke for that path.
3. Direct `install.ps1` or `install.sh` still covers source installs if applicable.
4. Repair and uninstall can clean the resources created by the Rust path.
5. Release notes documented the changed install behavior.

**Files:**

- Modify: fallback branches in `scripts/install.ps1`, `scripts/install.sh`,
  `apps/bootstrap-installer/src-tauri/src/orchestrator.rs`, and desktop bootstrap files only when all gates above are
  satisfied.
- Add: `docs/release/fallback-burn-down.json` records retained fallback branches, removal gates, and release evidence.
- Add: `scripts/validate_fallback_burn_down.py` verifies every registry entry points at a live source marker.

**Status:** Started. Key installer/desktop fallbacks now have stable burn-down markers and a CI-enforced registry; no
fallback has been removed yet because release evidence is not available.

## Phase 9: Larger Rust Candidate Review

**Purpose:** Evaluate deeper Rust migration only after install dependencies are no longer the dominant problem.

**Candidate list:**

- archive/download/hash helpers duplicated across Rust, Python, and scripts;
- process-tree supervision and health checks;
- file indexing and ignore-rule helpers;
- stable config/schema validation helpers;
- gateway-adjacent low-level utilities with measurable dependency payoff.

**Rejection rule:**

Do not Rust-migrate fast-changing agent logic, provider logic, gateway behavior, or plugin execution unless a later
candidate document proves:

- exact parity tests;
- measurable dependency or reliability gain;
- no prompt-cache or model-tool footprint regression;
- no user-visible feature loss.

## Global Verification Before Claiming Progress

For documentation-only changes:

```powershell
git diff --check
```

For installer validation changes:

```powershell
python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
git diff --check
```

For Rust bootstrap changes:

```powershell
cargo test --manifest-path apps/bootstrap-installer/src-tauri/Cargo.toml
python -m unittest tests.scripts.test_prepare_bootstrap_tools tests.scripts.test_validate_installer_artifacts
git diff --check
```

For manager or desktop uninstall changes:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
node --test apps/desktop/electron/desktop-uninstall.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs
git diff --check
```

## Execution Order From This Commit

1. Finish the Phase 7 Windows native bridge burn-down before deeper rewrites:
   `desktop` native-first stage, then a stage-budget test that documents any intentionally retained fallback.
2. Tighten Phase 2 shell-budget tests so packaged normal path cannot regain silent script-only stages after the bridge
   covers the current Windows script manifest.
3. Finish Phase 1 packaged artifact gates that are still release-workflow-only: Windows NSIS artifact validation stays
   conservative unless the installer can safely forward no-UI self-check flags.
4. Promote Phase 3 Python runtime from optional resource to default only after packaged smoke proves the runtime,
   wheelhouse, venv, and dependency stages work together on Windows x64.
5. Extend the Python runtime default path to macOS and Linux after Windows packaged smoke is stable and size/security
   notes are written.
6. Expand Phase 6 bundles only when the default package still downloads a high-frequency dependency during first launch;
   each new bundle needs manifest ownership, checksum validation, and a size gate.
7. Start Phase 8 fallback removal only after one release ships the native path on Windows, macOS, and Linux with
   packaged smoke evidence.
8. Defer Phase 9 deeper Rust migration until install-time dependency reduction is no longer the dominant user pain.

## Self-Review

Spec coverage:

- The plan directly targets install-time dependency reduction, self-contained packaged releases, and no feature deletion.
- The plan keeps Python/Electron feature layers intact and moves only the release boundary into Rust.
- The plan includes install, update, repair, uninstall, fallback, and release validation gates.

Placeholder scan:

- No task depends on an undefined future component.
- Every immediate next slice has concrete files, commands, and expected outcomes.

Risk check:

- The plan avoids fake NSIS CLI smoke unless the installer supports it.
- Python runtime bundling is behind an explicit decision gate because it affects package size and security updates.
- Fallback removal is delayed until release evidence exists.
