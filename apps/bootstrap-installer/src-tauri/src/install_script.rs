//! Resolves and downloads `scripts/install.ps1` (and `install.sh`).
//!
//! Resolution order:
//!   1. Dev shortcut: a sibling repo checkout via $HERMES_SETUP_DEV_REPO_ROOT
//!      env var. Lets devs iterate without re-publishing the script.
//!   2. Bundled fallback: installers serve the script compiled into this
//!      binary, avoiding a first-run network dependency.
//!   3. Cache/network: download from GitHub raw only when no local script can
//!      be used.
//!
//! Mirrors `apps/desktop/electron/bootstrap-runner.cjs`'s `resolveInstallScript`,
//! but the dev-checkout resolution is driven by an env var rather than the
//! Electron app's APP_ROOT/../.. trick, because Hermes-Setup.exe is meant
//! to live OUTSIDE any repo checkout.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::paths;

/// Identity of the install.ps1 we'll execute. Used by both the manifest
/// fetch and the per-stage runs.
#[derive(Debug, Clone)]
pub struct ResolvedScript {
    pub path: PathBuf,
    pub source: ScriptSource,
    /// Commit pin (40-char SHA) if known. install.ps1's `-Commit` arg is
    /// what makes the repo stage clone the exact tested SHA.
    pub commit: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    DevCheckout,
    Bundled,
    Cached,
    Downloaded,
}

/// What flavor of script (Windows .ps1 vs Unix .sh).
#[derive(Debug, Clone, Copy)]
pub enum ScriptKind {
    Ps1,
    Sh,
}

/// Metadata for an install script embedded into this binary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BundledScriptResource {
    pub filename: &'static str,
    pub size_bytes: usize,
    pub sha256: String,
}

impl ScriptKind {
    pub fn for_current_os() -> Self {
        if cfg!(target_os = "windows") {
            Self::Ps1
        } else {
            Self::Sh
        }
    }

    fn filename(&self) -> &'static str {
        match self {
            Self::Ps1 => "install.ps1",
            Self::Sh => "install.sh",
        }
    }
}

/// Validates a string looks like a git SHA (7+ hex chars). Mirrors
/// `STAMP_COMMIT_RE` from bootstrap-runner.cjs.
fn is_valid_commit(s: &str) -> bool {
    let len = s.len();
    (7..=40).contains(&len) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Resolves the install script to use for this run.
///
/// `pin` is the commit-or-branch from either Hermes-Setup's build-time
/// constant (compiled into the installer) or a runtime override.
pub async fn resolve(
    kind: ScriptKind,
    pin: &Pin,
    emit_log: &impl Fn(&str),
) -> Result<ResolvedScript> {
    // 1. Dev shortcut.
    if let Ok(repo_root) = std::env::var("HERMES_SETUP_DEV_REPO_ROOT") {
        let candidate = PathBuf::from(repo_root)
            .join("scripts")
            .join(kind.filename());
        if candidate.exists() {
            emit_log(&format!(
                "[bootstrap] dev mode — using local {} at {}",
                kind.filename(),
                candidate.display()
            ));
            return Ok(ResolvedScript {
                path: candidate,
                source: ScriptSource::DevCheckout,
                commit: pin.commit.clone(),
                branch: pin.branch.clone(),
            });
        }
    }

    // 2. Bundled fallback. Installers should not need network just to obtain
    // the small orchestration script shipped with this binary.
    if let Some(ref_name) = bundled_script_ref(pin) {
        let bundled = bundled_cached_path(kind, ref_name);
        emit_log(&format!(
            "[bootstrap] using bundled {} for {}",
            kind.filename(),
            truncate_ref(ref_name)
        ));
        crate::artifact::write_atomic_verified(&bundled, bundled_script_bytes(kind), None)
            .await
            .with_context(|| format!("writing bundled {}", kind.filename()))?;
        return Ok(ResolvedScript {
            path: bundled,
            source: ScriptSource::Bundled,
            commit: pin.commit.clone(),
            branch: pin.branch.clone(),
        });
    }

    // 3. Network. Pin must be a real commit or a branch ref.
    let commit_or_ref = match (&pin.commit, &pin.branch) {
        (Some(c), _) if is_valid_commit(c) => c.clone(),
        (_, Some(b)) if !b.trim().is_empty() => b.clone(),
        (Some(other), _) => {
            return Err(anyhow!(
                "install script pin commit `{other}` is not a valid git SHA"
            ));
        }
        _ => {
            return Err(anyhow!(
                "no install-script pin supplied — installer cannot resolve a script source"
            ));
        }
    };

    let cached = cached_path(kind, &commit_or_ref);
    if cached.exists() {
        emit_log(&format!(
            "[bootstrap] using cached {} for {}",
            kind.filename(),
            truncate_ref(&commit_or_ref)
        ));
        return Ok(ResolvedScript {
            path: cached,
            source: ScriptSource::Cached,
            commit: pin.commit.clone(),
            branch: pin.branch.clone(),
        });
    }

    emit_log(&format!(
        "[bootstrap] downloading {} for {} from GitHub",
        kind.filename(),
        truncate_ref(&commit_or_ref)
    ));

    download(kind, &commit_or_ref, &cached).await?;

    emit_log(&format!("[bootstrap] cached to {}", cached.display()));

    Ok(ResolvedScript {
        path: cached,
        source: ScriptSource::Downloaded,
        commit: pin.commit.clone(),
        branch: pin.branch.clone(),
    })
}

#[derive(Debug, Clone, Default)]
pub struct Pin {
    pub commit: Option<String>,
    pub branch: Option<String>,
}

fn cached_path(kind: ScriptKind, commit_or_ref: &str) -> PathBuf {
    let safe = sanitize_ref(commit_or_ref);
    let filename = match kind {
        ScriptKind::Ps1 => format!("install-{safe}.ps1"),
        ScriptKind::Sh => format!("install-{safe}.sh"),
    };
    paths::bootstrap_cache_dir().join(filename)
}

fn bundled_cached_path(kind: ScriptKind, commit: &str) -> PathBuf {
    let safe = sanitize_ref(commit);
    let filename = match kind {
        ScriptKind::Ps1 => format!("install-{safe}-bundled.ps1"),
        ScriptKind::Sh => format!("install-{safe}-bundled.sh"),
    };
    paths::bootstrap_cache_dir().join(filename)
}

fn bundled_script_bytes(kind: ScriptKind) -> &'static [u8] {
    match kind {
        ScriptKind::Ps1 => include_bytes!("../../../../scripts/install.ps1"),
        ScriptKind::Sh => include_bytes!("../../../../scripts/install.sh"),
    }
}

/// Return metadata for all install scripts compiled into this binary.
pub fn bundled_script_manifest() -> Vec<BundledScriptResource> {
    [ScriptKind::Ps1, ScriptKind::Sh]
        .into_iter()
        .map(bundled_script_resource)
        .collect()
}

fn bundled_script_resource(kind: ScriptKind) -> BundledScriptResource {
    let bytes = bundled_script_bytes(kind);
    BundledScriptResource {
        filename: kind.filename(),
        size_bytes: bytes.len(),
        sha256: crate::artifact::sha256_hex(bytes),
    }
}

fn bundled_script_ref(pin: &Pin) -> Option<&str> {
    match (&pin.commit, &pin.branch) {
        (Some(commit), _) if is_valid_commit(commit) => Some(commit.as_str()),
        (None, Some(branch)) if !branch.trim().is_empty() => Some(branch.as_str()),
        _ => None,
    }
}

/// Replace anything that's not [A-Za-z0-9._-] with `_`. Branch refs can
/// contain `/`, dots, etc.; we want a flat filename.
fn sanitize_ref(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn truncate_ref(s: &str) -> &str {
    if is_valid_commit(s) && s.len() >= 12 {
        &s[..12]
    } else {
        s
    }
}

/// Downloads to `dest_path` via reqwest with rustls. Atomically renames
/// `dest_path.tmp` → `dest_path` so partial writes don't poison the cache.
async fn download(kind: ScriptKind, commit_or_ref: &str, dest_path: &Path) -> Result<()> {
    let url = format!(
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/{}/scripts/{}",
        commit_or_ref,
        kind.filename()
    );

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating bootstrap-cache parent dir {}", parent.display()))?;
    }

    crate::artifact::download_to_cache(
        crate::artifact::DownloadSpec {
            url,
            user_agent: "hermes-setup/0.0.1",
            expected_sha256: None,
        },
        dest_path,
    )
    .await
    .with_context(|| format!("downloading {}", kind.filename()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_commit_accepts_short_and_full_shas() {
        assert!(is_valid_commit("02d26981d3d4ad50e142399b8476f59ad5953ff0"));
        assert!(is_valid_commit("02d2698"));
        assert!(!is_valid_commit("02d269"));
        assert!(!is_valid_commit("not-a-sha"));
        assert!(!is_valid_commit(""));
    }

    #[test]
    fn sanitize_ref_replaces_slashes() {
        assert_eq!(sanitize_ref("bb/gui"), "bb_gui");
        assert_eq!(sanitize_ref("main"), "main");
        assert_eq!(sanitize_ref("release/1.2.3"), "release_1.2.3");
    }

    #[test]
    fn bundled_script_is_available_for_both_platform_scripts() {
        assert!(bundled_script_bytes(ScriptKind::Ps1).starts_with(b"#"));
        assert!(bundled_script_bytes(ScriptKind::Sh).starts_with(b"#!/"));
    }

    #[test]
    fn bundled_script_ref_accepts_commit_and_branch_pins() {
        assert_eq!(
            bundled_script_ref(&Pin {
                commit: Some("02d26981d3d4ad50e142399b8476f59ad5953ff0".into()),
                branch: Some("main".into()),
            }),
            Some("02d26981d3d4ad50e142399b8476f59ad5953ff0")
        );
        assert_eq!(
            bundled_script_ref(&Pin {
                commit: None,
                branch: Some("main".into()),
            }),
            Some("main")
        );
        assert_eq!(
            bundled_script_ref(&Pin {
                commit: Some("not-a-sha".into()),
                branch: Some("main".into()),
            }),
            None
        );
    }

    #[test]
    fn bundled_cache_path_does_not_collide_with_download_cache_path() {
        let commit = "02d26981d3d4ad50e142399b8476f59ad5953ff0";
        assert_ne!(
            cached_path(ScriptKind::Ps1, commit),
            bundled_cached_path(ScriptKind::Ps1, commit)
        );
        assert_ne!(
            cached_path(ScriptKind::Sh, commit),
            bundled_cached_path(ScriptKind::Sh, commit)
        );
    }

    #[test]
    fn bundled_script_manifest_reports_size_and_checksum() {
        let manifest = bundled_script_manifest();

        assert_eq!(manifest.len(), 2);
        assert!(manifest
            .iter()
            .any(|resource| resource.filename == "install.ps1"));
        assert!(manifest
            .iter()
            .any(|resource| resource.filename == "install.sh"));
        for resource in manifest {
            assert!(resource.size_bytes > 0);
            assert_eq!(resource.sha256.len(), 64);
        }
    }
}
