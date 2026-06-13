//! Smoke tests for the hermes-manager command-line binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manager_binary() -> PathBuf {
    manager_binary_from_env(std::env::var_os("HERMES_MANAGER_SMOKE_BIN"))
}

fn manager_binary_from_env(override_path: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(path) = override_path {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(env!("CARGO_BIN_EXE_hermes-manager"))
}

fn run_manager(args: &[&str]) -> String {
    let output = Command::new(manager_binary())
        .args(args)
        .output()
        .expect("manager command should run");
    assert!(
        output.status.success(),
        "manager failed\nstatus: {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout should be utf-8")
}

fn create_runtime_dirs(hermes_home: &Path) -> Vec<PathBuf> {
    let paths = hermes_manager::paths::managed_runtime_roots(hermes_home);
    for path in &paths {
        fs::create_dir_all(path).expect("runtime dir should be created");
    }
    paths
}

fn create_runtime_files(hermes_home: &Path) -> Vec<PathBuf> {
    let paths = hermes_manager::paths::managed_runtime_files(hermes_home);
    for path in &paths {
        fs::write(path, "managed").expect("runtime file should be created");
    }
    paths
}

fn create_source_gui_build_artifacts(hermes_home: &Path) -> Vec<PathBuf> {
    let mut paths = hermes_manager::paths::source_gui_build_roots(hermes_home);
    paths.extend(hermes_manager::paths::source_gui_build_files(hermes_home));
    for path in &paths {
        if path.extension().is_some() {
            fs::write(path, "managed").expect("runtime file should be created");
        } else {
            fs::create_dir_all(path).expect("runtime dir should be created");
        }
    }
    paths
}

#[test]
fn cli_smoke_manages_runtime_metadata_repair_and_lite_uninstall() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let user_config = hermes_home.join("config.yaml");
    fs::write(&user_config, "model: test").expect("user config should be created");

    let runtime_dirs = create_runtime_dirs(&hermes_home);
    let runtime_files = create_runtime_files(&hermes_home);
    let runtime_path_count = runtime_dirs.len() + runtime_files.len();
    let hermes_home_text = hermes_home.display().to_string();

    let install_out = run_manager(&["--hermes-home", &hermes_home_text, "install-metadata"]);
    assert!(install_out.contains("install_metadata=ok"));
    assert!(hermes_home
        .join("manager")
        .join("installed-files.json")
        .is_file());

    let dry_run_out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "uninstall-lite",
        "--dry-run",
    ]);
    let dry_run: serde_json::Value =
        serde_json::from_str(&dry_run_out).expect("dry-run output should be json");
    assert_eq!(dry_run["command"], "uninstall-lite");
    assert_eq!(dry_run["dryRun"], true);
    assert_eq!(
        dry_run["paths"]
            .as_array()
            .expect("paths should be array")
            .len(),
        runtime_path_count
    );

    let uninstall_out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "uninstall-lite",
    ]);
    let uninstall: serde_json::Value =
        serde_json::from_str(&uninstall_out).expect("uninstall output should be json");
    assert_eq!(uninstall["command"], "uninstall-lite");
    for path in &runtime_dirs {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
    for path in &runtime_files {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
    assert!(user_config.exists());

    let repaired_runtime_dirs = create_runtime_dirs(&hermes_home);
    let repaired_runtime_files = create_runtime_files(&hermes_home);
    let repaired_path_count = repaired_runtime_dirs.len() + repaired_runtime_files.len();
    let repair_out = run_manager(&["--hermes-home", &hermes_home_text, "--json", "repair-clean"]);
    let repair: serde_json::Value =
        serde_json::from_str(&repair_out).expect("repair output should be json");
    assert_eq!(repair["command"], "repair-clean");
    assert_eq!(
        repair["paths"]
            .as_array()
            .expect("paths should be array")
            .len(),
        repaired_path_count
    );
    for path in repaired_runtime_dirs {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
    for path in repaired_runtime_files {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
    assert!(user_config.exists());
}

#[test]
fn cli_smoke_uninstall_gui_build_cleans_source_artifacts_only() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let agent_root = hermes_manager::paths::agent_root(&hermes_home);
    let package_source = agent_root.join("hermes_cli").join("__init__.py");
    let user_config = hermes_home.join("config.yaml");
    fs::create_dir_all(package_source.parent().unwrap()).expect("package source should exist");
    fs::write(&package_source, "").expect("package source should be written");
    fs::write(&user_config, "model: test").expect("user config should be created");
    let artifacts = create_source_gui_build_artifacts(&hermes_home);
    let hermes_home_text = hermes_home.display().to_string();

    let dry_run_out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "uninstall-gui-build",
        "--dry-run",
    ]);
    let dry_run: serde_json::Value =
        serde_json::from_str(&dry_run_out).expect("dry-run output should be json");
    assert_eq!(dry_run["command"], "uninstall-gui-build");
    assert_eq!(dry_run["dryRun"], true);
    assert_eq!(
        dry_run["paths"]
            .as_array()
            .expect("paths should be array")
            .len(),
        artifacts.len()
    );

    let uninstall_out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "uninstall-gui-build",
    ]);
    let uninstall: serde_json::Value =
        serde_json::from_str(&uninstall_out).expect("uninstall output should be json");
    assert_eq!(uninstall["command"], "uninstall-gui-build");
    for path in artifacts {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
    assert!(package_source.exists());
    assert!(user_config.exists());
}

#[test]
fn manager_binary_uses_packaged_smoke_override() {
    let override_path = if cfg!(target_os = "windows") {
        std::ffi::OsString::from("target/release/hermes-manager.exe")
    } else {
        std::ffi::OsString::from("target/release/hermes-manager")
    };

    assert_eq!(
        manager_binary_from_env(Some(override_path.clone())),
        PathBuf::from(override_path)
    );
    assert_eq!(
        manager_binary_from_env(Some(std::ffi::OsString::new())),
        PathBuf::from(env!("CARGO_BIN_EXE_hermes-manager"))
    );
}

#[test]
fn cli_smoke_reports_bootstrap_bridge_capabilities() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-capabilities",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("capabilities output should be json");

    assert_eq!(report["command"], "bootstrap-capabilities");
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["canRunFullBootstrap"], false);
    assert!(report["supportedStages"]
        .as_array()
        .expect("supportedStages should be array")
        .iter()
        .any(|stage| stage.as_str() == Some("install-metadata")));
}

#[test]
fn cli_smoke_reports_native_bootstrap_manifest() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-manifest",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("bootstrap manifest output should be json");

    assert_eq!(report["command"], "bootstrap-manifest");
    assert_eq!(report["protocol_version"], 1);
    let stages = report["stages"].as_array().expect("stages should be array");
    assert_eq!(stages[0]["name"], "install-metadata");
    assert_eq!(stages[0]["needs_user_input"], false);
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("bootstrap-marker")));
}

#[test]
fn cli_smoke_runs_native_install_metadata_bootstrap_stage() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    fs::write(hermes_home.join("config.yaml"), "model: test")
        .expect("user config should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "install-metadata",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("bootstrap stage output should be json");

    assert_eq!(report["stage"], "install-metadata");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], false);
    assert!(hermes_home
        .join("manager")
        .join("installed-files.json")
        .is_file());
}

#[test]
fn cli_smoke_runs_native_bootstrap_marker_stage() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let agent_root = hermes_manager::paths::agent_root(&hermes_home);
    fs::create_dir_all(&agent_root).expect("agent root should be created");
    let hermes_home_text = hermes_home.display().to_string();
    let commit = "abcdef1234567890";

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "bootstrap-marker",
        "--commit",
        commit,
        "--branch",
        "main",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("bootstrap marker output should be json");
    let marker_path = agent_root.join(".hermes-bootstrap-complete");
    let marker: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&marker_path).expect("marker should exist"))
            .expect("marker should be json");

    assert_eq!(report["stage"], "bootstrap-marker");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], false);
    assert_eq!(marker["schemaVersion"], 1);
    assert_eq!(marker["pinnedCommit"], commit);
    assert_eq!(marker["pinnedBranch"], "main");
    assert!(marker["completedAt"]
        .as_str()
        .unwrap_or_default()
        .ends_with('Z'));
}
