<!--
文件意图：集中记录 Hermes Rust 自包含发布路线图、剩余工作和可验证完成标准。
-->

# Rust Self-Contained Release Roadmap

This roadmap tracks the work needed to make packaged Hermes installs smaller, faster, and easier to install or
uninstall without removing existing script-based install paths.

## Target

Packaged desktop installers should use Rust-owned bootstrap, repair, update, and uninstall paths by default when the
signed release evidence proves parity. Direct source installs through `install.ps1` and `install.sh` stay supported.

## Done

- Rust bootstrap manager and Tauri installer lifecycle paths exist and are covered by CI release-binary smoke checks.
- Packaged resource manifests cover bootstrap tools, source archives, wheelhouse inputs, and Python runtime archives.
- Signed release workflows require audited runtime/source archive metadata before recording fallback burn-down evidence.
- `scripts/validate_fallback_burn_down.py --require-all-complete` gates removal of retained script fallbacks.
- `docs/release/rust-candidate-decisions.md` defines which deeper Rust migrations are approved or deferred.

## Remaining Work

| Item | Completion standard |
| --- | --- |
| Keep the branch green | All GitHub checks on `docs/rust-self-contained-release` pass after each pushed change. |
| Signed release evidence | `docs/release/fallback-burn-down.json` contains Windows, macOS, and Linux evidence for all retained entries. |
| Native default switch | Packaged installers set the native path as default only after `--require-all-complete` passes on real signed evidence. |
| Fallback removal | Script fallback code is removed only for entries whose registry evidence is complete and validated. |
| Release notes | The signed release notes include install behavior, bundled Python version/SHA/size delta, and fallback policy. |
| Deeper Rust migration | Any new Rust scope first updates `docs/release/rust-candidate-decisions.md` with parity tests and measurable gain. |

## Goal Command Standard

The route is complete when these commands pass on the release branch with real signed evidence:

```bash
python scripts/validate_fallback_burn_down.py --print-status
python scripts/validate_fallback_burn_down.py --require-all-complete
python -m unittest tests.scripts.test_validate_fallback_burn_down -v
python -m unittest tests.scripts.test_prepare_bootstrap_tools -v
```

`--print-status` must report `"allComplete": true`. The GitHub branch checks must also be green. Until then, the route
is still in progress.
