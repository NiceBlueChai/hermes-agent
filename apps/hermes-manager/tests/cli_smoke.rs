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

fn run_manager_output(args: &[&str]) -> std::process::Output {
    Command::new(manager_binary())
        .args(args)
        .output()
        .expect("manager command should run")
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
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("path")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("config-templates")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("platform-sdks")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("node-deps")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("system-packages")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("node")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("uv")));
    #[cfg(target_os = "windows")]
    assert!(stages
        .iter()
        .any(|stage| stage["name"].as_str() == Some("git")));
    #[cfg(target_os = "windows")]
    assert!(stages.iter().any(|stage| {
        stage["name"].as_str() == Some("configure") && stage["needs_user_input"] == true
    }));
    #[cfg(target_os = "windows")]
    assert!(stages.iter().any(|stage| {
        stage["name"].as_str() == Some("gateway") && stage["needs_user_input"] == true
    }));
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

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_dry_runs_native_path_bootstrap_stage() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let install_root = hermes_manager::paths::agent_root(&hermes_home);
    fs::create_dir_all(install_root.join("venv").join("Scripts"))
        .expect("install root should be created");
    let hermes_home_text = hermes_home.display().to_string();
    let install_root_text = install_root.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "path",
        "--install-root",
        &install_root_text,
        "--current-path",
        "C:\\Windows\\System32",
        "--dry-run",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("bootstrap path output should be json");

    assert_eq!(report["stage"], "path");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], false);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_runs_native_config_templates_bootstrap_stage() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let install_root = hermes_manager::paths::agent_root(&hermes_home);
    fs::create_dir_all(install_root.join("skills").join("example"))
        .expect("skills should be created");
    fs::write(install_root.join(".env.example"), "OPENAI_API_KEY=\n")
        .expect("env template should be written");
    fs::write(
        install_root.join("cli-config.yaml.example"),
        "model: test\n",
    )
    .expect("config template should be written");
    fs::write(
        install_root.join("skills").join("example").join("SKILL.md"),
        "# Example\n",
    )
    .expect("skill should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let install_root_text = install_root.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "config-templates",
        "--install-root",
        &install_root_text,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("bootstrap config output should be json");

    assert_eq!(report["stage"], "config-templates");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], false);
    assert!(hermes_home.join("cron").is_dir());
    assert!(hermes_home.join("sessions").is_dir());
    assert_eq!(
        fs::read_to_string(hermes_home.join(".env")).expect("env should exist"),
        "OPENAI_API_KEY=\n"
    );
    assert_eq!(
        fs::read_to_string(hermes_home.join("config.yaml")).expect("config should exist"),
        "model: test\n"
    );
    assert!(fs::read_to_string(hermes_home.join("SOUL.md"))
        .expect("SOUL.md should exist")
        .contains("Hermes Agent Persona"));
    assert!(hermes_home
        .join("skills")
        .join("example")
        .join("SKILL.md")
        .is_file());
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_interactive_bootstrap_stages() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    for stage in ["configure", "gateway"] {
        let out = run_manager(&[
            "--hermes-home",
            &hermes_home_text,
            "--json",
            "bootstrap-stage",
            stage,
        ]);
        let report: serde_json::Value =
            serde_json::from_str(&out).expect("interactive skip output should be json");

        assert_eq!(report["stage"], stage);
        assert_eq!(report["ok"], true);
        assert_eq!(report["skipped"], true);
        assert!(report["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("non-interactive"));
    }
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_platform_sdks_when_no_tokens_are_configured() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let install_root = hermes_manager::paths::agent_root(&hermes_home);
    fs::create_dir_all(install_root.join("venv").join("Scripts")).expect("venv should be created");
    fs::write(
        install_root.join("venv").join("Scripts").join("python.exe"),
        "",
    )
    .expect("python placeholder should be written");
    fs::write(
        hermes_home.join(".env"),
        "TELEGRAM_BOT_TOKEN=your-token-here\n# DISCORD_BOT_TOKEN=abc\n",
    )
    .expect("env file should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let install_root_text = install_root.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "platform-sdks",
        "--install-root",
        &install_root_text,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("platform sdk skip output should be json");

    assert_eq!(report["stage"], "platform-sdks");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_platform_sdks_when_tokens_are_configured() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let install_root = hermes_manager::paths::agent_root(&hermes_home);
    fs::create_dir_all(install_root.join("venv").join("Scripts")).expect("venv should be created");
    fs::write(
        install_root.join("venv").join("Scripts").join("python.exe"),
        "",
    )
    .expect("python placeholder should be written");
    fs::write(hermes_home.join(".env"), "TELEGRAM_BOT_TOKEN=abc\n")
        .expect("env file should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let install_root_text = install_root.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "platform-sdks",
        "--install-root",
        &install_root_text,
    ]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("platform sdk fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "platform-sdks");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_node_deps_when_npm_is_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "node-deps",
        "--current-path",
        "",
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("node deps skip output should be json");

    assert_eq!(report["stage"], "node-deps");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_node_deps_when_npm_is_available() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let npm_dir = temp.path().join("node");
    fs::create_dir_all(&npm_dir).expect("npm dir should be created");
    fs::write(npm_dir.join("npm.cmd"), "@echo off\n").expect("npm shim should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let npm_path = npm_dir.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "node-deps",
        "--current-path",
        &npm_path,
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("node deps fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "node-deps");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_system_packages_when_tools_are_available() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let tool_dir = temp.path().join("tools");
    fs::create_dir_all(&tool_dir).expect("tool dir should be created");
    fs::write(tool_dir.join("rg.exe"), "").expect("rg should be written");
    fs::write(tool_dir.join("ffmpeg.exe"), "").expect("ffmpeg should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let tool_path = tool_dir.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "system-packages",
        "--current-path",
        &tool_path,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("system package skip output should be json");

    assert_eq!(report["stage"], "system-packages");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_system_packages_when_tools_are_missing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "system-packages",
        "--current-path",
        "",
    ]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("system package fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "system-packages");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_node_stage_when_supported_node_is_available() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let node_dir = temp.path().join("node");
    fs::create_dir_all(&node_dir).expect("node dir should be created");
    fs::write(node_dir.join("node.cmd"), "@echo v22.12.0\r\n")
        .expect("node shim should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let node_path = node_dir.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "node",
        "--current-path",
        &node_path,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("node stage skip output should be json");

    assert_eq!(report["stage"], "node");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_node_stage_when_node_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "node",
        "--current-path",
        "",
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("node fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "node");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_uv_stage_when_uv_is_available() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let uv_dir = temp.path().join("uv");
    fs::create_dir_all(&uv_dir).expect("uv dir should be created");
    fs::write(uv_dir.join("uv.cmd"), "@echo uv 0.8.0\r\n").expect("uv shim should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let uv_path = uv_dir.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "uv",
        "--current-path",
        &uv_path,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("uv stage skip output should be json");

    assert_eq!(report["stage"], "uv");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_uv_stage_when_uv_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "uv",
        "--current-path",
        "",
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("uv fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "uv");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_skips_native_git_stage_when_git_is_available() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    let git_dir = temp.path().join("git");
    fs::create_dir_all(&git_dir).expect("git dir should be created");
    fs::write(
        git_dir.join("git.cmd"),
        "@echo git version 2.54.0.windows.1\r\n",
    )
    .expect("git shim should be written");
    let hermes_home_text = hermes_home.display().to_string();
    let git_path = git_dir.display().to_string();

    let out = run_manager(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "git",
        "--current-path",
        &git_path,
    ]);
    let report: serde_json::Value =
        serde_json::from_str(&out).expect("git stage skip output should be json");

    assert_eq!(report["stage"], "git");
    assert_eq!(report["ok"], true);
    assert_eq!(report["skipped"], true);
}

#[cfg(target_os = "windows")]
#[test]
fn cli_smoke_falls_back_for_native_git_stage_when_git_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let hermes_home = temp.path().join("hermes");
    fs::create_dir_all(&hermes_home).expect("Hermes home should be created");
    let hermes_home_text = hermes_home.display().to_string();

    let output = run_manager_output(&[
        "--hermes-home",
        &hermes_home_text,
        "--json",
        "bootstrap-stage",
        "git",
        "--current-path",
        "",
    ]);
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("git fallback output should be json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["stage"], "git");
    assert_eq!(report["ok"], false);
    assert_eq!(report["failureCategory"], "fallback-to-script");
}
