# Source Archive

Place release-time Hermes source archives here before building the Tauri bundle.

The Rust bootstrapper validates `source-archive-manifest.json`, checks archive
size and SHA-256, and tries this bundled source snapshot before falling back to
the GitHub archive download. Keep the directory empty for normal development
builds.

Manifest files use schema version 1 and must describe exactly the repository
archive intended by the installer build pin.

Release workflow input format:

```text
NAME=HTTPS_URL=SHA256
```

Build a local archive before uploading it:

```powershell
python scripts/build_source_archive.py --archive-ref <commit-or-branch> --force
```

Example:

```text
hermes-agent-abcdef123.zip=https://example.invalid/hermes-agent-abcdef123.zip=<sha256>
```
