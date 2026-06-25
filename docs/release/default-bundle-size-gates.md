<!--
文件意图：记录任何新默认发布 bundle 在启用前必须满足的大小、安全、manifest 和回退门槛。
-->

# Default Bundle Size Gates

This document applies before any optional release resource becomes a default bundled payload.

## Required Gate For Every New Default Bundle

- The payload must have a manifest owner with schema version, platform, architecture, source URL, size, and SHA-256.
- The payload must be covered by `scripts/validate_installer_artifacts.py` or a narrower validator called by CI.
- CI must pass a documented `--max-artifact-bytes` or `--max-total-artifact-bytes` budget before upload.
- The packaged bootstrap path must keep stale, missing, checksum-mismatched, and wrong-target fallback behavior.
- Release notes must state the payload source, version or cache provenance, and update responsibility.

## Current Decision-Gated Payloads

| Payload | Default status | Required decision before default |
| --- | --- | --- |
| Python runtime | Optional | Use `docs/release/python-runtime-default-gate.md`. |
| ffmpeg/ripgrep tool archives | Optional except required tool targets | Record source, license, CVE/update cadence, and size delta. |
| Playwright browser cache | Optional | Record browser revision, security update policy, and package size delta. |
| Electron cache | Optional | Record Electron version, mirror/source, and package size delta. |
| npm cache | Optional | Record lockfile provenance, cache pruning rule, and package size delta. |
| Platform SDK wheels | Optional recovery input | Record wheelhouse source files, Python tag compatibility, and package size delta. |
| Voice/STT/TTS resources | Not defaulted | Require a separate model/data license, privacy, and update note before bundling. |

## Current CI Budgets

- Windows installer artifacts: per-artifact `2147483648`, total `3221225472`.
- Windows Python runtime artifacts: per-artifact `536870912`, total `536870912`.
- Hermes source archives: `268435456`.
- Unix installer artifacts use the same validator and must keep platform-specific budgets in workflow YAML.

If a proposed default bundle cannot fit within its documented budget, keep it optional and require explicit release
approval before raising the budget.
