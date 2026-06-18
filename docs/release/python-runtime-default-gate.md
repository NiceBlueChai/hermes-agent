<!--
文件意图：记录 Python runtime bundle 从可选资源提升为默认发布内容前必须满足的大小、安全和烟测条件。
-->

# Python Runtime Default Gate

This note keeps the bundled Python runtime as an explicit release decision, not an automatic default.

## Current Decision

- Status: optional audited resource.
- Default inclusion: not enabled.
- First candidate: Windows x64 only, because that is the highest-impact installer dependency reduction.
- Fallbacks: system Python discovery and `uv python install 3.11` remain available for direct installs and one release
  after the bundled runtime ships by default.

## Size Gate

- The Windows runtime archive must pass `scripts/validate_installer_artifacts.py` with
  `--max-artifact-bytes 536870912` and `--max-total-artifact-bytes 536870912`.
- The release PR must include the actual `python-runtime-manifest.json` size, SHA-256, Python tag, platform, and arch.
- The release PR must compare the signed installer size with and without the runtime bundle.
- If the installer plus runtime exceeds the budget above, keep the runtime optional and do not promote it to default.

## Security-Update Gate

- The runtime archive source must be HTTPS and checksum-pinned in `python-runtime-manifest.json`.
- Signed release workflows must receive the runtime archive as `NAME=HTTPS_URL=SHA256` input. The workflow may build a
  temporary runtime archive only for unsigned smoke validation.
- The runtime must be rebuilt when the bundled Python patch release receives a security update.
- Release notes must identify the Python runtime version and the archive source used for the signed build.
- If the runtime source cannot be audited or patched promptly, keep the runtime optional.

## Windows x64 Smoke Gate

Before default inclusion, CI must run the signed Windows `Hermes-Setup.exe` with:

- `--self-check` for bootstrap tools, wheelhouse, and Python runtime resources.
- `--self-check-lifecycle` with bootstrap tools, wheelhouse, and Python runtime resource arguments together.
- `scripts/validate_installer_artifacts.py` with both `--wheelhouse-dir` and `--python-runtime-dir`.

The smoke evidence must prove the runtime archive is manifest-owned, checksum-validated, platform-specific, and aligned
with the wheelhouse Python tag.

## Structured Release Evidence

Before enabling the bundled Python runtime by default, attach a JSON evidence file to the release PR and validate it
with:

```powershell
python scripts/validate_python_runtime_default_gate.py --evidence <signed-runtime-evidence.json>
```

For a release that promotes bundled runtimes across all packaged desktop platforms, validate all generated evidence
together so Windows, macOS, and Linux prove the same signed release, commit, and Python runtime version:

```powershell
python scripts/validate_python_runtime_default_gate.py `
  --evidence <signed-runtime-evidence-windows.json> `
  --evidence <signed-runtime-evidence-macos.json> `
  --evidence <signed-runtime-evidence-linux.json> `
  --require-platforms windows,macos,linux
```

`--require-platforms` is only valid with one or more `--evidence` files; the validator rejects it on its own so a release
operator cannot accidentally skip the platform evidence check. Multiple `--evidence` files also require
`--require-platforms`, so multi-platform evidence cannot be validated as unrelated single-platform files. Duplicate or
unknown required platforms are rejected.

Signed installer workflows generate this evidence instead of requiring release operators to hand-author it:

```powershell
python scripts/validate_python_runtime_default_gate.py `
  --print-evidence `
  --manifest apps/bootstrap-installer/src-tauri/python-runtime/python-runtime-manifest.json `
  --python-version <python-patch-version> `
  --with-runtime-artifact <signed-installer-with-runtime> `
  --without-runtime-bytes <signed-installer-size-without-runtime> `
  --release <tag> `
  --url <https-github-release-tag-url> `
  --release-notes <https-github-release-tag-url> `
  --commit <40-char-sha> `
  --signature <authenticode|developer-id-notarized|sigstore> `
  > signed-runtime-evidence.json
```

The evidence file must include:

- `pythonRuntime.version`, `sourceUrl`, `archiveSha256`, `securityUpdatePolicy`, and the actual
  `python-runtime-manifest.json` payload.
- `signedInstaller.platform`, `release`, `url`, `releaseNotes`, `commit`, `signature`, `withRuntimeBytes`,
  `withoutRuntimeBytes`, and `sizeDeltaBytes`.

The validator requires the runtime source and manifest file URLs to be HTTPS, the runtime archive SHA-256 to match a
manifest file, `signedInstaller.platform` to match the manifest platform, release URLs to be GitHub release tag URLs
for `signedInstaller.release` in the same repository, `signedInstaller.commit` to be a 40-character SHA,
`signedInstaller.signature` to match the platform signing method, `sizeDeltaBytes` to equal
`withRuntimeBytes - withoutRuntimeBytes`, and that delta to be positive. Template placeholders such as `OWNER/REPO`
and `vX.Y.Z` are rejected as release evidence.
