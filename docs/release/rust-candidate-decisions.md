<!--
文件意图：记录更深层 Rust 迁移候选项的批准或暂缓理由，避免把快速变化的功能误迁进 Rust。
-->

# Rust Candidate Decisions

This note applies after installer, packaged bootstrap, update, repair, and uninstall parity. A candidate is not approved
unless it has exact parity tests, a measurable dependency or reliability gain, and no prompt-cache or model-tool
footprint regression.

## Approved Scope

| Candidate | Decision | Parity tests | Measurable gain | Prompt/tool footprint |
| --- | --- | --- | --- | --- |
| Installer/bootstrap resources | Approved | Bootstrap self-check, lifecycle, archive, update, manager tests | Fewer install-time dependencies | No model tool change |
| Update/repair/uninstall boundaries | Approved | `apps/bootstrap-installer` update tests and `apps/hermes-manager` tests | Faster cleanup and safer path ownership | No model tool change |
| Release artifact validators | Approved | `tests.scripts.test_validate_installer_artifacts` and release workflow tests | CI catches stale or oversized bundles | No model tool change |

## Deferred Candidates

| Candidate | Decision | Required proof before approval |
| --- | --- | --- |
| Agent conversation loop | Deferred | Full role-alternation, prompt-cache, compression, provider, and tool-call parity suite. |
| Model/provider routing | Deferred | Provider matrix parity and evidence that Rust removes a runtime dependency without slowing provider updates. |
| Gateway platform behavior | Deferred | Per-platform adapter parity and a stable low-level helper boundary. |
| Plugin and skill execution | Deferred | No prompt-cache invalidation, no model-tool schema growth, and plugin compatibility proof. |
| Memory/provider plugins | Deferred | Provider abstraction tests plus migration evidence from at least one real plugin. |
| Voice/STT/TTS resources | Deferred | License, privacy, size, update cadence, and release smoke note for each model/data payload. |

Fast-changing agent logic, provider logic, gateway behavior, and plugin execution stay in Python or TypeScript until a
future candidate note replaces the deferred decision above with concrete parity evidence.
