//! Repository ZIP archive fallback planning.
//!
//! This module owns the low-level pieces needed for a future no-Git fresh
//! install path: choosing an immutable GitHub archive URL and unpacking it into
//! the managed checkout directory without overwriting user data.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;

/// Source marker written into archive-created checkouts.
pub const SOURCE_MARKER_NAME: &str = ".hermes-source.json";
const SOURCE_ARCHIVE_MANIFEST: &str = "source-archive-manifest.json";
const SOURCE_ARCHIVE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Where a repository archive came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoArchiveSourceKind {
    Bundled,
    Download,
}

impl RepoArchiveSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Download => "download",
        }
    }
}

/// Repository archive selected for a fresh archive install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRepoArchive {
    pub path: PathBuf,
    pub source: RepoArchiveSourceKind,
}

/// GitHub repository archive selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoArchiveSpec {
    pub owner: String,
    pub repo: String,
    pub commit: Option<String>,
    pub branch: Option<String>,
}

impl RepoArchiveSpec {
    /// Build the GitHub ZIP archive URL, preferring immutable commit pins.
    pub fn github_zip_url(&self) -> Result<String> {
        let archive_ref = self.archive_ref()?;
        Ok(format!(
            "https://github.com/{}/{}/archive/{}.zip",
            self.owner, self.repo, archive_ref
        ))
    }

    fn archive_ref(&self) -> Result<&str> {
        if let Some(commit) = self.commit.as_deref().filter(|value| !value.trim().is_empty()) {
            return Ok(commit);
        }
        if let Some(branch) = self.branch.as_deref().filter(|value| !value.trim().is_empty()) {
            return Ok(branch);
        }
        Err(anyhow!("repo archive requires a commit or branch ref"))
    }
}

#[derive(Debug, Deserialize)]
struct SourceArchiveManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    owner: String,
    repo: String,
    #[serde(rename = "archiveRef")]
    archive_ref: String,
    commit: Option<String>,
    branch: Option<String>,
    files: Vec<SourceArchiveManifestFile>,
}

#[derive(Debug, Deserialize)]
struct SourceArchiveManifestFile {
    name: String,
    url: String,
    #[serde(rename = "sizeBytes")]
    size_bytes: u64,
    sha256: String,
}

/// Resolve a manifest-owned bundled source archive for `spec`.
pub fn bundled_archive_for_spec(
    bundled_source_dir: Option<&Path>,
    spec: &RepoArchiveSpec,
) -> Option<ResolvedRepoArchive> {
    let bundled_source_dir = bundled_source_dir?;
    let manifest_path = bundled_source_dir.join(SOURCE_ARCHIVE_MANIFEST);
    let manifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| serde_json::from_str::<SourceArchiveManifest>(&text).ok())?;
    if manifest.schema_version != SOURCE_ARCHIVE_MANIFEST_SCHEMA_VERSION {
        return None;
    }
    if manifest.owner != spec.owner || manifest.repo != spec.repo {
        return None;
    }
    if manifest.archive_ref != spec.archive_ref().ok()? {
        return None;
    }
    if spec.commit.is_some() && manifest.commit != spec.commit {
        return None;
    }
    if spec.commit.is_none() && manifest.branch != spec.branch {
        return None;
    }

    manifest.files.into_iter().find_map(|file| {
        if !source_archive_name_is_plain_zip(&file.name) {
            return None;
        }
        if !file.url.starts_with("https://") {
            return None;
        }
        if file.sha256.len() != 64 || !file.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        let path = bundled_source_dir.join(&file.name);
        let Ok(bytes) = std::fs::read(&path) else {
            return None;
        };
        if file.size_bytes != bytes.len() as u64 {
            return None;
        }
        if !crate::artifact::sha256_hex(&bytes).eq_ignore_ascii_case(&file.sha256) {
            return None;
        }
        Some(ResolvedRepoArchive {
            path,
            source: RepoArchiveSourceKind::Bundled,
        })
    })
}

fn source_archive_name_is_plain_zip(name: &str) -> bool {
    !name.trim().is_empty()
        && name == name.trim()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && name.ends_with(".zip")
}

/// Run a local no-network archive/update/repair/uninstall lifecycle smoke.
pub fn archive_lifecycle_self_check() -> Result<serde_json::Value> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "hermes-repo-archive-self-check-{}-{stamp}",
        std::process::id()
    ));
    let result = archive_lifecycle_self_check_in(&root);
    let cleanup_result = std::fs::remove_dir_all(&root);
    let report = result?;
    if let Err(err) = cleanup_result {
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err).with_context(|| format!("cleaning self-check root {}", root.display()));
        }
    }
    Ok(report)
}

fn archive_lifecycle_self_check_in(root: &Path) -> Result<serde_json::Value> {
    let hermes_home = root.join("home");
    let install_root = hermes_home.join("hermes-agent");
    let archive = root.join("fresh.zip");
    let update_archive = root.join("update.zip");
    std::fs::create_dir_all(root)
        .with_context(|| format!("creating self-check root {}", root.display()))?;
    write_local_zip(
        &archive,
        &[
            ("hermes-agent-main/README.md", b"fresh"),
            ("hermes-agent-main/scripts/install.sh", b"#!/bin/sh\n"),
        ],
    )?;
    write_local_zip(
        &update_archive,
        &[
            ("hermes-agent-main/README.md", b"updated"),
            ("hermes-agent-main/scripts/install.sh", b"#!/bin/sh\necho update\n"),
        ],
    )?;

    extract_repo_archive_to_install_root(&archive, &install_root)?;
    let spec = RepoArchiveSpec {
        owner: "NousResearch".into(),
        repo: "hermes-agent".into(),
        commit: None,
        branch: Some("main".into()),
    };
    write_archive_source_marker(&install_root, &spec, &archive, false)?;
    let source_marker_before = std::fs::read(install_root.join(SOURCE_MARKER_NAME))
        .context("reading source marker before refresh")?;
    seed_managed_runtime_state(&hermes_home)?;
    std::fs::write(hermes_home.join("config.yaml"), b"model: test\n")
        .context("writing preserved config")?;
    hermes_manager::commands::install_metadata(&hermes_home).context("recording install metadata")?;

    refresh_existing_checkout_from_archive(&update_archive, &install_root)?;
    let readme = std::fs::read(install_root.join("README.md")).context("reading refreshed README")?;
    if readme != b"updated" {
        return Err(anyhow!("archive refresh did not update repository contents"));
    }
    let source_marker_after = std::fs::read(install_root.join(SOURCE_MARKER_NAME))
        .context("reading source marker after refresh")?;
    if source_marker_after != source_marker_before {
        return Err(anyhow!("archive refresh changed the source marker"));
    }
    if !hermes_home.join("config.yaml").exists() {
        return Err(anyhow!("archive refresh removed user config"));
    }

    let repaired = hermes_manager::commands::repair_clean(&hermes_home).context("running repair-clean")?;
    if !repaired.iter().any(|path| path.ends_with("hermes-agent")) {
        return Err(anyhow!("repair-clean did not remove the checkout"));
    }
    if !repaired.iter().any(|path| path.ends_with("bootstrap-cache")) {
        return Err(anyhow!("repair-clean did not remove bootstrap-cache"));
    }
    if install_root.exists() {
        return Err(anyhow!("repair-clean left the checkout behind"));
    }
    if !hermes_home.join("config.yaml").exists() {
        return Err(anyhow!("repair-clean removed user config"));
    }

    seed_managed_runtime_state(&hermes_home)?;
    hermes_manager::commands::install_metadata(&hermes_home).context("recording install metadata after repair")?;
    let planned =
        hermes_manager::commands::uninstall_lite_plan(&hermes_home).context("planning lite uninstall")?;
    let removed = hermes_manager::commands::uninstall_lite(&hermes_home).context("running lite uninstall")?;
    if removed != planned {
        return Err(anyhow!("lite uninstall removed a different set than it planned"));
    }
    if !hermes_home.join("config.yaml").exists() {
        return Err(anyhow!("lite uninstall removed user config"));
    }
    if install_root.exists() || hermes_home.join("bootstrap-cache").exists() {
        return Err(anyhow!("lite uninstall left managed install state behind"));
    }

    Ok(serde_json::json!({
        "archiveLifecycle": "ok",
        "plannedRemovals": planned.len(),
        "repairRemovals": repaired.len(),
    }))
}

fn seed_managed_runtime_state(hermes_home: &Path) -> Result<()> {
    for runtime_root in hermes_manager::paths::managed_runtime_roots(hermes_home) {
        std::fs::create_dir_all(&runtime_root)
            .with_context(|| format!("creating managed runtime root {}", runtime_root.display()))?;
    }
    for runtime_file in hermes_manager::paths::managed_runtime_files(hermes_home) {
        if let Some(parent) = runtime_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating runtime file parent {}", parent.display()))?;
        }
        std::fs::write(&runtime_file, b"managed")
            .with_context(|| format!("writing managed runtime file {}", runtime_file.display()))?;
    }
    Ok(())
}

fn write_local_zip(path: &Path, entries: &[(&str, &[u8])]) -> Result<()> {
    let file = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default())
            .with_context(|| format!("adding zip entry {name}"))?;
        zip.write_all(bytes)
            .with_context(|| format!("writing zip entry {name}"))?;
    }
    zip.finish().context("finishing zip archive")?;
    Ok(())
}

/// Download a GitHub repository archive and extract it into a fresh install root.
pub async fn download_and_extract_fresh(
    spec: &RepoArchiveSpec,
    cache_dir: &Path,
    install_root: &Path,
) -> Result<PathBuf> {
    let archive_path = archive_cache_path(cache_dir, spec)?;
    crate::artifact::download_to_cache(
        crate::artifact::DownloadSpec {
            url: spec.github_zip_url()?,
            user_agent: "hermes-setup/0.0.1",
            expected_sha256: None,
        },
        &archive_path,
    )
    .await
    .context("downloading repository archive")?;
    extract_repo_archive_to_install_root(&archive_path, install_root)?;
    Ok(archive_path)
}

/// Extract a repository archive into a fresh install root, preferring a bundled archive.
pub async fn extract_fresh_preferring_bundled(
    spec: &RepoArchiveSpec,
    cache_dir: &Path,
    install_root: &Path,
    bundled_source_dir: Option<&Path>,
) -> Result<ResolvedRepoArchive> {
    if let Some(resolved) = bundled_archive_for_spec(bundled_source_dir, spec) {
        extract_repo_archive_to_install_root(&resolved.path, install_root)?;
        return Ok(resolved);
    }

    let archive_path = download_and_extract_fresh(spec, cache_dir, install_root).await?;
    Ok(ResolvedRepoArchive {
        path: archive_path,
        source: RepoArchiveSourceKind::Download,
    })
}

/// Write the install source marker for an archive-created checkout.
pub fn write_archive_source_marker(
    install_root: &Path,
    spec: &RepoArchiveSpec,
    archive_path: &Path,
    git_initialized: bool,
) -> Result<serde_json::Value> {
    let marker = serde_json::json!({
        "schemaVersion": 1,
        "method": "github_archive",
        "owner": spec.owner,
        "repo": spec.repo,
        "ref": spec.archive_ref()?,
        "commit": spec.commit,
        "branch": spec.branch,
        "archive": archive_path,
        "gitInitialized": git_initialized,
    });
    let text = serde_json::to_string_pretty(&marker)
        .context("serializing repository source marker")?
        + "\n";
    let marker_path = install_root.join(SOURCE_MARKER_NAME);
    std::fs::write(&marker_path, text)
        .with_context(|| format!("writing repository source marker {}", marker_path.display()))?;
    Ok(marker)
}

/// Read the archive source marker when present.
pub fn read_archive_source_marker(install_root: &Path) -> Result<Option<serde_json::Value>> {
    let marker_path = install_root.join(SOURCE_MARKER_NAME);
    let text = match std::fs::read_to_string(&marker_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("reading repository source marker {}", marker_path.display()));
        }
    };
    serde_json::from_str(&text)
        .with_context(|| format!("parsing repository source marker {}", marker_path.display()))
        .map(Some)
}

/// Return the cache path for a repository ZIP archive.
pub fn archive_cache_path(cache_dir: &Path, spec: &RepoArchiveSpec) -> Result<PathBuf> {
    let archive_ref = sanitize_ref(spec.archive_ref()?);
    Ok(cache_dir.join(format!("{}-{}.zip", spec.repo, archive_ref)))
}

/// Extract a GitHub archive ZIP into `install_root`, stripping its single root dir.
///
/// The function intentionally refuses to write into an existing install root.
/// This keeps the first no-Git path safe while update semantics remain owned by
/// the existing Git-based scripts.
pub fn extract_repo_archive_to_install_root(archive_path: &Path, install_root: &Path) -> Result<()> {
    if install_root.exists() {
        return Err(anyhow!(
            "install root already exists: {}",
            install_root.display()
        ));
    }

    let parent = install_root.parent().ok_or_else(|| {
        anyhow!(
            "install root has no parent directory: {}",
            install_root.display()
        )
    })?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating install parent {}", parent.display()))?;

    let tmp_dir = archive_tmp_dir(install_root);
    remove_dir_if_exists(&tmp_dir)?;
    std::fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating archive temp dir {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let archive_root = single_top_level_dir(&tmp_dir)?;
        std::fs::rename(&archive_root, install_root).with_context(|| {
            format!(
                "moving extracted repo {} to {}",
                archive_root.display(),
                install_root.display()
            )
        })?;
        Ok(())
    })();

    let cleanup = remove_dir_if_exists(&tmp_dir);
    result?;
    cleanup
}

/// Refresh an existing archive-created checkout from a GitHub repository ZIP.
///
/// This mirrors the legacy Python ZIP update contract: replace repository
/// files from the archive, but preserve runtime/user state that does not belong
/// to the source archive.
pub fn refresh_existing_checkout_from_archive(archive_path: &Path, install_root: &Path) -> Result<()> {
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }

    let tmp_dir = archive_tmp_dir(install_root);
    remove_dir_if_exists(&tmp_dir)?;
    std::fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating archive temp dir {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let archive_root = single_top_level_dir(&tmp_dir)?;
        for entry in std::fs::read_dir(&archive_root)
            .with_context(|| format!("reading archive root {}", archive_root.display()))?
        {
            let entry = entry.with_context(|| format!("reading entry under {}", archive_root.display()))?;
            let name = entry.file_name();
            if should_preserve_refresh_entry(&name) {
                continue;
            }
            let source = entry.path();
            let dest = install_root.join(&name);
            remove_path_if_exists(&dest)?;
            std::fs::rename(&source, &dest).with_context(|| {
                format!("moving refreshed repo entry {} to {}", source.display(), dest.display())
            })?;
        }
        Ok(())
    })();

    let cleanup = remove_dir_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn should_preserve_refresh_entry(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("venv" | "node_modules" | ".git" | ".env" | SOURCE_MARKER_NAME)
    )
}

fn archive_tmp_dir(install_root: &Path) -> PathBuf {
    let name = install_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("hermes-agent");
    install_root.with_file_name(format!("{name}.archive-tmp-{}", std::process::id()))
}

fn single_top_level_dir(tmp_dir: &Path) -> Result<PathBuf> {
    let entries = std::fs::read_dir(tmp_dir)
        .with_context(|| format!("reading archive temp dir {}", tmp_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading entries under {}", tmp_dir.display()))?;
    if entries.len() != 1 {
        return Err(anyhow!(
            "repo archive must contain exactly one top-level directory, found {}",
            entries.len()
        ));
    }
    let path = entries[0].path();
    if !path.is_dir() {
        return Err(anyhow!(
            "repo archive top-level entry is not a directory: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        write_local_zip(path, entries).unwrap();
    }

    #[test]
    fn archive_url_prefers_commit_over_branch() {
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: Some("02d26981d3d4ad50e142399b8476f59ad5953ff0".into()),
            branch: Some("main".into()),
        };

        assert_eq!(
            spec.github_zip_url().unwrap(),
            "https://github.com/NousResearch/hermes-agent/archive/02d26981d3d4ad50e142399b8476f59ad5953ff0.zip"
        );
    }

    #[test]
    fn archive_extract_strips_single_top_level_directory() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-strip-{}",
            std::process::id()
        ));
        let archive = root.join("repo.zip");
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(&root).unwrap();
        write_test_zip(
            &archive,
            &[
                ("hermes-agent-main/README.md", b"ok"),
                ("hermes-agent-main/scripts/install.ps1", b"# install"),
            ],
        );

        extract_repo_archive_to_install_root(&archive, &install_root).unwrap();

        assert_eq!(std::fs::read(install_root.join("README.md")).unwrap(), b"ok");
        assert!(install_root.join("scripts").join("install.ps1").exists());
        assert!(!install_root.join("hermes-agent-main").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_extract_refuses_to_overwrite_existing_install_root() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-existing-{}",
            std::process::id()
        ));
        let archive = root.join("repo.zip");
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(&install_root).unwrap();
        std::fs::write(install_root.join("local.txt"), b"user").unwrap();
        write_test_zip(&archive, &[("hermes-agent-main/README.md", b"ok")]);

        let err = extract_repo_archive_to_install_root(&archive, &install_root).unwrap_err();

        assert!(err.to_string().contains("install root already exists"));
        assert_eq!(std::fs::read(install_root.join("local.txt")).unwrap(), b"user");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_cache_path_sanitizes_refs() {
        let cache_dir = std::path::PathBuf::from("C:/cache");
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: None,
            branch: Some("feature/native repo".into()),
        };

        assert_eq!(
            archive_cache_path(&cache_dir, &spec).unwrap(),
            cache_dir.join("hermes-agent-feature_native_repo.zip")
        );
    }

    #[test]
    fn bundled_archive_for_spec_accepts_manifest_verified_archive() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-bundled-{}",
            std::process::id()
        ));
        let bundled = root.join("source-archive");
        let archive_name = "hermes-agent-abcdef123.zip";
        let archive = bundled.join(archive_name);
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: Some("abcdef123".into()),
            branch: Some("main".into()),
        };
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(&archive, &[("hermes-agent-abcdef123/README.md", b"ok")]);
        let bytes = std::fs::read(&archive).unwrap();
        std::fs::write(
            bundled.join("source-archive-manifest.json"),
            format!(
                r#"{{
                    "schemaVersion": 1,
                    "owner": "NousResearch",
                    "repo": "hermes-agent",
                    "archiveRef": "abcdef123",
                    "commit": "abcdef123",
                    "branch": "main",
                    "files": [
                        {{
                            "name": "{}",
                            "url": "https://example.invalid/{}",
                            "sizeBytes": {},
                            "sha256": "{}"
                        }}
                    ]
                }}"#,
                archive_name,
                archive_name,
                bytes.len(),
                crate::artifact::sha256_hex(&bytes)
            ),
        )
        .unwrap();

        let resolved = bundled_archive_for_spec(Some(&bundled), &spec).unwrap();

        assert_eq!(resolved.path, archive);
        assert_eq!(resolved.source, RepoArchiveSourceKind::Bundled);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_archive_for_spec_rejects_mismatched_manifest() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-bundled-mismatch-{}",
            std::process::id()
        ));
        let bundled = root.join("source-archive");
        let archive_name = "hermes-agent-abcdef123.zip";
        let archive = bundled.join(archive_name);
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: Some("abcdef123".into()),
            branch: Some("main".into()),
        };
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(&archive, &[("hermes-agent-abcdef123/README.md", b"ok")]);
        std::fs::write(
            bundled.join("source-archive-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "owner": "NousResearch",
                "repo": "hermes-agent",
                "archiveRef": "main",
                "branch": "main",
                "files": [
                    {
                        "name": "hermes-agent-abcdef123.zip",
                        "url": "https://example.invalid/hermes-agent-abcdef123.zip",
                        "sizeBytes": 1,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert!(bundled_archive_for_spec(Some(&bundled), &spec).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn extract_fresh_preferring_bundled_uses_manifest_verified_archive() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-bundled-extract-{}",
            std::process::id()
        ));
        let bundled = root.join("source-archive");
        let install_root = root.join("install");
        let archive_name = "hermes-agent-abcdef123.zip";
        let archive = bundled.join(archive_name);
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: Some("abcdef123".into()),
            branch: Some("main".into()),
        };
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(
            &archive,
            &[("hermes-agent-abcdef123/README.md", b"bundled source")],
        );
        let bytes = std::fs::read(&archive).unwrap();
        std::fs::write(
            bundled.join("source-archive-manifest.json"),
            format!(
                r#"{{
                    "schemaVersion": 1,
                    "owner": "NousResearch",
                    "repo": "hermes-agent",
                    "archiveRef": "abcdef123",
                    "commit": "abcdef123",
                    "branch": "main",
                    "files": [
                        {{
                            "name": "{}",
                            "url": "https://example.invalid/{}",
                            "sizeBytes": {},
                            "sha256": "{}"
                        }}
                    ]
                }}"#,
                archive_name,
                archive_name,
                bytes.len(),
                crate::artifact::sha256_hex(&bytes)
            ),
        )
        .unwrap();

        let resolved = extract_fresh_preferring_bundled(
            &spec,
            &root.join("cache"),
            &install_root,
            Some(&bundled),
        )
        .await
        .unwrap();

        assert_eq!(resolved.source, RepoArchiveSourceKind::Bundled);
        assert_eq!(resolved.path, archive);
        assert_eq!(
            std::fs::read_to_string(install_root.join("README.md")).unwrap(),
            "bundled source"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_source_marker_round_trips_install_source() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-source-{}",
            std::process::id()
        ));
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(&install_root).unwrap();
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: Some("abcdef123".into()),
            branch: Some("main".into()),
        };

        let marker = write_archive_source_marker(
            &install_root,
            &spec,
            &root.join("hermes-agent-abcdef123.zip"),
            false,
        )
        .unwrap();
        let read_back = read_archive_source_marker(&install_root)
            .unwrap()
            .expect("marker should exist");

        assert_eq!(marker["schemaVersion"], 1);
        assert_eq!(read_back["method"], "github_archive");
        assert_eq!(read_back["ref"], "abcdef123");
        assert_eq!(read_back["branch"], "main");
        assert_eq!(read_back["gitInitialized"], false);
        let bytes = std::fs::read(install_root.join(SOURCE_MARKER_NAME)).unwrap();
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_refresh_replaces_repo_files_and_preserves_runtime_state() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-refresh-{}",
            std::process::id()
        ));
        let archive = root.join("repo.zip");
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(install_root.join("scripts")).unwrap();
        std::fs::create_dir_all(install_root.join("venv")).unwrap();
        std::fs::create_dir_all(install_root.join("node_modules")).unwrap();
        std::fs::write(install_root.join("README.md"), b"old").unwrap();
        std::fs::write(install_root.join("scripts").join("install.ps1"), b"old script").unwrap();
        std::fs::write(install_root.join("venv").join("pyvenv.cfg"), b"keep venv").unwrap();
        std::fs::write(install_root.join("node_modules").join("cache.txt"), b"keep node").unwrap();
        std::fs::write(install_root.join(".env"), b"keep env").unwrap();
        std::fs::write(install_root.join(SOURCE_MARKER_NAME), b"keep marker").unwrap();
        write_test_zip(
            &archive,
            &[
                ("hermes-agent-main/README.md", b"new"),
                ("hermes-agent-main/scripts/install.ps1", b"new script"),
                ("hermes-agent-main/venv/pyvenv.cfg", b"archive venv"),
            ],
        );

        refresh_existing_checkout_from_archive(&archive, &install_root).unwrap();

        assert_eq!(std::fs::read(install_root.join("README.md")).unwrap(), b"new");
        assert_eq!(
            std::fs::read(install_root.join("scripts").join("install.ps1")).unwrap(),
            b"new script"
        );
        assert_eq!(
            std::fs::read(install_root.join("venv").join("pyvenv.cfg")).unwrap(),
            b"keep venv"
        );
        assert_eq!(
            std::fs::read(install_root.join("node_modules").join("cache.txt")).unwrap(),
            b"keep node"
        );
        assert_eq!(std::fs::read(install_root.join(".env")).unwrap(), b"keep env");
        assert_eq!(
            std::fs::read(install_root.join(SOURCE_MARKER_NAME)).unwrap(),
            b"keep marker"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_lifecycle_smoke_covers_update_repair_and_lite_uninstall() {
        let root = std::env::temp_dir().join(format!(
            "hermes-repo-archive-lifecycle-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let install_root = hermes_home.join("hermes-agent");
        let archive = root.join("fresh.zip");
        let update_archive = root.join("update.zip");
        std::fs::create_dir_all(&root).unwrap();
        write_test_zip(
            &archive,
            &[
                ("hermes-agent-main/README.md", b"fresh"),
                ("hermes-agent-main/scripts/install.sh", b"#!/bin/sh\n"),
            ],
        );
        write_test_zip(
            &update_archive,
            &[
                ("hermes-agent-main/README.md", b"updated"),
                ("hermes-agent-main/scripts/install.sh", b"#!/bin/sh\necho update\n"),
            ],
        );

        extract_repo_archive_to_install_root(&archive, &install_root).unwrap();
        let spec = RepoArchiveSpec {
            owner: "NousResearch".into(),
            repo: "hermes-agent".into(),
            commit: None,
            branch: Some("main".into()),
        };
        write_archive_source_marker(&install_root, &spec, &archive, false).unwrap();
        let source_marker_before = std::fs::read(install_root.join(SOURCE_MARKER_NAME)).unwrap();
        for runtime_root in hermes_manager::paths::managed_runtime_roots(&hermes_home) {
            std::fs::create_dir_all(&runtime_root).unwrap();
        }
        for runtime_file in hermes_manager::paths::managed_runtime_files(&hermes_home) {
            std::fs::write(runtime_file, b"managed").unwrap();
        }
        std::fs::write(hermes_home.join("config.yaml"), b"model: test\n").unwrap();
        hermes_manager::commands::install_metadata(&hermes_home).unwrap();

        refresh_existing_checkout_from_archive(&update_archive, &install_root).unwrap();

        assert_eq!(std::fs::read(install_root.join("README.md")).unwrap(), b"updated");
        assert_eq!(
            std::fs::read(install_root.join(SOURCE_MARKER_NAME)).unwrap(),
            source_marker_before
        );
        assert!(hermes_home.join("config.yaml").exists());

        let repaired = hermes_manager::commands::repair_clean(&hermes_home).unwrap();
        assert!(repaired
            .iter()
            .any(|path| path.ends_with("hermes-agent")));
        assert!(repaired
            .iter()
            .any(|path| path.ends_with("bootstrap-cache")));
        assert!(!install_root.exists());
        assert!(hermes_home.join("config.yaml").exists());

        for runtime_root in hermes_manager::paths::managed_runtime_roots(&hermes_home) {
            std::fs::create_dir_all(&runtime_root).unwrap();
        }
        for runtime_file in hermes_manager::paths::managed_runtime_files(&hermes_home) {
            std::fs::write(runtime_file, b"managed").unwrap();
        }
        hermes_manager::commands::install_metadata(&hermes_home).unwrap();
        let planned = hermes_manager::commands::uninstall_lite_plan(&hermes_home).unwrap();
        let removed = hermes_manager::commands::uninstall_lite(&hermes_home).unwrap();

        assert_eq!(removed, planned);
        assert!(hermes_home.join("config.yaml").exists());
        assert!(!install_root.exists());
        assert!(!hermes_home.join("bootstrap-cache").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn archive_lifecycle_self_check_returns_report() {
        let report = archive_lifecycle_self_check().unwrap();
        assert_eq!(report["archiveLifecycle"], "ok");
        assert!(report["plannedRemovals"].as_u64().unwrap() > 0);
        assert!(report["repairRemovals"].as_u64().unwrap() > 0);
    }
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        return remove_dir_if_exists(path);
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
    }
}

fn sanitize_ref(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
