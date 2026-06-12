//! Command implementations for the Hermes install manager.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::installed_manifest::{InstalledKind, InstalledManifest};
use crate::ownership::ensure_safe_to_delete;
use crate::paths;
use crate::{ManagerError, Result};

/// Print status information for diagnostics.
pub fn doctor(hermes_home: &Path) -> Vec<String> {
    vec![
        format!("hermes_home={}", hermes_home.display()),
        format!("agent_root={}", paths::agent_root(hermes_home).display()),
        format!(
            "installed_manifest={}",
            paths::installed_manifest_path(hermes_home).display()
        ),
    ]
}

/// Create manager state and an initial installed-files manifest if missing.
pub fn install_metadata(hermes_home: &Path) -> Result<()> {
    let manifest_path = paths::installed_manifest_path(hermes_home);
    if manifest_path.exists() {
        return Ok(());
    }
    let mut manifest = InstalledManifest::new(hermes_home.to_path_buf());
    for runtime_root in paths::managed_runtime_roots(hermes_home) {
        if runtime_root == paths::agent_root(hermes_home) || runtime_root.exists() {
            manifest.add_entry(runtime_root, InstalledKind::Directory);
        }
    }
    for runtime_file in paths::managed_runtime_files(hermes_home) {
        if runtime_file.exists() {
            manifest.add_entry(runtime_file, InstalledKind::File);
        }
    }
    manifest.write_atomic(&manifest_path)
}

/// Remove managed runtime paths while preserving user data.
pub fn uninstall_lite(hermes_home: &Path) -> Result<Vec<String>> {
    let manifest_path = paths::installed_manifest_path(hermes_home);
    let manifest = read_installed_manifest_or_default(hermes_home, &manifest_path)?;
    validate_manifest_home(hermes_home, &manifest)?;
    preflight_uninstall_lite_entries(hermes_home, &manifest)?;

    let mut removed = remove_managed_windows_path_entries(hermes_home)?;
    removed.extend(remove_managed_windows_env_vars(hermes_home)?);
    removed.extend(remove_managed_profile_updates(hermes_home)?);
    removed.extend(remove_managed_command_links(hermes_home)?);

    for entry in manifest.entries.iter().rev() {
        if !entry.path.exists() {
            continue;
        }
        match entry.kind {
            InstalledKind::File => {
                fs::remove_file(&entry.path).map_err(|err| ManagerError::io(&entry.path, err))?;
            }
            InstalledKind::Directory => {
                fs::remove_dir_all(&entry.path)
                    .map_err(|err| ManagerError::io(&entry.path, err))?;
            }
        }
        removed.push(entry.path.display().to_string());
    }

    Ok(removed)
}

/// Report paths that lite uninstall would remove without deleting them.
pub fn uninstall_lite_plan(hermes_home: &Path) -> Result<Vec<String>> {
    let manifest_path = paths::installed_manifest_path(hermes_home);
    let manifest = read_installed_manifest_or_default(hermes_home, &manifest_path)?;
    validate_manifest_home(hermes_home, &manifest)?;
    preflight_uninstall_lite_entries(hermes_home, &manifest)?;

    let mut planned = managed_windows_path_entry_plan(hermes_home)?;
    planned.extend(managed_windows_env_var_plan(hermes_home)?);
    planned.extend(
        managed_profile_update_plan(hermes_home)?
            .into_iter()
            .map(|path| path.display().to_string()),
    );
    planned.extend(
        managed_command_link_plan(hermes_home)?
            .into_iter()
            .map(|path| path.display().to_string()),
    );
    for entry in manifest.entries.iter().rev() {
        if !entry.path.exists() {
            continue;
        }
        planned.push(entry.path.display().to_string());
    }

    Ok(planned)
}

/// Remove source-built desktop GUI artifacts while preserving the agent.
pub fn uninstall_gui_build(hermes_home: &Path) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for root in paths::source_gui_build_roots(hermes_home) {
        ensure_safe_to_delete(hermes_home, &root)?;
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|err| ManagerError::io(&root, err))?;
            removed.push(root.display().to_string());
        }
    }
    for file in paths::source_gui_build_files(hermes_home) {
        ensure_safe_to_delete(hermes_home, &file)?;
        if file.exists() {
            fs::remove_file(&file).map_err(|err| ManagerError::io(&file, err))?;
            removed.push(file.display().to_string());
        }
    }
    Ok(removed)
}

/// Report source-built desktop GUI artifacts that would be removed.
pub fn uninstall_gui_build_plan(hermes_home: &Path) -> Result<Vec<String>> {
    let mut planned = Vec::new();
    for root in paths::source_gui_build_roots(hermes_home) {
        ensure_safe_to_delete(hermes_home, &root)?;
        if root.exists() {
            planned.push(root.display().to_string());
        }
    }
    for file in paths::source_gui_build_files(hermes_home) {
        ensure_safe_to_delete(hermes_home, &file)?;
        if file.exists() {
            planned.push(file.display().to_string());
        }
    }
    Ok(planned)
}

/// Remove source-built desktop GUI artifacts and Electron userData.
pub fn uninstall_gui_build_with_user_data(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_with_options_paths(hermes_home, paths::desktop_user_data_dir(), Vec::new())
}

/// Report source-built desktop GUI artifacts and Electron userData that would be removed.
pub fn uninstall_gui_build_plan_with_user_data(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_plan_with_options_paths(
        hermes_home,
        paths::desktop_user_data_dir(),
        Vec::new(),
    )
}

/// Remove source-built desktop GUI artifacts, userData, and Linux desktop entries.
pub fn uninstall_gui_build_with_gui_state(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_with_options_paths(
        hermes_home,
        paths::desktop_user_data_dir(),
        paths::linux_desktop_entry_files(),
    )
}

/// Report source-built desktop GUI artifacts, userData, and Linux desktop entries.
pub fn uninstall_gui_build_plan_with_gui_state(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_plan_with_options_paths(
        hermes_home,
        paths::desktop_user_data_dir(),
        paths::linux_desktop_entry_files(),
    )
}

/// Remove source-built desktop GUI artifacts and Linux desktop entries.
pub fn uninstall_gui_build_with_desktop_entries(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_with_options_paths(hermes_home, None, paths::linux_desktop_entry_files())
}

/// Report source-built desktop GUI artifacts and Linux desktop entries.
pub fn uninstall_gui_build_plan_with_desktop_entries(hermes_home: &Path) -> Result<Vec<String>> {
    uninstall_gui_build_plan_with_options_paths(
        hermes_home,
        None,
        paths::linux_desktop_entry_files(),
    )
}

fn uninstall_gui_build_with_options_paths(
    hermes_home: &Path,
    user_data_dir: Option<PathBuf>,
    desktop_entry_files: Vec<PathBuf>,
) -> Result<Vec<String>> {
    let mut removed = uninstall_gui_build(hermes_home)?;
    if let Some(user_data_dir) = user_data_dir {
        ensure_desktop_user_data_dir_allowed(&user_data_dir)?;
        if user_data_dir.exists() {
            fs::remove_dir_all(&user_data_dir)
                .map_err(|err| ManagerError::io(&user_data_dir, err))?;
            removed.push(user_data_dir.display().to_string());
        }
    }
    for entry in desktop_entry_files {
        ensure_linux_desktop_entry_file_allowed(&entry)?;
        if entry.exists() {
            fs::remove_file(&entry).map_err(|err| ManagerError::io(&entry, err))?;
            removed.push(entry.display().to_string());
        }
    }
    Ok(removed)
}

fn uninstall_gui_build_plan_with_options_paths(
    hermes_home: &Path,
    user_data_dir: Option<PathBuf>,
    desktop_entry_files: Vec<PathBuf>,
) -> Result<Vec<String>> {
    let mut planned = uninstall_gui_build_plan(hermes_home)?;
    if let Some(user_data_dir) = user_data_dir {
        ensure_desktop_user_data_dir_allowed(&user_data_dir)?;
        if user_data_dir.exists() {
            planned.push(user_data_dir.display().to_string());
        }
    }
    for entry in desktop_entry_files {
        ensure_linux_desktop_entry_file_allowed(&entry)?;
        if entry.exists() {
            planned.push(entry.display().to_string());
        }
    }
    Ok(planned)
}

fn ensure_desktop_user_data_dir_allowed(path: &Path) -> Result<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some("Hermes") {
        return Ok(());
    }

    Err(ManagerError::InvalidManifest(format!(
        "desktop userData cleanup path is not a Hermes userData directory: {}",
        path.display()
    )))
}

fn ensure_linux_desktop_entry_file_allowed(path: &Path) -> Result<()> {
    let allowed_name = matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("hermes.desktop" | "Hermes.desktop")
    );
    let under_applications = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        == Some("applications");
    if allowed_name && under_applications {
        return Ok(());
    }

    Err(ManagerError::InvalidManifest(format!(
        "desktop entry cleanup path is not a Hermes launcher entry: {}",
        path.display()
    )))
}

fn read_installed_manifest_or_default(
    hermes_home: &Path,
    manifest_path: &Path,
) -> Result<InstalledManifest> {
    match InstalledManifest::read(manifest_path) {
        Ok(manifest) => Ok(manifest),
        Err(ManagerError::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {
            let mut manifest = InstalledManifest::new(hermes_home.to_path_buf());
            manifest.add_entry(paths::agent_root(hermes_home), InstalledKind::Directory);
            Ok(manifest)
        }
        Err(err) => Err(err),
    }
}

fn validate_manifest_home(hermes_home: &Path, manifest: &InstalledManifest) -> Result<()> {
    match (
        fs::canonicalize(hermes_home),
        fs::canonicalize(&manifest.hermes_home),
    ) {
        (Ok(active), Ok(recorded)) if active == recorded => Ok(()),
        (Ok(_), Ok(_)) => Err(manifest_home_mismatch_error(hermes_home, manifest)),
        _ if manifest.hermes_home == hermes_home => Ok(()),
        _ => Err(manifest_home_mismatch_error(hermes_home, manifest)),
    }
}

fn manifest_home_mismatch_error(hermes_home: &Path, manifest: &InstalledManifest) -> ManagerError {
    ManagerError::InvalidManifest(format!(
        "installed manifest hermes_home {} does not match active Hermes home {}",
        manifest.hermes_home.display(),
        hermes_home.display()
    ))
}

fn preflight_uninstall_lite_entries(
    hermes_home: &Path,
    manifest: &InstalledManifest,
) -> Result<()> {
    for entry in &manifest.entries {
        ensure_safe_to_delete(hermes_home, &entry.path)?;
        ensure_lite_uninstall_entry_allowed(hermes_home, &entry.path)?;
    }
    Ok(())
}

fn ensure_lite_uninstall_entry_allowed(hermes_home: &Path, candidate: &Path) -> Result<()> {
    if paths::managed_runtime_roots(hermes_home)
        .iter()
        .any(|root| crate::ownership::is_inside_root(root, candidate))
        || paths::managed_runtime_files(hermes_home)
            .iter()
            .any(|file| crate::ownership::is_inside_root(file, candidate))
    {
        return Ok(());
    }

    Err(ManagerError::InvalidManifest(format!(
        "installed manifest entry is not a lite-uninstall runtime path: {}",
        candidate.display()
    )))
}

/// Remove the runtime checkout and bootstrap marker so the next launch repairs it.
pub fn repair_clean(hermes_home: &Path) -> Result<Vec<String>> {
    let mut removed = remove_managed_command_links(hermes_home)?;
    for runtime_root in paths::managed_runtime_roots(hermes_home) {
        ensure_safe_to_delete(hermes_home, &runtime_root)?;
        if runtime_root.exists() {
            fs::remove_dir_all(&runtime_root)
                .map_err(|err| ManagerError::io(&runtime_root, err))?;
            removed.push(runtime_root.display().to_string());
        }
    }
    for runtime_file in paths::managed_runtime_files(hermes_home) {
        ensure_safe_to_delete(hermes_home, &runtime_file)?;
        if runtime_file.exists() {
            fs::remove_file(&runtime_file).map_err(|err| ManagerError::io(&runtime_file, err))?;
            removed.push(runtime_file.display().to_string());
        }
    }

    let marker = paths::agent_root(hermes_home).join(".hermes-bootstrap-complete");
    ensure_safe_to_delete(hermes_home, &marker)?;
    if !paths::agent_root(hermes_home).exists() && marker.exists() {
        fs::remove_file(&marker).map_err(|err| ManagerError::io(&marker, err))?;
        removed.push(marker.display().to_string());
    }

    Ok(removed)
}

/// Report runtime checkout paths that repair cleanup would remove.
pub fn repair_clean_plan(hermes_home: &Path) -> Result<Vec<String>> {
    let mut planned = managed_command_link_plan(hermes_home)?
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    for runtime_root in paths::managed_runtime_roots(hermes_home) {
        ensure_safe_to_delete(hermes_home, &runtime_root)?;
        if runtime_root.exists() {
            planned.push(runtime_root.display().to_string());
        }
    }
    for runtime_file in paths::managed_runtime_files(hermes_home) {
        ensure_safe_to_delete(hermes_home, &runtime_file)?;
        if runtime_file.exists() {
            planned.push(runtime_file.display().to_string());
        }
    }

    let marker = paths::agent_root(hermes_home).join(".hermes-bootstrap-complete");
    ensure_safe_to_delete(hermes_home, &marker)?;
    if !paths::agent_root(hermes_home).exists() && marker.exists() {
        planned.push(marker.display().to_string());
    }

    Ok(planned)
}

fn remove_managed_command_links(hermes_home: &Path) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for link in managed_command_link_plan(hermes_home)? {
        if fs::remove_file(&link).is_ok() {
            removed.push(link.display().to_string());
        }
    }
    Ok(removed)
}

fn remove_managed_windows_path_entries(hermes_home: &Path) -> Result<Vec<String>> {
    let current_path = crate::platform::read_windows_user_path()?;
    let plan = crate::platform::plan_windows_user_path_cleanup(current_path, hermes_home);
    if crate::platform::write_windows_user_path_cleanup(&plan)? {
        return Ok(plan.removed_entries);
    }
    Ok(Vec::new())
}

fn managed_windows_path_entry_plan(hermes_home: &Path) -> Result<Vec<String>> {
    let current_path = crate::platform::read_windows_user_path()?;
    Ok(crate::platform::plan_windows_user_path_cleanup(current_path, hermes_home).removed_entries)
}

fn remove_managed_windows_env_vars(hermes_home: &Path) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for name in managed_windows_env_var_names(hermes_home)? {
        if crate::platform::remove_windows_user_env_var(&name)? {
            removed.push(windows_env_var_display_name(&name));
        }
    }
    Ok(removed)
}

fn managed_windows_env_var_plan(hermes_home: &Path) -> Result<Vec<String>> {
    Ok(managed_windows_env_var_names(hermes_home)?
        .into_iter()
        .map(|name| windows_env_var_display_name(&name))
        .collect())
}

fn managed_windows_env_var_names(hermes_home: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for name in ["HERMES_HOME", "HERMES_GIT_BASH_PATH"] {
        let Some(value) = crate::platform::read_windows_user_env_var(name)? else {
            continue;
        };
        if crate::platform::windows_env_var_matches_hermes_home(name, &value, hermes_home) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

fn windows_env_var_display_name(name: &str) -> String {
    format!("HKCU\\Environment\\{name}")
}

fn remove_managed_profile_updates(hermes_home: &Path) -> Result<Vec<String>> {
    remove_managed_profile_updates_in_paths(hermes_home, shell_profile_candidate_paths())
}

fn remove_managed_profile_updates_in_paths(
    hermes_home: &Path,
    profile_paths: Vec<PathBuf>,
) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for profile in managed_profile_update_plan_in_paths(hermes_home, profile_paths)? {
        if crate::platform::remove_shell_profile_update(&profile).unwrap_or(false) {
            removed.push(profile.display().to_string());
        }
    }
    Ok(removed)
}

fn managed_profile_update_plan(hermes_home: &Path) -> Result<Vec<PathBuf>> {
    managed_profile_update_plan_in_paths(hermes_home, shell_profile_candidate_paths())
}

fn managed_profile_update_plan_in_paths(
    hermes_home: &Path,
    profile_paths: Vec<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let mut planned = Vec::new();
    for profile in profile_paths {
        if shell_profile_has_managed_update_for_home(&profile, hermes_home) {
            planned.push(profile);
        }
    }
    Ok(planned)
}

fn shell_profile_has_managed_update_for_home(profile_path: &Path, hermes_home: &Path) -> bool {
    let Ok(content) = fs::read_to_string(profile_path) else {
        return false;
    };
    content.contains(crate::platform::HERMES_PROFILE_BEGIN)
        && content.contains(crate::platform::HERMES_PROFILE_END)
        && content.contains(&hermes_home.display().to_string())
}

fn shell_profile_candidate_paths() -> Vec<PathBuf> {
    shell_profile_candidate_paths_from_home(std::env::var_os("HOME").map(PathBuf::from))
}

#[cfg(unix)]
fn shell_profile_candidate_paths_from_home(home: Option<PathBuf>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    vec![
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".profile"),
        home.join(".zshrc"),
        home.join(".zprofile"),
        home.join(".config").join("fish").join("config.fish"),
    ]
}

#[cfg(not(unix))]
fn shell_profile_candidate_paths_from_home(_home: Option<PathBuf>) -> Vec<PathBuf> {
    Vec::new()
}

fn managed_command_link_plan(hermes_home: &Path) -> Result<Vec<PathBuf>> {
    let mut links = managed_hermes_wrapper_plan()?;
    links.extend(managed_node_symlink_plan(hermes_home)?);
    Ok(links)
}

fn managed_hermes_wrapper_plan() -> Result<Vec<PathBuf>> {
    managed_hermes_wrapper_plan_in_dirs(node_symlink_candidate_dirs())
}

fn managed_hermes_wrapper_plan_in_dirs(candidate_dirs: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut wrappers = Vec::new();
    for dir in candidate_dirs {
        let wrapper = dir.join("hermes");
        let metadata = match fs::symlink_metadata(&wrapper) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(_) => continue,
        };
        if metadata.is_dir() {
            continue;
        }
        let content = match fs::read_to_string(&wrapper) {
            Ok(content) => content,
            Err(_) => continue,
        };
        if hermes_wrapper_content_is_managed(&content) {
            wrappers.push(wrapper);
        }
    }
    Ok(wrappers)
}

fn hermes_wrapper_content_is_managed(content: &str) -> bool {
    content.contains("hermes_cli") || content.contains("hermes-agent")
}

fn managed_node_symlink_plan(hermes_home: &Path) -> Result<Vec<PathBuf>> {
    managed_node_symlink_plan_in_dirs(hermes_home, node_symlink_candidate_dirs())
}

fn managed_node_symlink_plan_in_dirs(
    hermes_home: &Path,
    candidate_dirs: Vec<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let mut links = Vec::new();
    for dir in candidate_dirs {
        for name in ["node", "npm", "npx"] {
            let link = dir.join(name);
            let metadata = match fs::symlink_metadata(&link) {
                Ok(metadata) => metadata,
                Err(err) if err.kind() == ErrorKind::NotFound => continue,
                Err(_) => continue,
            };
            if !metadata.file_type().is_symlink() {
                continue;
            }
            let target = match fs::read_link(&link) {
                Ok(target) => target,
                Err(_) => continue,
            };
            if node_symlink_target_is_managed(&link, &target, hermes_home) {
                links.push(link);
            }
        }
    }
    Ok(links)
}

fn node_symlink_candidate_dirs() -> Vec<PathBuf> {
    node_symlink_candidate_dirs_from_env(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("PREFIX").map(PathBuf::from),
    )
}

#[cfg(unix)]
fn node_symlink_candidate_dirs_from_env(
    home: Option<PathBuf>,
    prefix: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(home) = home {
        push_unique_path(&mut dirs, home.join(".local").join("bin"));
    }
    if let Some(prefix) = prefix {
        if prefix.to_string_lossy().contains("com.termux") {
            push_unique_path(&mut dirs, prefix.join("bin"));
        }
    }
    #[cfg(target_os = "linux")]
    push_unique_path(&mut dirs, PathBuf::from("/usr/local/bin"));

    dirs
}

#[cfg(not(unix))]
fn node_symlink_candidate_dirs_from_env(
    _home: Option<PathBuf>,
    _prefix: Option<PathBuf>,
) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(unix)]
fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn node_symlink_target_is_managed(link_path: &Path, target: &Path, hermes_home: &Path) -> bool {
    let resolved = if target.is_absolute() {
        target.to_path_buf()
    } else {
        link_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(target)
    };
    normalize_path_lexically(&resolved)
        .starts_with(normalize_path_lexically(&hermes_home.join("node")))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(value) => normalized.push(value),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::installed_manifest::{InstalledKind, InstalledManifest};
    use crate::paths;

    #[test]
    fn install_metadata_creates_manifest() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        fs::create_dir_all(&hermes_home).expect("Hermes home should be created");

        super::install_metadata(&hermes_home).expect("install metadata should be created");

        let manifest_path = paths::installed_manifest_path(&hermes_home);
        let manifest = InstalledManifest::read(&manifest_path).expect("manifest should be read");
        assert_eq!(manifest.hermes_home, hermes_home);
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(
            manifest.entries[0].path,
            paths::agent_root(&manifest.hermes_home)
        );
        assert_eq!(manifest.entries[0].kind, InstalledKind::Directory);
    }

    #[test]
    fn install_metadata_records_existing_managed_runtime_dirs() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let bin_dir = hermes_home.join("bin");
        let uv_cache = hermes_home.join("uv-cache");
        let pip_cache = hermes_home.join("pip-cache");
        let node_dir = hermes_home.join("node");
        let python_dir = hermes_home.join("python");
        let git_dir = hermes_home.join("git");
        let gateway_service_dir = hermes_home.join("gateway-service");
        let bootstrap_cache = hermes_home.join("bootstrap-cache");
        let installer = paths::managed_runtime_files(&hermes_home)[0].clone();
        let desktop_stamp = paths::managed_runtime_files(&hermes_home)[1].clone();
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        fs::create_dir_all(&uv_cache).expect("uv cache should be created");
        fs::create_dir_all(&pip_cache).expect("pip cache should be created");
        fs::create_dir_all(&node_dir).expect("node dir should be created");
        fs::create_dir_all(&python_dir).expect("python dir should be created");
        fs::create_dir_all(&git_dir).expect("git dir should be created");
        fs::create_dir_all(&gateway_service_dir).expect("gateway-service dir should be created");
        fs::create_dir_all(&bootstrap_cache).expect("bootstrap cache should be created");
        fs::write(&installer, "setup").expect("installer should be created");
        fs::write(&desktop_stamp, "{}").expect("desktop stamp should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        super::install_metadata(&hermes_home).expect("install metadata should be created");

        let manifest = InstalledManifest::read(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be read");
        let paths = manifest
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                agent_root,
                bin_dir,
                uv_cache,
                pip_cache,
                node_dir,
                python_dir,
                git_dir,
                gateway_service_dir,
                bootstrap_cache,
                installer,
                desktop_stamp,
            ]
        );
        assert!(!paths.contains(&user_config));
    }

    #[test]
    fn uninstall_gui_build_plan_reports_existing_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let desktop_dir = agent_root.join("apps").join("desktop");
        let dist_dir = desktop_dir.join("dist");
        let release_dir = desktop_dir.join("release");
        let stamp = hermes_home.join("desktop-build-stamp.json");
        fs::create_dir_all(&dist_dir).expect("dist should be created");
        fs::create_dir_all(&release_dir).expect("release should be created");
        fs::write(&stamp, "{}").expect("desktop stamp should be created");

        let planned = super::uninstall_gui_build_plan(&hermes_home)
            .expect("GUI build cleanup should be planned");

        assert_eq!(
            planned,
            vec![
                dist_dir.display().to_string(),
                release_dir.display().to_string(),
                stamp.display().to_string(),
            ]
        );
    }

    #[test]
    fn uninstall_gui_build_removes_only_desktop_build_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let desktop_dir = agent_root.join("apps").join("desktop");
        let dist_dir = desktop_dir.join("dist");
        let release_dir = desktop_dir.join("release");
        let desktop_node_modules = desktop_dir.join("node_modules");
        let workspace_node_modules = agent_root.join("node_modules");
        let package_source = agent_root.join("hermes_cli").join("__init__.py");
        let venv_dir = agent_root.join("venv");
        let config = hermes_home.join("config.yaml");
        let sessions = hermes_home.join("sessions");
        let stamp = hermes_home.join("desktop-build-stamp.json");
        fs::create_dir_all(&dist_dir).expect("dist should be created");
        fs::create_dir_all(&release_dir).expect("release should be created");
        fs::create_dir_all(&desktop_node_modules).expect("desktop node_modules should be created");
        fs::create_dir_all(&workspace_node_modules)
            .expect("workspace node_modules should be created");
        fs::create_dir_all(package_source.parent().unwrap())
            .expect("package source should be created");
        fs::create_dir_all(&venv_dir).expect("venv should be created");
        fs::create_dir_all(&sessions).expect("sessions should be created");
        fs::write(&package_source, "").expect("package source should be written");
        fs::write(&config, "model: test").expect("config should be written");
        fs::write(&stamp, "{}").expect("desktop stamp should be created");

        let removed = super::uninstall_gui_build(&hermes_home)
            .expect("GUI build artifacts should be removed");

        assert!(removed.contains(&dist_dir.display().to_string()));
        assert!(removed.contains(&release_dir.display().to_string()));
        assert!(removed.contains(&desktop_node_modules.display().to_string()));
        assert!(removed.contains(&workspace_node_modules.display().to_string()));
        assert!(removed.contains(&stamp.display().to_string()));
        assert!(!dist_dir.exists());
        assert!(!release_dir.exists());
        assert!(!desktop_node_modules.exists());
        assert!(!workspace_node_modules.exists());
        assert!(!stamp.exists());
        assert!(desktop_dir.exists());
        assert!(package_source.exists());
        assert!(venv_dir.exists());
        assert!(config.exists());
        assert!(sessions.exists());
    }

    #[test]
    fn uninstall_gui_build_with_user_data_removes_desktop_userdata() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let user_data = dir.path().join("Hermes-userData").join("Hermes");
        let config = hermes_home.join("config.yaml");
        fs::create_dir_all(&user_data).expect("desktop userData should be created");
        fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
        fs::write(user_data.join("connection.json"), "{}")
            .expect("desktop connection state should be created");
        fs::write(&config, "model: test").expect("config should be written");

        let removed = super::uninstall_gui_build_with_options_paths(
            &hermes_home,
            Some(user_data.clone()),
            Vec::new(),
        )
        .expect("desktop userData should be removed");

        assert!(removed.contains(&user_data.display().to_string()));
        assert!(!user_data.exists());
        assert!(config.exists());
    }

    #[test]
    fn uninstall_gui_build_with_desktop_entries_removes_linux_launchers() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let entry = dir.path().join("applications").join("hermes.desktop");
        let config = hermes_home.join("config.yaml");
        fs::create_dir_all(entry.parent().unwrap()).expect("applications dir should be created");
        fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
        fs::write(&entry, "[Desktop Entry]\nName=Hermes\n")
            .expect("desktop entry should be written");
        fs::write(&config, "model: test").expect("config should be written");

        let removed =
            super::uninstall_gui_build_with_options_paths(&hermes_home, None, vec![entry.clone()])
                .expect("desktop entry should be removed");

        assert!(removed.contains(&entry.display().to_string()));
        assert!(!entry.exists());
        assert!(config.exists());
    }

    #[test]
    fn node_symlink_target_filter_only_accepts_hermes_node_targets() {
        let hermes_home = std::path::PathBuf::from("home/.hermes");
        let link_path = std::path::PathBuf::from("home/.local/bin/node");

        assert!(super::node_symlink_target_is_managed(
            &link_path,
            std::path::Path::new("../../.hermes/node/bin/node"),
            &hermes_home
        ));
        assert!(!super::node_symlink_target_is_managed(
            &link_path,
            std::path::Path::new("../../.hermes/python/bin/python"),
            &hermes_home
        ));
        assert!(!super::node_symlink_target_is_managed(
            &link_path,
            std::path::Path::new("../../.nvm/versions/node/v22/bin/node"),
            &hermes_home
        ));
    }

    #[cfg(not(unix))]
    #[test]
    fn node_symlink_candidate_dirs_are_unix_only() {
        let dirs = super::node_symlink_candidate_dirs_from_env(
            Some(std::path::PathBuf::from("C:/Users/tester")),
            Some(std::path::PathBuf::from(
                "C:/Users/tester/AppData/Local/Termux/com.termux/files/usr",
            )),
        );

        assert!(dirs.is_empty());
    }

    #[test]
    fn managed_node_symlink_plan_skips_invalid_candidate_dirs() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let file_as_dir = dir.path().join("not-a-directory");
        fs::write(&file_as_dir, "plain file").expect("file should be created");

        let planned = super::managed_node_symlink_plan_in_dirs(&hermes_home, vec![file_as_dir])
            .expect("invalid candidate dirs should not fail cleanup planning");

        assert!(planned.is_empty());
    }

    #[test]
    fn hermes_wrapper_plan_only_accepts_managed_scripts() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        let wrapper = bin_dir.join("hermes");
        fs::write(
            &wrapper,
            "#!/usr/bin/env bash\nexec /tmp/hermes-agent/venv/bin/hermes \"$@\"\n",
        )
        .expect("managed wrapper should be created");

        let planned = super::managed_hermes_wrapper_plan_in_dirs(vec![bin_dir.clone()])
            .expect("managed wrapper plan should be created");
        assert_eq!(planned, vec![wrapper.clone()]);

        fs::write(&wrapper, "#!/usr/bin/env bash\necho user hermes\n")
            .expect("user wrapper should be created");
        let planned = super::managed_hermes_wrapper_plan_in_dirs(vec![bin_dir])
            .expect("user wrapper plan should be created");
        assert!(planned.is_empty());
    }

    #[test]
    fn managed_profile_cleanup_removes_only_managed_blocks() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let managed = dir.path().join(".profile");
        let other_home = dir.path().join("other-hermes");
        let other = dir.path().join(".bashrc");
        let user = dir.path().join(".zshrc");
        fs::write(
            &managed,
            format!(
                "export EDITOR=vim\n\n{}\nexport PATH=\"{}:$PATH\"\n{}\n",
                crate::platform::HERMES_PROFILE_BEGIN,
                hermes_home.join("bin").display(),
                crate::platform::HERMES_PROFILE_END
            ),
        )
        .expect("managed profile should be created");
        fs::write(
            &other,
            format!(
                "{}\nexport PATH=\"{}:$PATH\"\n{}\n",
                crate::platform::HERMES_PROFILE_BEGIN,
                other_home.join("bin").display(),
                crate::platform::HERMES_PROFILE_END
            ),
        )
        .expect("other profile should be created");
        fs::write(&user, "export PATH=\"$HOME/bin:$PATH\"\n")
            .expect("user profile should be created");

        let removed = super::remove_managed_profile_updates_in_paths(
            &hermes_home,
            vec![managed.clone(), other.clone(), user.clone()],
        )
        .expect("managed profile updates should be removed");

        assert_eq!(removed, vec![managed.display().to_string()]);
        assert!(!fs::read_to_string(&managed)
            .expect("managed profile should be readable")
            .contains(crate::platform::HERMES_PROFILE_BEGIN));
        assert!(fs::read_to_string(&other)
            .expect("other profile should be readable")
            .contains(crate::platform::HERMES_PROFILE_BEGIN));
        assert_eq!(
            fs::read_to_string(&user).expect("user profile should be readable"),
            "export PATH=\"$HOME/bin:$PATH\"\n"
        );
    }

    #[test]
    fn uninstall_lite_removes_managed_runtime_dirs_outside_agent_root() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let bin_dir = hermes_home.join("bin");
        let node_dir = hermes_home.join("node");
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        fs::create_dir_all(&node_dir).expect("node dir should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let mut manifest = InstalledManifest::new(hermes_home.clone());
        manifest.add_entry(agent_root.clone(), InstalledKind::Directory);
        manifest.add_entry(bin_dir.clone(), InstalledKind::Directory);
        manifest.add_entry(node_dir.clone(), InstalledKind::Directory);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        let removed = super::uninstall_lite(&hermes_home).expect("runtime dirs should be removed");

        assert_eq!(
            removed,
            vec![
                node_dir.display().to_string(),
                bin_dir.display().to_string(),
                agent_root.display().to_string(),
            ]
        );
        assert!(!agent_root.exists());
        assert!(!bin_dir.exists());
        assert!(!node_dir.exists());
        assert!(user_config.exists());
    }

    #[test]
    fn uninstall_lite_removes_only_manifest_entries() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let managed_file = agent_root.join("managed.txt");
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::write(&managed_file, "managed").expect("managed file should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let mut manifest = InstalledManifest::new(hermes_home.clone());
        manifest.add_entry(managed_file.clone(), InstalledKind::File);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        let removed =
            super::uninstall_lite(&hermes_home).expect("manifest entries should be removed");

        assert_eq!(removed, vec![managed_file.display().to_string()]);
        assert!(!managed_file.exists());
        assert!(agent_root.exists());
        assert!(user_config.exists());
    }

    #[test]
    fn uninstall_lite_plan_reports_entries_without_removing_them() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let managed_file = agent_root.join("managed.txt");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::write(&managed_file, "managed").expect("managed file should be created");

        let mut manifest = InstalledManifest::new(hermes_home.clone());
        manifest.add_entry(managed_file.clone(), InstalledKind::File);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        let planned = super::uninstall_lite_plan(&hermes_home).expect("plan should be created");

        assert_eq!(planned, vec![managed_file.display().to_string()]);
        assert!(managed_file.exists());
    }

    #[test]
    fn repair_clean_plan_reports_runtime_paths_without_removing_them() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let marker = agent_root.join(".hermes-bootstrap-complete");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::write(&marker, "{}").expect("marker should be created");

        let planned = super::repair_clean_plan(&hermes_home).expect("plan should be created");

        assert_eq!(planned, vec![agent_root.display().to_string()]);
        assert!(agent_root.exists());
        assert!(marker.exists());
    }

    #[test]
    fn repair_clean_removes_managed_runtime_dirs_but_preserves_user_data() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let bin_dir = hermes_home.join("bin");
        let uv_cache = hermes_home.join("uv-cache");
        let pip_cache = hermes_home.join("pip-cache");
        let node_dir = hermes_home.join("node");
        let python_dir = hermes_home.join("python");
        let git_dir = hermes_home.join("git");
        let bootstrap_cache = hermes_home.join("bootstrap-cache");
        let installer = paths::managed_runtime_files(&hermes_home)[0].clone();
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        fs::create_dir_all(&uv_cache).expect("uv cache should be created");
        fs::create_dir_all(&pip_cache).expect("pip cache should be created");
        fs::create_dir_all(&node_dir).expect("node dir should be created");
        fs::create_dir_all(&python_dir).expect("python dir should be created");
        fs::create_dir_all(&git_dir).expect("git dir should be created");
        fs::create_dir_all(&bootstrap_cache).expect("bootstrap cache should be created");
        fs::write(&installer, "setup").expect("installer should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let planned = super::repair_clean_plan(&hermes_home).expect("plan should be created");
        assert_eq!(
            planned,
            vec![
                agent_root.display().to_string(),
                bin_dir.display().to_string(),
                uv_cache.display().to_string(),
                pip_cache.display().to_string(),
                node_dir.display().to_string(),
                python_dir.display().to_string(),
                git_dir.display().to_string(),
                bootstrap_cache.display().to_string(),
                installer.display().to_string(),
            ]
        );

        let removed = super::repair_clean(&hermes_home).expect("runtime dirs should be removed");

        assert_eq!(removed, planned);
        assert!(!agent_root.exists());
        assert!(!bin_dir.exists());
        assert!(!uv_cache.exists());
        assert!(!pip_cache.exists());
        assert!(!node_dir.exists());
        assert!(!python_dir.exists());
        assert!(!git_dir.exists());
        assert!(!bootstrap_cache.exists());
        assert!(!installer.exists());
        assert!(user_config.exists());
    }

    #[test]
    fn uninstall_lite_defaults_to_agent_root_when_manifest_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let managed_file = agent_root.join("managed.txt");
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::write(&managed_file, "managed").expect("managed file should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let removed = super::uninstall_lite(&hermes_home)
            .expect("missing manifest should fall back to agent root");

        assert_eq!(removed, vec![agent_root.display().to_string()]);
        assert!(!agent_root.exists());
        assert!(user_config.exists());
    }

    #[test]
    fn uninstall_lite_rejects_config_manifest_entry() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let mut manifest = InstalledManifest::new(hermes_home.clone());
        manifest.add_entry(user_config.clone(), InstalledKind::File);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        assert!(super::uninstall_lite(&hermes_home).is_err());
        assert!(user_config.exists());
    }

    #[test]
    fn uninstall_lite_rejects_manifest_home_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let other_home = dir.path().join("other-hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let managed_file = agent_root.join("managed.txt");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::create_dir_all(&other_home).expect("other home should be created");
        fs::write(&managed_file, "managed").expect("managed file should be created");

        let mut manifest = InstalledManifest::new(other_home);
        manifest.add_entry(managed_file.clone(), InstalledKind::File);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        assert!(super::uninstall_lite(&hermes_home).is_err());
        assert!(managed_file.exists());
    }

    #[test]
    fn uninstall_lite_preflight_rejects_all_when_any_entry_is_disallowed() {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let hermes_home = dir.path().join("hermes");
        let agent_root = paths::agent_root(&hermes_home);
        let managed_file = agent_root.join("managed.txt");
        let user_config = hermes_home.join("config.yaml");
        fs::create_dir_all(&agent_root).expect("agent root should be created");
        fs::write(&managed_file, "managed").expect("managed file should be created");
        fs::write(&user_config, "model: test").expect("user config should be created");

        let mut manifest = InstalledManifest::new(hermes_home.clone());
        manifest.add_entry(managed_file.clone(), InstalledKind::File);
        manifest.add_entry(user_config.clone(), InstalledKind::File);
        manifest
            .write_atomic(&paths::installed_manifest_path(&hermes_home))
            .expect("manifest should be written");

        assert!(super::uninstall_lite(&hermes_home).is_err());
        assert!(managed_file.exists());
        assert!(user_config.exists());
    }
}
