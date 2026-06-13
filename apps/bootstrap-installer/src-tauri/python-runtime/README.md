# Python Runtime

Place release-time Python runtime archives here before building the Tauri bundle.

The Rust bootstrapper validates `python-runtime-manifest.json`, checks archive
size and SHA-256, and tries this bundled runtime before falling back to
`uv python install 3.11`. Keep the directory empty for normal development
builds.

Release workflow input format:

```text
NAME=HTTPS_URL=SHA256
```

Build a local archive before uploading it:

```powershell
python scripts/build_python_runtime_archive.py --platform windows --arch x64 --force
```

The builder expects `--platform` and `--arch` to match the host that is running
`uv python install`. Use `--allow-target-mismatch` only when a separate audited
cross-target builder creates the runtime payload.

Example:

```text
python-runtime-windows-x64.zip=https://example.invalid/python-runtime-windows-x64.zip=<sha256>
```
