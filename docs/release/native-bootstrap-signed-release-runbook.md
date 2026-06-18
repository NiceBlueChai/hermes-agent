<!--
文件意图：记录启用 Rust 原生 packaged bootstrap 前，签名发布证据的收集和验证步骤。
-->

# Native Bootstrap Signed Release Runbook

Use this only for a real signed release. Unsigned smoke runs keep proving packaging shape, but they do not unlock
`canRunFullBootstrap` and must not be recorded as fallback burn-down evidence.

## Required Inputs

- `release-tag`: the final GitHub release tag. It must be one path segment: no whitespace, `/`, `\`, `?`, or `#`.
- `release-notes-url`: `https://github.com/<owner>/<repo>/releases/tag/<release-tag>`.
- `python-runtime-version`: the bundled Python patch version.
- `python-runtime-archive`, `linux-python-runtime-archive`, and `macos-python-runtime-archive`:
  `NAME=HTTPS_URL=SHA256` audited runtime archives.
- `source-archive`, `linux-source-archive`, and `macos-source-archive`: `NAME=HTTPS_URL=SHA256` audited Hermes
  source archives.
- `python-runtime-without-bytes`, `linux-python-runtime-without-bytes`, and
  `macos-python-runtime-without-bytes`: signed installer sizes from matching no-runtime baseline builds.
- Signing configuration: Azure Artifact Signing for Windows, Apple signing/notarization for macOS, and Sigstore for
  Linux.
- Fallback burn-down evidence for `desktop-bootstrap-script-fallback` must include the matching `--signature` value:
  `authenticode`, `developer-id-notarized`, or `sigstore`.
- Fallback burn-down evidence for `installer-source-archive-download-fallback` and
  `installer-python-runtime-download-fallback` must be recorded only after the signed artifacts prove the corresponding
  registry checks in `docs/release/fallback-burn-down.json`. If a `signature` field is recorded for those entries, it
  must match the platform signing type.
- Complete fallback burn-down evidence for Windows, macOS, and Linux must point at the same GitHub owner/repository,
  release tag, and commit SHA.

## Dispatch Signed Workflows

Run the Windows signed path:

```powershell
gh workflow run build-windows-installer.yml `
  --repo <owner>/<repo> `
  --ref <release-branch-or-tag> `
  -f release-tag=<release-tag> `
  -f release-notes-url=https://github.com/<owner>/<repo>/releases/tag/<release-tag> `
  -f python-runtime-version=<python-patch-version> `
  -f python-runtime-archive=<name=https-url=sha256> `
  -f source-archive=<name=https-url=sha256> `
  -f python-runtime-without-bytes=<signed-no-runtime-size>
```

Run the signed Unix matrix through the fork-dispatchable workflow:

```powershell
gh workflow run build-windows-installer.yml `
  --repo <owner>/<repo> `
  --ref <release-branch-or-tag> `
  -f unix-smoke-only=true `
  -f release-tag=<release-tag> `
  -f release-notes-url=https://github.com/<owner>/<repo>/releases/tag/<release-tag> `
  -f python-runtime-version=<python-patch-version> `
  -f linux-python-runtime-archive=<name=https-url=sha256> `
  -f macos-python-runtime-archive=<name=https-url=sha256> `
  -f source-archive=<name=https-url=sha256> `
  -f linux-python-runtime-without-bytes=<signed-linux-no-runtime-size> `
  -f macos-python-runtime-without-bytes=<signed-macos-no-runtime-size>
```

Do not pass `unsigned-smoke-only=true` for signed evidence.

## Collect Evidence Artifacts

Download these artifacts from the completed signed runs:

- `*-python-runtime-default-evidence` JSON files for Windows, macOS, and Linux.
- `*-fallback-evidence` command and JSON artifacts for Windows, macOS, and Linux. These cover every retained entry in
  `docs/release/fallback-burn-down.json`.
- Signed installer artifacts and signature/notarization outputs.

Validate the runtime default evidence together:

```powershell
python scripts/validate_python_runtime_default_gate.py `
  --evidence <windows-runtime-evidence.json> `
  --evidence <macos-runtime-evidence.json> `
  --evidence <linux-runtime-evidence.json> `
  --require-platforms windows,macos,linux
```

Record the fallback evidence using the generated `fallback-burn-down-*.sh` command files, or import the generated JSON
artifacts directly:

```powershell
python scripts/validate_fallback_burn_down.py `
  --add-evidence-json <fallback-burn-down-windows-desktop-bootstrap-script-fallback.json> `
  --add-evidence-json <fallback-burn-down-windows-installer-source-archive-download-fallback.json> `
  --add-evidence-json <fallback-burn-down-windows-installer-python-runtime-download-fallback.json> `
  --add-evidence-json <fallback-burn-down-macos-desktop-bootstrap-script-fallback.json> `
  --add-evidence-json <fallback-burn-down-macos-installer-source-archive-download-fallback.json> `
  --add-evidence-json <fallback-burn-down-macos-installer-python-runtime-download-fallback.json> `
  --add-evidence-json <fallback-burn-down-linux-desktop-bootstrap-script-fallback.json> `
  --add-evidence-json <fallback-burn-down-linux-installer-source-archive-download-fallback.json> `
  --add-evidence-json <fallback-burn-down-linux-installer-python-runtime-download-fallback.json>
```

Verify the desktop bootstrap gate before using `canRunFullBootstrap=true`:

```powershell
python scripts/validate_fallback_burn_down.py --require-complete desktop-bootstrap-script-fallback
```

Then verify every retained fallback entry before removing any script fallback:

```powershell
python scripts/validate_fallback_burn_down.py --print-status
python scripts/validate_fallback_burn_down.py --require-all-complete
```

Only after all retained fallback checks pass may script fallback removal be considered.
