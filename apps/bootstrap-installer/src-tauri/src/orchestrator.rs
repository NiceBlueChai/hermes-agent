//! Rust-side bootstrap orchestration planning.
//!
//! This module starts Phase 4 by keeping low-risk install state probes and
//! stage planning in Rust while the actual stage execution still falls back to
//! `install.ps1` / `install.sh` until individual stages reach parity.

use crate::events::{Manifest, StageInfo};
use crate::install_script::ScriptKind;
use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

const BOOTSTRAP_TOOLS_MANIFEST: &str = "bootstrap-tools-manifest.json";
const BOOTSTRAP_TOOLS_MANIFEST_SCHEMA_VERSION: u32 = 1;
const WHEELHOUSE_MANIFEST: &str = "wheelhouse-manifest.json";
const WHEELHOUSE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const ALLOWED_WHEELHOUSE_METADATA: [&str; 2] = [".gitignore", "README.md"];
const DESKTOP_ELECTRON_FALLBACK_MIRROR: &str = "https://npmmirror.com/mirrors/electron/";
const PYTHON_KNOWN_BROKEN_EXTRAS: &[&str] = &[];
const SCRIPT_REASON_INTERACTIVE: &str = "requires user input; handled by post-install UI";
const SCRIPT_REASON_UNPORTED: &str = "not yet ported to Rust; delegated to install script";

/// PATH probe result for one external tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProbe {
    pub name: String,
    pub path: Option<PathBuf>,
}

/// Read-only install state gathered before script-backed stages run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallStateReport {
    pub hermes_home: PathBuf,
    pub install_root: PathBuf,
    pub bootstrap_marker_exists: bool,
    pub tools: Vec<ToolProbe>,
}

/// Stage execution plan for the current mixed Rust/script bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStage {
    pub name: String,
    pub execution: StageExecutionMode,
    pub rust_probe: bool,
    pub script_fallback: bool,
    pub script_reason: Option<String>,
}

/// Native Python virtual environment stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonVenvStagePlan {
    pub uv: PathBuf,
    pub cwd: PathBuf,
    pub venv: PathBuf,
    pub uv_cache_dir: PathBuf,
    pub python_install_dir: PathBuf,
    pub python_bin_dir: PathBuf,
}

/// Native Python runtime stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonRuntimeStagePlan {
    pub uv: PathBuf,
    pub uv_cache_dir: PathBuf,
    pub python_install_dir: PathBuf,
    pub python_bin_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PythonRuntimeDirs {
    pub install_dir: PathBuf,
    pub bin_dir: PathBuf,
}

/// Native Python dependency sync stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonDependenciesStagePlan {
    pub uv: PathBuf,
    pub cwd: PathBuf,
    pub venv: PathBuf,
    pub python: PathBuf,
    pub lockfile: PathBuf,
    pub uv_cache_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PythonDependencyInstallTier {
    name: String,
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PyprojectToml {
    project: Option<PyprojectProject>,
}

#[derive(Debug, Deserialize)]
struct PyprojectProject {
    #[serde(rename = "optional-dependencies")]
    optional_dependencies: Option<BTreeMap<String, Vec<String>>>,
}

/// Native Node dependency stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDependenciesStagePlan {
    pub npm: PathBuf,
    pub npx: Option<PathBuf>,
    pub cwd: PathBuf,
    pub npm_cache_dir: PathBuf,
    pub playwright_browsers_dir: PathBuf,
    pub browser_tools: bool,
    pub tui_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaywrightInstallPlan {
    npx_args: Vec<String>,
    system_package_commands: Vec<UnixPackageInstallCommandPlan>,
    system_deps: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserInstallDecision {
    system_browser: Option<PathBuf>,
    playwright: Option<PlaywrightInstallPlan>,
    system_deps: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopPackResult {
    fallback_mirror_used: bool,
    purged_paths: Vec<PathBuf>,
}

/// Native desktop build stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopBuildStagePlan {
    pub npm: PathBuf,
    pub cwd: PathBuf,
    pub npm_cache_dir: PathBuf,
    pub electron_cache_dir: PathBuf,
    pub desktop_dir: PathBuf,
}

/// Native Windows Node runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsNodeRuntimeStagePlan {
    pub version_major: u32,
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub node_exe: PathBuf,
}

/// Native Windows uv runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUvRuntimeStagePlan {
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub uv_exe: PathBuf,
}

/// Native Unix uv runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixUvRuntimeStagePlan {
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub uv_bin: PathBuf,
    pub uvx_bin: PathBuf,
}

/// Native Windows Git runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsGitRuntimeStagePlan {
    pub tag: &'static str,
    pub version: &'static str,
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub git_exe: PathBuf,
    pub bash_exe: PathBuf,
    pub is_zip: bool,
}

/// Command used by the native Unix Git acquisition fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixGitInstallCommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

/// Command used by native Unix package-manager recovery stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixPackageInstallCommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

/// Command used by native Windows package-manager recovery stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsPackageInstallCommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub path_after_install: Option<PathBuf>,
}

/// Native Windows ripgrep runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRipgrepRuntimeStagePlan {
    pub version: &'static str,
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub rg_exe: PathBuf,
}

/// Native Unix ripgrep runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixRipgrepRuntimeStagePlan {
    pub version: &'static str,
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub rg_bin: PathBuf,
}

/// Native Windows ffmpeg runtime installation plan for bundled release archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsFfmpegRuntimeStagePlan {
    pub archive_name: String,
    pub install_dir: PathBuf,
    pub ffmpeg_exe: PathBuf,
}

/// Native Unix ffmpeg runtime installation plan for bundled release archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixFfmpegRuntimeStagePlan {
    pub archive_name: String,
    pub install_dir: PathBuf,
    pub ffmpeg_bin: PathBuf,
}

/// Native Playwright browser cache installation plan for bundled release archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaywrightBrowsersRuntimeStagePlan {
    pub archive_name: String,
    pub install_dir: PathBuf,
}

/// Native Electron cache installation plan for bundled release archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElectronCacheRuntimeStagePlan {
    pub archive_name: String,
    pub install_dir: PathBuf,
}

/// Native npm cache installation plan for bundled release archives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmCacheRuntimeStagePlan {
    pub archive_name: String,
    pub install_dir: PathBuf,
}

/// Native Unix Node runtime installation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixNodeRuntimeStagePlan {
    pub version_major: u32,
    pub archive_name: String,
    pub download_url: String,
    pub archive_path: PathBuf,
    pub install_dir: PathBuf,
    pub node_bin: PathBuf,
    pub npm_bin: PathBuf,
    pub npx_bin: PathBuf,
}

/// Messaging-platform SDK requirement derived from user configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformSdkRequirement {
    pub env_var: &'static str,
    pub import_name: &'static str,
    pub pip_spec: &'static str,
}

/// Native platform SDK verification stage execution plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformSdkStagePlan {
    pub python: PathBuf,
    pub uv: Option<PathBuf>,
    pub pip_cache_dir: PathBuf,
    pub uv_cache_dir: PathBuf,
    pub wheelhouse_dir: Option<PathBuf>,
    pub requirements: Vec<PlatformSdkRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlatformSdkInstallCommandPlan {
    method: &'static str,
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, PathBuf)>,
}

/// How a bootstrap stage is currently handled by the Rust orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageExecutionMode {
    Native,
    NativeWithScriptFallback,
    ProbeThenScript,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapArchiveSourceKind {
    Bundled,
    Cache,
}

impl BootstrapArchiveSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Cache => "cache",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBootstrapArchive {
    path: PathBuf,
    cache_path: PathBuf,
    kind: BootstrapArchiveSourceKind,
    expected_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapToolsManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    archives: Vec<BootstrapToolsManifestArchive>,
}

#[derive(Debug, Deserialize)]
struct BootstrapToolsManifestArchive {
    name: String,
    platform: Option<String>,
    arch: Option<String>,
    url: Option<String>,
    #[serde(rename = "sizeBytes", default)]
    size_bytes: Option<u64>,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct WheelhouseManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "sourceFiles", default)]
    source_files: Vec<WheelhouseManifestSourceFile>,
    wheels: Vec<WheelhouseManifestWheel>,
}

#[derive(Debug, Deserialize)]
struct WheelhouseManifestSourceFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct WheelhouseManifestWheel {
    name: String,
    platform: Option<String>,
    arch: Option<String>,
    python: Option<String>,
    #[serde(rename = "sizeBytes", default)]
    size_bytes: Option<u64>,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BootstrapArchiveTarget {
    platform: &'static str,
    arch: &'static str,
}

/// Build the bootstrap stage manifest without invoking the platform script.
pub fn native_bootstrap_manifest(kind: ScriptKind, include_desktop: bool) -> Manifest {
    let stages = match kind {
        ScriptKind::Ps1 => windows_manifest_stages(include_desktop),
        ScriptKind::Sh => unix_manifest_stages(include_desktop),
    };
    Manifest {
        stages,
        protocol_version: Some(1),
    }
}

/// Build a read-only state report without including user config or session data.
pub fn install_state_report(hermes_home: &Path, tools: Vec<ToolProbe>) -> InstallStateReport {
    let install_root = hermes_home.join("hermes-agent");
    InstallStateReport {
        hermes_home: hermes_home.to_path_buf(),
        bootstrap_marker_exists: crate::paths::likely_bootstrap_marker(&install_root).exists(),
        install_root,
        tools,
    }
}

/// Probe the current process environment for install-time helper tools.
pub fn probe_install_state(hermes_home: &Path) -> InstallStateReport {
    let tools = ["uv", "git", "node", "npm", "python"]
        .into_iter()
        .map(probe_tool)
        .collect();
    install_state_report(hermes_home, tools)
}

/// Build the initial mixed execution plan from the script manifest.
pub fn build_stage_plan(stages: &[StageInfo], _include_desktop: bool) -> Vec<PlannedStage> {
    stages
        .iter()
        .map(|stage| {
            let execution = stage_execution_mode(&stage.name);
            PlannedStage {
                name: stage.name.clone(),
                execution,
                rust_probe: execution == StageExecutionMode::ProbeThenScript,
                script_fallback: matches!(
                    execution,
                    StageExecutionMode::NativeWithScriptFallback
                        | StageExecutionMode::ProbeThenScript
                        | StageExecutionMode::Script
                ),
                script_reason: script_stage_reason(stage, execution),
            }
        })
        .collect()
}

fn script_stage_reason(stage: &StageInfo, execution: StageExecutionMode) -> Option<String> {
    if execution != StageExecutionMode::Script {
        return None;
    }
    if stage.needs_user_input {
        Some(SCRIPT_REASON_INTERACTIVE.to_string())
    } else {
        Some(SCRIPT_REASON_UNPORTED.to_string())
    }
}

/// Return a Rust-side skip result for stages that must be handled by UI.
pub fn interactive_stage_skip_result(stage: &StageInfo) -> Option<crate::events::StageResultPayload> {
    if !stage.needs_user_input {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some(SCRIPT_REASON_INTERACTIVE.to_string()),
        data: None,
    })
}

/// Return a Rust-side skip result for tool stages already satisfied locally.
#[cfg(test)]
pub fn satisfied_tool_stage_skip_result<P>(
    stage: &StageInfo,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Option<crate::events::StageResultPayload>
where
    P: AsRef<OsStr>,
{
    satisfied_tool_stage_skip_result_with_node_probe(
        stage,
        hermes_home,
        path_env,
        pathext,
        |_| false,
    )
}

fn satisfied_tool_stage_skip_result_with_node_probe<P, F>(
    stage: &StageInfo,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
    node_version_ok: F,
) -> Option<crate::events::StageResultPayload>
where
    P: AsRef<OsStr>,
    F: Fn(&Path) -> bool,
{
    let available = match stage.name.as_str() {
        name if name.eq_ignore_ascii_case("uv") => {
            managed_tool_path(hermes_home, "uv").is_file()
                || find_executable_on_path("uv", path_env.as_ref(), pathext).is_some()
        }
        name if name.eq_ignore_ascii_case("git") => {
            find_executable_on_path("git", path_env.as_ref(), pathext).is_some()
        }
        name if name.eq_ignore_ascii_case("node") => {
            let node = find_node_executable(hermes_home, path_env.as_ref(), pathext);
            let npm = find_npm_executable(hermes_home, path_env.as_ref(), pathext);
            matches!(node, Some(path) if npm.is_some() && node_version_ok(&path))
        }
        name if name.eq_ignore_ascii_case("system-packages") => {
            find_executable_on_path("rg", path_env.as_ref(), pathext).is_some()
                && find_executable_on_path("ffmpeg", path_env.as_ref(), pathext).is_some()
        }
        _ => false,
    };
    if !available {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some("required tool already available".to_string()),
        data: None,
    })
}

/// Return a Rust-side skip result for tool stages satisfied in this process.
pub fn satisfied_tool_stage_skip_result_from_env(
    stage: &StageInfo,
    hermes_home: &Path,
) -> Option<crate::events::StageResultPayload> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    satisfied_tool_stage_skip_result_with_node_probe(
        stage,
        hermes_home,
        path_env,
        &pathext,
        node_version_satisfies_build,
    )
}

/// Return a Rust-side skip result when the required Python runtime exists.
pub fn python_stage_skip_result(
    stage: &StageInfo,
    hermes_home: &Path,
) -> Option<crate::events::StageResultPayload> {
    python_stage_skip_result_with_probe(stage, || python_runtime_available(hermes_home))
}

fn python_stage_skip_result_with_probe<F>(
    stage: &StageInfo,
    python_runtime_available: F,
) -> Option<crate::events::StageResultPayload>
where
    F: FnOnce() -> bool,
{
    if !stage.name.eq_ignore_ascii_case("python") {
        return None;
    }
    if !python_runtime_available() {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some("required Python runtime already available".to_string()),
        data: None,
    })
}

fn python_runtime_available(hermes_home: &Path) -> bool {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let Ok(uv) = uv_tool_path(hermes_home, path_env, &pathext) else {
        return false;
    };
    Command::new(uv)
        .args(["python", "find", "3.11"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Return a Rust-side skip result for node-deps when npm is absent.
pub fn node_deps_skip_result<P>(
    stage: &StageInfo,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Option<crate::events::StageResultPayload>
where
    P: AsRef<OsStr>,
{
    if !stage.name.eq_ignore_ascii_case("node-deps") {
        return None;
    }
    if find_npm_executable(hermes_home, path_env, pathext).is_some() {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some("npm not available".to_string()),
        data: None,
    })
}

/// Return a Rust-side skip result for node-deps in this process.
pub fn node_deps_skip_result_from_env(
    stage: &StageInfo,
    hermes_home: &Path,
) -> Option<crate::events::StageResultPayload> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    node_deps_skip_result(stage, hermes_home, path_env, &pathext)
}

/// Return a Rust-side skip result for desktop builds without the desktop package.
pub fn desktop_stage_skip_result(
    stage: &StageInfo,
    install_root: &Path,
) -> Option<crate::events::StageResultPayload> {
    if !stage.name.eq_ignore_ascii_case("desktop") {
        return None;
    }
    if install_root
        .join("apps")
        .join("desktop")
        .join("package.json")
        .is_file()
    {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some("apps/desktop not present".to_string()),
        data: None,
    })
}

/// Return a Rust-side skip result for platform SDK verification with no tokens.
pub fn platform_sdks_skip_result(
    stage: &StageInfo,
    hermes_home: &Path,
) -> Option<crate::events::StageResultPayload> {
    if !stage.name.eq_ignore_ascii_case("platform-sdks") {
        return None;
    }
    if platform_env_has_configured_tokens(&hermes_home.join(".env")) {
        return None;
    }
    Some(crate::events::StageResultPayload {
        stage: stage.name.clone(),
        ok: true,
        skipped: true,
        reason: Some("no messaging platform tokens configured".to_string()),
        data: None,
    })
}

/// Build the native platform SDK verification stage plan.
pub fn platform_sdk_stage_plan(
    hermes_home: &Path,
    install_root: &Path,
    bundled_wheelhouse_dir: Option<&Path>,
) -> Result<PlatformSdkStagePlan> {
    let env_path = hermes_home.join(".env");
    let env_text = fs::read_to_string(&env_path)
        .with_context(|| format!("reading {}", env_path.display()))?;
    let requirements = platform_sdk_requirements_from_env(&env_text);
    if requirements.is_empty() {
        return Err(anyhow!("no messaging platform tokens configured"));
    }
    let python = venv_python_path(&install_root.join("venv"));
    if !python.is_file() {
        return Err(anyhow!("venv Python not found at {}", python.display()));
    }
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let uv = uv_tool_path(hermes_home, path_env, &pathext).ok();
    let checkout_wheelhouse = install_root.join("resources").join("wheelhouse");
    let wheelhouse_dir = bundled_wheelhouse_dir
        .filter(|path| wheelhouse_has_wheels(path, Some(install_root)))
        .map(Path::to_path_buf)
        .or_else(|| {
            wheelhouse_has_wheels(&checkout_wheelhouse, Some(install_root))
                .then_some(checkout_wheelhouse)
        });
    Ok(PlatformSdkStagePlan {
        python,
        uv,
        pip_cache_dir: hermes_home.join("pip-cache"),
        uv_cache_dir: hermes_home.join("uv-cache"),
        wheelhouse_dir,
        requirements,
    })
}

/// Verify and install configured messaging platform SDKs natively.
pub fn install_platform_sdks_stage(
    hermes_home: &Path,
    install_root: &Path,
    bundled_wheelhouse_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let plan = platform_sdk_stage_plan(hermes_home, install_root, bundled_wheelhouse_dir)?;
    let missing = plan
        .requirements
        .iter()
        .copied()
        .filter(|sdk| !python_import_available(&plan.python, sdk.import_name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(serde_json::json!({
            "python": plan.python,
            "uv": plan.uv,
            "pipCacheDir": plan.pip_cache_dir,
            "uvCacheDir": plan.uv_cache_dir,
            "wheelhouseDir": plan.wheelhouse_dir,
            "checked": plan.requirements.len(),
            "installed": [],
        }));
    }
    let mut installed_methods = Vec::new();
    for sdk in &missing {
        let method = install_platform_sdk_requirement(&plan, *sdk)?;
        installed_methods.push(serde_json::json!({
            "spec": sdk.pip_spec,
            "method": method,
        }));
    }
    Ok(serde_json::json!({
        "python": plan.python,
        "uv": plan.uv,
        "pipCacheDir": plan.pip_cache_dir,
        "uvCacheDir": plan.uv_cache_dir,
        "wheelhouseDir": plan.wheelhouse_dir,
        "checked": plan.requirements.len(),
        "installed": missing.iter().map(|sdk| sdk.pip_spec).collect::<Vec<_>>(),
        "installMethods": installed_methods,
    }))
}

fn platform_env_has_configured_tokens(env_path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(env_path) else {
        return false;
    };
    !platform_sdk_requirements_from_env(&text).is_empty()
}

fn platform_sdk_requirements_from_env(text: &str) -> Vec<PlatformSdkRequirement> {
    const SDK_MAP: [PlatformSdkRequirement; 5] = [
        PlatformSdkRequirement {
            env_var: "TELEGRAM_BOT_TOKEN",
            import_name: "telegram",
            pip_spec: "python-telegram-bot[webhooks]>=22.6,<23",
        },
        PlatformSdkRequirement {
            env_var: "DISCORD_BOT_TOKEN",
            import_name: "discord",
            pip_spec: "discord.py[voice]>=2.7.1,<3",
        },
        PlatformSdkRequirement {
            env_var: "SLACK_BOT_TOKEN",
            import_name: "slack_sdk",
            pip_spec: "slack-sdk>=3.27.0,<4",
        },
        PlatformSdkRequirement {
            env_var: "SLACK_APP_TOKEN",
            import_name: "slack_bolt",
            pip_spec: "slack-bolt>=1.18.0,<2",
        },
        PlatformSdkRequirement {
            env_var: "WHATSAPP_ENABLED",
            import_name: "qrcode",
            pip_spec: "qrcode>=7.0,<8",
        },
    ];
    SDK_MAP
        .into_iter()
        .filter(|sdk| env_has_configured_platform_value(text, sdk.env_var))
        .collect()
}

fn env_has_configured_platform_value(text: &str, env_var: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return false;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return false;
        };
        if key.trim() != env_var {
            return false;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        let disabled = ["0", "false", "no", "off", "none", "null"];
        if value.is_empty() || value.eq_ignore_ascii_case("your-token-here") {
            return false;
        }
        if disabled
            .iter()
            .any(|disabled| value.eq_ignore_ascii_case(disabled))
        {
            return false;
        }
        if env_var == "WHATSAPP_ENABLED" {
            return value.eq_ignore_ascii_case("true");
        }
        true
    })
}

fn python_import_available(python: &Path, import_name: &str) -> bool {
    Command::new(python)
        .args(["-c", &format!("import {import_name}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn ensure_pip_available(python: &Path) -> Result<()> {
    let has_pip = Command::new(python)
        .args(["-m", "pip", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if has_pip {
        return Ok(());
    }
    let status = Command::new(python)
        .args(["-m", "ensurepip", "--upgrade"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running ensurepip with {}", python.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("ensurepip failed with exit {:?}", status.code()))
    }
}

fn install_platform_sdk_requirement(
    plan: &PlatformSdkStagePlan,
    sdk: PlatformSdkRequirement,
) -> Result<&'static str> {
    let commands = platform_sdk_install_commands(plan, sdk);
    let mut errors = Vec::new();
    let mut pip_checked = false;
    let mut pip_ready = false;
    for command in &commands {
        if command.method != "uv" && !pip_checked {
            pip_checked = true;
            match ensure_pip_available(&plan.python) {
                Ok(()) => pip_ready = true,
                Err(err) => {
                    errors.push(err.to_string());
                    continue;
                }
            }
        }
        if command.method != "uv" && !pip_ready {
            continue;
        }
        match run_platform_sdk_install_command(command) {
            Ok(()) => return Ok(command.method),
            Err(err) => errors.push(err.to_string()),
        }
    }
    Err(anyhow!(
        "failed to install {} through native platform SDK recovery: {}",
        sdk.pip_spec,
        errors.join("; ")
    ))
}

fn platform_sdk_install_commands(
    plan: &PlatformSdkStagePlan,
    sdk: PlatformSdkRequirement,
) -> Vec<PlatformSdkInstallCommandPlan> {
    let mut commands = Vec::new();
    if let Some(wheelhouse) = &plan.wheelhouse_dir {
        commands.push(PlatformSdkInstallCommandPlan {
            method: "wheelhouse",
            program: plan.python.clone(),
            args: vec![
                "-m".to_string(),
                "pip".to_string(),
                "install".to_string(),
                "--no-index".to_string(),
                "--find-links".to_string(),
                wheelhouse.display().to_string(),
                sdk.pip_spec.to_string(),
            ],
            env: vec![("PIP_CACHE_DIR".to_string(), plan.pip_cache_dir.clone())],
        });
    }
    commands.push(PlatformSdkInstallCommandPlan {
        method: "pip",
        program: plan.python.clone(),
        args: vec![
            "-m".to_string(),
            "pip".to_string(),
            "install".to_string(),
            sdk.pip_spec.to_string(),
        ],
        env: vec![("PIP_CACHE_DIR".to_string(), plan.pip_cache_dir.clone())],
    });
    if let Some(uv) = &plan.uv {
        commands.push(PlatformSdkInstallCommandPlan {
            method: "uv",
            program: uv.clone(),
            args: vec![
                "pip".to_string(),
                "install".to_string(),
                "--python".to_string(),
                plan.python.display().to_string(),
                sdk.pip_spec.to_string(),
            ],
            env: vec![("UV_CACHE_DIR".to_string(), plan.uv_cache_dir.clone())],
        });
    }
    commands
}

fn run_platform_sdk_install_command(command: &PlatformSdkInstallCommandPlan) -> Result<()> {
    let mut child = Command::new(&command.program);
    child.args(&command.args);
    for (key, value) in &command.env {
        child.env(key, value);
    }
    let status = child
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {}", command.program.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "{} {} failed with exit {:?}",
            command.program.display(),
            command.args.join(" "),
            status.code()
        ))
    }
}

/// Return a compact log line for the current Rust orchestration coverage.
pub fn summarize_plan(report: &InstallStateReport, plan: &[PlannedStage]) -> String {
    let tool_summary = report
        .tools
        .iter()
        .map(|tool| match &tool.path {
            Some(path) => format!("{}={}", tool.name, path.display()),
            None => format!("{}=missing", tool.name),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let native_count = plan
        .iter()
        .filter(|stage| {
            matches!(
                stage.execution,
                StageExecutionMode::Native | StageExecutionMode::NativeWithScriptFallback
            )
        })
        .count();
    let probe_count = plan
        .iter()
        .filter(|stage| stage.execution == StageExecutionMode::ProbeThenScript)
        .count();
    let script_count = plan
        .iter()
        .filter(|stage| stage.execution == StageExecutionMode::Script)
        .count();
    let script_reasons = plan
        .iter()
        .filter_map(|stage| {
            stage
                .script_reason
                .as_ref()
                .map(|reason| format!("{}={}", stage.name, reason))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        concat!(
            "[bootstrap] rust orchestrator: install_root={} marker_exists={} ",
            "native_stages={} probe_stages={} script_stages={} total_stages={} tools=[{}] ",
            "script_reasons=[{}]"
        ),
        report.install_root.display(),
        report.bootstrap_marker_exists,
        native_count,
        probe_count,
        script_count,
        plan.len(),
        tool_summary,
        script_reasons
    )
}

fn windows_manifest_stages(include_desktop: bool) -> Vec<StageInfo> {
    let mut stages = vec![
        stage_info("uv", "Installing uv package manager", "prereqs", false),
        stage_info("python", "Verifying Python 3.11", "prereqs", false),
        stage_info("git", "Installing Git", "prereqs", false),
        stage_info("node", "Detecting Node.js", "prereqs", false),
        stage_info(
            "system-packages",
            "Installing ripgrep and ffmpeg",
            "prereqs",
            false,
        ),
        stage_info("repository", "Cloning Hermes repository", "install", false),
        stage_info(
            "venv",
            "Creating Python virtual environment",
            "install",
            false,
        ),
        stage_info(
            "dependencies",
            "Installing Python dependencies",
            "install",
            false,
        ),
        stage_info("node-deps", "Installing Node.js dependencies", "install", false),
    ];
    if include_desktop {
        stages.push(stage_info("desktop", "Building desktop app", "install", false));
    }
    stages.extend([
        stage_info("path", "Adding Hermes to PATH", "finalize", false),
        stage_info(
            "config-templates",
            "Writing configuration templates",
            "finalize",
            false,
        ),
        stage_info(
            "platform-sdks",
            "Installing messaging platform SDKs",
            "finalize",
            false,
        ),
        stage_info(
            "bootstrap-marker",
            "Marking install complete",
            "finalize",
            false,
        ),
        stage_info(
            "configure",
            "Configuring API keys and models",
            "post-install",
            true,
        ),
        stage_info(
            "gateway",
            "Starting messaging gateway",
            "post-install",
            true,
        ),
    ]);
    stages
}

fn unix_manifest_stages(include_desktop: bool) -> Vec<StageInfo> {
    let mut stages = vec![
        stage_info("uv", "Install uv package manager", "runtime", false),
        stage_info("node", "Detect Node.js", "runtime", false),
        stage_info("python", "Verify Python 3.11", "runtime", false),
        stage_info(
            "system-packages",
            "Install system packages",
            "runtime",
            false,
        ),
        stage_info("repository", "Download Hermes Agent", "runtime", false),
        stage_info(
            "venv",
            "Create Python virtual environment",
            "runtime",
            false,
        ),
        stage_info(
            "python-deps",
            "Install Python dependencies",
            "runtime",
            false,
        ),
        stage_info(
            "node-deps",
            "Install browser-tool dependencies",
            "runtime",
            false,
        ),
        stage_info("path", "Install hermes command", "runtime", false),
        stage_info("config", "Prepare config and skills", "configuration", false),
        stage_info(
            "platform-sdks",
            "Install messaging platform SDKs",
            "configuration",
            false,
        ),
        stage_info(
            "setup",
            "Configure API keys and settings",
            "configuration",
            true,
        ),
        stage_info(
            "gateway",
            "Configure gateway service",
            "configuration",
            true,
        ),
    ];
    if include_desktop {
        stages.push(stage_info("desktop", "Build desktop app", "runtime", false));
    }
    stages.push(stage_info(
        "bootstrap-marker",
        "Mark install complete",
        "runtime",
        false,
    ));
    stages.push(stage_info("complete", "Finish install", "runtime", false));
    stages
}

fn stage_info(name: &str, title: &str, category: &str, needs_user_input: bool) -> StageInfo {
    StageInfo {
        name: name.to_string(),
        title: title.to_string(),
        category: category.to_string(),
        needs_user_input,
    }
}

/// Write the bootstrap-complete marker consumed by the desktop launcher.
pub fn write_bootstrap_marker(
    install_root: &Path,
    pinned_commit: Option<&str>,
    pinned_branch: Option<&str>,
) -> Result<serde_json::Value> {
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }

    let commit = pinned_commit
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| resolve_git_head(install_root))
        .unwrap_or_default();
    let branch = pinned_branch
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("main");

    let marker = serde_json::json!({
        "schemaVersion": 1,
        "pinnedCommit": commit,
        "pinnedBranch": branch,
        "completedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    let marker_path = install_root.join(".hermes-bootstrap-complete");
    let text = serde_json::to_string_pretty(&marker)
        .context("serializing bootstrap marker")?
        + "\n";
    std::fs::write(&marker_path, text)
        .with_context(|| format!("writing bootstrap marker {}", marker_path.display()))?;
    Ok(marker)
}

/// Create Hermes home config directories and initial template files.
pub fn configure_templates(hermes_home: &Path, install_root: &Path) -> Result<serde_json::Value> {
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }

    for dir in [
        "cron",
        "sessions",
        "logs",
        "pairing",
        "hooks",
        "image_cache",
        "audio_cache",
        "memories",
        "skills",
    ] {
        fs::create_dir_all(hermes_home.join(dir))
            .with_context(|| format!("creating Hermes home directory {dir}"))?;
    }

    let env_created = ensure_file_from_template(
        &hermes_home.join(".env"),
        &install_root.join(".env.example"),
        Some(""),
    )?;
    let config_created = ensure_file_from_template(
        &hermes_home.join("config.yaml"),
        &install_root.join("cli-config.yaml.example"),
        None,
    )?;
    let soul_created = ensure_soul_file(&hermes_home.join("SOUL.md"))?;
    let skills_sync = sync_bundled_skills(hermes_home, install_root)?;

    Ok(serde_json::json!({
        "hermesHome": hermes_home,
        "envCreated": env_created,
        "configCreated": config_created,
        "soulCreated": soul_created,
        "skillsSync": skills_sync,
    }))
}

/// Stamp the install method used by status and update recommendations.
pub fn write_install_method_stamp(hermes_home: &Path) -> Result<serde_json::Value> {
    let stamp_path = hermes_home.join(".install_method");
    if let Some(parent) = stamp_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&stamp_path, "git\n")
        .with_context(|| format!("writing install method stamp {}", stamp_path.display()))?;
    Ok(serde_json::json!({
        "installMethod": "git",
        "stampPath": stamp_path.display().to_string(),
    }))
}

/// Build the native Python runtime stage plan for the active install layout.
pub fn python_runtime_stage_plan_for_layout<P>(
    hermes_home: &Path,
    install_root: &Path,
    path_env: P,
    pathext: &str,
) -> Result<PythonRuntimeStagePlan>
where
    P: AsRef<OsStr>,
{
    let dirs = python_runtime_dirs_for_layout(hermes_home, install_root);
    Ok(PythonRuntimeStagePlan {
        uv: uv_tool_path(hermes_home, path_env, pathext)?,
        uv_cache_dir: hermes_home.join("uv-cache"),
        python_install_dir: dirs.install_dir,
        python_bin_dir: dirs.bin_dir,
    })
}

/// Install the required Python runtime natively through uv.
pub fn install_python_runtime_stage(hermes_home: &Path, install_root: &Path) -> Result<serde_json::Value> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let plan = python_runtime_stage_plan_for_layout(hermes_home, install_root, path_env, &pathext)?;
    let status = Command::new(&plan.uv)
        .args(["python", "install", "3.11"])
        .env("UV_CACHE_DIR", &plan.uv_cache_dir)
        .env("UV_PYTHON_INSTALL_DIR", &plan.python_install_dir)
        .env("UV_PYTHON_BIN_DIR", &plan.python_bin_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {}", plan.uv.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "uv python install failed with exit {:?}",
            status.code()
        ));
    }
    let output = Command::new(&plan.uv)
        .args(["python", "find", "3.11"])
        .env("UV_CACHE_DIR", &plan.uv_cache_dir)
        .env("UV_PYTHON_INSTALL_DIR", &plan.python_install_dir)
        .env("UV_PYTHON_BIN_DIR", &plan.python_bin_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("locating Python with {}", plan.uv.display()))?;
    if !output.status.success() {
        return Err(anyhow!("uv python find failed after install"));
    }
    let python = String::from_utf8(output.stdout)
        .context("decoding uv python find output")?
        .trim()
        .to_string();
    if python.is_empty() {
        return Err(anyhow!("uv python find returned an empty path after install"));
    }
    Ok(serde_json::json!({
        "uv": plan.uv,
        "python": python,
        "uvCacheDir": plan.uv_cache_dir,
        "pythonInstallDir": plan.python_install_dir,
        "pythonBinDir": plan.python_bin_dir,
    }))
}

/// Build a Windows Node runtime plan from the Node.js latest-v22.x index.
pub fn windows_node_runtime_stage_plan_from_index(
    hermes_home: &Path,
    arch: &str,
    index_html: &str,
) -> Result<WindowsNodeRuntimeStagePlan> {
    let version_major = 22;
    let archive_name = latest_windows_node_archive_name(index_html, version_major, arch)
        .ok_or_else(|| anyhow!("Node.js v{version_major} Windows {arch} archive not found"))?;
    windows_node_runtime_stage_plan_from_archive_name(hermes_home, archive_name)
}

fn windows_node_runtime_stage_plan_from_archive_name(
    hermes_home: &Path,
    archive_name: String,
) -> Result<WindowsNodeRuntimeStagePlan> {
    let version_major = node_archive_version_tuple(&archive_name).0;
    if version_major == 0 {
        return Err(anyhow!("invalid Node.js archive name: {archive_name}"));
    }
    let download_url =
        format!("https://nodejs.org/dist/latest-v{version_major}.x/{archive_name}");
    let archive_path = bootstrap_archive_cache_path(hermes_home, &archive_name);
    let install_dir = hermes_home.join("node");
    let node_exe = install_dir.join("node.exe");
    Ok(WindowsNodeRuntimeStagePlan {
        version_major,
        archive_name,
        download_url,
        archive_path,
        install_dir,
        node_exe,
    })
}

/// Install the Windows Node.js runtime from the official portable ZIP archive.
pub async fn install_windows_node_runtime_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if !cfg!(target_os = "windows") {
        return Err(anyhow!("native Node runtime stage is only available on Windows"));
    }
    let arch = windows_node_arch_slug();
    if arch != "x64" && arch != "arm64" && arch != "x86" {
        return Err(anyhow!("unsupported Windows architecture for Node.js: {arch}"));
    }
    let version_major = 22;
    let plan = if let Some(archive_name) =
        latest_bundled_windows_node_archive_name(bundled_tools_dir, version_major, &arch)
    {
        windows_node_runtime_stage_plan_from_archive_name(hermes_home, archive_name)?
    } else {
        let index_url = format!("https://nodejs.org/dist/latest-v{version_major}.x/");
        let index_html = reqwest::Client::new()
            .get(&index_url)
            .header("User-Agent", "Hermes-Setup")
            .send()
            .await
            .with_context(|| format!("GET {index_url}"))?
            .text()
            .await
            .with_context(|| format!("reading body of {index_url}"))?;
        windows_node_runtime_stage_plan_from_index(hermes_home, &arch, &index_html)?
    };
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind == BootstrapArchiveSourceKind::Cache {
        let expected_sha256 = archive_source.expected_sha256.as_deref();
        crate::artifact::download_to_cache(
            crate::artifact::DownloadSpec {
                url: plan.download_url.clone(),
                user_agent: "Hermes-Setup",
                expected_sha256,
            },
            &archive_source.path,
        )
        .await
        .with_context(|| format!("downloading {}", plan.archive_name))?;
    }
    install_windows_node_archive(&archive_source.path, &plan.install_dir)?;
    if !node_version_satisfies_build(&plan.node_exe) {
        return Err(anyhow!(
            "installed Node.js does not satisfy desktop build requirements"
        ));
    }
    prepend_process_path(&plan.install_dir);
    persist_windows_path_entry(&plan.install_dir)?;
    Ok(serde_json::json!({
        "node": plan.node_exe,
        "archive": plan.archive_name,
        "archiveSource": archive_source.kind.as_str(),
        "installDir": plan.install_dir,
    }))
}

/// Build a Unix Node runtime plan from the Node.js latest-v22.x index.
pub fn unix_node_runtime_stage_plan_from_index(
    hermes_home: &Path,
    node_os: &str,
    arch: &str,
    index_html: &str,
) -> Result<UnixNodeRuntimeStagePlan> {
    let version_major = 22;
    let archive_name = latest_unix_node_archive_name(index_html, version_major, node_os, arch)
        .ok_or_else(|| anyhow!("Node.js v{version_major} {node_os}-{arch} archive not found"))?;
    unix_node_runtime_stage_plan_from_archive_name(hermes_home, archive_name)
}

fn unix_node_runtime_stage_plan_from_archive_name(
    hermes_home: &Path,
    archive_name: String,
) -> Result<UnixNodeRuntimeStagePlan> {
    let version_major = node_archive_version_tuple(&archive_name).0;
    if version_major == 0 {
        return Err(anyhow!("invalid Node.js archive name: {archive_name}"));
    }
    let download_url =
        format!("https://nodejs.org/dist/latest-v{version_major}.x/{archive_name}");
    let archive_path = bootstrap_archive_cache_path(hermes_home, &archive_name);
    let install_dir = hermes_home.join("node");
    let bin_dir = install_dir.join("bin");
    Ok(UnixNodeRuntimeStagePlan {
        version_major,
        archive_name,
        download_url,
        archive_path,
        install_dir,
        node_bin: bin_dir.join("node"),
        npm_bin: bin_dir.join("npm"),
        npx_bin: bin_dir.join("npx"),
    })
}

/// Install a Hermes-managed Unix Node.js runtime from the official tarball.
pub async fn install_unix_node_runtime_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if cfg!(target_os = "windows") {
        return Err(anyhow!("native Unix Node runtime stage is not available on Windows"));
    }
    if node_version_satisfies_build(&find_unix_managed_node(hermes_home)) {
        let node = find_unix_managed_node(hermes_home);
        prepend_process_path(node.parent().unwrap_or(hermes_home));
        return Ok(serde_json::json!({
            "node": node,
            "skipped": true,
            "reason": "Hermes-managed Node already satisfies build requirements",
        }));
    }

    let node_os = unix_node_os_slug()?;
    let arch = current_unix_node_arch_slug()?;
    let version_major = 22;
    let plan = if let Some(archive_name) =
        latest_bundled_unix_node_archive_name(bundled_tools_dir, version_major, &node_os, &arch)
    {
        unix_node_runtime_stage_plan_from_archive_name(hermes_home, archive_name)?
    } else {
        let index_url = format!("https://nodejs.org/dist/latest-v{version_major}.x/");
        let index_html = reqwest::Client::new()
            .get(&index_url)
            .header("User-Agent", "Hermes-Setup")
            .send()
            .await
            .with_context(|| format!("GET {index_url}"))?
            .text()
            .await
            .with_context(|| format!("reading body of {index_url}"))?;
        unix_node_runtime_stage_plan_from_index(hermes_home, &node_os, &arch, &index_html)?
    };
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind == BootstrapArchiveSourceKind::Cache {
        let expected_sha256 = archive_source.expected_sha256.as_deref();
        crate::artifact::download_to_cache(
            crate::artifact::DownloadSpec {
                url: plan.download_url.clone(),
                user_agent: "Hermes-Setup",
                expected_sha256,
            },
            &archive_source.path,
        )
        .await
        .with_context(|| format!("downloading {}", plan.archive_name))?;
    }
    install_unix_node_archive(&archive_source.path, &plan.install_dir)?;
    if !node_version_satisfies_build(&plan.node_bin) {
        return Err(anyhow!(
            "installed Node.js does not satisfy desktop build requirements"
        ));
    }
    prepend_process_path(plan.node_bin.parent().unwrap_or(&plan.install_dir));
    link_unix_node_tools(&plan)?;
    Ok(serde_json::json!({
        "node": plan.node_bin,
        "archive": plan.archive_name,
        "archiveSource": archive_source.kind.as_str(),
        "installDir": plan.install_dir,
    }))
}

/// Build a Windows uv runtime plan from the GitHub release asset matrix.
pub fn windows_uv_runtime_stage_plan(
    hermes_home: &Path,
    arch: &str,
) -> Result<WindowsUvRuntimeStagePlan> {
    let archive_name = windows_uv_archive_name(arch)
        .ok_or_else(|| anyhow!("unsupported Windows architecture for uv: {arch}"))?
        .to_string();
    let download_url =
        format!("https://github.com/astral-sh/uv/releases/latest/download/{archive_name}");
    let archive_path = hermes_home
        .join("bootstrap-cache")
        .join(&archive_name);
    let install_dir = hermes_home.join("bin");
    let uv_exe = install_dir.join("uv.exe");
    Ok(WindowsUvRuntimeStagePlan {
        archive_name,
        download_url,
        archive_path,
        install_dir,
        uv_exe,
    })
}

/// Build a Unix uv runtime plan from the GitHub release asset matrix.
pub fn unix_uv_runtime_stage_plan(
    hermes_home: &Path,
    uv_os: &str,
    arch: &str,
) -> Result<UnixUvRuntimeStagePlan> {
    let archive_name = unix_uv_archive_name(uv_os, arch)
        .ok_or_else(|| anyhow!("unsupported Unix uv platform: {uv_os}-{arch}"))?
        .to_string();
    let download_url =
        format!("https://github.com/astral-sh/uv/releases/latest/download/{archive_name}");
    let archive_path = bootstrap_archive_cache_path(hermes_home, &archive_name);
    let install_dir = hermes_home.join("bin");
    Ok(UnixUvRuntimeStagePlan {
        archive_name,
        download_url,
        archive_path,
        uv_bin: install_dir.join("uv"),
        uvx_bin: install_dir.join("uvx"),
        install_dir,
    })
}

/// Install uv natively on Windows from a bundled or official GitHub release ZIP.
pub async fn install_windows_uv_runtime_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if !cfg!(target_os = "windows") {
        return Err(anyhow!("native uv stage is only available on Windows"));
    }
    let arch = windows_node_arch_slug();
    let plan = windows_uv_runtime_stage_plan(hermes_home, &arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind == BootstrapArchiveSourceKind::Cache {
        let expected_sha256 = archive_source.expected_sha256.as_deref();
        crate::artifact::download_to_cache(
            crate::artifact::DownloadSpec {
                url: plan.download_url.clone(),
                user_agent: "Hermes-Setup",
                expected_sha256,
            },
            &archive_source.path,
        )
        .await
        .with_context(|| format!("downloading {}", plan.archive_name))?;
    }
    install_windows_uv_archive(&archive_source.path, &plan.install_dir)?;
    let status = Command::new(&plan.uv_exe)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("checking {}", plan.uv_exe.display()))?;
    if !status.success() {
        return Err(anyhow!("installed uv failed version check"));
    }
    Ok(serde_json::json!({
        "uv": plan.uv_exe,
        "archive": plan.archive_name,
        "archiveSource": archive_source.kind.as_str(),
    }))
}

/// Install uv natively on Unix from a bundled or official GitHub release tarball.
pub async fn install_unix_uv_runtime_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if cfg!(target_os = "windows") {
        return Err(anyhow!("native Unix uv stage is not available on Windows"));
    }
    if is_termux_environment() {
        return Err(anyhow!("native uv stage is unavailable on Termux"));
    }
    let uv_os = unix_uv_os_slug()?;
    let arch = current_unix_node_arch_slug()?;
    let plan = unix_uv_runtime_stage_plan(hermes_home, &uv_os, &arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind == BootstrapArchiveSourceKind::Cache {
        let expected_sha256 = archive_source.expected_sha256.as_deref();
        crate::artifact::download_to_cache(
            crate::artifact::DownloadSpec {
                url: plan.download_url.clone(),
                user_agent: "Hermes-Setup",
                expected_sha256,
            },
            &archive_source.path,
        )
        .await
        .with_context(|| format!("downloading {}", plan.archive_name))?;
    }
    install_unix_uv_archive(&archive_source.path, &plan.install_dir)?;
    let status = Command::new(&plan.uv_bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("checking {}", plan.uv_bin.display()))?;
    if !status.success() {
        return Err(anyhow!("installed uv failed version check"));
    }
    prepend_process_path(&plan.install_dir);
    Ok(serde_json::json!({
        "uv": plan.uv_bin,
        "archive": plan.archive_name,
        "archiveSource": archive_source.kind.as_str(),
    }))
}

/// Build a Windows Git runtime plan matching install.ps1's pinned release.
pub fn windows_git_runtime_stage_plan(
    hermes_home: &Path,
    arch: &str,
) -> Result<WindowsGitRuntimeStagePlan> {
    let tag = "v2.54.0.windows.1";
    let version = "2.54.0";
    let (archive_name, is_zip, bash_relative) = match arch {
        "arm64" => (
            format!("PortableGit-{version}-arm64.7z.exe"),
            false,
            PathBuf::from("bin").join("bash.exe"),
        ),
        "x64" => (
            format!("PortableGit-{version}-64-bit.7z.exe"),
            false,
            PathBuf::from("bin").join("bash.exe"),
        ),
        "x86" => (
            format!("MinGit-{version}-32-bit.zip"),
            true,
            PathBuf::from("usr").join("bin").join("bash.exe"),
        ),
        other => return Err(anyhow!("unsupported Windows architecture for Git: {other}")),
    };
    let download_url =
        format!("https://github.com/git-for-windows/git/releases/download/{tag}/{archive_name}");
    let archive_path = hermes_home
        .join("bootstrap-cache")
        .join(&archive_name);
    let install_dir = hermes_home.join("git");
    let git_exe = install_dir.join("cmd").join("git.exe");
    let bash_exe = install_dir.join(bash_relative);
    Ok(WindowsGitRuntimeStagePlan {
        tag,
        version,
        archive_name,
        download_url,
        archive_path,
        install_dir,
        git_exe,
        bash_exe,
        is_zip,
    })
}

fn unix_git_install_command_plan(
    target_os: &str,
    distro: &str,
    user_is_root: bool,
    sudo_available: bool,
    brew_available: bool,
) -> Result<Vec<UnixGitInstallCommandPlan>> {
    if target_os == "android" || distro_matches(distro, &["termux"]) {
        return Ok(vec![unix_git_command("pkg", ["install", "-y", "git"])]);
    }
    if target_os == "macos" {
        if brew_available {
            return Ok(vec![unix_git_command("brew", ["install", "git"])]);
        }
        return Err(anyhow!("Homebrew is not available for native Git install"));
    }
    if target_os != "linux" {
        return Err(anyhow!("unsupported Unix Git install target: {target_os}"));
    }
    if distro_matches(distro, &["ubuntu", "debian"]) {
        return Ok(vec![
            unix_git_apt_command(user_is_root, sudo_available, ["update", "-qq"])?,
            unix_git_apt_command(
                user_is_root,
                sudo_available,
                ["install", "-y", "-qq", "git"],
            )?,
        ]);
    }
    if distro_matches(distro, &["fedora", "rhel", "centos", "rocky", "alma"]) {
        return Ok(vec![unix_git_privileged_command(
            user_is_root,
            sudo_available,
            "dnf",
            ["install", "-y", "git"],
        )?]);
    }
    if distro_matches(distro, &["arch", "manjaro", "cachyos", "endeavouros", "garuda"]) {
        return Ok(vec![unix_git_privileged_command(
            user_is_root,
            sudo_available,
            "pacman",
            ["-S", "--noconfirm", "git"],
        )?]);
    }
    Err(anyhow!("unsupported Linux Git install distro: {distro}"))
}

fn unix_git_apt_command<const N: usize>(
    user_is_root: bool,
    sudo_available: bool,
    args: [&str; N],
) -> Result<UnixGitInstallCommandPlan> {
    if user_is_root {
        return Ok(unix_git_command("apt-get", args));
    }
    if sudo_available {
        let mut sudo_args = vec![
            "env".to_string(),
            "DEBIAN_FRONTEND=noninteractive".to_string(),
            "apt-get".to_string(),
        ];
        sudo_args.extend(args.into_iter().map(str::to_string));
        return Ok(UnixGitInstallCommandPlan {
            program: "sudo".to_string(),
            args: sudo_args,
        });
    }
    Err(anyhow!("sudo is required for native apt Git install"))
}

fn unix_git_privileged_command<const N: usize>(
    user_is_root: bool,
    sudo_available: bool,
    program: &str,
    args: [&str; N],
) -> Result<UnixGitInstallCommandPlan> {
    if user_is_root {
        return Ok(unix_git_command(program, args));
    }
    if sudo_available {
        let mut sudo_args = vec![program.to_string()];
        sudo_args.extend(args.into_iter().map(str::to_string));
        return Ok(UnixGitInstallCommandPlan {
            program: "sudo".to_string(),
            args: sudo_args,
        });
    }
    Err(anyhow!("sudo is required for native Git install"))
}

fn unix_git_command<const N: usize>(program: &str, args: [&str; N]) -> UnixGitInstallCommandPlan {
    UnixGitInstallCommandPlan {
        program: program.to_string(),
        args: args.into_iter().map(str::to_string).collect(),
    }
}

fn unix_system_package_install_command_plan(
    target_os: &str,
    distro: &str,
    packages: &[&str],
    user_is_root: bool,
    sudo_available: bool,
    brew_available: bool,
) -> Result<Vec<UnixPackageInstallCommandPlan>> {
    if packages.is_empty() {
        return Err(anyhow!("at least one Unix package is required"));
    }
    if target_os == "android" || distro_matches(distro, &["termux"]) {
        return Ok(vec![unix_package_command_with_packages(
            "pkg",
            &["install", "-y"],
            packages,
        )]);
    }
    if target_os == "macos" {
        if brew_available {
            return Ok(vec![unix_package_command_with_packages(
                "brew",
                &["install"],
                packages,
            )]);
        }
        return Err(anyhow!(
            "Homebrew is not available for native Unix package install"
        ));
    }
    if target_os != "linux" {
        return Err(anyhow!("unsupported Unix package install target: {target_os}"));
    }
    if distro_matches(distro, &["ubuntu", "debian"]) {
        return Ok(vec![unix_apt_package_install_command(
            user_is_root,
            sudo_available,
            packages,
        )?]);
    }
    if distro_matches(distro, &["fedora", "rhel", "centos", "rocky", "alma"]) {
        return Ok(vec![unix_privileged_package_install_command(
            user_is_root,
            sudo_available,
            "dnf",
            &["install", "-y"],
            packages,
        )?]);
    }
    if distro_matches(distro, &["arch", "manjaro", "cachyos", "endeavouros", "garuda"]) {
        return Ok(vec![unix_privileged_package_install_command(
            user_is_root,
            sudo_available,
            "pacman",
            &["-S", "--noconfirm"],
            packages,
        )?]);
    }
    Err(anyhow!("unsupported Linux package install distro: {distro}"))
}

fn unix_apt_package_install_command(
    user_is_root: bool,
    sudo_available: bool,
    packages: &[&str],
) -> Result<UnixPackageInstallCommandPlan> {
    if user_is_root {
        return Ok(unix_package_command_with_packages(
            "apt-get",
            &["install", "-y", "-qq"],
            packages,
        ));
    }
    if sudo_available {
        let mut args = vec![
            "env".to_string(),
            "DEBIAN_FRONTEND=noninteractive".to_string(),
            "NEEDRESTART_MODE=a".to_string(),
            "apt-get".to_string(),
            "install".to_string(),
            "-y".to_string(),
            "-qq".to_string(),
        ];
        args.extend(packages.iter().map(|package| (*package).to_string()));
        return Ok(UnixPackageInstallCommandPlan {
            program: "sudo".to_string(),
            args,
        });
    }
    Err(anyhow!("sudo is required for native apt package install"))
}

fn unix_privileged_package_install_command(
    user_is_root: bool,
    sudo_available: bool,
    program: &str,
    prefix_args: &[&str],
    packages: &[&str],
) -> Result<UnixPackageInstallCommandPlan> {
    if user_is_root {
        return Ok(unix_package_command_with_packages(program, prefix_args, packages));
    }
    if sudo_available {
        let mut args = vec![program.to_string()];
        args.extend(prefix_args.iter().map(|arg| (*arg).to_string()));
        args.extend(packages.iter().map(|package| (*package).to_string()));
        return Ok(UnixPackageInstallCommandPlan {
            program: "sudo".to_string(),
            args,
        });
    }
    Err(anyhow!("sudo is required for native Unix package install"))
}

fn unix_package_command_with_packages(
    program: &str,
    prefix_args: &[&str],
    packages: &[&str],
) -> UnixPackageInstallCommandPlan {
    let mut args = prefix_args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    args.extend(packages.iter().map(|package| (*package).to_string()));
    UnixPackageInstallCommandPlan {
        program: program.to_string(),
        args,
    }
}

fn windows_system_package_install_command_plan(
    packages: &[&str],
    winget_available: bool,
    choco_available: bool,
    scoop_available: bool,
    local_app_data: Option<&Path>,
) -> Result<Vec<WindowsPackageInstallCommandPlan>> {
    if packages.is_empty() {
        return Err(anyhow!("at least one Windows package is required"));
    }

    let mut commands = Vec::new();
    if winget_available {
        for package in packages {
            let package_id = windows_winget_package_id(package)?;
            commands.push(WindowsPackageInstallCommandPlan {
                program: "winget".to_string(),
                args: vec![
                    "install".to_string(),
                    "--exact".to_string(),
                    "--id".to_string(),
                    package_id.to_string(),
                    "--source".to_string(),
                    "winget".to_string(),
                    "--silent".to_string(),
                    "--accept-package-agreements".to_string(),
                    "--accept-source-agreements".to_string(),
                ],
                path_after_install: local_app_data
                    .map(|path| path.join("Microsoft").join("WinGet").join("Links")),
            });
        }
    }
    if choco_available {
        for package in packages {
            commands.push(WindowsPackageInstallCommandPlan {
                program: "choco".to_string(),
                args: vec!["install".to_string(), (*package).to_string(), "-y".to_string()],
                path_after_install: None,
            });
        }
    }
    if scoop_available {
        for package in packages {
            commands.push(WindowsPackageInstallCommandPlan {
                program: "scoop".to_string(),
                args: vec!["install".to_string(), (*package).to_string()],
                path_after_install: None,
            });
        }
    }
    if commands.is_empty() {
        return Err(anyhow!("no Windows package manager is available"));
    }
    Ok(commands)
}

fn windows_winget_package_id(package: &str) -> Result<&'static str> {
    match package {
        "ffmpeg" => Ok("Gyan.FFmpeg"),
        "ripgrep" => Ok("BurntSushi.ripgrep.MSVC"),
        other => Err(anyhow!("unsupported Windows winget package: {other}")),
    }
}

/// Build a Windows ripgrep runtime plan matching the pinned release asset.
pub fn windows_ripgrep_runtime_stage_plan(
    hermes_home: &Path,
    arch: &str,
) -> Result<WindowsRipgrepRuntimeStagePlan> {
    let version = "15.1.0";
    let target = match arch {
        "arm64" => "aarch64-pc-windows-msvc",
        "x64" => "x86_64-pc-windows-msvc",
        "x86" => "i686-pc-windows-msvc",
        other => return Err(anyhow!("unsupported Windows architecture for ripgrep: {other}")),
    };
    let archive_name = format!("ripgrep-{version}-{target}.zip");
    let download_url =
        format!("https://github.com/BurntSushi/ripgrep/releases/download/{version}/{archive_name}");
    let archive_path = hermes_home
        .join("bootstrap-cache")
        .join(&archive_name);
    let install_dir = hermes_home.join("bin");
    let rg_exe = install_dir.join("rg.exe");
    Ok(WindowsRipgrepRuntimeStagePlan {
        version,
        archive_name,
        download_url,
        archive_path,
        install_dir,
        rg_exe,
    })
}

/// Build a Unix ripgrep runtime plan matching pinned release assets.
pub fn unix_ripgrep_runtime_stage_plan(
    hermes_home: &Path,
    target_os: &str,
    arch: &str,
) -> Result<UnixRipgrepRuntimeStagePlan> {
    let version = "15.1.0";
    let target = match (target_os, arch) {
        ("linux", "arm64") => "aarch64-unknown-linux-gnu",
        ("linux", "armv7l") => "armv7-unknown-linux-gnueabihf",
        ("linux", "x64") => "x86_64-unknown-linux-musl",
        ("macos", "arm64") => "aarch64-apple-darwin",
        ("macos", "x64") => "x86_64-apple-darwin",
        (os, arch) => return Err(anyhow!("unsupported Unix ripgrep target: {os}-{arch}")),
    };
    let archive_name = format!("ripgrep-{version}-{target}.tar.gz");
    let download_url =
        format!("https://github.com/BurntSushi/ripgrep/releases/download/{version}/{archive_name}");
    let archive_path = hermes_home
        .join("bootstrap-cache")
        .join(&archive_name);
    let install_dir = hermes_home.join("bin");
    let rg_bin = install_dir.join("rg");
    Ok(UnixRipgrepRuntimeStagePlan {
        version,
        archive_name,
        download_url,
        archive_path,
        install_dir,
        rg_bin,
    })
}

/// Build a Windows ffmpeg runtime plan for a bundled release archive.
pub fn windows_ffmpeg_runtime_stage_plan(
    hermes_home: &Path,
    arch: &str,
) -> Result<WindowsFfmpegRuntimeStagePlan> {
    match arch {
        "arm64" | "x64" | "x86" => {}
        other => return Err(anyhow!("unsupported Windows architecture for ffmpeg: {other}")),
    }
    let archive_name = format!("ffmpeg-windows-{arch}.zip");
    let install_dir = hermes_home.join("bin");
    let ffmpeg_exe = install_dir.join("ffmpeg.exe");
    Ok(WindowsFfmpegRuntimeStagePlan {
        archive_name,
        install_dir,
        ffmpeg_exe,
    })
}

/// Build a Unix ffmpeg runtime plan for a bundled release archive.
pub fn unix_ffmpeg_runtime_stage_plan(
    hermes_home: &Path,
    target_os: &str,
    arch: &str,
) -> Result<UnixFfmpegRuntimeStagePlan> {
    let platform = match target_os {
        "darwin" | "macos" => "macos",
        "linux" => "linux",
        other => return Err(anyhow!("unsupported Unix ffmpeg platform: {other}")),
    };
    match arch {
        "arm64" | "x64" => {}
        other => return Err(anyhow!("unsupported Unix architecture for ffmpeg: {other}")),
    }
    let archive_name = format!("ffmpeg-{platform}-{arch}.tar.gz");
    let install_dir = hermes_home.join("bin");
    let ffmpeg_bin = install_dir.join("ffmpeg");
    Ok(UnixFfmpegRuntimeStagePlan {
        archive_name,
        install_dir,
        ffmpeg_bin,
    })
}

/// Build a Playwright browser cache plan for a bundled release archive.
pub fn playwright_browsers_runtime_stage_plan(
    hermes_home: &Path,
    target_os: &str,
    arch: &str,
) -> Result<PlaywrightBrowsersRuntimeStagePlan> {
    let platform = match target_os {
        "windows" => "windows",
        "darwin" | "macos" => "macos",
        "linux" => "linux",
        other => return Err(anyhow!("unsupported Playwright browser platform: {other}")),
    };
    match (platform, arch) {
        ("windows", "arm64" | "x64" | "x86")
        | ("linux", "arm64" | "x64")
        | ("macos", "arm64" | "x64") => {}
        (_, other) => {
            return Err(anyhow!(
                "unsupported Playwright browser architecture for {platform}: {other}"
            ));
        }
    }
    let extension = if platform == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(PlaywrightBrowsersRuntimeStagePlan {
        archive_name: format!("playwright-browsers-{platform}-{arch}.{extension}"),
        install_dir: hermes_home.join("playwright-browsers"),
    })
}

fn install_bundled_playwright_browsers_if_available(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
    target_os: &str,
    arch: &str,
) -> Result<Option<(String, BootstrapArchiveSourceKind)>> {
    let plan = playwright_browsers_runtime_stage_plan(hermes_home, target_os, arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind != BootstrapArchiveSourceKind::Bundled {
        return Ok(None);
    }
    extract_playwright_browsers_archive(&archive_source.path, &plan.install_dir)?;
    Ok(Some((plan.archive_name, archive_source.kind)))
}

/// Build an Electron cache plan for a bundled release archive.
pub fn electron_cache_runtime_stage_plan(
    hermes_home: &Path,
    target_os: &str,
    arch: &str,
) -> Result<ElectronCacheRuntimeStagePlan> {
    let platform = match target_os {
        "windows" => "windows",
        "darwin" | "macos" => "macos",
        "linux" => "linux",
        other => return Err(anyhow!("unsupported Electron cache platform: {other}")),
    };
    match (platform, arch) {
        ("windows", "arm64" | "x64" | "x86")
        | ("linux", "arm64" | "x64")
        | ("macos", "arm64" | "x64") => {}
        (_, other) => {
            return Err(anyhow!(
                "unsupported Electron cache architecture for {platform}: {other}"
            ));
        }
    }
    let extension = if platform == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(ElectronCacheRuntimeStagePlan {
        archive_name: format!("electron-cache-{platform}-{arch}.{extension}"),
        install_dir: hermes_home.join("electron-cache"),
    })
}

fn install_bundled_electron_cache_if_available(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
    target_os: &str,
    arch: &str,
) -> Result<Option<(String, BootstrapArchiveSourceKind)>> {
    let plan = electron_cache_runtime_stage_plan(hermes_home, target_os, arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind != BootstrapArchiveSourceKind::Bundled {
        return Ok(None);
    }
    extract_electron_cache_archive(&archive_source.path, &plan.install_dir)?;
    Ok(Some((plan.archive_name, archive_source.kind)))
}

/// Build an npm cache plan for a bundled release archive.
pub fn npm_cache_runtime_stage_plan(
    hermes_home: &Path,
    target_os: &str,
    arch: &str,
) -> Result<NpmCacheRuntimeStagePlan> {
    let platform = match target_os {
        "windows" => "windows",
        "darwin" | "macos" => "macos",
        "linux" => "linux",
        other => return Err(anyhow!("unsupported npm cache platform: {other}")),
    };
    match (platform, arch) {
        ("windows", "arm64" | "x64" | "x86")
        | ("linux", "arm64" | "x64")
        | ("macos", "arm64" | "x64") => {}
        (_, other) => {
            return Err(anyhow!(
                "unsupported npm cache architecture for {platform}: {other}"
            ));
        }
    }
    let extension = if platform == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(NpmCacheRuntimeStagePlan {
        archive_name: format!("npm-cache-{platform}-{arch}.{extension}"),
        install_dir: hermes_home.join("npm-cache"),
    })
}

fn install_bundled_npm_cache_if_available(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
    target_os: &str,
    arch: &str,
) -> Result<Option<(String, BootstrapArchiveSourceKind)>> {
    let plan = npm_cache_runtime_stage_plan(hermes_home, target_os, arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind != BootstrapArchiveSourceKind::Bundled {
        return Ok(None);
    }
    extract_npm_cache_archive(&archive_source.path, &plan.install_dir)?;
    Ok(Some((plan.archive_name, archive_source.kind)))
}

/// Install Windows ripgrep natively and prefer bundled ffmpeg before package-manager recovery.
pub async fn install_windows_system_packages_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if !cfg!(target_os = "windows") {
        return Err(anyhow!(
            "native Windows system package stage is only available on Windows"
        ));
    }

    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let managed_rg = managed_tool_path(hermes_home, "rg");
    let rg_before = find_executable_on_path("rg", &path_env, &pathext)
        .or_else(|| managed_rg.is_file().then_some(managed_rg.clone()));
    let mut archive_name = None;
    let mut archive_source_kind = None;

    if rg_before.is_none() {
        let arch = windows_node_arch_slug();
        let plan = windows_ripgrep_runtime_stage_plan(hermes_home, &arch)?;
        let archive_source =
            resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
        if archive_source.kind == BootstrapArchiveSourceKind::Cache {
            let expected_sha256 = archive_source.expected_sha256.as_deref();
            crate::artifact::download_to_cache(
                crate::artifact::DownloadSpec {
                    url: plan.download_url.clone(),
                    user_agent: "Hermes-Setup",
                    expected_sha256,
                },
                &archive_source.path,
            )
            .await
            .with_context(|| format!("downloading {}", plan.archive_name))?;
        }

        install_windows_ripgrep_archive(&archive_source.path, &plan.install_dir)?;
        if !plan.rg_exe.is_file() {
            return Err(anyhow!(
                "ripgrep extraction did not produce {}",
                plan.rg_exe.display()
            ));
        }
        let status = Command::new(&plan.rg_exe)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("checking {}", plan.rg_exe.display()))?;
        if !status.success() {
            return Err(anyhow!("installed ripgrep failed version check"));
        }
        prepend_process_path(&plan.install_dir);
        persist_windows_path_entries(&[plan.install_dir.clone()])?;
        archive_name = Some(plan.archive_name);
        archive_source_kind = Some(archive_source.kind.as_str().to_string());
    } else if rg_before.as_deref() == Some(managed_rg.as_path()) {
        let bin_dir = hermes_home.join("bin");
        prepend_process_path(&bin_dir);
        persist_windows_path_entries(&[bin_dir])?;
    }

    let mut refreshed_path = std::env::var_os("PATH").unwrap_or_default();
    let managed_ffmpeg = managed_tool_path(hermes_home, "ffmpeg");
    let mut ffmpeg = find_executable_on_path("ffmpeg", &refreshed_path, &pathext)
        .or_else(|| managed_ffmpeg.is_file().then_some(managed_ffmpeg.clone()));
    let mut ffmpeg_commands = Vec::new();
    let mut ffmpeg_archive_name = None;
    let mut ffmpeg_archive_source_kind = None;
    if ffmpeg.is_none() {
        let arch = windows_node_arch_slug();
        let plan = windows_ffmpeg_runtime_stage_plan(hermes_home, &arch)?;
        let archive_source =
            resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
        if archive_source.kind == BootstrapArchiveSourceKind::Bundled {
            install_windows_ffmpeg_archive(&archive_source.path, &plan.install_dir)?;
            if !plan.ffmpeg_exe.is_file() {
                return Err(anyhow!(
                    "ffmpeg extraction did not produce {}",
                    plan.ffmpeg_exe.display()
                ));
            }
            prepend_process_path(&plan.install_dir);
            persist_windows_path_entries(&[plan.install_dir.clone()])?;
            refreshed_path = std::env::var_os("PATH").unwrap_or_default();
            ffmpeg = Some(plan.ffmpeg_exe);
            ffmpeg_archive_name = Some(plan.archive_name);
            ffmpeg_archive_source_kind = Some(archive_source.kind.as_str().to_string());
        }
    } else if ffmpeg.as_deref() == Some(managed_ffmpeg.as_path()) {
        let bin_dir = hermes_home.join("bin");
        prepend_process_path(&bin_dir);
        persist_windows_path_entries(&[bin_dir])?;
        refreshed_path = std::env::var_os("PATH").unwrap_or_default();
    }
    if ffmpeg.is_none() {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let commands = windows_system_package_install_command_plan(
            &["ffmpeg"],
            find_executable_on_path("winget", &refreshed_path, &pathext).is_some(),
            find_executable_on_path("choco", &refreshed_path, &pathext).is_some(),
            find_executable_on_path("scoop", &refreshed_path, &pathext).is_some(),
            local_app_data.as_deref(),
        )?;
        let mut errors = Vec::new();
        for command in &commands {
            match run_windows_system_package_install_command(command) {
                Ok(()) => {
                    if let Some(path) = &command.path_after_install {
                        if path.is_dir() {
                            prepend_process_path(path);
                        }
                    }
                    refreshed_path = std::env::var_os("PATH").unwrap_or_default();
                    ffmpeg = find_executable_on_path("ffmpeg", &refreshed_path, &pathext);
                    if ffmpeg.is_some() {
                        break;
                    }
                    errors.push(format!(
                        "{} completed but ffmpeg is still unavailable",
                        windows_package_command_display(command)
                    ));
                }
                Err(err) => errors.push(err.to_string()),
            }
        }
        ffmpeg_commands = commands
            .iter()
            .map(windows_package_command_display)
            .collect::<Vec<_>>();
        if ffmpeg.is_none() {
            return Err(anyhow!(
                "ffmpeg is not available after native Windows package recovery: {}",
                errors.join("; ")
            ));
        }
    }

    Ok(serde_json::json!({
        "ripgrep": find_executable_on_path("rg", &refreshed_path, &pathext),
        "ffmpeg": ffmpeg,
        "ffmpegCommands": ffmpeg_commands,
        "ffmpegArchive": ffmpeg_archive_name,
        "ffmpegArchiveSource": ffmpeg_archive_source_kind,
        "archive": archive_name,
        "archiveSource": archive_source_kind,
    }))
}

/// Install Unix ripgrep natively and prefer bundled ffmpeg before package-manager recovery.
pub async fn install_unix_system_packages_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if cfg!(target_os = "windows") {
        return Err(anyhow!(
            "native Unix system package stage is not available on Windows"
        ));
    }

    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let managed_rg = managed_tool_path(hermes_home, "rg");
    let rg_before =
        find_executable_on_path("rg", &path_env, "").or_else(|| managed_rg.is_file().then_some(managed_rg.clone()));
    let mut archive_name = None;
    let mut archive_source_kind = None;

    if rg_before.is_none() {
        let arch = current_unix_node_arch_slug()?;
        let plan = unix_ripgrep_runtime_stage_plan(hermes_home, std::env::consts::OS, &arch)?;
        let archive_source =
            resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
        if archive_source.kind == BootstrapArchiveSourceKind::Cache {
            let expected_sha256 = archive_source.expected_sha256.as_deref();
            crate::artifact::download_to_cache(
                crate::artifact::DownloadSpec {
                    url: plan.download_url.clone(),
                    user_agent: "Hermes-Setup",
                    expected_sha256,
                },
                &archive_source.path,
            )
            .await
            .with_context(|| format!("downloading {}", plan.archive_name))?;
        }

        install_unix_ripgrep_archive(&archive_source.path, &plan.install_dir)?;
        if !plan.rg_bin.is_file() {
            return Err(anyhow!(
                "ripgrep extraction did not produce {}",
                plan.rg_bin.display()
            ));
        }
        let status = Command::new(&plan.rg_bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("checking {}", plan.rg_bin.display()))?;
        if !status.success() {
            return Err(anyhow!("installed ripgrep failed version check"));
        }
        prepend_process_path(&plan.install_dir);
        archive_name = Some(plan.archive_name);
        archive_source_kind = Some(archive_source.kind.as_str().to_string());
    } else if rg_before.as_deref() == Some(managed_rg.as_path()) {
        prepend_process_path(&hermes_home.join("bin"));
    }

    let mut refreshed_path = std::env::var_os("PATH").unwrap_or_default();
    let managed_ffmpeg = managed_tool_path(hermes_home, "ffmpeg");
    let mut ffmpeg = find_executable_on_path("ffmpeg", &refreshed_path, "")
        .or_else(|| managed_ffmpeg.is_file().then_some(managed_ffmpeg.clone()));
    let mut ffmpeg_commands = Vec::new();
    let mut ffmpeg_archive_name = None;
    let mut ffmpeg_archive_source_kind = None;
    if ffmpeg.is_none() {
        let arch = current_unix_node_arch_slug()?;
        let plan = unix_ffmpeg_runtime_stage_plan(hermes_home, std::env::consts::OS, &arch)?;
        let archive_source =
            resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
        if archive_source.kind == BootstrapArchiveSourceKind::Bundled {
            extract_unix_ffmpeg_tar_gz(&archive_source.path, &plan.install_dir)?;
            if !plan.ffmpeg_bin.is_file() {
                return Err(anyhow!(
                    "ffmpeg extraction did not produce {}",
                    plan.ffmpeg_bin.display()
                ));
            }
            prepend_process_path(&plan.install_dir);
            refreshed_path = std::env::var_os("PATH").unwrap_or_default();
            ffmpeg = Some(plan.ffmpeg_bin);
            ffmpeg_archive_name = Some(plan.archive_name);
            ffmpeg_archive_source_kind = Some(archive_source.kind.as_str().to_string());
        }
    } else if ffmpeg.as_deref() == Some(managed_ffmpeg.as_path()) {
        prepend_process_path(&hermes_home.join("bin"));
        refreshed_path = std::env::var_os("PATH").unwrap_or_default();
    }
    if ffmpeg.is_none() {
        let termux = is_termux_environment();
        let target_os = if termux { "android" } else { std::env::consts::OS };
        let distro = if termux {
            "termux".to_string()
        } else if target_os == "linux" {
            current_linux_distro_family()
        } else {
            String::new()
        };
        let commands = unix_system_package_install_command_plan(
            target_os,
            &distro,
            &["ffmpeg"],
            current_process_is_root(),
            noninteractive_sudo_available(&refreshed_path),
            find_executable_on_path("brew", &refreshed_path, "").is_some(),
        )?;
        for command in &commands {
            run_unix_system_package_install_command(command)?;
        }
        refreshed_path = std::env::var_os("PATH").unwrap_or_default();
        ffmpeg = find_executable_on_path("ffmpeg", &refreshed_path, "");
        if ffmpeg.is_none() {
            return Err(anyhow!(
                "ffmpeg install command completed but ffmpeg is still unavailable"
            ));
        }
        ffmpeg_commands = commands
            .iter()
            .map(unix_package_command_display)
            .collect::<Vec<_>>();
    }

    Ok(serde_json::json!({
        "ripgrep": find_executable_on_path("rg", &refreshed_path, ""),
        "ffmpeg": ffmpeg,
        "ffmpegCommands": ffmpeg_commands,
        "ffmpegArchive": ffmpeg_archive_name,
        "ffmpegArchiveSource": ffmpeg_archive_source_kind,
        "archive": archive_name,
        "archiveSource": archive_source_kind,
    }))
}

/// Install Git for Windows natively before falling back to PowerShell.
pub async fn install_windows_git_runtime_stage(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if !cfg!(target_os = "windows") {
        return Err(anyhow!("native Git stage is only available on Windows"));
    }
    let arch = windows_node_arch_slug();
    let plan = windows_git_runtime_stage_plan(hermes_home, &arch)?;
    let archive_source =
        resolve_bootstrap_archive_source(hermes_home, bundled_tools_dir, &plan.archive_name);
    if archive_source.kind == BootstrapArchiveSourceKind::Cache {
        let expected_sha256 = archive_source.expected_sha256.as_deref();
        crate::artifact::download_to_cache(
            crate::artifact::DownloadSpec {
                url: plan.download_url.clone(),
                user_agent: "Hermes-Setup",
                expected_sha256,
            },
            &archive_source.path,
        )
        .await
        .with_context(|| format!("downloading {}", plan.archive_name))?;
    }
    let install_plan = WindowsGitRuntimeStagePlan {
        archive_path: archive_source.path.clone(),
        ..plan
    };
    install_windows_git_archive(&install_plan)?;
    if !install_plan.git_exe.is_file() {
        return Err(anyhow!(
            "Git extraction did not produce {}",
            install_plan.git_exe.display()
        ));
    }
    let path_entries = [
        install_plan.install_dir.join("cmd"),
        install_plan.install_dir.join("bin"),
        install_plan.install_dir.join("usr").join("bin"),
    ];
    prepend_process_paths(&path_entries);
    persist_windows_path_entries(&path_entries)?;
    let bash = find_windows_git_bash(&install_plan.install_dir);
    if let Some(bash) = &bash {
        persist_windows_env_var("HERMES_GIT_BASH_PATH", &bash.display().to_string())?;
        std::env::set_var("HERMES_GIT_BASH_PATH", bash);
    }
    Ok(serde_json::json!({
        "git": install_plan.git_exe,
        "bash": bash,
        "archive": install_plan.archive_name,
        "archiveSource": archive_source.kind.as_str(),
    }))
}

/// Install or verify Git natively on Unix before falling back to the shell script.
pub async fn install_unix_git_runtime_stage() -> Result<serde_json::Value> {
    if cfg!(target_os = "windows") {
        return Err(anyhow!("native Unix Git stage is not available on Windows"));
    }
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    if let Some(git) = usable_git_on_path(&path_env) {
        return Ok(serde_json::json!({
            "git": git,
            "skipped": true,
            "reason": "Git already available",
        }));
    }

    let termux = is_termux_environment();
    let target_os = if termux { "android" } else { std::env::consts::OS };
    let distro = if termux {
        "termux".to_string()
    } else if target_os == "linux" {
        current_linux_distro_family()
    } else {
        String::new()
    };
    let commands = unix_git_install_command_plan(
        target_os,
        &distro,
        current_process_is_root(),
        noninteractive_sudo_available(&path_env),
        find_executable_on_path("brew", &path_env, "").is_some(),
    )?;
    for command in &commands {
        run_unix_git_install_command(command)?;
    }

    let refreshed_path = std::env::var_os("PATH").unwrap_or_default();
    let git = usable_git_on_path(&refreshed_path)
        .ok_or_else(|| anyhow!("Git install command completed but git is still unavailable"))?;
    Ok(serde_json::json!({
        "git": git,
        "commands": commands
            .iter()
            .map(unix_git_command_display)
            .collect::<Vec<_>>(),
    }))
}

fn usable_git_on_path<P>(path_env: P) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let git = find_executable_on_path("git", path_env, "")?;
    let status = Command::new(&git)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(git)
}

fn run_unix_git_install_command(command: &UnixGitInstallCommandPlan) -> Result<()> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", unix_git_command_display(command)))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(process_failure_message(
        &unix_git_command_display(command),
        &output,
    )))
}

fn process_failure_message(display: &str, output: &Output) -> String {
    let mut message = format!(
        "{display} failed with exit {:?}",
        output.status.code()
    );
    let output_text = process_output_text(output);
    if !output_text.trim().is_empty() {
        message.push_str("; process output: ");
        message.push_str(output_text.trim());
    }
    message
}

fn unix_git_command_display(command: &UnixGitInstallCommandPlan) -> String {
    if command.args.is_empty() {
        return command.program.clone();
    }
    format!("{} {}", command.program, command.args.join(" "))
}

fn run_unix_system_package_install_command(command: &UnixPackageInstallCommandPlan) -> Result<()> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", unix_package_command_display(command)))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(process_failure_message(
        &unix_package_command_display(command),
        &output,
    )))
}

fn unix_package_command_display(command: &UnixPackageInstallCommandPlan) -> String {
    if command.args.is_empty() {
        return command.program.clone();
    }
    format!("{} {}", command.program, command.args.join(" "))
}

fn run_windows_system_package_install_command(command: &WindowsPackageInstallCommandPlan) -> Result<()> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", windows_package_command_display(command)))?;
    if output.status.success() {
        return Ok(());
    }
    Err(anyhow!(process_failure_message(
        &windows_package_command_display(command),
        &output,
    )))
}

fn windows_package_command_display(command: &WindowsPackageInstallCommandPlan) -> String {
    if command.args.is_empty() {
        return command.program.clone();
    }
    format!("{} {}", command.program, command.args.join(" "))
}

fn noninteractive_sudo_available<P>(path_env: P) -> bool
where
    P: AsRef<OsStr>,
{
    let Some(sudo) = find_executable_on_path("sudo", path_env, "") else {
        return false;
    };
    Command::new(sudo)
        .args(["-n", "true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn current_linux_distro_family() -> String {
    let Some(text) = std::fs::read_to_string("/etc/os-release").ok() else {
        return String::new();
    };
    linux_distro_ids_from_os_release(&text).join(" ")
}

fn linux_distro_ids_from_os_release(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            push_linux_distro_ids(&mut ids, value);
        } else if let Some(value) = line.strip_prefix("ID_LIKE=") {
            push_linux_distro_ids(&mut ids, value);
        }
    }
    ids
}

fn push_linux_distro_ids(ids: &mut Vec<String>, value: &str) {
    let cleaned = value.trim().trim_matches('"').trim_matches('\'');
    for id in cleaned.split_whitespace() {
        let normalized = id.to_ascii_lowercase();
        if !normalized.is_empty() && !ids.contains(&normalized) {
            ids.push(normalized);
        }
    }
}

fn distro_matches(distro: &str, supported: &[&str]) -> bool {
    distro
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .any(|candidate| {
            let candidate = candidate.to_ascii_lowercase();
            supported.iter().any(|item| candidate == *item)
        })
}

fn distro_any_starts_with(distro: &str, prefix: &str) -> bool {
    distro
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .map(str::trim)
        .any(|candidate| candidate.to_ascii_lowercase().starts_with(prefix))
}

/// Build the native Python virtual environment stage plan.
pub fn python_venv_stage_plan<P>(
    install_root: &Path,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Result<PythonVenvStagePlan>
where
    P: AsRef<OsStr>,
{
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }
    let uv = uv_tool_path(hermes_home, path_env, pathext)?;
    let venv = install_root.join("venv");
    let dirs = python_runtime_dirs_for_layout(hermes_home, install_root);
    Ok(PythonVenvStagePlan {
        uv,
        cwd: install_root.to_path_buf(),
        venv,
        uv_cache_dir: hermes_home.join("uv-cache"),
        python_install_dir: dirs.install_dir,
        python_bin_dir: dirs.bin_dir,
    })
}

/// Create the Python virtual environment natively through uv.
pub fn create_python_venv_stage(
    install_root: &Path,
    hermes_home: &Path,
) -> Result<serde_json::Value> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let plan = python_venv_stage_plan(install_root, hermes_home, path_env, &pathext)?;
    if plan.venv.exists() {
        fs::remove_dir_all(&plan.venv)
            .with_context(|| format!("removing existing venv {}", plan.venv.display()))?;
    }
    let status = Command::new(&plan.uv)
        .args(["venv", "venv", "--python", "3.11"])
        .current_dir(&plan.cwd)
        .env("UV_CACHE_DIR", &plan.uv_cache_dir)
        .env("UV_PYTHON_INSTALL_DIR", &plan.python_install_dir)
        .env("UV_PYTHON_BIN_DIR", &plan.python_bin_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {}", plan.uv.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "uv venv failed with exit {:?}",
            status.code()
        ));
    }
    let python = venv_python_path(&plan.venv);
    if !python.is_file() {
        return Err(anyhow!(
            "uv venv completed but Python was not created at {}",
            python.display()
        ));
    }
    Ok(serde_json::json!({
        "uv": plan.uv,
        "venv": plan.venv,
        "python": python,
    }))
}

/// Build the native Python dependencies stage plan.
pub fn python_dependencies_stage_plan<P>(
    install_root: &Path,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Result<PythonDependenciesStagePlan>
where
    P: AsRef<OsStr>,
{
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }
    let lockfile = install_root.join("uv.lock");
    if !lockfile.is_file() {
        return Err(anyhow!("uv.lock not found at {}", lockfile.display()));
    }
    let uv = uv_tool_path(hermes_home, path_env, pathext)?;
    let venv = install_root.join("venv");
    let python = venv_python_path(&venv);
    Ok(PythonDependenciesStagePlan {
        uv,
        cwd: install_root.to_path_buf(),
        venv,
        python,
        lockfile,
        uv_cache_dir: hermes_home.join("uv-cache"),
    })
}

/// Install Python dependencies through the hash-verified uv.lock path.
pub fn sync_python_dependencies_stage(
    install_root: &Path,
    hermes_home: &Path,
    wheelhouse_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let plan = python_dependencies_stage_plan(install_root, hermes_home, path_env, &pathext)?;
    let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&plan.cwd, wheelhouse_dir);
    let mut selected_tier = None;
    let mut last_exit_code = None;
    for tier in &tiers {
        let status = run_python_dependency_install_tier(&plan, tier)?;
        if status.success() {
            selected_tier = Some(tier.name.clone());
            break;
        }
        last_exit_code = status.code();
    }
    let selected_tier = selected_tier.ok_or_else(|| {
        anyhow!(
            "Python dependency install failed; last tier exited {:?}",
            last_exit_code
        )
    })?;
    let baseline = Command::new(&plan.python)
        .args(["-c", "import dotenv, openai, rich, prompt_toolkit"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("checking baseline imports with {}", plan.python.display()))?;
    if !baseline.success() {
        return Err(anyhow!(
            "baseline imports failed after {selected_tier} with exit {:?}",
            baseline.code()
        ));
    }
    Ok(serde_json::json!({
        "uv": plan.uv,
        "venv": plan.venv,
        "python": plan.python,
        "uvCacheDir": plan.uv_cache_dir,
        "tier": selected_tier,
    }))
}

fn python_dependency_install_tiers_for_cwd_with_wheelhouse(
    cwd: &Path,
    wheelhouse_dir: Option<&Path>,
) -> Vec<PythonDependencyInstallTier> {
    let pyproject = fs::read_to_string(cwd.join("pyproject.toml")).unwrap_or_default();
    let mut tiers = python_dependency_install_tiers_for_pyproject(&pyproject, PYTHON_KNOWN_BROKEN_EXTRAS);
    let checkout_wheelhouse = cwd.join("resources").join("wheelhouse");
    let wheelhouse = wheelhouse_dir
        .filter(|path| wheelhouse_has_wheels(path, Some(cwd)))
        .map(Path::to_path_buf)
        .or_else(|| wheelhouse_has_wheels(&checkout_wheelhouse, Some(cwd)).then_some(checkout_wheelhouse));
    if let Some(wheelhouse) = wheelhouse {
        tiers.insert(
            0,
            PythonDependencyInstallTier {
                name: "local wheelhouse (all)".to_string(),
                args: vec![
                    "pip".to_string(),
                    "install".to_string(),
                    "--no-index".to_string(),
                    "--find-links".to_string(),
                    wheelhouse.display().to_string(),
                    "-e".to_string(),
                    ".[all]".to_string(),
                ],
            },
        );
    }
    tiers
}

fn wheelhouse_has_wheels(path: &Path, repo_root: Option<&Path>) -> bool {
    if path.join(WHEELHOUSE_MANIFEST).is_file() {
        return wheelhouse_manifest_is_valid(path, repo_root);
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|extension| extension == OsStr::new("whl"))
    })
}

fn wheelhouse_manifest_is_valid(path: &Path, repo_root: Option<&Path>) -> bool {
    let manifest_path = path.join(WHEELHOUSE_MANIFEST);
    let Some(manifest) = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| serde_json::from_str::<WheelhouseManifest>(&text).ok())
    else {
        return false;
    };
    if manifest.schema_version != WHEELHOUSE_MANIFEST_SCHEMA_VERSION || manifest.wheels.is_empty() {
        return false;
    }
    if !wheelhouse_source_files_match(&manifest.source_files, repo_root) {
        return false;
    }

    let mut expected = BTreeSet::from([WHEELHOUSE_MANIFEST.to_string()]);
    for name in ALLOWED_WHEELHOUSE_METADATA {
        expected.insert(name.to_string());
    }
    for wheel in &manifest.wheels {
        if !wheel_name_is_plain_file(&wheel.name) || !expected.insert(wheel.name.clone()) {
            return false;
        }
        if !matches!(wheel.platform.as_deref(), Some("windows" | "linux" | "macos")) {
            return false;
        }
        if wheel.platform.as_deref() != Some(current_wheelhouse_platform()) {
            return false;
        }
        if wheel
            .arch
            .as_deref()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
        {
            return false;
        }
        if wheel.arch.as_deref() != current_wheelhouse_arch() {
            return false;
        }
        if wheel
            .python
            .as_deref()
            .map(|value| !value.starts_with("cp") || value.len() <= 2)
            .unwrap_or(true)
        {
            return false;
        }
        if wheel.sha256.len() != 64 || !wheel.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return false;
        }
        let Ok(bytes) = fs::read(path.join(&wheel.name)) else {
            return false;
        };
        if wheel.size_bytes != Some(bytes.len() as u64) {
            return false;
        }
        if !crate::artifact::sha256_hex(&bytes).eq_ignore_ascii_case(&wheel.sha256) {
            return false;
        }
    }

    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !expected.contains(&name) || !entry.path().is_file() {
            return false;
        }
    }
    true
}

fn wheelhouse_source_files_match(
    source_files: &[WheelhouseManifestSourceFile],
    repo_root: Option<&Path>,
) -> bool {
    let Some(repo_root) = repo_root else {
        return source_files.is_empty();
    };
    if source_files.is_empty() {
        return false;
    }
    let mut seen = BTreeSet::new();
    for source in source_files {
        if !bootstrap_archive_name_is_plain_file(&source.path) || !seen.insert(source.path.clone()) {
            return false;
        }
        if source.sha256.len() != 64 || !source.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return false;
        }
        let Ok(bytes) = fs::read(repo_root.join(&source.path)) else {
            return false;
        };
        if !crate::artifact::sha256_hex(&bytes).eq_ignore_ascii_case(&source.sha256) {
            return false;
        }
    }
    true
}

fn current_wheelhouse_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "linux" => "linux",
        "macos" => "macos",
        _ => "unsupported",
    }
}

fn current_wheelhouse_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x64"),
        "aarch64" | "arm64" => Some("arm64"),
        "x86" | "i686" => Some("x86"),
        _ => None,
    }
}

fn wheel_name_is_plain_file(name: &str) -> bool {
    !name.is_empty()
        && name.ends_with(".whl")
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
}

fn python_dependency_install_tiers_for_pyproject(
    pyproject_text: &str,
    broken_extras: &[&str],
) -> Vec<PythonDependencyInstallTier> {
    let broken_label = if broken_extras.is_empty() {
        "none".to_string()
    } else {
        broken_extras.join(", ")
    };
    let safe_all_spec = python_safe_all_extra_spec(pyproject_text, broken_extras);
    vec![
        PythonDependencyInstallTier {
            name: "hash-verified (uv.lock)".to_string(),
            args: dependency_tier_args(&["sync", "--extra", "all", "--locked"]),
        },
        PythonDependencyInstallTier {
            name: "all".to_string(),
            args: dependency_tier_args(&["pip", "install", "-e", ".[all]"]),
        },
        PythonDependencyInstallTier {
            name: format!("all minus known-broken ({broken_label})"),
            args: vec![
                "pip".to_string(),
                "install".to_string(),
                "-e".to_string(),
                safe_all_spec,
            ],
        },
        PythonDependencyInstallTier {
            name: "core only (no extras)".to_string(),
            args: dependency_tier_args(&["pip", "install", "-e", "."]),
        },
    ]
}

fn dependency_tier_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn python_safe_all_extra_spec(pyproject_text: &str, broken_extras: &[&str]) -> String {
    if broken_extras.is_empty() {
        return ".[all]".to_string();
    }
    let Ok(all_extras) = pyproject_all_extra_names(pyproject_text) else {
        return ".[all]".to_string();
    };
    if all_extras.is_empty() {
        return ".[all]".to_string();
    }
    let safe_extras = all_extras
        .into_iter()
        .filter(|extra| !broken_extras.iter().any(|broken| extra == broken))
        .collect::<Vec<_>>();
    format!(".[{}]", safe_extras.join(","))
}

fn pyproject_all_extra_names(pyproject_text: &str) -> Result<Vec<String>> {
    let parsed = toml::from_str::<PyprojectToml>(pyproject_text)?;
    let extras = parsed
        .project
        .and_then(|project| project.optional_dependencies)
        .and_then(|optional| optional.get("all").cloned())
        .ok_or_else(|| anyhow!("pyproject.toml does not define [project.optional-dependencies].all"))?;
    Ok(extras
        .iter()
        .filter_map(|spec| hermes_agent_extra_name(spec))
        .collect())
}

fn hermes_agent_extra_name(spec: &str) -> Option<String> {
    let (_, after_prefix) = spec.split_once("hermes-agent[")?;
    let (extra, _) = after_prefix.split_once(']')?;
    if extra.is_empty() {
        return None;
    }
    Some(extra.to_string())
}

fn run_python_dependency_install_tier(
    plan: &PythonDependenciesStagePlan,
    tier: &PythonDependencyInstallTier,
) -> Result<ExitStatus> {
    Command::new(&plan.uv)
        .args(&tier.args)
        .current_dir(&plan.cwd)
        .env("VIRTUAL_ENV", &plan.venv)
        .env("UV_PYTHON", &plan.python)
        .env("UV_PROJECT_ENVIRONMENT", &plan.venv)
        .env("UV_CACHE_DIR", &plan.uv_cache_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {} for {}", plan.uv.display(), tier.name))
}

/// Build the native Node dependencies stage plan.
pub fn node_dependencies_stage_plan<P>(
    install_root: &Path,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Result<NodeDependenciesStagePlan>
where
    P: AsRef<OsStr>,
{
    if !install_root.is_dir() {
        return Err(anyhow!(
            "install root does not exist: {}",
            install_root.display()
        ));
    }
    let npm = find_npm_executable(hermes_home, path_env.as_ref(), pathext)
        .ok_or_else(|| anyhow!("npm is not available"))?;
    let npx = find_npx_executable(&npm, path_env, pathext);
    let tui_dir = install_root.join("ui-tui");
    let tui_dir = if tui_dir.join("package.json").is_file() {
        Some(tui_dir)
    } else {
        None
    };
    Ok(NodeDependenciesStagePlan {
        npm,
        npx,
        cwd: install_root.to_path_buf(),
        npm_cache_dir: hermes_home.join("npm-cache"),
        playwright_browsers_dir: hermes_home.join("playwright-browsers"),
        browser_tools: install_root.join("package.json").is_file(),
        tui_dir,
    })
}

fn playwright_install_plan(
    target_os: &str,
    distro: &str,
    user_is_root: bool,
    sudo_available: bool,
) -> Result<PlaywrightInstallPlan> {
    let mut npx_args = vec![
        "--yes".to_string(),
        "playwright".to_string(),
        "install".to_string(),
    ];
    let mut system_package_commands = Vec::new();
    let mut system_deps = "browser-only".to_string();
    if target_os == "linux" && playwright_apt_distro_supports_with_deps(distro) {
        if user_is_root || sudo_available {
            npx_args.push("--with-deps".to_string());
            system_deps = "playwright-with-deps".to_string();
        }
    } else if target_os == "linux"
        && playwright_arch_distro_supports_pacman_deps(distro)
        && (user_is_root || sudo_available)
    {
        system_package_commands.push(unix_privileged_package_install_command(
            user_is_root,
            sudo_available,
            "pacman",
            &["-S", "--noconfirm", "--needed"],
            playwright_arch_system_packages(),
        )?);
        system_deps = "pacman".to_string();
    } else if target_os == "linux"
        && playwright_dnf_distro_supports_deps(distro)
        && (user_is_root || sudo_available)
    {
        system_package_commands.push(unix_privileged_package_install_command(
            user_is_root,
            sudo_available,
            "dnf",
            &["install", "-y"],
            playwright_dnf_system_packages(),
        )?);
        system_deps = "dnf".to_string();
    } else if target_os == "linux"
        && playwright_zypper_distro_supports_deps(distro)
        && (user_is_root || sudo_available)
    {
        system_package_commands.push(unix_privileged_package_install_command(
            user_is_root,
            sudo_available,
            "zypper",
            &["--non-interactive", "install"],
            playwright_zypper_system_packages(),
        )?);
        system_deps = "zypper".to_string();
    }
    npx_args.push("chromium".to_string());
    Ok(PlaywrightInstallPlan {
        npx_args,
        system_package_commands,
        system_deps,
    })
}

fn browser_install_decision(
    system_browser: Option<PathBuf>,
    target_os: &str,
    distro: &str,
    user_is_root: bool,
    sudo_available: bool,
) -> Result<BrowserInstallDecision> {
    if let Some(browser) = system_browser {
        return Ok(BrowserInstallDecision {
            system_browser: Some(browser),
            playwright: None,
            system_deps: "system-browser".to_string(),
        });
    }
    let playwright = playwright_install_plan(target_os, distro, user_is_root, sudo_available)?;
    Ok(BrowserInstallDecision {
        system_browser: None,
        system_deps: playwright.system_deps.clone(),
        playwright: Some(playwright),
    })
}

fn install_playwright_with_system_recovery(
    npx: &Path,
    playwright_plan: &PlaywrightInstallPlan,
    cwd: &Path,
    npm_cache_dir: &Path,
    playwright_browsers_dir: &Path,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    for command in &playwright_plan.system_package_commands {
        if let Err(err) = run_unix_system_package_install_command(command) {
            failures.push(err.to_string());
        }
    }
    run_node_dependency_command_args(
        npx,
        &playwright_plan.npx_args,
        cwd,
        npm_cache_dir,
        Some(playwright_browsers_dir),
    )
    .context("installing Playwright Chromium")?;
    Ok(failures)
}

fn find_system_browser<P>(path_env: P, pathext: &str) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let configured = std::env::var("AGENT_BROWSER_EXECUTABLE_PATH").ok();
    find_system_browser_with_config(configured.as_deref(), path_env, pathext)
}

fn find_system_browser_with_config<P>(
    configured_browser: Option<&str>,
    path_env: P,
    pathext: &str,
) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    if let Some(value) = configured_browser {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let configured = PathBuf::from(trimmed);
            if configured.is_file() {
                return Some(configured);
            }
            if let Some(found) = find_executable_on_path(trimmed, path_env.as_ref(), pathext) {
                return Some(found);
            }
        }
    }

    for candidate in system_browser_command_candidates() {
        if let Some(found) = find_executable_on_path(candidate, path_env.as_ref(), pathext) {
            return Some(found);
        }
    }
    for candidate in system_browser_file_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn system_browser_command_candidates() -> &'static [&'static str] {
    system_browser_command_candidates_for_target(std::env::consts::OS)
}

fn system_browser_command_candidates_for_target(target_os: &str) -> &'static [&'static str] {
    match target_os {
        "windows" => &[
            "chrome.exe",
            "chrome",
            "chromium.exe",
            "chromium",
            "brave.exe",
            "brave",
            "msedge.exe",
            "msedge",
        ],
        "linux" => &[
            "google-chrome",
            "google-chrome-stable",
            "chromium-browser",
            "chromium",
            "brave-browser",
            "brave-browser-stable",
            "brave",
            "microsoft-edge",
            "microsoft-edge-stable",
            "msedge",
        ],
        _ => &[],
    }
}

fn system_browser_file_candidates() -> Vec<PathBuf> {
    let roots = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name, value)))
        .collect::<Vec<_>>();
    system_browser_file_candidates_for_target(std::env::consts::OS, roots)
}

fn system_browser_file_candidates_for_target<I, K, V>(target_os: &str, env_roots: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut candidates = Vec::new();
    match target_os {
        "windows" => {
            for (_, root) in env_roots {
                let root = root.as_ref();
                if root.is_empty() {
                    continue;
                }
                for parts in [
                    &["Google", "Chrome", "Application", "chrome.exe"][..],
                    &["Chromium", "Application", "chrome.exe"],
                    &["Chromium", "Application", "chromium.exe"],
                    &["BraveSoftware", "Brave-Browser", "Application", "brave.exe"],
                    &["Microsoft", "Edge", "Application", "msedge.exe"],
                ] {
                    let mut path = PathBuf::from(root);
                    for part in parts {
                        path.push(part);
                    }
                    candidates.push(path);
                }
            }
        }
        "macos" => {
            candidates.extend([
                PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
                PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
                PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            ]);
        }
        "linux" => {
            candidates.extend([
                PathBuf::from("/opt/google/chrome/chrome"),
                PathBuf::from("/usr/bin/google-chrome"),
                PathBuf::from("/usr/bin/google-chrome-stable"),
                PathBuf::from("/usr/bin/chromium-browser"),
                PathBuf::from("/usr/bin/chromium"),
                PathBuf::from("/usr/bin/brave-browser"),
                PathBuf::from("/usr/bin/brave-browser-stable"),
                PathBuf::from("/usr/bin/brave"),
                PathBuf::from("/snap/bin/brave"),
                PathBuf::from("/opt/brave.com/brave/brave-browser"),
                PathBuf::from("/opt/brave.com/brave/brave"),
                PathBuf::from("/opt/brave-bin/brave"),
                PathBuf::from("/usr/bin/microsoft-edge"),
                PathBuf::from("/usr/bin/microsoft-edge-stable"),
                PathBuf::from("/opt/microsoft/msedge/microsoft-edge"),
                PathBuf::from("/opt/microsoft/msedge/msedge"),
            ]);
        }
        _ => {}
    }
    candidates
}

fn write_browser_env_from_system_browser(hermes_home: &Path, browser: &Path) -> Result<bool> {
    fs::create_dir_all(hermes_home)
        .with_context(|| format!("creating Hermes home {}", hermes_home.display()))?;
    let env_file = hermes_home.join(".env");
    let existing = fs::read_to_string(&env_file).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim_start().starts_with("AGENT_BROWSER_EXECUTABLE_PATH="))
    {
        return Ok(false);
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.is_empty() {
        next.push('\n');
    }
    next.push_str("# Hermes Agent browser tools - use the system Chrome/Chromium binary.\n");
    next.push_str("AGENT_BROWSER_EXECUTABLE_PATH=");
    next.push_str(&browser.display().to_string());
    next.push('\n');
    fs::write(&env_file, next).with_context(|| format!("writing {}", env_file.display()))?;
    Ok(true)
}

fn playwright_apt_distro_supports_with_deps(distro: &str) -> bool {
    distro_matches(
        distro,
        &[
            "ubuntu",
            "debian",
            "raspbian",
            "pop",
            "linuxmint",
            "elementary",
            "zorin",
            "kali",
            "parrot",
        ],
    )
}

fn playwright_arch_distro_supports_pacman_deps(distro: &str) -> bool {
    distro_matches(
        distro,
        &["arch", "manjaro", "cachyos", "endeavouros", "garuda"],
    )
}

fn playwright_dnf_distro_supports_deps(distro: &str) -> bool {
    distro_matches(distro, &["fedora", "rhel", "centos", "rocky", "alma"])
}

fn playwright_zypper_distro_supports_deps(distro: &str) -> bool {
    distro_any_starts_with(distro, "opensuse") || distro_matches(distro, &["sles"])
}

fn playwright_arch_system_packages() -> &'static [&'static str] {
    &[
        "nss",
        "atk",
        "at-spi2-core",
        "cups",
        "libdrm",
        "libxkbcommon",
        "mesa",
        "pango",
        "cairo",
        "alsa-lib",
    ]
}

fn playwright_dnf_system_packages() -> &'static [&'static str] {
    &[
        "nss",
        "atk",
        "at-spi2-core",
        "cups-libs",
        "libdrm",
        "libxkbcommon",
        "mesa-libgbm",
        "pango",
        "cairo",
        "alsa-lib",
    ]
}

fn playwright_zypper_system_packages() -> &'static [&'static str] {
    &[
        "mozilla-nss",
        "libatk-1_0-0",
        "at-spi2-core",
        "cups-libs",
        "libdrm2",
        "libxkbcommon0",
        "Mesa-libgbm1",
        "pango",
        "cairo",
        "libasound2",
    ]
}

/// Install Node dependencies natively with platform-specific browser dependency recovery.
pub fn install_node_dependencies_stage(
    install_root: &Path,
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let plan = node_dependencies_stage_plan(install_root, hermes_home, &path_env, &pathext)?;
    let mut playwright_system_deps = "skipped".to_string();
    let mut playwright_system_commands = Vec::new();
    let mut playwright_system_failures = Vec::new();
    let mut playwright_browsers_archive_name = None;
    let mut playwright_browsers_archive_source_kind = None;
    let mut npm_cache_archive_name = None;
    let mut npm_cache_archive_source_kind = None;
    let mut optional_node_failures = Vec::new();
    let arch = current_wheelhouse_arch()
        .ok_or_else(|| anyhow!("unsupported npm cache architecture: {}", std::env::consts::ARCH))?;
    if let Some((archive_name, source_kind)) = install_bundled_npm_cache_if_available(
        hermes_home,
        bundled_tools_dir,
        current_wheelhouse_platform(),
        arch,
    )? {
        npm_cache_archive_name = Some(archive_name);
        npm_cache_archive_source_kind = Some(source_kind.as_str().to_string());
    }
    if plan.browser_tools {
        let npm_ci_ok = plan.cwd.join("package-lock.json").is_file()
            && run_node_dependency_command(
                &plan.npm,
                ["ci", "--prefer-offline", "--no-audit", "--fund=false"],
                &plan.cwd,
                &plan.npm_cache_dir,
                Some(&plan.playwright_browsers_dir),
            )
            .is_ok();
        if !npm_ci_ok {
            run_node_dependency_command(
                &plan.npm,
                [
                    "install",
                    "--silent",
                    "--prefer-offline",
                    "--no-audit",
                    "--fund=false",
                ],
                &plan.cwd,
                &plan.npm_cache_dir,
                Some(&plan.playwright_browsers_dir),
            )
            .context("installing root Node dependencies")?;
        }
        let distro = if std::env::consts::OS == "linux" {
            current_linux_distro_family()
        } else {
            String::new()
        };
        let browser_decision = browser_install_decision(
            find_system_browser(&path_env, &pathext),
            std::env::consts::OS,
            &distro,
            current_process_is_root(),
            noninteractive_sudo_available(&path_env),
        )?;
        playwright_system_deps = browser_decision.system_deps.clone();
        if let Some(browser) = browser_decision.system_browser {
            write_browser_env_from_system_browser(hermes_home, &browser)
                .context("configuring system browser for browser tools")?;
        } else if let Some(playwright_plan) = browser_decision.playwright {
            let arch = current_wheelhouse_arch().ok_or_else(|| {
                anyhow!(
                    "unsupported Playwright browser architecture: {}",
                    std::env::consts::ARCH
                )
            })?;
            if let Some((archive_name, source_kind)) = install_bundled_playwright_browsers_if_available(
                hermes_home,
                bundled_tools_dir,
                current_wheelhouse_platform(),
                arch,
            )? {
                playwright_system_deps = "bundled-playwright-browsers".to_string();
                playwright_browsers_archive_name = Some(archive_name);
                playwright_browsers_archive_source_kind = Some(source_kind.as_str().to_string());
            } else {
                let npx = plan
                    .npx
                    .as_ref()
                    .ok_or_else(|| anyhow!("npx is not available"))?;
                playwright_system_commands = playwright_plan
                    .system_package_commands
                    .iter()
                    .map(unix_package_command_display)
                    .collect::<Vec<_>>();
                playwright_system_failures = install_playwright_with_system_recovery(
                    npx,
                    &playwright_plan,
                    &plan.cwd,
                    &plan.npm_cache_dir,
                    &plan.playwright_browsers_dir,
                )?;
            }
        }
    }
    if let Some(tui_dir) = &plan.tui_dir {
        if let Some(failure) = run_optional_node_dependency_command(
            &plan.npm,
            [
                "install",
                "--silent",
                "--prefer-offline",
                "--no-audit",
                "--fund=false",
            ],
            tui_dir,
            &plan.npm_cache_dir,
            Some(&plan.playwright_browsers_dir),
        ) {
            optional_node_failures.push(format!("TUI npm install failed: {failure}"));
        }
    }
    Ok(serde_json::json!({
        "npm": plan.npm,
        "npx": plan.npx,
        "npmCacheDir": plan.npm_cache_dir,
        "npmCacheArchive": npm_cache_archive_name,
        "npmCacheArchiveSource": npm_cache_archive_source_kind,
        "playwrightBrowsersDir": plan.playwright_browsers_dir,
        "playwrightBrowsersArchive": playwright_browsers_archive_name,
        "playwrightBrowsersArchiveSource": playwright_browsers_archive_source_kind,
        "playwrightSystemDeps": playwright_system_deps,
        "playwrightSystemCommands": playwright_system_commands,
        "playwrightSystemFailures": playwright_system_failures,
        "optionalNodeFailures": optional_node_failures,
        "browserTools": plan.browser_tools,
        "tui": plan.tui_dir.is_some(),
    }))
}

/// Build the native desktop app stage plan.
pub fn desktop_build_stage_plan<P>(
    install_root: &Path,
    hermes_home: &Path,
    path_env: P,
    pathext: &str,
) -> Result<DesktopBuildStagePlan>
where
    P: AsRef<OsStr>,
{
    let npm = find_npm_executable(hermes_home, path_env, pathext)
        .ok_or_else(|| anyhow!("npm is not available"))?;
    let desktop_dir = install_root.join("apps").join("desktop");
    if !desktop_dir.join("package.json").is_file() {
        return Err(anyhow!(
            "desktop package not found at {}",
            desktop_dir.join("package.json").display()
        ));
    }
    Ok(DesktopBuildStagePlan {
        npm,
        cwd: install_root.to_path_buf(),
        npm_cache_dir: hermes_home.join("npm-cache"),
        electron_cache_dir: hermes_home.join("electron-cache"),
        desktop_dir,
    })
}

/// Build the desktop app natively and apply platform-specific post-build fixups.
pub fn build_desktop_stage(
    install_root: &Path,
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let plan = desktop_build_stage_plan(install_root, hermes_home, path_env, &pathext)?;
    let arch = current_wheelhouse_arch()
        .ok_or_else(|| anyhow!("unsupported Electron cache architecture: {}", std::env::consts::ARCH))?;
    let electron_cache_archive =
        install_bundled_electron_cache_if_available(
            hermes_home,
            bundled_tools_dir,
            current_wheelhouse_platform(),
            arch,
        )?;
    if run_node_dependency_command(
        &plan.npm,
        ["ci", "--prefer-offline", "--no-audit", "--fund=false"],
        &plan.cwd,
        &plan.npm_cache_dir,
        None,
    )
    .is_err()
    {
        run_node_dependency_command(
            &plan.npm,
            ["install", "--prefer-offline", "--no-audit", "--fund=false"],
            &plan.cwd,
            &plan.npm_cache_dir,
            None,
        )
            .context("installing desktop workspace Node dependencies")?;
    }
    run_desktop_pack_command(
        &plan.npm,
        &plan.desktop_dir,
        &plan.npm_cache_dir,
        &plan.electron_cache_dir,
    )?;
    let desktop_app = find_built_desktop_app(install_root, std::env::consts::OS)
        .ok_or_else(|| anyhow!("desktop build completed but no app was found"))?;
    if cfg!(target_os = "linux") {
        configure_linux_chrome_sandbox(install_root)?;
    }
    let mut result = serde_json::json!({
        "npm": plan.npm,
        "npmCacheDir": plan.npm_cache_dir,
        "electronCacheDir": plan.electron_cache_dir,
        "electronCacheArchive": electron_cache_archive.as_ref().map(|(name, _)| name),
        "electronCacheArchiveSource": electron_cache_archive.as_ref().map(|(_, source)| source.as_str()),
        "desktopDir": plan.desktop_dir,
        "desktopApp": &desktop_app,
    });
    if cfg!(target_os = "windows") {
        result["desktopExe"] = serde_json::json!(&desktop_app);
    }
    Ok(result)
}

/// Build a native Windows PATH stage report without mutating user state.
pub fn windows_path_stage_plan(
    hermes_home: &Path,
    install_root: &Path,
    current_user_path: Option<String>,
    current_user_hermes_home: Option<String>,
) -> serde_json::Value {
    let path_plan = hermes_manager::platform::plan_path_update(install_root, current_user_path, true);
    let desired_home = hermes_home.display().to_string();
    let hermes_home_changed = current_user_hermes_home
        .as_deref()
        .map(|value| !value.eq_ignore_ascii_case(&desired_home))
        .unwrap_or(true);

    serde_json::json!({
        "hermesHome": hermes_home,
        "hermesBin": path_plan.hermes_bin,
        "pathChanged": path_plan.changed,
        "hermesHomeChanged": hermes_home_changed,
        "applied": false,
    })
}

/// Apply the native Windows PATH stage.
#[cfg(target_os = "windows")]
pub fn configure_windows_path_stage(hermes_home: &Path, install_root: &Path) -> Result<serde_json::Value> {
    let current_user_path = hermes_manager::platform::read_windows_user_path()?;
    let current_user_hermes_home =
        hermes_manager::platform::read_windows_user_env_var("HERMES_HOME")?;
    let mut report = windows_path_stage_plan(
        hermes_home,
        install_root,
        current_user_path.clone(),
        current_user_hermes_home.clone(),
    );
    let path_plan = hermes_manager::platform::plan_path_update(install_root, current_user_path, true);
    let path_applied = hermes_manager::platform::write_windows_user_path_update(&path_plan)?;
    let desired_home = hermes_home.display().to_string();
    let home_changed = current_user_hermes_home
        .as_deref()
        .map(|value| !value.eq_ignore_ascii_case(&desired_home))
        .unwrap_or(true);
    if home_changed {
        hermes_manager::platform::write_windows_user_env_var("HERMES_HOME", &desired_home)?;
    }
    std::env::set_var("HERMES_HOME", &desired_home);
    refresh_process_path(install_root);
    report["applied"] = serde_json::Value::Bool(path_applied || home_changed);
    Ok(report)
}

/// Reject accidental Windows PATH calls on Unix builds.
#[cfg(not(target_os = "windows"))]
pub fn configure_windows_path_stage(_hermes_home: &Path, _install_root: &Path) -> Result<serde_json::Value> {
    Err(anyhow!("native PATH stage is only available on Windows"))
}

/// Apply the native Unix shell-profile PATH stage.
pub fn configure_unix_path_stage(hermes_home: &Path, install_root: &Path) -> Result<serde_json::Value> {
    let profile_paths = default_unix_profile_paths()
        .ok_or_else(|| anyhow!("could not resolve a writable Unix shell profile path"))?;
    configure_unix_path_stage_with_profiles(
        hermes_home,
        install_root,
        &profile_paths,
        std::env::var("PATH").ok(),
    )
}

fn configure_unix_path_stage_with_profiles(
    hermes_home: &Path,
    install_root: &Path,
    profile_paths: &[PathBuf],
    current_path: Option<String>,
) -> Result<serde_json::Value> {
    if profile_paths.is_empty() {
        return Err(anyhow!("could not resolve a writable Unix shell profile path"));
    }

    let command_link_dir = unix_command_link_dir_for_layout(hermes_home, install_root);
    let extra_path_entries = [command_link_dir.clone()];
    let plan = hermes_manager::platform::plan_path_update_with_extra_entries(
        install_root,
        &extra_path_entries,
        current_path,
        false,
    );
    let mut profile_changed = false;
    for profile_path in profile_paths {
        let before = std::fs::read_to_string(profile_path).ok();
        hermes_manager::platform::write_shell_profile_update(profile_path, &plan)
            .map_err(|err| anyhow!("writing Unix shell profile update: {err}"))?;
        let after = std::fs::read_to_string(profile_path).ok();
        profile_changed |= before != after;
    }
    let launcher_path = command_link_dir.join("hermes");
    let launcher_target = install_root.join("venv").join("bin").join("hermes");
    let launcher_changed =
        hermes_manager::platform::write_unix_launcher(&launcher_path, &launcher_target)
            .map_err(|err| anyhow!("writing Unix launcher: {err}"))?;
    std::env::set_var("PATH", &plan.next_path);
    let profile_path_values = profile_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "profilePath": profile_path_values[0],
        "profilePaths": profile_path_values,
        "hermesBin": plan.hermes_bin,
        "commandLinkDir": command_link_dir.display().to_string(),
        "launcherPath": launcher_path.display().to_string(),
        "launcherChanged": launcher_changed,
        "pathEntries": plan.path_entries,
        "pathChanged": plan.changed,
        "profileChanged": profile_changed,
        "applied": plan.changed || profile_changed || launcher_changed,
    }))
}

#[cfg(test)]
fn configure_unix_path_stage_with_profile(
    hermes_home: &Path,
    install_root: &Path,
    profile_path: &Path,
    current_path: Option<String>,
) -> Result<serde_json::Value> {
    configure_unix_path_stage_with_profiles(hermes_home, install_root, &[profile_path.to_path_buf()], current_path)
}

fn default_unix_profile_paths() -> Option<Vec<PathBuf>> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    Some(default_unix_profile_paths_for(&home, &shell))
}

#[cfg(test)]
fn default_unix_profile_path_for(home: &Path, shell: &str) -> Option<PathBuf> {
    default_unix_profile_paths_for(home, shell).into_iter().next()
}

fn default_unix_profile_paths_for(home: &Path, shell: &str) -> Vec<PathBuf> {
    if shell.ends_with("fish") {
        return vec![home.join(".config").join("fish").join("config.fish")];
    }
    if shell.ends_with("zsh") {
        return existing_or_default_profile_paths(home, &[".zshrc", ".zprofile"], ".zshrc");
    }
    existing_or_default_profile_paths(home, &[".bashrc", ".bash_profile", ".profile"], ".bashrc")
}

fn existing_or_default_profile_paths(home: &Path, names: &[&str], default_name: &str) -> Vec<PathBuf> {
    let existing = names
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if existing.is_empty() {
        return vec![home.join(default_name)];
    }
    existing
}

fn unix_command_link_dir_for_layout(hermes_home: &Path, install_root: &Path) -> PathBuf {
    if install_root_uses_linux_fhs_layout(install_root) {
        return PathBuf::from("/usr/local/bin");
    }
    hermes_home.join("bin")
}

pub(crate) fn python_runtime_dirs_for_layout(
    hermes_home: &Path,
    install_root: &Path,
) -> PythonRuntimeDirs {
    if install_root_uses_linux_fhs_layout(install_root) {
        return PythonRuntimeDirs {
            install_dir: PathBuf::from("/usr/local/share/uv/python"),
            bin_dir: PathBuf::from("/usr/local/share/uv/bin"),
        };
    }
    PythonRuntimeDirs {
        install_dir: hermes_home.join("python"),
        bin_dir: hermes_home.join("bin"),
    }
}

fn install_root_uses_linux_fhs_layout(install_root: &Path) -> bool {
    install_root == Path::new("/usr/local/lib/hermes-agent")
}

#[cfg(target_os = "windows")]
fn refresh_process_path(install_root: &Path) {
    let current = std::env::var("PATH").ok();
    let plan = hermes_manager::platform::plan_path_update(install_root, current, true);
    std::env::set_var("PATH", plan.next_path);
}

/// Build the repository archive selector used by the native fresh-install path.
pub fn repository_archive_spec(commit: Option<&str>, branch: Option<&str>) -> crate::repo_archive::RepoArchiveSpec {
    let commit = commit.filter(|value| !value.trim().is_empty()).map(str::to_string);
    let branch = branch
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("main")
        .to_string();
    crate::repo_archive::RepoArchiveSpec {
        owner: "NousResearch".to_string(),
        repo: "hermes-agent".to_string(),
        commit,
        branch: Some(branch),
    }
}

/// Download a GitHub archive into a fresh install root and prepare best-effort Git metadata.
pub async fn install_repository_archive_fresh(
    install_root: &Path,
    commit: Option<&str>,
    branch: Option<&str>,
) -> Result<serde_json::Value> {
    if install_root.exists() {
        return Err(anyhow!(
            "install root already exists; native archive fallback is fresh-install only: {}",
            install_root.display()
        ));
    }

    let spec = repository_archive_spec(commit, branch);
    let archive_path =
        crate::repo_archive::download_and_extract_fresh(&spec, &crate::paths::bootstrap_cache_dir(), install_root)
            .await?;
    let git_initialized = initialize_archive_git_repo(install_root);
    let source_marker = crate::repo_archive::write_archive_source_marker(
        install_root,
        &spec,
        &archive_path,
        git_initialized,
    )?;

    Ok(serde_json::json!({
        "installRoot": install_root,
        "archive": archive_path,
        "gitInitialized": git_initialized,
        "source": source_marker,
    }))
}

fn initialize_archive_git_repo(install_root: &Path) -> bool {
    if !run_git(install_root, ["init"]) {
        return false;
    }
    let _ = run_git(install_root, ["config", "windows.appendAtomically", "false"]);
    let _ = run_git(install_root, ["config", "core.autocrlf", "false"]);
    let _ = run_git(
        install_root,
        ["remote", "add", "origin", "https://github.com/NousResearch/hermes-agent.git"],
    );
    true
}

fn run_git<const N: usize>(install_root: &Path, args: [&str; N]) -> bool {
    Command::new("git")
        .args(["-c", "windows.appendAtomically=false"])
        .args(args)
        .current_dir(install_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn ensure_file_from_template(dest: &Path, template: &Path, empty_fallback: Option<&str>) -> Result<bool> {
    if dest.exists() {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if template.exists() {
        fs::copy(template, dest).with_context(|| {
            format!(
                "copying template {} to {}",
                template.display(),
                dest.display()
            )
        })?;
        return Ok(true);
    }
    if let Some(contents) = empty_fallback {
        fs::write(dest, contents).with_context(|| format!("creating {}", dest.display()))?;
        return Ok(true);
    }
    Ok(false)
}

fn ensure_soul_file(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, SOUL_TEMPLATE).with_context(|| format!("creating {}", path.display()))?;
    Ok(true)
}

fn sync_bundled_skills(hermes_home: &Path, install_root: &Path) -> Result<&'static str> {
    let script = install_root.join("tools").join("skills_sync.py");
    let python = if cfg!(target_os = "windows") {
        install_root.join("venv").join("Scripts").join("python.exe")
    } else {
        install_root.join("venv").join("bin").join("python")
    };
    if python.exists() && script.exists() {
        let (env_key, env_value) = skills_sync_env(hermes_home);
        let status = Command::new(&python)
            .arg(&script)
            .env(env_key, env_value)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if matches!(status, Ok(status) if status.success()) {
            return Ok("python");
        }
    }

    let bundled = install_root.join("skills");
    let user_skills = hermes_home.join("skills");
    if bundled.is_dir() && !user_skills_has_non_manifest_entries(&user_skills)? {
        copy_dir_contents(&bundled, &user_skills)?;
        return Ok("copied");
    }
    Ok("skipped")
}

fn skills_sync_env(hermes_home: &Path) -> (&'static str, PathBuf) {
    ("HERMES_HOME", hermes_home.to_path_buf())
}

fn venv_python_path(venv: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

fn user_skills_has_non_manifest_entries(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        let entry = entry.with_context(|| format!("reading entry under {}", path.display()))?;
        if entry.file_name() != ".bundled_manifest" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry.with_context(|| format!("reading entry under {}", src.display()))?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_contents(&src_path, &dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!("copying {} to {}", src_path.display(), dest_path.display())
            })?;
        }
    }
    Ok(())
}

fn resolve_git_head(install_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-c", "windows.appendAtomically=false", "rev-parse", "HEAD"])
        .current_dir(install_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

const SOUL_TEMPLATE: &str = r#"# Hermes Agent Persona

<!--
This file defines the agent's personality and tone.
The agent will embody whatever you write here.
Edit this to customize how Hermes communicates with you.

Examples:
  - "You are a warm, playful assistant who uses kaomoji occasionally."
  - "You are a concise technical expert. No fluff, just facts."
  - "You speak like a friendly coworker who happens to know everything."

This file is loaded fresh each message -- no restart needed.
Delete the contents (or this file) to use the default personality.
-->
"#;

fn probe_tool(name: &str) -> ToolProbe {
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    ToolProbe {
        name: name.to_string(),
        path: find_executable_on_path(name, path_env, &pathext),
    }
}

fn managed_tool_path(hermes_home: &Path, name: &str) -> PathBuf {
    let filename = if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    hermes_home.join("bin").join(filename)
}

fn uv_tool_path<P>(hermes_home: &Path, path_env: P, pathext: &str) -> Result<PathBuf>
where
    P: AsRef<OsStr>,
{
    let managed = managed_tool_path(hermes_home, "uv");
    if managed.is_file() {
        return Ok(managed);
    }
    find_executable_on_path("uv", path_env, pathext)
        .ok_or_else(|| anyhow!("uv is not available"))
}

fn latest_windows_node_archive_name(
    index_html: &str,
    version_major: u32,
    arch: &str,
) -> Option<String> {
    let prefix = format!("node-v{version_major}.");
    let suffix = format!("-win-{arch}.zip");
    index_html
        .split('"')
        .flat_map(|part| part.split_whitespace())
        .filter_map(|part| {
            let name = part.trim_matches(|ch: char| {
                matches!(ch, '<' | '>' | '\'' | '"' | '=')
            });
            if name.starts_with(&prefix) && name.ends_with(&suffix) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .max_by(|left, right| compare_node_archive_versions(left, right))
}

fn latest_unix_node_archive_name(
    index_html: &str,
    version_major: u32,
    node_os: &str,
    arch: &str,
) -> Option<String> {
    let xz_suffix = format!("-{node_os}-{arch}.tar.xz");
    let gz_suffix = format!("-{node_os}-{arch}.tar.gz");
    latest_node_archive_name_with_suffix(index_html, version_major, &gz_suffix).or_else(|| {
        latest_node_archive_name_with_suffix(index_html, version_major, &xz_suffix)
    })
}

fn latest_node_archive_name_with_suffix(
    index_html: &str,
    version_major: u32,
    suffix: &str,
) -> Option<String> {
    let prefix = format!("node-v{version_major}.");
    index_html
        .split('"')
        .flat_map(|part| part.split_whitespace())
        .filter_map(|part| {
            let name = part.trim_matches(|ch: char| {
                matches!(ch, '<' | '>' | '\'' | '"' | '=')
            });
            if name.starts_with(&prefix) && name.ends_with(suffix) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .max_by(|left, right| compare_node_archive_versions(left, right))
}

fn unix_node_arch_slug(uname_arch: &str) -> Option<&'static str> {
    match uname_arch {
        "x86_64" => Some("x64"),
        "aarch64" | "arm64" => Some("arm64"),
        "armv7l" => Some("armv7l"),
        _ => None,
    }
}

fn current_unix_node_arch_slug() -> Result<String> {
    unix_node_arch_slug(std::env::consts::ARCH)
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("unsupported Unix architecture for Node.js: {}", std::env::consts::ARCH))
}

fn unix_node_os_slug() -> Result<String> {
    match std::env::consts::OS {
        "linux" => Ok("linux".to_string()),
        "macos" => Ok("darwin".to_string()),
        other => Err(anyhow!("unsupported Unix OS for Node.js: {other}")),
    }
}

fn unix_uv_os_slug() -> Result<String> {
    match std::env::consts::OS {
        "linux" => Ok("linux".to_string()),
        "macos" => Ok("darwin".to_string()),
        other => Err(anyhow!("unsupported Unix OS for uv: {other}")),
    }
}

fn is_termux_environment() -> bool {
    std::env::var_os("TERMUX_VERSION").is_some()
        || std::env::var("PREFIX")
            .map(|value| value.contains("com.termux/files/usr"))
            .unwrap_or(false)
}

fn latest_bundled_windows_node_archive_name(
    bundled_tools_dir: Option<&Path>,
    version_major: u32,
    arch: &str,
) -> Option<String> {
    let bundled_tools_dir = bundled_tools_dir?;
    let entries = fs::read_dir(bundled_tools_dir).ok()?;
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| windows_node_archive_matches(name, version_major, arch))
        .filter(|name| bundled_archive_is_manifest_listed(bundled_tools_dir, name))
        .max_by(|left, right| compare_node_archive_versions(left, right))
}

fn latest_bundled_unix_node_archive_name(
    bundled_tools_dir: Option<&Path>,
    version_major: u32,
    node_os: &str,
    arch: &str,
) -> Option<String> {
    latest_bundled_unix_node_archive_name_with_extension(
        bundled_tools_dir,
        version_major,
        node_os,
        arch,
        "tar.gz",
    )
    .or_else(|| {
        latest_bundled_unix_node_archive_name_with_extension(
            bundled_tools_dir,
            version_major,
            node_os,
            arch,
            "tar.xz",
        )
    })
}

fn latest_bundled_unix_node_archive_name_with_extension(
    bundled_tools_dir: Option<&Path>,
    version_major: u32,
    node_os: &str,
    arch: &str,
    extension: &str,
) -> Option<String> {
    let bundled_tools_dir = bundled_tools_dir?;
    let entries = fs::read_dir(bundled_tools_dir).ok()?;
    let expected_suffix = format!("-{node_os}-{arch}.{extension}");
    entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            name.starts_with("node-v")
                && name.ends_with(&expected_suffix)
                && node_archive_version_tuple(name).0 == version_major
        })
        .filter(|name| bundled_archive_is_manifest_listed(bundled_tools_dir, name))
        .max_by(|left, right| compare_node_archive_versions(left, right))
}

fn windows_node_archive_matches(name: &str, version_major: u32, arch: &str) -> bool {
    let expected_suffix = format!("-win-{arch}.zip");
    name.starts_with("node-v")
        && name.ends_with(&expected_suffix)
        && node_archive_version_tuple(name).0 == version_major
}

fn bootstrap_archive_cache_path(hermes_home: &Path, archive_name: &str) -> PathBuf {
    hermes_home.join("bootstrap-cache").join(archive_name)
}

fn resolve_bootstrap_archive_source(
    hermes_home: &Path,
    bundled_tools_dir: Option<&Path>,
    archive_name: &str,
) -> ResolvedBootstrapArchive {
    let cache_path = bootstrap_archive_cache_path(hermes_home, archive_name);
    let manifest_record =
        bundled_tools_dir.and_then(|dir| bootstrap_tools_manifest_archive(dir, archive_name));
    let expected_sha256 = manifest_record.as_ref().map(|record| record.sha256.clone());
    let expected_size_bytes = manifest_record.as_ref().and_then(|record| record.size_bytes);
    let bundled_path = bundled_tools_dir
        .map(|dir| dir.join(archive_name))
        .filter(|path| path.is_file())
        .filter(|path| {
            bundled_archive_matches_manifest(path, expected_sha256.as_deref(), expected_size_bytes)
        });
    if let Some(path) = bundled_path {
        return ResolvedBootstrapArchive {
            path,
            cache_path,
            kind: BootstrapArchiveSourceKind::Bundled,
            expected_sha256,
        };
    }
    ResolvedBootstrapArchive {
        path: cache_path.clone(),
        cache_path,
        kind: BootstrapArchiveSourceKind::Cache,
        expected_sha256,
    }
}

fn bootstrap_tools_manifest_sha256(bundled_tools_dir: &Path, archive_name: &str) -> Option<String> {
    bootstrap_tools_manifest_archive(bundled_tools_dir, archive_name).map(|record| record.sha256)
}

fn bootstrap_tools_manifest_archive(
    bundled_tools_dir: &Path,
    archive_name: &str,
) -> Option<BootstrapToolsManifestArchive> {
    if !bootstrap_archive_name_is_plain_file(archive_name) {
        return None;
    }
    let manifest_path = bundled_tools_dir.join(BOOTSTRAP_TOOLS_MANIFEST);
    if !manifest_path.is_file() {
        return None;
    }
    let manifest = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| serde_json::from_str::<BootstrapToolsManifest>(&text).ok())?;
    if manifest.schema_version != BOOTSTRAP_TOOLS_MANIFEST_SCHEMA_VERSION {
        return None;
    }
    manifest
        .archives
        .into_iter()
        .find(|archive| {
            bootstrap_archive_name_is_plain_file(&archive.name) && archive.name == archive_name
        })
        .and_then(|record| {
            let valid = record.sha256.len() == 64
                && record.sha256.chars().all(|ch| ch.is_ascii_hexdigit())
                && record.size_bytes.is_some()
                && record
                    .url
                    .as_deref()
                    .is_some_and(|url| url.starts_with("https://"))
                && bootstrap_tools_manifest_archive_matches_target(&record, archive_name);
            valid.then_some(record)
        })
}

fn bootstrap_tools_manifest_archive_matches_target(
    record: &BootstrapToolsManifestArchive,
    archive_name: &str,
) -> bool {
    let Some(target) = bootstrap_archive_target_from_name(archive_name) else {
        return record
            .platform
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            && record
                .arch
                .as_deref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
    };
    record.platform.as_deref() == Some(target.platform)
        && record.arch.as_deref() == Some(target.arch)
}

fn bootstrap_archive_target_from_name(name: &str) -> Option<BootstrapArchiveTarget> {
    for (suffix, platform, arch) in [
        ("-win-x64.zip", "windows", "x64"),
        ("-win-arm64.zip", "windows", "arm64"),
        ("-win-x86.zip", "windows", "x86"),
        ("-linux-x64.tar.gz", "linux", "x64"),
        ("-linux-arm64.tar.gz", "linux", "arm64"),
        ("-linux-x64.tar.xz", "linux", "x64"),
        ("-linux-arm64.tar.xz", "linux", "arm64"),
        ("-darwin-x64.tar.gz", "macos", "x64"),
        ("-darwin-arm64.tar.gz", "macos", "arm64"),
        ("-darwin-x64.tar.xz", "macos", "x64"),
        ("-darwin-arm64.tar.xz", "macos", "arm64"),
    ] {
        if name.starts_with("node-v") && name.ends_with(suffix) {
            return Some(BootstrapArchiveTarget { platform, arch });
        }
    }
    match name {
        "uv-x86_64-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-x86_64-pc-windows-msvc.zip"
        | "PortableGit-2.54.0-64-bit.7z.exe" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x64",
        }),
        "uv-aarch64-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-aarch64-pc-windows-msvc.zip"
        | "PortableGit-2.54.0-arm64.7z.exe" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "arm64",
        }),
        "uv-i686-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-i686-pc-windows-msvc.zip"
        | "MinGit-2.54.0-32-bit.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x86",
        }),
        "ffmpeg-windows-x64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x64",
        }),
        "ffmpeg-windows-arm64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "arm64",
        }),
        "ffmpeg-windows-x86.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x86",
        }),
        "playwright-browsers-windows-x64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x64",
        }),
        "playwright-browsers-windows-arm64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "arm64",
        }),
        "playwright-browsers-windows-x86.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x86",
        }),
        "electron-cache-windows-x64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x64",
        }),
        "electron-cache-windows-arm64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "arm64",
        }),
        "electron-cache-windows-x86.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x86",
        }),
        "npm-cache-windows-x64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x64",
        }),
        "npm-cache-windows-arm64.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "arm64",
        }),
        "npm-cache-windows-x86.zip" => Some(BootstrapArchiveTarget {
            platform: "windows",
            arch: "x86",
        }),
        "uv-x86_64-unknown-linux-gnu.tar.gz"
        | "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "x64",
        }),
        "uv-aarch64-unknown-linux-gnu.tar.gz"
        | "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "arm64",
        }),
        "uv-x86_64-apple-darwin.tar.gz"
        | "ripgrep-15.1.0-x86_64-apple-darwin.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "x64",
        }),
        "uv-aarch64-apple-darwin.tar.gz"
        | "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "arm64",
        }),
        "ffmpeg-linux-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "x64",
        }),
        "ffmpeg-linux-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "arm64",
        }),
        "playwright-browsers-linux-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "x64",
        }),
        "playwright-browsers-linux-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "arm64",
        }),
        "electron-cache-linux-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "x64",
        }),
        "electron-cache-linux-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "arm64",
        }),
        "npm-cache-linux-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "x64",
        }),
        "npm-cache-linux-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "linux",
            arch: "arm64",
        }),
        "ffmpeg-macos-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "x64",
        }),
        "ffmpeg-macos-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "arm64",
        }),
        "playwright-browsers-macos-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "x64",
        }),
        "playwright-browsers-macos-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "arm64",
        }),
        "electron-cache-macos-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "x64",
        }),
        "electron-cache-macos-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "arm64",
        }),
        "npm-cache-macos-x64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "x64",
        }),
        "npm-cache-macos-arm64.tar.gz" => Some(BootstrapArchiveTarget {
            platform: "macos",
            arch: "arm64",
        }),
        _ => None,
    }
}

fn bootstrap_archive_name_is_plain_file(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn bundled_archive_is_manifest_listed(bundled_tools_dir: &Path, archive_name: &str) -> bool {
    let manifest_path = bundled_tools_dir.join(BOOTSTRAP_TOOLS_MANIFEST);
    if !manifest_path.is_file() {
        return false;
    }
    bootstrap_tools_manifest_sha256(bundled_tools_dir, archive_name).is_some()
}

fn bundled_archive_matches_manifest(
    archive_path: &Path,
    expected_sha256: Option<&str>,
    expected_size_bytes: Option<u64>,
) -> bool {
    let Some(bundled_tools_dir) = archive_path.parent() else {
        return false;
    };
    let manifest_path = bundled_tools_dir.join(BOOTSTRAP_TOOLS_MANIFEST);
    if !manifest_path.is_file() {
        return false;
    }
    let Some(expected_sha256) = expected_sha256 else {
        return false;
    };
    if let Some(expected_size_bytes) = expected_size_bytes {
        let Ok(metadata) = fs::metadata(archive_path) else {
            return false;
        };
        if metadata.len() != expected_size_bytes {
            return false;
        }
    }
    let bytes = match fs::read(archive_path) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    crate::artifact::sha256_hex(&bytes).eq_ignore_ascii_case(expected_sha256)
}

fn windows_uv_archive_name(arch: &str) -> Option<&'static str> {
    match arch {
        "x64" => Some("uv-x86_64-pc-windows-msvc.zip"),
        "arm64" => Some("uv-aarch64-pc-windows-msvc.zip"),
        "x86" => Some("uv-i686-pc-windows-msvc.zip"),
        _ => None,
    }
}

fn unix_uv_archive_name(uv_os: &str, arch: &str) -> Option<&'static str> {
    match (uv_os, arch) {
        ("linux", "x64") => Some("uv-x86_64-unknown-linux-gnu.tar.gz"),
        ("linux", "arm64") => Some("uv-aarch64-unknown-linux-gnu.tar.gz"),
        ("darwin", "x64") => Some("uv-x86_64-apple-darwin.tar.gz"),
        ("darwin", "arm64") => Some("uv-aarch64-apple-darwin.tar.gz"),
        _ => None,
    }
}

fn compare_node_archive_versions(left: &str, right: &str) -> std::cmp::Ordering {
    node_archive_version_tuple(left).cmp(&node_archive_version_tuple(right))
}

fn node_archive_version_tuple(name: &str) -> (u32, u32, u32) {
    let version = name
        .strip_prefix("node-v")
        .and_then(|rest| rest.split_once('-').map(|(version, _)| version))
        .unwrap_or_default();
    let mut parts = version
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok());
    (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    )
}

fn install_windows_node_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    let parent = install_dir.parent().ok_or_else(|| {
        anyhow!(
            "Node install directory has no parent: {}",
            install_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating Node install parent {}", parent.display()))?;
    let tmp_dir = install_dir.with_extension("extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating Node extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let extracted_root = single_child_dir(&tmp_dir)?;
        remove_path_if_exists(install_dir)?;
        fs::rename(&extracted_root, install_dir).with_context(|| {
            format!(
                "moving Node runtime {} to {}",
                extracted_root.display(),
                install_dir.display()
            )
        })?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn install_unix_node_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    extract_unix_node_tar_gz(archive_path, install_dir)
}

fn extract_unix_node_tar_gz(archive_path: &Path, install_dir: &Path) -> Result<()> {
    let parent = install_dir.parent().ok_or_else(|| {
        anyhow!(
            "Node install directory has no parent: {}",
            install_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating Node install parent {}", parent.display()))?;
    let tmp_dir = install_dir.with_extension("extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating Node extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        extract_tar_gz_archive(archive_path, &tmp_dir, "Node")?;
        let extracted_root = single_child_dir(&tmp_dir)?;
        remove_path_if_exists(install_dir)?;
        fs::rename(&extracted_root, install_dir).with_context(|| {
            format!(
                "moving Node runtime {} to {}",
                extracted_root.display(),
                install_dir.display()
            )
        })?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn find_unix_managed_node(hermes_home: &Path) -> PathBuf {
    hermes_home.join("node").join("bin").join("node")
}

#[cfg(unix)]
fn link_unix_node_tools(plan: &UnixNodeRuntimeStagePlan) -> Result<()> {
    let link_dir = unix_node_link_dir();
    fs::create_dir_all(&link_dir)
        .with_context(|| format!("creating Node link dir {}", link_dir.display()))?;
    for (source, name) in [
        (&plan.node_bin, "node"),
        (&plan.npm_bin, "npm"),
        (&plan.npx_bin, "npx"),
    ] {
        let dest = link_dir.join(name);
        remove_path_if_exists(&dest)?;
        std::os::unix::fs::symlink(source, &dest).with_context(|| {
            format!("linking {} to {}", dest.display(), source.display())
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn link_unix_node_tools(_plan: &UnixNodeRuntimeStagePlan) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn unix_node_link_dir() -> PathBuf {
    if std::env::var_os("TERMUX_VERSION").is_some() {
        if let Some(prefix) = std::env::var_os("PREFIX") {
            return PathBuf::from(prefix).join("bin");
        }
    }
    if std::env::var("PREFIX")
        .map(|value| value.contains("com.termux/files/usr"))
        .unwrap_or(false)
    {
        return PathBuf::from(std::env::var_os("PREFIX").unwrap_or_default()).join("bin");
    }
    if cfg!(target_os = "linux") && current_user_is_root() {
        return PathBuf::from("/usr/local/bin");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("bin")
}

#[cfg(unix)]
fn current_user_is_root() -> bool {
    std::env::var("USER").map(|user| user == "root").unwrap_or(false)
        || std::env::var("SUDO_UID").is_ok()
}

fn install_windows_uv_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating uv install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("uv-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating uv extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let uv = find_file_named(&tmp_dir, "uv.exe")?;
        fs::copy(&uv, install_dir.join("uv.exe")).with_context(|| {
            format!(
                "copying uv binary {} to {}",
                uv.display(),
                install_dir.join("uv.exe").display()
            )
        })?;
        if let Ok(uvx) = find_file_named(&tmp_dir, "uvx.exe") {
            fs::copy(&uvx, install_dir.join("uvx.exe")).with_context(|| {
                format!(
                    "copying uvx binary {} to {}",
                    uvx.display(),
                    install_dir.join("uvx.exe").display()
                )
            })?;
        }
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn install_unix_uv_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    extract_unix_uv_tar_gz(archive_path, install_dir)
}

fn extract_unix_uv_tar_gz(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating uv install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("uv-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating uv extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        extract_tar_gz_archive(archive_path, &tmp_dir, "uv")?;
        let uv = find_file_named(&tmp_dir, "uv")?;
        let uv_dest = install_dir.join("uv");
        fs::copy(&uv, &uv_dest).with_context(|| {
            format!("copying uv binary {} to {}", uv.display(), uv_dest.display())
        })?;
        make_executable(&uv_dest)?;
        if let Ok(uvx) = find_file_named(&tmp_dir, "uvx") {
            let uvx_dest = install_dir.join("uvx");
            fs::copy(&uvx, &uvx_dest).with_context(|| {
                format!(
                    "copying uvx binary {} to {}",
                    uvx.display(),
                    uvx_dest.display()
                )
            })?;
            make_executable(&uvx_dest)?;
        }
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn install_windows_git_archive(plan: &WindowsGitRuntimeStagePlan) -> Result<()> {
    remove_path_if_exists(&plan.install_dir)?;
    fs::create_dir_all(&plan.install_dir)
        .with_context(|| format!("creating Git install dir {}", plan.install_dir.display()))?;
    if plan.is_zip {
        crate::artifact::extract_zip_archive(&plan.archive_path, &plan.install_dir)?;
        return Ok(());
    }
    let output_arg = format!("-o{}", plan.install_dir.display());
    let status = Command::new(&plan.archive_path)
        .args([output_arg.as_str(), "-y"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("extracting {}", plan.archive_path.display()))?;
    if status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "PortableGit extraction failed with exit {:?}",
        status.code()
    ))
}

fn install_windows_ripgrep_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating ripgrep install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("ripgrep-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating ripgrep extraction directory {}", tmp_dir.display()))?;

    let result = (|| -> Result<()> {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let extracted_rg = find_file_named(&tmp_dir, "rg.exe")?;
        fs::copy(&extracted_rg, install_dir.join("rg.exe")).with_context(|| {
            format!(
                "copying ripgrep binary {} to {}",
                extracted_rg.display(),
                install_dir.display()
            )
        })?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn install_windows_ffmpeg_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating ffmpeg install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("ffmpeg-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating ffmpeg extraction directory {}", tmp_dir.display()))?;

    let result = (|| -> Result<()> {
        crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        let extracted_ffmpeg = find_file_named(&tmp_dir, "ffmpeg.exe")?;
        fs::copy(&extracted_ffmpeg, install_dir.join("ffmpeg.exe")).with_context(|| {
            format!(
                "copying ffmpeg binary {} to {}",
                extracted_ffmpeg.display(),
                install_dir.display()
            )
        })?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn install_unix_ripgrep_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    extract_unix_ripgrep_tar_gz(archive_path, install_dir)
}

fn extract_unix_ripgrep_tar_gz(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating ripgrep install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("ripgrep-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating ripgrep extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        extract_tar_gz_archive(archive_path, &tmp_dir, "ripgrep")?;
        let rg = find_file_named(&tmp_dir, "rg")?;
        let rg_dest = install_dir.join("rg");
        fs::copy(&rg, &rg_dest).with_context(|| {
            format!(
                "copying ripgrep binary {} to {}",
                rg.display(),
                rg_dest.display()
            )
        })?;
        make_executable(&rg_dest)?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn extract_unix_ffmpeg_tar_gz(archive_path: &Path, install_dir: &Path) -> Result<()> {
    fs::create_dir_all(install_dir)
        .with_context(|| format!("creating ffmpeg install dir {}", install_dir.display()))?;
    let tmp_dir = install_dir.join("ffmpeg-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("creating ffmpeg extraction directory {}", tmp_dir.display()))?;

    let result: Result<()> = (|| {
        extract_tar_gz_archive(archive_path, &tmp_dir, "ffmpeg")?;
        let ffmpeg = find_file_named(&tmp_dir, "ffmpeg")?;
        let ffmpeg_dest = install_dir.join("ffmpeg");
        fs::copy(&ffmpeg, &ffmpeg_dest).with_context(|| {
            format!(
                "copying ffmpeg binary {} to {}",
                ffmpeg.display(),
                ffmpeg_dest.display()
            )
        })?;
        make_executable(&ffmpeg_dest)?;
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn extract_playwright_browsers_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    let parent = install_dir.parent().ok_or_else(|| {
        anyhow!(
            "Playwright browser install directory has no parent: {}",
            install_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating Playwright browser parent {}", parent.display()))?;
    let tmp_dir = parent.join("playwright-browsers-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "creating Playwright browser extraction directory {}",
            tmp_dir.display()
        )
    })?;

    let result: Result<()> = (|| {
        let archive_name = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if archive_name.ends_with(".zip") {
            crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        } else if archive_name.ends_with(".tar.gz") {
            extract_tar_gz_archive(archive_path, &tmp_dir, "Playwright browsers")?;
        } else {
            return Err(anyhow!(
                "unsupported Playwright browser archive format: {}",
                archive_path.display()
            ));
        }
        let cache_root = if tmp_dir.join("playwright-browsers").is_dir() {
            tmp_dir.join("playwright-browsers")
        } else {
            tmp_dir.clone()
        };
        if !playwright_browsers_dir_has_chromium(&cache_root)? {
            return Err(anyhow!(
                "Playwright browser archive did not contain Chromium cache directories"
            ));
        }
        remove_path_if_exists(install_dir)?;
        fs::create_dir_all(install_dir).with_context(|| {
            format!(
                "creating Playwright browser install dir {}",
                install_dir.display()
            )
        })?;
        copy_dir_contents(&cache_root, install_dir)?;
        if !playwright_browsers_dir_has_chromium(install_dir)? {
            return Err(anyhow!(
                "Playwright browser extraction did not produce Chromium cache directories"
            ));
        }
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn playwright_browsers_dir_has_chromium(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        let entry = entry.with_context(|| format!("reading entry under {}", path.display()))?;
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("chromium-") || name.starts_with("chromium_headless_shell-") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn extract_electron_cache_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    let parent = install_dir.parent().ok_or_else(|| {
        anyhow!(
            "Electron cache install directory has no parent: {}",
            install_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating Electron cache parent {}", parent.display()))?;
    let tmp_dir = parent.join("electron-cache-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "creating Electron cache extraction directory {}",
            tmp_dir.display()
        )
    })?;

    let result: Result<()> = (|| {
        let archive_name = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if archive_name.ends_with(".zip") {
            crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        } else if archive_name.ends_with(".tar.gz") {
            extract_tar_gz_archive(archive_path, &tmp_dir, "Electron cache")?;
        } else {
            return Err(anyhow!(
                "unsupported Electron cache archive format: {}",
                archive_path.display()
            ));
        }
        let cache_root = if tmp_dir.join("electron-cache").is_dir() {
            tmp_dir.join("electron-cache")
        } else {
            tmp_dir.clone()
        };
        if !electron_cache_dir_has_zip(&cache_root)? {
            return Err(anyhow!("Electron cache archive did not contain Electron zip files"));
        }
        remove_path_if_exists(install_dir)?;
        fs::create_dir_all(install_dir).with_context(|| {
            format!("creating Electron cache install dir {}", install_dir.display())
        })?;
        copy_dir_contents(&cache_root, install_dir)?;
        if !electron_cache_dir_has_zip(install_dir)? {
            return Err(anyhow!(
                "Electron cache extraction did not produce Electron zip files"
            ));
        }
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn electron_cache_dir_has_zip(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading file type for {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if file_type.is_file()
                && electron_zip_file_name(path.file_name().and_then(OsStr::to_str))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn extract_npm_cache_archive(archive_path: &Path, install_dir: &Path) -> Result<()> {
    let parent = install_dir.parent().ok_or_else(|| {
        anyhow!(
            "npm cache install directory has no parent: {}",
            install_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating npm cache parent {}", parent.display()))?;
    let tmp_dir = parent.join("npm-cache-extracting");
    remove_path_if_exists(&tmp_dir)?;
    fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "creating npm cache extraction directory {}",
            tmp_dir.display()
        )
    })?;

    let result: Result<()> = (|| {
        let archive_name = archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if archive_name.ends_with(".zip") {
            crate::artifact::extract_zip_archive(archive_path, &tmp_dir)?;
        } else if archive_name.ends_with(".tar.gz") {
            extract_tar_gz_archive(archive_path, &tmp_dir, "npm cache")?;
        } else {
            return Err(anyhow!(
                "unsupported npm cache archive format: {}",
                archive_path.display()
            ));
        }
        let cache_root = if tmp_dir.join("npm-cache").is_dir() {
            tmp_dir.join("npm-cache")
        } else {
            tmp_dir.clone()
        };
        if !npm_cache_dir_has_content(&cache_root)? {
            return Err(anyhow!("npm cache archive did not contain _cacache entries"));
        }
        remove_path_if_exists(install_dir)?;
        fs::create_dir_all(install_dir)
            .with_context(|| format!("creating npm cache install dir {}", install_dir.display()))?;
        copy_dir_contents(&cache_root, install_dir)?;
        if !npm_cache_dir_has_content(install_dir)? {
            return Err(anyhow!("npm cache extraction did not produce _cacache entries"));
        }
        Ok(())
    })();
    let cleanup = remove_path_if_exists(&tmp_dir);
    result?;
    cleanup
}

fn npm_cache_dir_has_content(path: &Path) -> Result<bool> {
    let cacache = path.join("_cacache");
    if !cacache.is_dir() {
        return Ok(false);
    }
    let mut stack = vec![cacache];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("reading file type for {}", path.display()))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn extract_tar_gz_archive(archive_path: &Path, destination_dir: &Path, label: &str) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("opening {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .with_context(|| format!("reading {}", archive_path.display()))?
    {
        let mut entry =
            entry.with_context(|| format!("reading entry from {}", archive_path.display()))?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(anyhow!(
                "{label} archive contains unsupported link entry: {}",
                entry.path()?.display()
            ));
        }
        let path = entry
            .path()
            .with_context(|| format!("reading entry path from {}", archive_path.display()))?
            .into_owned();
        if !archive_member_path_is_safe(&path) {
            return Err(anyhow!(
                "{label} archive contains unsafe entry path: {}",
                path.display()
            ));
        }
        let destination = destination_dir.join(&path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating archive directory {}", parent.display()))?;
        }
        entry
            .unpack(&destination)
            .with_context(|| format!("extracting {label} archive entry {}", path.display()))?;
    }
    Ok(())
}

fn archive_member_path_is_safe(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn find_windows_git_bash(install_dir: &Path) -> Option<PathBuf> {
    [
        install_dir.join("bin").join("bash.exe"),
        install_dir.join("usr").join("bin").join("bash.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn find_file_named(root: &Path, name: &str) -> Result<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry.with_context(|| format!("reading entry under {}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case(name))
                .unwrap_or(false)
            {
                return Ok(path);
            }
        }
    }
    Err(anyhow!("{} not found under {}", name, root.display()))
}

fn single_child_dir(parent: &Path) -> Result<PathBuf> {
    let mut dirs = fs::read_dir(parent)
        .with_context(|| format!("reading {}", parent.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    if dirs.len() != 1 {
        return Err(anyhow!(
            "expected exactly one extracted directory under {}, found {}",
            parent.display(),
            dirs.len()
        ));
    }
    Ok(dirs.remove(0))
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("removing directory {}", path.display()))
    } else {
        fs::remove_file(path)
            .with_context(|| format!("removing file {}", path.display()))
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("reading permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("setting executable permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn windows_node_arch_slug() -> String {
    let arch = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_else(|_| std::env::consts::ARCH.to_string());
    match arch.to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" => "x64".to_string(),
        "arm64" | "aarch64" => "arm64".to_string(),
        "x86" | "i386" | "i686" => "x86".to_string(),
        other => other.to_string(),
    }
}

fn prepend_process_path(entry: &Path) {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![entry.to_path_buf()];
    parts.extend(std::env::split_paths(&current));
    if let Ok(next) = std::env::join_paths(parts) {
        std::env::set_var("PATH", next);
    }
}

fn prepend_process_paths(entries: &[PathBuf]) {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = entries.to_vec();
    parts.extend(std::env::split_paths(&current));
    if let Ok(next) = std::env::join_paths(parts) {
        std::env::set_var("PATH", next);
    }
}

#[cfg(target_os = "windows")]
fn persist_windows_path_entry(entry: &Path) -> Result<()> {
    persist_windows_path_entries(&[entry.to_path_buf()])
}

#[cfg(not(target_os = "windows"))]
fn persist_windows_path_entry(_entry: &Path) -> Result<()> {
    Err(anyhow!("Windows PATH persistence is only available on Windows"))
}

#[cfg(target_os = "windows")]
fn persist_windows_path_entries(entries: &[PathBuf]) -> Result<()> {
    let current = hermes_manager::platform::read_windows_user_path()?;
    let mut parts = current
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .filter(|part| !part.trim().is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut changed = false;
    for entry in entries.iter().rev() {
        let entry_text = entry.display().to_string();
        let exists = parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case(&entry_text));
        if !exists {
            parts.insert(0, entry_text);
            changed = true;
        }
    }
    if changed {
        persist_windows_env_var("Path", &parts.join(";"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn persist_windows_path_entries(_entries: &[PathBuf]) -> Result<()> {
    Err(anyhow!("Windows PATH persistence is only available on Windows"))
}

#[cfg(target_os = "windows")]
fn persist_windows_env_var(name: &str, value: &str) -> Result<()> {
    hermes_manager::platform::write_windows_user_env_var(name, value)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn persist_windows_env_var(_name: &str, _value: &str) -> Result<()> {
    Err(anyhow!("Windows environment persistence is only available on Windows"))
}

fn find_node_executable<P>(hermes_home: &Path, path_env: P, pathext: &str) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let managed = if cfg!(target_os = "windows") {
        hermes_home.join("node").join("node.exe")
    } else {
        hermes_home.join("node").join("bin").join("node")
    };
    if managed.is_file() {
        return Some(managed);
    }
    find_executable_on_path("node", path_env, pathext)
}

fn find_npm_executable<P>(hermes_home: &Path, path_env: P, pathext: &str) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let managed = if cfg!(target_os = "windows") {
        hermes_home.join("node").join("npm.cmd")
    } else {
        hermes_home.join("node").join("bin").join("npm")
    };
    if managed.is_file() {
        return Some(managed);
    }
    find_executable_on_path("npm", path_env, pathext)
}

fn find_npx_executable<P>(npm: &Path, path_env: P, pathext: &str) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let npm_dir = npm.parent()?;
    for candidate in executable_candidates("npx", pathext) {
        let path = npm_dir.join(candidate);
        if path.is_file() {
            return Some(path);
        }
    }
    find_executable_on_path("npx", path_env, pathext)
}

fn run_node_dependency_command<const N: usize>(
    command: &Path,
    args: [&str; N],
    cwd: &Path,
    npm_cache_dir: &Path,
    playwright_browsers_dir: Option<&Path>,
) -> Result<()> {
    let args = args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>();
    run_node_dependency_command_args(
        command,
        &args,
        cwd,
        npm_cache_dir,
        playwright_browsers_dir,
    )
}

fn run_optional_node_dependency_command<const N: usize>(
    command: &Path,
    args: [&str; N],
    cwd: &Path,
    npm_cache_dir: &Path,
    playwright_browsers_dir: Option<&Path>,
) -> Option<String> {
    run_node_dependency_command(command, args, cwd, npm_cache_dir, playwright_browsers_dir)
        .err()
        .map(|err| err.to_string())
}

fn run_node_dependency_command_args(
    command: &Path,
    args: &[String],
    cwd: &Path,
    npm_cache_dir: &Path,
    playwright_browsers_dir: Option<&Path>,
) -> Result<()> {
    let mut child = Command::new(command);
    child
        .args(args)
        .current_dir(cwd)
        .env("npm_config_cache", npm_cache_dir);
    if let Some(path) = playwright_browsers_dir {
        child.env("PLAYWRIGHT_BROWSERS_PATH", path);
    }
    let output = child
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", command.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let output_text = process_output_text(&output);
    let mut message = format!(
        "{} {} failed with exit {:?}",
        command.display(),
        args.join(" "),
        output.status.code()
    );
    if let Some(hint) = npm_permission_diagnostic(
        &output_text,
        npm_cache_dir,
        cwd,
        std::env::consts::OS,
    ) {
        message.push_str("; ");
        message.push_str(&hint);
    }
    if !output_text.trim().is_empty() {
        message.push_str("; npm output: ");
        message.push_str(output_text.trim());
    }
    Err(anyhow!(message))
}

fn process_output_text(output: &Output) -> String {
    let mut text = String::new();
    text.push_str(String::from_utf8_lossy(&output.stdout).as_ref());
    if !text.is_empty() && !output.stderr.is_empty() {
        text.push('\n');
    }
    text.push_str(String::from_utf8_lossy(&output.stderr).as_ref());
    text
}

fn npm_permission_diagnostic(
    output: &str,
    npm_cache_dir: &Path,
    cwd: &Path,
    target_os: &str,
) -> Option<String> {
    let lower = output.to_ascii_lowercase();
    let permission_error = ["eacces", "eperm", "permission denied", "operation not permitted"]
        .iter()
        .any(|needle| lower.contains(needle));
    if !permission_error {
        return None;
    }
    let node_modules = cwd.join("node_modules");
    let locations = format!("{} and {}", npm_cache_dir.display(), node_modules.display());
    if target_os == "windows" {
        return Some(format!(
            "npm reported a filesystem permission problem; ensure this user can write to {locations}, \
             or delete those directories and retry"
        ));
    }
    Some(format!(
        "npm reported a filesystem permission problem; ensure this user can write to {locations}. \
         If ownership is stale, run: sudo chown -R \"$(id -un)\" \"{}\" \"{}\"; \
         then run npm --cache \"{}\" cache verify",
        npm_cache_dir.display(),
        node_modules.display(),
        npm_cache_dir.display()
    ))
}

fn run_desktop_pack_command(
    npm: &Path,
    desktop_dir: &Path,
    npm_cache_dir: &Path,
    electron_cache_dir: &Path,
) -> Result<()> {
    let user_mirror_is_set = std::env::var_os("ELECTRON_MIRROR").is_some();
    run_desktop_pack_command_with_mirror_policy(
        npm,
        desktop_dir,
        npm_cache_dir,
        electron_cache_dir,
        user_mirror_is_set,
    )
    .map(|_| ())
}

fn run_desktop_pack_command_with_mirror_policy(
    npm: &Path,
    desktop_dir: &Path,
    npm_cache_dir: &Path,
    electron_cache_dir: &Path,
    user_mirror_is_set: bool,
) -> Result<DesktopPackResult> {
    let mut electron_cache_dirs = vec![electron_cache_dir.to_path_buf()];
    electron_cache_dirs.extend(electron_cache_dirs_from_env(std::env::consts::OS));
    run_desktop_pack_command_with_recovery_policy(
        npm,
        desktop_dir,
        npm_cache_dir,
        electron_cache_dir,
        user_mirror_is_set,
        &electron_cache_dirs,
    )
}

fn run_desktop_pack_command_with_recovery_policy(
    npm: &Path,
    desktop_dir: &Path,
    npm_cache_dir: &Path,
    electron_cache_dir: &Path,
    user_mirror_is_set: bool,
    electron_cache_dirs: &[PathBuf],
) -> Result<DesktopPackResult> {
    let status = run_desktop_pack_attempt(
        npm,
        desktop_dir,
        npm_cache_dir,
        electron_cache_dir,
        None,
        !user_mirror_is_set,
    )?;
    if status.success() {
        return Ok(DesktopPackResult {
            fallback_mirror_used: false,
            purged_paths: Vec::new(),
        });
    }
    let purged_paths = clear_electron_build_cache(desktop_dir, electron_cache_dirs);
    if !purged_paths.is_empty() {
        let retry_status = run_desktop_pack_attempt(
            npm,
            desktop_dir,
            npm_cache_dir,
            electron_cache_dir,
            None,
            !user_mirror_is_set,
        )?;
        if retry_status.success() {
            return Ok(DesktopPackResult {
                fallback_mirror_used: false,
                purged_paths,
            });
        }
    }
    if !user_mirror_is_set {
        let retry_status = run_desktop_pack_attempt(
            npm,
            desktop_dir,
            npm_cache_dir,
            electron_cache_dir,
            Some(DESKTOP_ELECTRON_FALLBACK_MIRROR),
            false,
        )?;
        if retry_status.success() {
            return Ok(DesktopPackResult {
                fallback_mirror_used: true,
                purged_paths,
            });
        }
        return Err(anyhow!(
            "{} run pack failed after fallback mirror with exit {:?}",
            npm.display(),
            retry_status.code()
        ));
    }
    Err(anyhow!(
        "{} run pack failed with exit {:?}",
        npm.display(),
        status.code()
    ))
}

fn clear_electron_build_cache(desktop_dir: &Path, cache_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for dir in cache_dirs {
        remove_electron_zip_files(dir, &mut removed);
    }
    let release_dir = desktop_dir.join("release");
    let Ok(entries) = fs::read_dir(&release_dir) else {
        return removed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if name.ends_with("-unpacked") && fs::remove_dir_all(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

fn remove_electron_zip_files(dir: &Path, removed: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            remove_electron_zip_files(&path, removed);
        } else if file_type.is_file()
            && electron_zip_file_name(path.file_name().and_then(OsStr::to_str))
            && fs::remove_file(&path).is_ok()
        {
            removed.push(path);
        }
    }
}

fn electron_zip_file_name(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.starts_with("electron-") && name.ends_with(".zip"))
}

fn electron_cache_dirs_from_env(target_os: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_unique_env_path(&mut dirs, "electron_config_cache");
    push_unique_env_path(&mut dirs, "ELECTRON_CACHE");
    push_unique_env_path(&mut dirs, "ELECTRON_BUILDER_CACHE");
    match target_os {
        "windows" => {
            if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
                push_unique_path(
                    &mut dirs,
                    PathBuf::from(local_app_data).join("electron").join("Cache"),
                );
            }
            if let Some(home) = dirs::home_dir() {
                push_unique_path(
                    &mut dirs,
                    home.join("AppData").join("Local").join("electron").join("Cache"),
                );
            }
        }
        "macos" => {
            if let Some(home) = dirs::home_dir() {
                push_unique_path(&mut dirs, home.join("Library").join("Caches").join("electron"));
            }
        }
        _ => {
            if let Some(xdg_cache_home) = std::env::var_os("XDG_CACHE_HOME") {
                push_unique_path(&mut dirs, PathBuf::from(xdg_cache_home).join("electron"));
            }
            if let Some(home) = dirs::home_dir() {
                push_unique_path(&mut dirs, home.join(".cache").join("electron"));
            }
        }
    }
    dirs
}

fn push_unique_env_path(paths: &mut Vec<PathBuf>, name: &str) {
    if let Some(path) = std::env::var_os(name) {
        push_unique_path(paths, PathBuf::from(path));
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn run_desktop_pack_attempt(
    npm: &Path,
    desktop_dir: &Path,
    npm_cache_dir: &Path,
    electron_cache_dir: &Path,
    electron_mirror: Option<&str>,
    clear_electron_mirror: bool,
) -> Result<ExitStatus> {
    let mut child = Command::new(npm);
    child
        .args(["run", "pack"])
        .current_dir(desktop_dir)
        .env("npm_config_cache", npm_cache_dir)
        .env("electron_config_cache", electron_cache_dir)
        .env("ELECTRON_CACHE", electron_cache_dir)
        .env("ELECTRON_BUILDER_CACHE", electron_cache_dir)
        .env("CSC_IDENTITY_AUTO_DISCOVERY", "false")
        .env("WIN_CSC_LINK", "")
        .env("WIN_CSC_KEY_PASSWORD", "");
    if let Some(mirror) = electron_mirror {
        child.env("ELECTRON_MIRROR", mirror);
    } else if clear_electron_mirror {
        child.env_remove("ELECTRON_MIRROR");
    }
    child
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {} run pack", npm.display()))
}

fn find_built_desktop_app(install_root: &Path, target_os: &str) -> Option<PathBuf> {
    let release = install_root.join("apps").join("desktop").join("release");
    let candidates = match target_os {
        "windows" => vec![
            release.join("win-unpacked").join("Hermes.exe"),
            release.join("win-arm64-unpacked").join("Hermes.exe"),
        ],
        "macos" => vec![
            release.join("mac-arm64").join("Hermes.app"),
            release.join("mac").join("Hermes.app"),
        ],
        "linux" => vec![
            release.join("linux-unpacked").join("Hermes"),
            release.join("linux-unpacked").join("hermes"),
        ],
        _ => Vec::new(),
    };
    candidates.into_iter().find(|path| {
        if target_os == "macos" {
            path.is_dir()
        } else {
            path.is_file()
        }
    })
}

fn linux_chrome_sandbox_path(install_root: &Path) -> PathBuf {
    install_root
        .join("apps")
        .join("desktop")
        .join("release")
        .join("linux-unpacked")
        .join("chrome-sandbox")
}

fn configure_linux_chrome_sandbox(install_root: &Path) -> Result<()> {
    let sandbox = linux_chrome_sandbox_path(install_root);
    let metadata = match fs::symlink_metadata(&sandbox) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("reading sandbox metadata {}", sandbox.display()))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(());
    }
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    match linux_chrome_sandbox_repair_strategy(
        current_process_is_root(),
        noninteractive_sudo_available(&path_env),
    )? {
        LinuxChromeSandboxRepairStrategy::Root => {
            apply_linux_chrome_sandbox_root_permissions(&sandbox)?;
            Ok(())
        }
        LinuxChromeSandboxRepairStrategy::Sudo => {
            run_privileged_file_command("sudo", ["-n", "chown", "root:root"], &sandbox)?;
            run_privileged_file_command("sudo", ["-n", "chmod", "4755"], &sandbox)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxChromeSandboxRepairStrategy {
    Root,
    Sudo,
}

fn linux_chrome_sandbox_repair_strategy(
    user_is_root: bool,
    sudo_available: bool,
) -> Result<LinuxChromeSandboxRepairStrategy> {
    if user_is_root {
        return Ok(LinuxChromeSandboxRepairStrategy::Root);
    }
    if sudo_available {
        return Ok(LinuxChromeSandboxRepairStrategy::Sudo);
    }
    Err(anyhow!(
        "Cannot configure Electron sandbox helper without non-interactive sudo"
    ))
}

fn current_process_is_root() -> bool {
    current_process_euid().is_some_and(process_euid_is_root)
}

fn process_euid_is_root(euid: u32) -> bool {
    euid == 0
}

fn linux_chrome_sandbox_mode() -> u32 {
    0o4755
}

fn apply_linux_chrome_sandbox_root_permissions(path: &Path) -> Result<()> {
    set_file_owner_root(path)?;
    set_file_mode(path, linux_chrome_sandbox_mode())
}

#[cfg(unix)]
fn set_file_owner_root(path: &Path) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        anyhow!(
            "cannot set owner for path containing interior NUL: {}",
            path.display()
        )
    })?;
    let status = unsafe { libc::chown(c_path.as_ptr(), 0, 0) };
    if status == 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
        .with_context(|| format!("setting root owner on {}", path.display()))
}

#[cfg(not(unix))]
fn set_file_owner_root(path: &Path) -> Result<()> {
    Err(anyhow!(
        "root owner repair is only supported on Unix: {}",
        path.display()
    ))
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("reading permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(mode);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("setting mode {mode:o} on {}", path.display()))
}

#[cfg(not(unix))]
fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    Err(anyhow!(
        "file mode repair {mode:o} is only supported on Unix: {}",
        path.display()
    ))
}

#[cfg(unix)]
fn current_process_euid() -> Option<u32> {
    Some(unsafe { libc::geteuid() as u32 })
}

#[cfg(not(unix))]
fn current_process_euid() -> Option<u32> {
    None
}

fn run_privileged_file_command<const N: usize>(
    program: &str,
    args: [&str; N],
    path: &Path,
) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("running {program} for {}", path.display()))?;
    if status.success() {
        return Ok(());
    }
    Err(anyhow!(
        "{program} failed for {} with exit {:?}",
        path.display(),
        status.code()
    ))
}

fn node_version_satisfies_build(node: &Path) -> bool {
    let output = Command::new(node)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let Ok(version) = String::from_utf8(output.stdout) else {
        return false;
    };
    node_version_string_satisfies_build(version.trim())
}

fn node_version_string_satisfies_build(version: &str) -> bool {
    let cleaned = version
        .trim_start_matches('v')
        .split_once('-')
        .map(|(base, _)| base)
        .unwrap_or(version);
    let parts = cleaned
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return false;
    }
    let major = parts[0];
    let minor = parts[1];
    (major == 20 && minor >= 19) || (major == 22 && minor >= 12) || major > 22
}

fn stage_execution_mode(name: &str) -> StageExecutionMode {
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "repository" | "python" | "venv" | "dependencies" | "python-deps" | "platform-sdks"
    ) {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if node_deps_stage_is_native_first_for_target(std::env::consts::OS)
        && name.eq_ignore_ascii_case("node-deps") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if desktop_stage_is_native_first_for_target(std::env::consts::OS)
        && name.eq_ignore_ascii_case("desktop") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if node_stage_is_native_first_for_target(std::env::consts::OS)
        && name.eq_ignore_ascii_case("node") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if name.eq_ignore_ascii_case("uv") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if cfg!(target_os = "windows") && name.eq_ignore_ascii_case("git") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if cfg!(target_os = "windows") && name.eq_ignore_ascii_case("system-packages") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if !cfg!(target_os = "windows") && name.eq_ignore_ascii_case("system-packages") {
        return StageExecutionMode::NativeWithScriptFallback;
    }
    if !cfg!(target_os = "windows") && name.eq_ignore_ascii_case("prerequisites") {
        return StageExecutionMode::ProbeThenScript;
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "bootstrap-marker" | "config" | "config-templates" | "complete" | "path"
    ) {
        return StageExecutionMode::Native;
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "git" | "node" | "system-packages" | "node-deps" | "desktop"
    ) {
        return StageExecutionMode::ProbeThenScript;
    }
    StageExecutionMode::Script
}

fn node_deps_stage_is_native_first_for_target(target_os: &str) -> bool {
    matches!(target_os, "windows" | "macos" | "linux")
}

fn node_stage_is_native_first_for_target(target_os: &str) -> bool {
    matches!(target_os, "windows" | "macos" | "linux")
}

fn desktop_stage_is_native_first_for_target(target_os: &str) -> bool {
    matches!(target_os, "windows" | "macos" | "linux")
}

fn find_executable_on_path<P>(name: &str, path_env: P, pathext: &str) -> Option<PathBuf>
where
    P: AsRef<OsStr>,
{
    let candidates = executable_candidates(name, pathext);
    for dir in std::env::split_paths(path_env.as_ref()) {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn executable_candidates(name: &str, pathext: &str) -> Vec<String> {
    let has_extension = Path::new(name).extension().is_some();
    if has_extension {
        return vec![name.to_string()];
    }

    let mut out = vec![name.to_string()];
    for ext in pathext.split(';') {
        let ext = ext.trim();
        if ext.is_empty() {
            continue;
        }
        out.push(format!("{name}{ext}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::StageInfo;
    use std::io::Write;
    use std::path::PathBuf;
    use zip::write::SimpleFileOptions;

    fn stage(name: &str) -> StageInfo {
        StageInfo {
            name: name.to_string(),
            title: format!("Stage {name}"),
            category: "install".to_string(),
            needs_user_input: false,
        }
    }

    fn write_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write_test_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(name).unwrap();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive.append(&header, *bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn find_executable_on_path_uses_windows_pathext_candidates() {
        let root = std::env::temp_dir().join(format!(
            "hermes-orchestrator-path-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("uv.EXE");
        std::fs::write(&exe, b"stub").unwrap();

        let found = find_executable_on_path("uv", &root, ".COM;.EXE;.BAT").unwrap();

        assert_eq!(found, exe);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_stage_plan_classifies_native_probe_and_script_stages() {
        let stages = vec![
            stage("repository"),
            stage("path"),
            stage("python"),
            stage("uv"),
            stage("git"),
            stage("node"),
            stage("system-packages"),
            stage("platform-sdks"),
            stage("node-deps"),
            stage("desktop"),
            stage("venv"),
            stage("dependencies"),
            stage("python-deps"),
        ];
        let plan = build_stage_plan(&stages, false);

        assert_eq!(plan.len(), 13);
        assert_eq!(plan[0].name, "repository");
        assert_eq!(plan[0].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[0].script_fallback, true);
        assert_eq!(plan[1].name, "path");
        assert_eq!(plan[1].execution, StageExecutionMode::Native);
        assert_eq!(plan[1].script_fallback, false);
        assert_eq!(plan[2].name, "python");
        assert_eq!(plan[2].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[2].script_fallback, true);
        assert_eq!(plan[3].name, "uv");
        assert_eq!(plan[3].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[3].rust_probe, false);
        assert_eq!(plan[3].script_fallback, true);
        let git_execution = if cfg!(target_os = "windows") {
            StageExecutionMode::NativeWithScriptFallback
        } else {
            StageExecutionMode::ProbeThenScript
        };
        assert_eq!(plan[4].name, "git");
        assert_eq!(plan[4].execution, git_execution);
        assert_eq!(plan[4].rust_probe, !cfg!(target_os = "windows"));
        assert_eq!(plan[4].script_fallback, true);
        let node_native = node_stage_is_native_first_for_target(std::env::consts::OS);
        let node_execution = if node_native {
            StageExecutionMode::NativeWithScriptFallback
        } else {
            StageExecutionMode::ProbeThenScript
        };
        assert_eq!(plan[5].name, "node");
        assert_eq!(plan[5].execution, node_execution);
        assert_eq!(plan[5].rust_probe, !node_native);
        assert_eq!(plan[5].script_fallback, true);
        assert_eq!(plan[6].name, "system-packages");
        assert_eq!(plan[6].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[6].rust_probe, false);
        assert_eq!(plan[6].script_fallback, true);
        assert_eq!(plan[7].name, "platform-sdks");
        assert_eq!(plan[7].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[7].script_fallback, true);
        let node_deps_native = node_deps_stage_is_native_first_for_target(std::env::consts::OS);
        let node_deps_execution = if node_deps_native {
            StageExecutionMode::NativeWithScriptFallback
        } else {
            StageExecutionMode::ProbeThenScript
        };
        assert_eq!(plan[8].name, "node-deps");
        assert_eq!(plan[8].execution, node_deps_execution);
        assert_eq!(plan[8].rust_probe, !node_deps_native);
        assert_eq!(plan[8].script_fallback, true);
        assert_eq!(plan[9].name, "desktop");
        let desktop_native = desktop_stage_is_native_first_for_target(std::env::consts::OS);
        let desktop_execution = if desktop_native {
            StageExecutionMode::NativeWithScriptFallback
        } else {
            StageExecutionMode::ProbeThenScript
        };
        assert_eq!(plan[9].execution, desktop_execution);
        assert_eq!(plan[9].rust_probe, !desktop_native);
        assert_eq!(plan[9].script_fallback, true);
        assert_eq!(plan[10].name, "venv");
        assert_eq!(plan[10].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[10].script_fallback, true);
        assert_eq!(plan[11].name, "dependencies");
        assert_eq!(plan[11].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[11].script_fallback, true);
        assert_eq!(plan[12].name, "python-deps");
        assert_eq!(plan[12].execution, StageExecutionMode::NativeWithScriptFallback);
        assert_eq!(plan[12].script_fallback, true);
    }

    #[test]
    fn build_stage_plan_records_script_only_reasons() {
        let stages = vec![
            StageInfo {
                name: "configure".to_string(),
                title: "Configure".to_string(),
                category: "post-install".to_string(),
                needs_user_input: true,
            },
            stage("legacy"),
        ];
        let plan = build_stage_plan(&stages, false);

        assert_eq!(plan[0].execution, StageExecutionMode::Script);
        assert_eq!(
            plan[0].script_reason.as_deref(),
            Some("requires user input; handled by post-install UI")
        );
        assert_eq!(plan[1].execution, StageExecutionMode::Script);
        assert_eq!(
            plan[1].script_reason.as_deref(),
            Some("not yet ported to Rust; delegated to install script")
        );
    }

    #[test]
    fn node_deps_stage_is_native_first_on_desktop_platforms() {
        for target_os in ["windows", "macos", "linux"] {
            assert!(
                node_deps_stage_is_native_first_for_target(target_os),
                "{target_os} should run node-deps natively before script fallback"
            );
        }
        assert!(!node_deps_stage_is_native_first_for_target("freebsd"));
    }

    #[test]
    fn desktop_stage_is_native_first_on_desktop_platforms() {
        for target_os in ["windows", "macos", "linux"] {
            assert!(
                desktop_stage_is_native_first_for_target(target_os),
                "{target_os} should run desktop build natively before script fallback"
            );
        }
        assert!(!desktop_stage_is_native_first_for_target("freebsd"));
    }

    #[test]
    fn node_stage_is_native_first_on_packaged_platforms() {
        for target_os in ["windows", "macos", "linux"] {
            assert!(
                node_stage_is_native_first_for_target(target_os),
                "{target_os} should run Node natively before script fallback"
            );
        }
        assert!(!node_stage_is_native_first_for_target("freebsd"));
    }

    #[test]
    fn install_state_report_keeps_user_data_out_of_the_report() {
        let hermes_home = PathBuf::from("C:/Users/example/AppData/Local/hermes");
        let report = install_state_report(&hermes_home, Vec::new());

        assert_eq!(report.hermes_home, hermes_home);
        assert_eq!(report.install_root, hermes_home.join("hermes-agent"));
        assert!(report.tools.is_empty());
    }

    #[test]
    fn summarize_plan_reports_native_probe_and_script_coverage() {
        let hermes_home = PathBuf::from("C:/Users/example/AppData/Local/hermes");
        let report = install_state_report(
            &hermes_home,
            vec![ToolProbe {
                name: "uv".to_string(),
                path: None,
            }],
        );
        let stages = vec![stage("repository"), stage("path"), stage("uv"), stage("venv")];
        let plan = build_stage_plan(&stages, false);

        let summary = summarize_plan(&report, &plan);

        let native_count = 4;
        let probe_count = 0;
        assert!(summary.contains(&format!("native_stages={native_count}")));
        assert!(summary.contains(&format!("probe_stages={probe_count}")));
        assert!(summary.contains("script_stages=0"));
        assert!(summary.contains("total_stages=4"));
        assert!(summary.contains("uv=missing"));
    }

    #[test]
    fn summarize_plan_reports_script_only_reasons() {
        let hermes_home = PathBuf::from("C:/Users/example/AppData/Local/hermes");
        let report = install_state_report(&hermes_home, Vec::new());
        let stages = vec![StageInfo {
            name: "configure".to_string(),
            title: "Configure".to_string(),
            category: "post-install".to_string(),
            needs_user_input: true,
        }];
        let plan = build_stage_plan(&stages, false);

        let summary = summarize_plan(&report, &plan);

        assert!(summary.contains(
            "script_reasons=[configure=requires user input; handled by post-install UI]"
        ));
    }

    #[test]
    fn native_bootstrap_manifest_matches_windows_stage_contract() {
        let manifest = native_bootstrap_manifest(crate::install_script::ScriptKind::Ps1, true);
        let names = manifest
            .stages
            .iter()
            .map(|stage| stage.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(manifest.protocol_version, Some(1));
        assert_eq!(
            names,
            vec![
                "uv",
                "python",
                "git",
                "node",
                "system-packages",
                "repository",
                "venv",
                "dependencies",
                "node-deps",
                "desktop",
                "path",
                "config-templates",
                "platform-sdks",
                "bootstrap-marker",
                "configure",
                "gateway",
            ]
        );
        assert_eq!(manifest.stages[9].title, "Building desktop app");
        assert!(manifest.stages[14].needs_user_input);
    }

    #[test]
    fn native_bootstrap_manifest_matches_unix_stage_contract() {
        let manifest = native_bootstrap_manifest(crate::install_script::ScriptKind::Sh, false);
        let names = manifest
            .stages
            .iter()
            .map(|stage| stage.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(manifest.protocol_version, Some(1));
        assert_eq!(
            names,
            vec![
                "uv",
                "node",
                "python",
                "system-packages",
                "repository",
                "venv",
                "python-deps",
                "node-deps",
                "path",
                "config",
                "platform-sdks",
                "setup",
                "gateway",
                "bootstrap-marker",
                "complete",
            ]
        );
        assert_eq!(manifest.stages[9].title, "Prepare config and skills");
        assert_eq!(
            manifest.stages[10].title,
            "Install messaging platform SDKs"
        );
        assert!(manifest.stages[11].needs_user_input);
    }

    #[test]
    fn interactive_stage_skip_result_only_skips_user_input_stages() {
        let setup = stage_info("setup", "Configure API keys and settings", "configuration", true);
        let path = stage_info("path", "Install hermes command", "runtime", false);

        let skipped = interactive_stage_skip_result(&setup).unwrap();

        assert_eq!(skipped.stage, "setup");
        assert_eq!(skipped.ok, true);
        assert_eq!(skipped.skipped, true);
        assert_eq!(
            skipped.reason.as_deref(),
            Some("requires user input; handled by post-install UI")
        );
        assert!(skipped.data.is_none());
        assert!(interactive_stage_skip_result(&path).is_none());
    }

    #[test]
    fn satisfied_tool_stage_skip_result_skips_only_when_tools_are_present() {
        let root = std::env::temp_dir().join(format!(
            "hermes-tool-skip-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let tools = root.join("tools");
        std::fs::create_dir_all(hermes_home.join("bin")).unwrap();
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(hermes_home.join("bin").join("uv.exe"), b"uv").unwrap();
        std::fs::write(tools.join("git.exe"), b"git").unwrap();
        std::fs::write(tools.join("node.exe"), b"node").unwrap();
        std::fs::write(tools.join("npm.cmd"), b"npm").unwrap();
        std::fs::write(tools.join("rg.exe"), b"rg").unwrap();

        let uv = stage_info("uv", "Installing uv package manager", "prereqs", false);
        let git = stage_info("git", "Installing Git", "prereqs", false);
        let node = stage_info("node", "Detecting Node.js", "prereqs", false);
        let system_packages = stage_info(
            "system-packages",
            "Installing ripgrep and ffmpeg",
            "prereqs",
            false,
        );
        let venv = stage_info(
            "venv",
            "Creating Python virtual environment",
            "install",
            false,
        );

        let uv_result = satisfied_tool_stage_skip_result(&uv, &hermes_home, &tools, ".EXE").unwrap();
        let git_result = satisfied_tool_stage_skip_result(&git, &hermes_home, &tools, ".EXE").unwrap();
        let old_node_result = satisfied_tool_stage_skip_result(&node, &hermes_home, &tools, ".EXE;.CMD");

        assert_eq!(uv_result.stage, "uv");
        assert_eq!(uv_result.ok, true);
        assert_eq!(uv_result.skipped, true);
        assert_eq!(
            uv_result.reason.as_deref(),
            Some("required tool already available")
        );
        assert_eq!(git_result.stage, "git");
        assert!(old_node_result.is_none());
        let new_node_result = satisfied_tool_stage_skip_result_with_node_probe(
            &node,
            &hermes_home,
            &tools,
            ".EXE;.CMD",
            |_| true,
        )
        .unwrap();
        assert_eq!(new_node_result.stage, "node");
        assert_eq!(new_node_result.reason.as_deref(), Some("required tool already available"));
        assert!(satisfied_tool_stage_skip_result(
            &system_packages,
            &hermes_home,
            &tools,
            ".EXE"
        )
        .is_none());
        std::fs::write(tools.join("ffmpeg.exe"), b"ffmpeg").unwrap();
        assert!(satisfied_tool_stage_skip_result(
            &system_packages,
            &hermes_home,
            &tools,
            ".EXE"
        )
        .is_some());
        assert!(satisfied_tool_stage_skip_result(&venv, &hermes_home, &tools, ".EXE").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn node_deps_skip_result_skips_only_when_npm_is_absent() {
        let root = std::env::temp_dir().join(format!(
            "hermes-node-deps-skip-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let tools = root.join("tools");
        std::fs::create_dir_all(&tools).unwrap();

        let node_deps = stage_info("node-deps", "Installing Node.js dependencies", "install", false);
        let skipped = node_deps_skip_result(&node_deps, &hermes_home, &tools, ".EXE;.CMD")
            .unwrap();

        assert_eq!(skipped.stage, "node-deps");
        assert_eq!(skipped.ok, true);
        assert_eq!(skipped.skipped, true);
        assert_eq!(skipped.reason.as_deref(), Some("npm not available"));
        std::fs::write(tools.join("npm.cmd"), b"npm").unwrap();
        assert!(node_deps_skip_result(&node_deps, &hermes_home, &tools, ".EXE;.CMD").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_ripgrep_runtime_stage_plan_matches_pinned_release_asset() {
        let root = std::env::temp_dir().join(format!(
            "hermes-ripgrep-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let x64 = windows_ripgrep_runtime_stage_plan(&hermes_home, "x64").unwrap();
        assert_eq!(x64.version, "15.1.0");
        assert_eq!(
            x64.archive_name,
            "ripgrep-15.1.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            x64.download_url,
            concat!(
                "https://github.com/BurntSushi/ripgrep/releases/download/",
                "15.1.0/ripgrep-15.1.0-x86_64-pc-windows-msvc.zip"
            )
        );
        assert_eq!(x64.install_dir, hermes_home.join("bin"));
        assert_eq!(x64.rg_exe, hermes_home.join("bin").join("rg.exe"));

        let arm = windows_ripgrep_runtime_stage_plan(&hermes_home, "arm64").unwrap();
        assert_eq!(
            arm.archive_name,
            "ripgrep-15.1.0-aarch64-pc-windows-msvc.zip"
        );
        let x86 = windows_ripgrep_runtime_stage_plan(&hermes_home, "x86").unwrap();
        assert_eq!(x86.archive_name, "ripgrep-15.1.0-i686-pc-windows-msvc.zip");
        assert!(windows_ripgrep_runtime_stage_plan(&hermes_home, "mips").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_windows_ripgrep_archive_copies_rg_exe_from_nested_zip() {
        let root = std::env::temp_dir().join(format!(
            "hermes-ripgrep-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("ripgrep.zip");
        let install_dir = root.join("bin");
        write_test_zip(
            &archive,
            &[("ripgrep-15.1.0-x86_64-pc-windows-msvc/rg.exe", b"fake rg")],
        );

        install_windows_ripgrep_archive(&archive, &install_dir).unwrap();

        assert_eq!(std::fs::read(install_dir.join("rg.exe")).unwrap(), b"fake rg");
        assert!(!install_dir.join("ripgrep-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_unix_ripgrep_archive_copies_rg_from_nested_tar_gz() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-ripgrep-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("ripgrep.tar.gz");
        write_test_tar_gz(
            &archive,
            &[("ripgrep-15.1.0-x86_64-unknown-linux-musl/rg", b"fake rg")],
        );
        let install_dir = root.join("bin");

        extract_unix_ripgrep_tar_gz(&archive, &install_dir).unwrap();

        assert_eq!(std::fs::read(install_dir.join("rg")).unwrap(), b"fake rg");
        assert!(!install_dir.join("ripgrep-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_ripgrep_runtime_stage_plan_matches_pinned_release_assets() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-ripgrep-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let linux_x64 = unix_ripgrep_runtime_stage_plan(&hermes_home, "linux", "x64").unwrap();
        assert_eq!(linux_x64.version, "15.1.0");
        assert_eq!(
            linux_x64.archive_name,
            "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            linux_x64.download_url,
            concat!(
                "https://github.com/BurntSushi/ripgrep/releases/download/",
                "15.1.0/ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz"
            )
        );
        assert_eq!(linux_x64.install_dir, hermes_home.join("bin"));
        assert_eq!(linux_x64.rg_bin, hermes_home.join("bin").join("rg"));

        let linux_arm = unix_ripgrep_runtime_stage_plan(&hermes_home, "linux", "arm64").unwrap();
        assert_eq!(
            linux_arm.archive_name,
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz"
        );
        let mac_arm = unix_ripgrep_runtime_stage_plan(&hermes_home, "macos", "arm64").unwrap();
        assert_eq!(
            mac_arm.archive_name,
            "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz"
        );
        assert!(unix_ripgrep_runtime_stage_plan(&hermes_home, "freebsd", "x64").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_ffmpeg_runtime_stage_plan_matches_bundled_archive_contract() {
        let root = std::env::temp_dir().join(format!(
            "hermes-windows-ffmpeg-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let plan = windows_ffmpeg_runtime_stage_plan(&hermes_home, "x64").unwrap();

        assert_eq!(plan.archive_name, "ffmpeg-windows-x64.zip");
        assert_eq!(plan.install_dir, hermes_home.join("bin"));
        assert_eq!(plan.ffmpeg_exe, hermes_home.join("bin").join("ffmpeg.exe"));
        assert!(windows_ffmpeg_runtime_stage_plan(&hermes_home, "mips").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_windows_ffmpeg_archive_copies_ffmpeg_exe_from_nested_zip() {
        let root = std::env::temp_dir().join(format!(
            "hermes-windows-ffmpeg-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("ffmpeg.zip");
        let install_dir = root.join("bin");
        write_test_zip(&archive, &[("ffmpeg/bin/ffmpeg.exe", b"fake ffmpeg")]);

        install_windows_ffmpeg_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("ffmpeg.exe")).unwrap(),
            b"fake ffmpeg"
        );
        assert!(!install_dir.join("ffmpeg-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_ffmpeg_runtime_stage_plan_matches_bundled_archive_contract() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-ffmpeg-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let linux = unix_ffmpeg_runtime_stage_plan(&hermes_home, "linux", "x64").unwrap();
        assert_eq!(linux.archive_name, "ffmpeg-linux-x64.tar.gz");
        assert_eq!(linux.install_dir, hermes_home.join("bin"));
        assert_eq!(linux.ffmpeg_bin, hermes_home.join("bin").join("ffmpeg"));

        let macos = unix_ffmpeg_runtime_stage_plan(&hermes_home, "macos", "arm64").unwrap();
        assert_eq!(macos.archive_name, "ffmpeg-macos-arm64.tar.gz");
        assert!(unix_ffmpeg_runtime_stage_plan(&hermes_home, "freebsd", "x64").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_unix_ffmpeg_archive_copies_ffmpeg_from_nested_tar_gz() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-ffmpeg-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("ffmpeg.tar.gz");
        write_test_tar_gz(&archive, &[("ffmpeg/bin/ffmpeg", b"fake ffmpeg")]);
        let install_dir = root.join("bin");

        extract_unix_ffmpeg_tar_gz(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("ffmpeg")).unwrap(),
            b"fake ffmpeg"
        );
        assert!(!install_dir.join("ffmpeg-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn playwright_browsers_runtime_stage_plan_matches_bundled_archive_contract() {
        let root = std::env::temp_dir().join(format!(
            "hermes-playwright-browsers-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let windows = playwright_browsers_runtime_stage_plan(&hermes_home, "windows", "x64")
            .expect("Windows x64 Playwright browser archive should be supported");
        assert_eq!(windows.archive_name, "playwright-browsers-windows-x64.zip");
        assert_eq!(windows.install_dir, hermes_home.join("playwright-browsers"));

        let linux = playwright_browsers_runtime_stage_plan(&hermes_home, "linux", "arm64")
            .expect("Linux arm64 Playwright browser archive should be supported");
        assert_eq!(linux.archive_name, "playwright-browsers-linux-arm64.tar.gz");

        let macos = playwright_browsers_runtime_stage_plan(&hermes_home, "darwin", "x64")
            .expect("macOS x64 Playwright browser archive should be supported");
        assert_eq!(macos.archive_name, "playwright-browsers-macos-x64.tar.gz");
        assert!(playwright_browsers_runtime_stage_plan(&hermes_home, "freebsd", "x64").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_playwright_browsers_archive_copies_nested_zip_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-playwright-browsers-zip-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("playwright-browsers.zip");
        write_test_zip(
            &archive,
            &[("playwright-browsers/chromium-1208/chrome-linux/chrome", b"chrome")],
        );
        let install_dir = root.join("home").join("playwright-browsers");

        extract_playwright_browsers_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("chromium-1208").join("chrome-linux").join("chrome"))
                .unwrap(),
            b"chrome"
        );
        assert!(!install_dir
            .parent()
            .unwrap()
            .join("playwright-browsers-extracting")
            .exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_playwright_browsers_archive_copies_nested_tar_gz_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-playwright-browsers-tar-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("playwright-browsers.tar.gz");
        write_test_tar_gz(
            &archive,
            &[("playwright-browsers/chromium_headless_shell-1208/headless_shell", b"shell")],
        );
        let install_dir = root.join("home").join("playwright-browsers");

        extract_playwright_browsers_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("chromium_headless_shell-1208").join("headless_shell"))
                .unwrap(),
            b"shell"
        );
        assert!(!install_dir
            .parent()
            .unwrap()
            .join("playwright-browsers-extracting")
            .exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_playwright_browsers_archive_installs_before_npx_download() {
        let root = std::env::temp_dir().join(format!(
            "hermes-playwright-browsers-bundled-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(
            &bundled.join("playwright-browsers-windows-x64.zip"),
            &[("playwright-browsers/chromium-1208/chrome-win/chrome.exe", b"chrome")],
        );

        let source = install_bundled_playwright_browsers_if_available(
            &hermes_home,
            Some(&bundled),
            "windows",
            "x64",
        )
        .unwrap();

        assert_eq!(
            source,
            Some((
                "playwright-browsers-windows-x64.zip".to_string(),
                BootstrapArchiveSourceKind::Bundled,
            ))
        );
        assert_eq!(
            std::fs::read(
                hermes_home
                    .join("playwright-browsers")
                    .join("chromium-1208")
                    .join("chrome-win")
                    .join("chrome.exe")
            )
            .unwrap(),
            b"chrome"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn electron_cache_runtime_stage_plan_matches_bundled_archive_contract() {
        let root = std::env::temp_dir().join(format!(
            "hermes-electron-cache-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let windows = electron_cache_runtime_stage_plan(&hermes_home, "windows", "x64")
            .expect("Windows x64 Electron cache archive should be supported");
        assert_eq!(windows.archive_name, "electron-cache-windows-x64.zip");
        assert_eq!(windows.install_dir, hermes_home.join("electron-cache"));

        let linux = electron_cache_runtime_stage_plan(&hermes_home, "linux", "arm64")
            .expect("Linux arm64 Electron cache archive should be supported");
        assert_eq!(linux.archive_name, "electron-cache-linux-arm64.tar.gz");

        let macos = electron_cache_runtime_stage_plan(&hermes_home, "darwin", "x64")
            .expect("macOS x64 Electron cache archive should be supported");
        assert_eq!(macos.archive_name, "electron-cache-macos-x64.tar.gz");
        assert!(electron_cache_runtime_stage_plan(&hermes_home, "freebsd", "x64").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_electron_cache_archive_copies_nested_zip_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-electron-cache-zip-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("electron-cache.zip");
        write_test_zip(
            &archive,
            &[("electron-cache/electron-v40.9.3-win32-x64.zip", b"electron zip")],
        );
        let install_dir = root.join("home").join("electron-cache");

        extract_electron_cache_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("electron-v40.9.3-win32-x64.zip")).unwrap(),
            b"electron zip"
        );
        assert!(!install_dir.parent().unwrap().join("electron-cache-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_electron_cache_archive_copies_nested_tar_gz_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-electron-cache-tar-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("electron-cache.tar.gz");
        write_test_tar_gz(
            &archive,
            &[("electron-cache/electron-v40.9.3-linux-arm64.zip", b"electron zip")],
        );
        let install_dir = root.join("home").join("electron-cache");

        extract_electron_cache_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(install_dir.join("electron-v40.9.3-linux-arm64.zip")).unwrap(),
            b"electron zip"
        );
        assert!(!install_dir.parent().unwrap().join("electron-cache-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_electron_cache_archive_installs_before_desktop_pack() {
        let root = std::env::temp_dir().join(format!(
            "hermes-electron-cache-bundled-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(
            &bundled.join("electron-cache-windows-x64.zip"),
            &[("electron-cache/electron-v40.9.3-win32-x64.zip", b"electron zip")],
        );

        let source = install_bundled_electron_cache_if_available(
            &hermes_home,
            Some(&bundled),
            "windows",
            "x64",
        )
        .unwrap();

        assert_eq!(
            source,
            Some((
                "electron-cache-windows-x64.zip".to_string(),
                BootstrapArchiveSourceKind::Bundled,
            ))
        );
        assert_eq!(
            std::fs::read(
                hermes_home
                    .join("electron-cache")
                    .join("electron-v40.9.3-win32-x64.zip")
            )
            .unwrap(),
            b"electron zip"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn npm_cache_runtime_stage_plan_matches_bundled_archive_contract() {
        let root = std::env::temp_dir().join(format!(
            "hermes-npm-cache-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let windows = npm_cache_runtime_stage_plan(&hermes_home, "windows", "x64")
            .expect("Windows x64 npm cache archive should be supported");
        assert_eq!(windows.archive_name, "npm-cache-windows-x64.zip");
        assert_eq!(windows.install_dir, hermes_home.join("npm-cache"));

        let linux = npm_cache_runtime_stage_plan(&hermes_home, "linux", "arm64")
            .expect("Linux arm64 npm cache archive should be supported");
        assert_eq!(linux.archive_name, "npm-cache-linux-arm64.tar.gz");

        let macos = npm_cache_runtime_stage_plan(&hermes_home, "darwin", "x64")
            .expect("macOS x64 npm cache archive should be supported");
        assert_eq!(macos.archive_name, "npm-cache-macos-x64.tar.gz");
        assert!(npm_cache_runtime_stage_plan(&hermes_home, "freebsd", "x64").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_npm_cache_archive_copies_nested_zip_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-npm-cache-zip-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("npm-cache.zip");
        write_test_zip(
            &archive,
            &[("npm-cache/_cacache/content-v2/sha512/aa/bb", b"cached package")],
        );
        let install_dir = root.join("home").join("npm-cache");

        extract_npm_cache_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(
                install_dir
                    .join("_cacache")
                    .join("content-v2")
                    .join("sha512")
                    .join("aa")
                    .join("bb")
            )
            .unwrap(),
            b"cached package"
        );
        assert!(!install_dir.parent().unwrap().join("npm-cache-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_npm_cache_archive_copies_nested_tar_gz_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-npm-cache-tar-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("npm-cache.tar.gz");
        write_test_tar_gz(
            &archive,
            &[("npm-cache/_cacache/index-v5/aa/bb", b"cached index")],
        );
        let install_dir = root.join("home").join("npm-cache");

        extract_npm_cache_archive(&archive, &install_dir).unwrap();

        assert_eq!(
            std::fs::read(
                install_dir
                    .join("_cacache")
                    .join("index-v5")
                    .join("aa")
                    .join("bb")
            )
            .unwrap(),
            b"cached index"
        );
        assert!(!install_dir.parent().unwrap().join("npm-cache-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_npm_cache_archive_installs_before_npm_commands() {
        let root = std::env::temp_dir().join(format!(
            "hermes-npm-cache-bundled-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        write_test_zip(
            &bundled.join("npm-cache-windows-x64.zip"),
            &[("npm-cache/_cacache/content-v2/sha512/aa/bb", b"cached package")],
        );

        let source = install_bundled_npm_cache_if_available(
            &hermes_home,
            Some(&bundled),
            "windows",
            "x64",
        )
        .unwrap();

        assert_eq!(
            source,
            Some((
                "npm-cache-windows-x64.zip".to_string(),
                BootstrapArchiveSourceKind::Bundled,
            ))
        );
        assert_eq!(
            std::fs::read(
                hermes_home
                    .join("npm-cache")
                    .join("_cacache")
                    .join("content-v2")
                    .join("sha512")
                    .join("aa")
                    .join("bb")
            )
            .unwrap(),
            b"cached package"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_node_runtime_stage_plan_parses_latest_v22_zip() {
        let html = r#"
            <a href="node-v22.18.0-win-x64.zip">node-v22.18.0-win-x64.zip</a>
            <a href="node-v22.19.1-win-x64.zip">node-v22.19.1-win-x64.zip</a>
            <a href="node-v22.19.1-win-arm64.zip">node-v22.19.1-win-arm64.zip</a>
        "#;
        let root = std::env::temp_dir().join(format!(
            "hermes-node-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let plan = windows_node_runtime_stage_plan_from_index(&hermes_home, "x64", html).unwrap();

        assert_eq!(plan.version_major, 22);
        assert_eq!(plan.archive_name, "node-v22.19.1-win-x64.zip");
        assert_eq!(
            plan.download_url,
            "https://nodejs.org/dist/latest-v22.x/node-v22.19.1-win-x64.zip"
        );
        assert_eq!(plan.install_dir, hermes_home.join("node"));
        assert_eq!(plan.node_exe, hermes_home.join("node").join("node.exe"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_node_runtime_stage_plan_prefers_latest_gz_tarball() {
        let html = r#"
            <a href="node-v22.18.0-linux-x64.tar.xz">node-v22.18.0-linux-x64.tar.xz</a>
            <a href="node-v22.19.1-linux-arm64.tar.xz">node-v22.19.1-linux-arm64.tar.xz</a>
            <a href="node-v22.19.2-linux-x64.tar.gz">node-v22.19.2-linux-x64.tar.gz</a>
            <a href="node-v22.19.1-linux-x64.tar.xz">node-v22.19.1-linux-x64.tar.xz</a>
        "#;
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-node-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let plan = unix_node_runtime_stage_plan_from_index(
            &hermes_home,
            "linux",
            "x64",
            html,
        )
        .unwrap();

        assert_eq!(plan.version_major, 22);
        assert_eq!(plan.archive_name, "node-v22.19.2-linux-x64.tar.gz");
        assert_eq!(
            plan.download_url,
            "https://nodejs.org/dist/latest-v22.x/node-v22.19.2-linux-x64.tar.gz"
        );
        assert_eq!(plan.install_dir, hermes_home.join("node"));
        assert_eq!(plan.node_bin, hermes_home.join("node").join("bin").join("node"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_node_runtime_stage_plan_falls_back_to_latest_gz_tarball() {
        let html = r#"
            <a href="node-v22.18.0-darwin-arm64.tar.gz">node-v22.18.0-darwin-arm64.tar.gz</a>
            <a href="node-v22.19.1-darwin-arm64.tar.gz">node-v22.19.1-darwin-arm64.tar.gz</a>
        "#;
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-node-runtime-gz-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let plan = unix_node_runtime_stage_plan_from_index(
            &hermes_home,
            "darwin",
            "arm64",
            html,
        )
        .unwrap();

        assert_eq!(plan.archive_name, "node-v22.19.1-darwin-arm64.tar.gz");
        assert_eq!(
            plan.npm_bin,
            hermes_home.join("node").join("bin").join("npm")
        );
        assert_eq!(
            plan.npx_bin,
            hermes_home.join("node").join("bin").join("npx")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_unix_node_archive_moves_single_root_from_nested_tar_gz() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-node-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("node.tar.gz");
        write_test_tar_gz(
            &archive,
            &[
                ("node-v22.19.2-linux-x64/bin/node", b"fake node"),
                ("node-v22.19.2-linux-x64/bin/npm", b"fake npm"),
                ("node-v22.19.2-linux-x64/bin/npx", b"fake npx"),
            ],
        );
        let install_dir = root.join("home").join("node");

        extract_unix_node_tar_gz(&archive, &install_dir).unwrap();

        assert_eq!(std::fs::read(install_dir.join("bin").join("node")).unwrap(), b"fake node");
        assert_eq!(std::fs::read(install_dir.join("bin").join("npm")).unwrap(), b"fake npm");
        assert!(!install_dir.with_extension("extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_node_arch_slug_maps_supported_uname_arches() {
        assert_eq!(unix_node_arch_slug("x86_64"), Some("x64"));
        assert_eq!(unix_node_arch_slug("aarch64"), Some("arm64"));
        assert_eq!(unix_node_arch_slug("arm64"), Some("arm64"));
        assert_eq!(unix_node_arch_slug("armv7l"), Some("armv7l"));
        assert_eq!(unix_node_arch_slug("mips"), None);
    }

    #[test]
    fn bootstrap_archive_source_rejects_unmanifested_bundled_resource() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join("uv-x86_64-pc-windows-msvc.zip"), b"uv").unwrap();

        let source = resolve_bootstrap_archive_source(
            &hermes_home,
            Some(&bundled),
            "uv-x86_64-pc-windows-msvc.zip",
        );

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(
            source.path,
            hermes_home
                .join("bootstrap-cache")
                .join("uv-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(
            source.cache_path,
            hermes_home
                .join("bootstrap-cache")
                .join("uv-x86_64-pc-windows-msvc.zip")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_accepts_manifest_verified_bundled_resource() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-verified-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "url": "https://example.invalid/uv.zip",
                        "sizeBytes": 2,
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Bundled);
        assert_eq!(source.path, bundled.join(archive_name));
        assert_eq!(
            source.expected_sha256.as_deref(),
            Some("e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_manifest_without_size_bytes() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-missing-size-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "url": "https://example.invalid/uv.zip",
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(
            source.path,
            hermes_home.join("bootstrap-cache").join(archive_name)
        );
        assert_eq!(source.expected_sha256, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_manifest_mismatched_bundled_resource() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-mismatch-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "url": "https://example.invalid/uv.zip",
                        "sizeBytes": 2,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(
            source.path,
            hermes_home.join("bootstrap-cache").join(archive_name)
        );
        assert_eq!(
            source.expected_sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_manifest_target_mismatch() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-target-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "linux",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(source.expected_sha256, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_tools_manifest_rejects_unsafe_archive_name() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-unsafe-name-test-{}",
            std::process::id()
        ));
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "name": "../uv-x86_64-pc-windows-msvc.zip",
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            bootstrap_tools_manifest_sha256(&bundled, "../uv-x86_64-pc-windows-msvc.zip"),
            None
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_manifest_insecure_url() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-insecure-url-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "url": "http://example.invalid/uv.zip",
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(source.expected_sha256, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_unsupported_tools_manifest_schema() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-schema-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 2,
                "archives": [
                    {
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(source.expected_sha256, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_rejects_manifest_size_mismatched_bundled_resource() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-size-source-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        let archive_name = "uv-x86_64-pc-windows-msvc.zip";
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join(archive_name), b"uv").unwrap();
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "uv-x86_64-pc-windows-msvc.zip",
                        "url": "https://example.invalid/uv.zip",
                        "sizeBytes": 99,
                        "sha256": "e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9"
                    }
                ]
            }"#,
        )
        .unwrap();

        let source = resolve_bootstrap_archive_source(&hermes_home, Some(&bundled), archive_name);

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(
            source.expected_sha256.as_deref(),
            Some("e6184ce10e266134fdcfa401e8f1a95005bcd4f18d16b62b757323e2833fe9a9")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bootstrap_archive_source_falls_back_to_cache_when_resource_is_absent() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-archive-cache-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();

        let source = resolve_bootstrap_archive_source(
            &hermes_home,
            Some(&bundled),
            "uv-x86_64-pc-windows-msvc.zip",
        );

        assert_eq!(source.kind, BootstrapArchiveSourceKind::Cache);
        assert_eq!(
            source.path,
            hermes_home
                .join("bootstrap-cache")
                .join("uv-x86_64-pc-windows-msvc.zip")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_node_archive_picker_rejects_unmanifested_archives() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bundled-node-archive-test-{}",
            std::process::id()
        ));
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        for name in [
            "node-v22.18.0-win-x64.zip",
            "node-v22.20.1-win-arm64.zip",
            "node-v22.19.1-win-x64.zip",
            "node-v21.7.3-win-x64.zip",
            "node-v22.19.2-win-x86.zip",
        ] {
            std::fs::write(bundled.join(name), b"node").unwrap();
        }

        let picked = latest_bundled_windows_node_archive_name(Some(&bundled), 22, "x64");

        assert_eq!(picked, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_node_archive_picker_ignores_unmanifested_archive_when_manifest_exists() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bundled-node-manifest-picker-test-{}",
            std::process::id()
        ));
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        for name in [
            "node-v22.19.1-win-x64.zip",
            "node-v22.20.0-win-x64.zip",
        ] {
            std::fs::write(bundled.join(name), b"node").unwrap();
        }
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "windows",
                        "name": "node-v22.19.1-win-x64.zip",
                        "url": "https://nodejs.org/dist/latest-v22.x/node-v22.19.1-win-x64.zip",
                        "sizeBytes": 4,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }
                ]
            }"#,
        )
        .unwrap();

        let picked = latest_bundled_windows_node_archive_name(Some(&bundled), 22, "x64");

        assert_eq!(picked.as_deref(), Some("node-v22.19.1-win-x64.zip"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bundled_unix_node_archive_picker_prefers_matching_manifested_gz() {
        let root = std::env::temp_dir().join(format!(
            "hermes-bundled-unix-node-archive-test-{}",
            std::process::id()
        ));
        let bundled = root.join("resources").join("bootstrap-tools");
        std::fs::create_dir_all(&bundled).unwrap();
        for name in [
            "node-v22.18.0-linux-x64.tar.xz",
            "node-v22.20.1-linux-arm64.tar.xz",
            "node-v22.19.3-linux-x64.tar.gz",
            "node-v22.19.2-linux-x64.tar.xz",
            "node-v21.7.3-linux-x64.tar.xz",
        ] {
            std::fs::write(bundled.join(name), b"node").unwrap();
        }
        std::fs::write(
            bundled.join("bootstrap-tools-manifest.json"),
            r#"{
                "schemaVersion": 1,
                "archives": [
                    {
                        "arch": "x64",
                        "platform": "linux",
                        "name": "node-v22.19.3-linux-x64.tar.gz",
                        "url": "https://nodejs.org/dist/latest-v22.x/node-v22.19.3-linux-x64.tar.gz",
                        "sizeBytes": 4,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    },
                    {
                        "arch": "x64",
                        "platform": "linux",
                        "name": "node-v22.19.2-linux-x64.tar.xz",
                        "url": "https://nodejs.org/dist/latest-v22.x/node-v22.19.2-linux-x64.tar.xz",
                        "sizeBytes": 4,
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    }
                ]
            }"#,
        )
        .unwrap();

        let picked =
            latest_bundled_unix_node_archive_name(Some(&bundled), 22, "linux", "x64");

        assert_eq!(picked.as_deref(), Some("node-v22.19.3-linux-x64.tar.gz"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn node_dependencies_stage_plan_prefers_managed_tools_and_detects_packages() {
        let root = std::env::temp_dir().join(format!(
            "hermes-node-deps-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let install_root = root.join("checkout");
        let node_home = hermes_home.join("node");
        std::fs::create_dir_all(&node_home).unwrap();
        std::fs::create_dir_all(install_root.join("ui-tui")).unwrap();
        let npm_name = if cfg!(target_os = "windows") { "npm.cmd" } else { "bin/npm" };
        let npx_name = if cfg!(target_os = "windows") { "npx.CMD" } else { "bin/npx" };
        let npm = hermes_home.join("node").join(npm_name);
        let npx = hermes_home.join("node").join(npx_name);
        std::fs::create_dir_all(npm.parent().unwrap()).unwrap();
        std::fs::write(&npm, b"npm").unwrap();
        std::fs::write(&npx, b"npx").unwrap();
        std::fs::write(install_root.join("package.json"), b"{}").unwrap();
        std::fs::write(install_root.join("ui-tui").join("package.json"), b"{}").unwrap();

        let plan =
            node_dependencies_stage_plan(&install_root, &hermes_home, "", ".EXE;.CMD").unwrap();

        assert_eq!(plan.npm, npm);
        assert_eq!(plan.npx.as_deref(), Some(npx.as_path()));
        assert_eq!(plan.cwd, install_root);
        assert_eq!(plan.npm_cache_dir, hermes_home.join("npm-cache"));
        assert_eq!(
            plan.playwright_browsers_dir,
            hermes_home.join("playwright-browsers")
        );
        assert_eq!(plan.browser_tools, true);
        assert_eq!(plan.tui_dir.as_deref(), Some(install_root.join("ui-tui").as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn node_dependencies_stage_plan_errors_when_install_root_is_missing() {
        let root = std::env::temp_dir().join(format!(
            "hermes-node-deps-missing-root-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let npm_name = if cfg!(target_os = "windows") { "npm.cmd" } else { "bin/npm" };
        let npm = hermes_home.join("node").join(npm_name);
        std::fs::create_dir_all(npm.parent().unwrap()).unwrap();
        std::fs::write(&npm, b"npm").unwrap();

        let err = node_dependencies_stage_plan(
            &root.join("missing-checkout"),
            &hermes_home,
            "",
            ".EXE;.CMD",
        )
        .unwrap_err();

        assert!(err.to_string().contains("install root does not exist"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn playwright_install_plan_matches_linux_system_dependency_recovery() {
        let debian = playwright_install_plan("linux", "debian", false, true)
            .expect("Debian with sudo should use Playwright with-deps");
        assert_eq!(
            debian.npx_args,
            vec!["--yes", "playwright", "install", "--with-deps", "chromium"]
        );
        assert_eq!(debian.system_package_commands, Vec::new());
        assert_eq!(debian.system_deps, "playwright-with-deps");

        let debian_without_sudo = playwright_install_plan("linux", "ubuntu", false, false)
            .expect("Ubuntu without sudo should keep browser-only install");
        assert_eq!(
            debian_without_sudo.npx_args,
            vec!["--yes", "playwright", "install", "chromium"]
        );
        assert_eq!(debian_without_sudo.system_package_commands, Vec::new());
        assert_eq!(debian_without_sudo.system_deps, "browser-only");

        let arch = playwright_install_plan("linux", "arch", true, false)
            .expect("Arch root install should plan pacman system dependencies");
        assert_eq!(arch.npx_args, vec!["--yes", "playwright", "install", "chromium"]);
        assert_eq!(arch.system_deps, "pacman");
        assert_eq!(
            arch.system_package_commands,
            vec![UnixPackageInstallCommandPlan {
                program: "pacman".to_string(),
                args: vec![
                    "-S".to_string(),
                    "--noconfirm".to_string(),
                    "--needed".to_string(),
                    "nss".to_string(),
                    "atk".to_string(),
                    "at-spi2-core".to_string(),
                    "cups".to_string(),
                    "libdrm".to_string(),
                    "libxkbcommon".to_string(),
                    "mesa".to_string(),
                    "pango".to_string(),
                    "cairo".to_string(),
                    "alsa-lib".to_string(),
                ],
            }]
        );

        let fedora = playwright_install_plan("linux", "fedora", true, false)
            .expect("Fedora should plan dnf system dependencies");
        assert_eq!(fedora.npx_args, vec!["--yes", "playwright", "install", "chromium"]);
        assert_eq!(fedora.system_deps, "dnf");
        assert_eq!(
            fedora.system_package_commands,
            vec![UnixPackageInstallCommandPlan {
                program: "dnf".to_string(),
                args: vec![
                    "install".to_string(),
                    "-y".to_string(),
                    "nss".to_string(),
                    "atk".to_string(),
                    "at-spi2-core".to_string(),
                    "cups-libs".to_string(),
                    "libdrm".to_string(),
                    "libxkbcommon".to_string(),
                    "mesa-libgbm".to_string(),
                    "pango".to_string(),
                    "cairo".to_string(),
                    "alsa-lib".to_string(),
                ],
            }]
        );

        let opensuse = playwright_install_plan("linux", "opensuse-tumbleweed", true, false)
            .expect("openSUSE should plan zypper system dependencies");
        assert_eq!(opensuse.npx_args, vec!["--yes", "playwright", "install", "chromium"]);
        assert_eq!(opensuse.system_deps, "zypper");
        assert_eq!(
            opensuse.system_package_commands,
            vec![UnixPackageInstallCommandPlan {
                program: "zypper".to_string(),
                args: vec![
                    "--non-interactive".to_string(),
                    "install".to_string(),
                    "mozilla-nss".to_string(),
                    "libatk-1_0-0".to_string(),
                    "at-spi2-core".to_string(),
                    "cups-libs".to_string(),
                    "libdrm2".to_string(),
                    "libxkbcommon0".to_string(),
                    "Mesa-libgbm1".to_string(),
                    "pango".to_string(),
                    "cairo".to_string(),
                    "libasound2".to_string(),
                ],
            }]
        );

        let macos = playwright_install_plan("macos", "", false, false)
            .expect("macOS should keep browser-only install");
        assert_eq!(macos.npx_args, vec!["--yes", "playwright", "install", "chromium"]);
        assert_eq!(macos.system_package_commands, Vec::new());
    }

    #[test]
    fn browser_install_decision_uses_system_browser_without_playwright_download() {
        let browser = PathBuf::from("/usr/bin/google-chrome");
        let decision = browser_install_decision(Some(browser.clone()), "linux", "ubuntu", false, true)
            .expect("system browser should not need Playwright planning");

        assert_eq!(decision.system_browser, Some(browser));
        assert!(decision.playwright.is_none());
        assert_eq!(decision.system_deps, "system-browser");
    }

    #[test]
    fn playwright_system_dependency_failure_does_not_skip_browser_install() {
        let root = std::env::temp_dir().join(format!(
            "hermes-playwright-recovery-test-{}",
            std::process::id()
        ));
        let cwd = root.join("checkout");
        let npm_cache = root.join("npm-cache");
        let browsers = root.join("playwright-browsers");
        let ran_file = root.join("npx-ran.txt");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&npm_cache).unwrap();
        std::fs::create_dir_all(&browsers).unwrap();

        let failing_system = if cfg!(target_os = "windows") {
            let command = root.join("fake-system-dep.cmd");
            std::fs::write(&command, "@echo off\r\necho system failed 1>&2\r\nexit /b 9\r\n")
                .unwrap();
            command
        } else {
            let command = root.join("fake-system-dep.sh");
            std::fs::write(&command, "#!/usr/bin/env sh\necho system failed >&2\nexit 9\n")
                .unwrap();
            make_executable(&command).unwrap();
            command
        };
        let npx = if cfg!(target_os = "windows") {
            let command = root.join("fake-npx.cmd");
            let script = format!("@echo off\r\n> \"{}\" echo %*\r\nexit /b 0\r\n", ran_file.display());
            std::fs::write(&command, script).unwrap();
            command
        } else {
            let command = root.join("fake-npx.sh");
            let script = format!(
                "#!/usr/bin/env sh\nprintf '%s' \"$*\" > '{}'\nexit 0\n",
                ran_file.display()
            );
            std::fs::write(&command, script).unwrap();
            make_executable(&command).unwrap();
            command
        };
        let plan = PlaywrightInstallPlan {
            npx_args: vec![
                "--yes".to_string(),
                "playwright".to_string(),
                "install".to_string(),
                "chromium".to_string(),
            ],
            system_package_commands: vec![UnixPackageInstallCommandPlan {
                program: failing_system.display().to_string(),
                args: Vec::new(),
            }],
            system_deps: "pacman".to_string(),
        };

        let failures =
            install_playwright_with_system_recovery(&npx, &plan, &cwd, &npm_cache, &browsers)
                .expect("browser install should continue after system dependency failure");

        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("system failed"));
        assert_eq!(
            std::fs::read_to_string(&ran_file).unwrap().trim(),
            "--yes playwright install chromium"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn system_browser_candidates_include_common_chromium_browsers() {
        let linux_commands = system_browser_command_candidates_for_target("linux");
        assert!(linux_commands.contains(&"brave-browser"));
        assert!(linux_commands.contains(&"microsoft-edge"));

        let macos_files = system_browser_file_candidates_for_target(
            "macos",
            [("ProgramFiles", ""), ("ProgramFiles(x86)", ""), ("LOCALAPPDATA", "")],
        );
        assert!(macos_files
            .iter()
            .any(|path| path.ends_with("Brave Browser.app/Contents/MacOS/Brave Browser")));
        assert!(macos_files
            .iter()
            .any(|path| path.ends_with("Microsoft Edge.app/Contents/MacOS/Microsoft Edge")));

        let windows_files = system_browser_file_candidates_for_target(
            "windows",
            [
                ("ProgramFiles", "C:/Program Files"),
                ("ProgramFiles(x86)", "C:/Program Files (x86)"),
                ("LOCALAPPDATA", "C:/Users/Alice/AppData/Local"),
            ],
        );
        assert!(windows_files.iter().any(|path| {
            path.ends_with("BraveSoftware/Brave-Browser/Application/brave.exe")
        }));
        assert!(windows_files
            .iter()
            .any(|path| path.ends_with("Microsoft/Edge/Application/msedge.exe")));
    }

    #[test]
    fn system_browser_probe_honors_configured_path_or_command() {
        let root = std::env::temp_dir().join(format!(
            "hermes-browser-probe-test-{}",
            std::process::id()
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();

        #[cfg(target_os = "windows")]
        let browser_command = {
            let command = bin.join("custom-browser.cmd");
            std::fs::write(&command, "@echo off\r\n").unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let browser_command = {
            let command = bin.join("custom-browser");
            std::fs::write(&command, "#!/usr/bin/env sh\n").unwrap();
            make_executable(&command).unwrap();
            command
        };

        let absolute = find_system_browser_with_config(
            Some(browser_command.display().to_string().as_str()),
            "",
            ".COM;.EXE;.BAT;.CMD",
        );
        assert_eq!(absolute, Some(browser_command.clone()));

        let command_name = browser_command.file_name().unwrap().to_string_lossy();
        let from_path = find_system_browser_with_config(
            Some(command_name.as_ref()),
            bin.display().to_string(),
            ".COM;.EXE;.BAT;.CMD",
        );
        assert_eq!(from_path, Some(browser_command));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn browser_env_writer_appends_system_browser_without_overwriting() {
        let root = std::env::temp_dir().join(format!(
            "hermes-browser-env-test-{}",
            std::process::id()
        ));
        let home = root.join("home");
        let browser = root.join("chrome");

        let changed = write_browser_env_from_system_browser(&home, &browser).unwrap();
        assert!(changed);
        assert!(std::fs::read_to_string(home.join(".env"))
            .unwrap()
            .contains("AGENT_BROWSER_EXECUTABLE_PATH="));

        let changed = write_browser_env_from_system_browser(&home, &PathBuf::from("/other")).unwrap();
        assert!(!changed);
        assert!(std::fs::read_to_string(home.join(".env"))
            .unwrap()
            .contains(&browser.display().to_string()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn run_node_dependency_command_sets_managed_npm_cache() {
        let root = std::env::temp_dir().join(format!(
            "hermes-node-cache-env-test-{}",
            std::process::id()
        ));
        let cwd = root.join("cwd");
        let cache = root.join("home").join("npm-cache");
        let browsers = root.join("home").join("playwright-browsers");
        let output = root.join("npm-cache-env.txt");
        let browser_output = root.join("playwright-browsers-env.txt");
        std::fs::create_dir_all(&cwd).unwrap();

        #[cfg(target_os = "windows")]
        let command = {
            let command = root.join("capture-npm-cache.cmd");
            let script = format!(
                "@echo off\r\n> \"{}\" echo %npm_config_cache%\r\n> \"{}\" echo %PLAYWRIGHT_BROWSERS_PATH%\r\n",
                output.display(),
                browser_output.display()
            );
            std::fs::write(&command, script).unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let command = root.join("capture-npm-cache.sh");
            let script = format!(
                "#!/usr/bin/env sh\nprintf '%s' \"$npm_config_cache\" > '{}'\n\
                 printf '%s' \"$PLAYWRIGHT_BROWSERS_PATH\" > '{}'\n",
                output.display(),
                browser_output.display()
            );
            std::fs::write(&command, script).unwrap();
            make_executable(&command).unwrap();
            command
        };

        run_node_dependency_command(&command, ["ignored"], &cwd, &cache, Some(&browsers)).unwrap();

        assert_eq!(std::fs::read_to_string(&output).unwrap().trim(), cache.display().to_string());
        assert_eq!(
            std::fs::read_to_string(&browser_output).unwrap().trim(),
            browsers.display().to_string()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn optional_node_dependency_command_reports_failure_without_error() {
        let root = std::env::temp_dir().join(format!(
            "hermes-optional-node-failure-test-{}",
            std::process::id()
        ));
        let cwd = root.join("checkout");
        let cache = root.join("npm-cache");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        let command = if cfg!(target_os = "windows") {
            let command = root.join("fake-npm-failure.cmd");
            std::fs::write(&command, "@echo off\r\necho tui failed 1>&2\r\nexit /b 5\r\n")
                .unwrap();
            command
        } else {
            let command = root.join("fake-npm-failure.sh");
            std::fs::write(&command, "#!/usr/bin/env sh\necho tui failed >&2\nexit 5\n")
                .unwrap();
            make_executable(&command).unwrap();
            command
        };

        let failure =
            run_optional_node_dependency_command(&command, ["install", "--silent"], &cwd, &cache, None)
                .expect("optional npm failure should be reported");

        assert!(failure.contains("tui failed"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn npm_permission_diagnostic_points_at_managed_cache_and_node_modules() {
        let cwd = PathBuf::from("/tmp/hermes-agent");
        let cache = PathBuf::from("/tmp/hermes-home/npm-cache");
        let output = "npm ERR! code EACCES\nnpm ERR! syscall mkdir\nnpm ERR! permission denied";

        let hint = npm_permission_diagnostic(output, &cache, &cwd, "linux").unwrap();

        assert!(hint.contains(&cache.display().to_string()));
        assert!(hint.contains(&cwd.join("node_modules").display().to_string()));
        assert!(hint.contains("sudo chown -R"));
    }

    #[test]
    fn desktop_stage_skip_result_skips_when_desktop_package_is_absent() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-skip-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let desktop = stage_info("desktop", "Building desktop app", "install", false);
        let skipped = desktop_stage_skip_result(&desktop, &root).unwrap();

        assert_eq!(skipped.stage, "desktop");
        assert_eq!(skipped.ok, true);
        assert_eq!(skipped.skipped, true);
        assert_eq!(
            skipped.reason.as_deref(),
            Some("apps/desktop not present")
        );
        std::fs::create_dir_all(root.join("apps").join("desktop")).unwrap();
        std::fs::write(
            root.join("apps").join("desktop").join("package.json"),
            b"{}",
        )
        .unwrap();
        assert!(desktop_stage_skip_result(&desktop, &root).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn desktop_build_stage_plan_requires_npm_and_desktop_package() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-build-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let install_root = root.join("checkout");
        let desktop_dir = install_root.join("apps").join("desktop");
        std::fs::create_dir_all(&desktop_dir).unwrap();
        std::fs::write(desktop_dir.join("package.json"), b"{}").unwrap();

        assert!(
            desktop_build_stage_plan(&install_root, &hermes_home, "", ".EXE;.CMD").is_err()
        );

        let npm_name = if cfg!(target_os = "windows") { "npm.cmd" } else { "bin/npm" };
        let npm = hermes_home.join("node").join(npm_name);
        std::fs::create_dir_all(npm.parent().unwrap()).unwrap();
        std::fs::write(&npm, b"npm").unwrap();

        let plan =
            desktop_build_stage_plan(&install_root, &hermes_home, "", ".EXE;.CMD").unwrap();

        assert_eq!(plan.npm, npm);
        assert_eq!(plan.cwd, install_root);
        assert_eq!(plan.npm_cache_dir, hermes_home.join("npm-cache"));
        assert_eq!(plan.electron_cache_dir, hermes_home.join("electron-cache"));
        assert_eq!(plan.desktop_dir, desktop_dir);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn desktop_pack_command_sets_managed_electron_cache_env() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-pack-env-test-{}",
            std::process::id()
        ));
        let desktop_dir = root.join("desktop");
        let npm_cache = root.join("npm-cache");
        let electron_cache = root.join("electron-cache");
        let electron_config_output = root.join("electron-config-cache.txt");
        let electron_output = root.join("electron-cache.txt");
        let builder_output = root.join("electron-builder-cache.txt");
        std::fs::create_dir_all(&desktop_dir).unwrap();
        std::fs::create_dir_all(&npm_cache).unwrap();
        std::fs::create_dir_all(&electron_cache).unwrap();

        #[cfg(target_os = "windows")]
        let command = {
            let command = root.join("fake-npm-electron-cache.cmd");
            let script = format!(
                concat!(
                    "@echo off\r\n",
                    "> \"{electron_config}\" echo(%electron_config_cache%\r\n",
                    "> \"{electron}\" echo(%ELECTRON_CACHE%\r\n",
                    "> \"{builder}\" echo(%ELECTRON_BUILDER_CACHE%\r\n",
                    "exit /b 0\r\n"
                ),
                electron_config = electron_config_output.display(),
                electron = electron_output.display(),
                builder = builder_output.display(),
            );
            std::fs::write(&command, script).unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let command = root.join("fake-npm-electron-cache.sh");
            let script = format!(
                concat!(
                    "#!/usr/bin/env sh\n",
                    "printf '%s' \"$electron_config_cache\" > '{electron_config}'\n",
                    "printf '%s' \"$ELECTRON_CACHE\" > '{electron}'\n",
                    "printf '%s' \"$ELECTRON_BUILDER_CACHE\" > '{builder}'\n",
                    "exit 0\n"
                ),
                electron_config = electron_config_output.display(),
                electron = electron_output.display(),
                builder = builder_output.display(),
            );
            std::fs::write(&command, script).unwrap();
            make_executable(&command).unwrap();
            command
        };

        run_desktop_pack_command(
            &command,
            &desktop_dir,
            &npm_cache,
            &electron_cache,
        )
        .unwrap();

        let expected = electron_cache.display().to_string();
        assert_eq!(std::fs::read_to_string(&electron_config_output).unwrap().trim(), expected);
        assert_eq!(std::fs::read_to_string(&electron_output).unwrap().trim(), expected);
        assert_eq!(std::fs::read_to_string(&builder_output).unwrap().trim(), expected);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn desktop_pack_command_retries_public_electron_mirror_when_default_fails() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-pack-mirror-test-{}",
            std::process::id()
        ));
        let desktop_dir = root.join("desktop");
        let cache = root.join("npm-cache");
        let electron_cache = root.join("electron-cache");
        let count_file = root.join("attempt.txt");
        let first_mirror = root.join("first-mirror.txt");
        let second_mirror = root.join("second-mirror.txt");
        std::fs::create_dir_all(&desktop_dir).unwrap();
        std::fs::create_dir_all(&cache).unwrap();

        #[cfg(target_os = "windows")]
        let command = {
            let command = root.join("fake-npm.cmd");
            let script = format!(
                concat!(
                    "@echo off\r\n",
                    "if not exist \"{count}\" (\r\n",
                    "  > \"{count}\" echo first\r\n",
                    "  > \"{first}\" echo(%ELECTRON_MIRROR%\r\n",
                    "  exit /b 1\r\n",
                    ")\r\n",
                    "> \"{second}\" echo(%ELECTRON_MIRROR%\r\n",
                    "if \"%ELECTRON_MIRROR%\"==\"{mirror}\" exit /b 0\r\n",
                    "exit /b 1\r\n"
                ),
                count = count_file.display(),
                first = first_mirror.display(),
                second = second_mirror.display(),
                mirror = DESKTOP_ELECTRON_FALLBACK_MIRROR,
            );
            std::fs::write(&command, script).unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let command = root.join("fake-npm.sh");
            let script = format!(
                concat!(
                    "#!/usr/bin/env sh\n",
                    "if [ ! -f '{count}' ]; then\n",
                    "  printf '%s' first > '{count}'\n",
                    "  printf '%s' \"$ELECTRON_MIRROR\" > '{first}'\n",
                    "  exit 1\n",
                    "fi\n",
                    "printf '%s' \"$ELECTRON_MIRROR\" > '{second}'\n",
                    "[ \"$ELECTRON_MIRROR\" = '{mirror}' ] && exit 0\n",
                    "exit 1\n"
                ),
                count = count_file.display(),
                first = first_mirror.display(),
                second = second_mirror.display(),
                mirror = DESKTOP_ELECTRON_FALLBACK_MIRROR,
            );
            std::fs::write(&command, script).unwrap();
            make_executable(&command).unwrap();
            command
        };

        let result =
            run_desktop_pack_command_with_mirror_policy(
                &command,
                &desktop_dir,
                &cache,
                &electron_cache,
                false,
            )
            .unwrap();

        assert_eq!(std::fs::read_to_string(&first_mirror).unwrap().trim(), "");
        assert_eq!(
            std::fs::read_to_string(&second_mirror).unwrap().trim(),
            DESKTOP_ELECTRON_FALLBACK_MIRROR
        );
        assert_eq!(result.fallback_mirror_used, true);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn desktop_pack_command_clears_electron_cache_before_mirror_retry() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-pack-cache-test-{}",
            std::process::id()
        ));
        let desktop_dir = root.join("desktop");
        let npm_cache = root.join("npm-cache");
        let electron_cache = root.join("electron-cache");
        let cache_zip = electron_cache.join("nested").join("electron-v1.zip");
        let stale_unpacked = desktop_dir.join("release").join("win-unpacked");
        let count_file = root.join("attempt.txt");
        std::fs::create_dir_all(cache_zip.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&stale_unpacked).unwrap();
        std::fs::create_dir_all(&npm_cache).unwrap();
        std::fs::write(&cache_zip, b"bad zip").unwrap();
        std::fs::write(stale_unpacked.join("partial"), b"partial").unwrap();

        #[cfg(target_os = "windows")]
        let command = {
            let command = root.join("fake-npm-cache.cmd");
            let script = format!(
                concat!(
                    "@echo off\r\n",
                    "if not exist \"{count}\" (\r\n",
                    "  > \"{count}\" echo first\r\n",
                    "  exit /b 1\r\n",
                    ")\r\n",
                    "if exist \"{zip}\" exit /b 1\r\n",
                    "if exist \"{unpacked}\" exit /b 1\r\n",
                    "if not \"%ELECTRON_MIRROR%\"==\"\" exit /b 1\r\n",
                    "exit /b 0\r\n"
                ),
                count = count_file.display(),
                zip = cache_zip.display(),
                unpacked = stale_unpacked.display(),
            );
            std::fs::write(&command, script).unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let command = root.join("fake-npm-cache.sh");
            let script = format!(
                concat!(
                    "#!/usr/bin/env sh\n",
                    "if [ ! -f '{count}' ]; then\n",
                    "  printf '%s' first > '{count}'\n",
                    "  exit 1\n",
                    "fi\n",
                    "[ -e '{zip}' ] && exit 1\n",
                    "[ -e '{unpacked}' ] && exit 1\n",
                    "[ -n \"$ELECTRON_MIRROR\" ] && exit 1\n",
                    "exit 0\n"
                ),
                count = count_file.display(),
                zip = cache_zip.display(),
                unpacked = stale_unpacked.display(),
            );
            std::fs::write(&command, script).unwrap();
            make_executable(&command).unwrap();
            command
        };

        let result = run_desktop_pack_command_with_recovery_policy(
            &command,
            &desktop_dir,
            &npm_cache,
            &electron_cache,
            false,
            &[electron_cache.clone()],
        )
        .unwrap();

        assert_eq!(result.fallback_mirror_used, false);
        assert!(result.purged_paths.iter().any(|path| path == &cache_zip));
        assert!(result
            .purged_paths
            .iter()
            .any(|path| path == &stale_unpacked));
        assert!(!cache_zip.exists());
        assert!(!stale_unpacked.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn built_desktop_app_resolver_finds_platform_outputs() {
        let root = std::env::temp_dir().join(format!(
            "hermes-desktop-output-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let release = root
            .join("apps")
            .join("desktop")
            .join("release");

        let win_exe = release.join("win-unpacked").join("Hermes.exe");
        std::fs::create_dir_all(win_exe.parent().unwrap()).unwrap();
        std::fs::write(&win_exe, b"exe").unwrap();
        assert_eq!(
            find_built_desktop_app(&root, "windows").as_deref(),
            Some(win_exe.as_path())
        );

        let mac_app = release.join("mac-arm64").join("Hermes.app");
        std::fs::create_dir_all(&mac_app).unwrap();
        assert_eq!(
            find_built_desktop_app(&root, "macos").as_deref(),
            Some(mac_app.as_path())
        );

        let linux_app = release.join("linux-unpacked").join("Hermes");
        std::fs::create_dir_all(linux_app.parent().unwrap()).unwrap();
        std::fs::write(&linux_app, b"bin").unwrap();
        assert_eq!(
            find_built_desktop_app(&root, "linux").as_deref(),
            Some(linux_app.as_path())
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn linux_chrome_sandbox_path_uses_desktop_release_dir() {
        let install_root = PathBuf::from("/tmp/hermes-agent");

        assert_eq!(
            linux_chrome_sandbox_path(&install_root),
            install_root
                .join("apps")
                .join("desktop")
                .join("release")
                .join("linux-unpacked")
                .join("chrome-sandbox")
        );
    }

    #[test]
    fn process_euid_root_check_uses_effective_uid_value() {
        assert!(process_euid_is_root(0));
        assert!(!process_euid_is_root(1000));
    }

    #[test]
    fn linux_chrome_sandbox_repair_strategy_requires_noninteractive_sudo() {
        assert_eq!(
            linux_chrome_sandbox_repair_strategy(true, false).unwrap(),
            LinuxChromeSandboxRepairStrategy::Root
        );
        assert_eq!(
            linux_chrome_sandbox_repair_strategy(false, true).unwrap(),
            LinuxChromeSandboxRepairStrategy::Sudo
        );
        assert!(linux_chrome_sandbox_repair_strategy(false, false).is_err());
    }

    #[test]
    fn linux_chrome_sandbox_mode_is_setuid_root_executable() {
        assert_eq!(linux_chrome_sandbox_mode(), 0o4755);
    }

    #[test]
    fn python_stage_skip_result_uses_probe_without_affecting_other_stages() {
        let python = stage_info("python", "Verifying Python 3.11", "prereqs", false);
        let uv = stage_info("uv", "Installing uv package manager", "prereqs", false);

        let skipped = python_stage_skip_result_with_probe(&python, || true).unwrap();

        assert_eq!(skipped.stage, "python");
        assert_eq!(skipped.ok, true);
        assert_eq!(skipped.skipped, true);
        assert_eq!(
            skipped.reason.as_deref(),
            Some("required Python runtime already available")
        );
        assert!(python_stage_skip_result_with_probe(&python, || false).is_none());
        assert!(python_stage_skip_result_with_probe(&uv, || true).is_none());
    }

    #[test]
    fn windows_uv_runtime_stage_plan_maps_arch_to_release_asset() {
        let root = std::env::temp_dir().join(format!(
            "hermes-uv-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let plan = windows_uv_runtime_stage_plan(&hermes_home, "x64").unwrap();

        assert_eq!(plan.archive_name, "uv-x86_64-pc-windows-msvc.zip");
        assert_eq!(
            plan.download_url,
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(plan.install_dir, hermes_home.join("bin"));
        assert_eq!(plan.uv_exe, hermes_home.join("bin").join("uv.exe"));

        let arm_plan = windows_uv_runtime_stage_plan(&hermes_home, "arm64").unwrap();
        assert_eq!(arm_plan.archive_name, "uv-aarch64-pc-windows-msvc.zip");
        let x86_plan = windows_uv_runtime_stage_plan(&hermes_home, "x86").unwrap();
        assert_eq!(x86_plan.archive_name, "uv-i686-pc-windows-msvc.zip");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_uv_runtime_stage_plan_maps_platform_release_assets() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-uv-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let linux_x64 = unix_uv_runtime_stage_plan(&hermes_home, "linux", "x64").unwrap();
        assert_eq!(linux_x64.archive_name, "uv-x86_64-unknown-linux-gnu.tar.gz");
        assert_eq!(
            linux_x64.download_url,
            "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(linux_x64.install_dir, hermes_home.join("bin"));
        assert_eq!(linux_x64.uv_bin, hermes_home.join("bin").join("uv"));

        let linux_arm64 = unix_uv_runtime_stage_plan(&hermes_home, "linux", "arm64").unwrap();
        assert_eq!(linux_arm64.archive_name, "uv-aarch64-unknown-linux-gnu.tar.gz");
        let mac_arm64 = unix_uv_runtime_stage_plan(&hermes_home, "darwin", "arm64").unwrap();
        assert_eq!(mac_arm64.archive_name, "uv-aarch64-apple-darwin.tar.gz");
        let mac_x64 = unix_uv_runtime_stage_plan(&hermes_home, "darwin", "x64").unwrap();
        assert_eq!(mac_x64.archive_name, "uv-x86_64-apple-darwin.tar.gz");
        assert!(unix_uv_runtime_stage_plan(&hermes_home, "linux", "mips").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_unix_uv_archive_copies_uv_and_uvx_from_nested_tar_gz() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-uv-archive-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let archive = root.join("uv.tar.gz");
        write_test_tar_gz(
            &archive,
            &[
                ("uv-x86_64-unknown-linux-gnu/uv", b"fake uv"),
                ("uv-x86_64-unknown-linux-gnu/uvx", b"fake uvx"),
            ],
        );
        let install_dir = root.join("bin");

        extract_unix_uv_tar_gz(&archive, &install_dir).unwrap();

        assert_eq!(std::fs::read(install_dir.join("uv")).unwrap(), b"fake uv");
        assert_eq!(std::fs::read(install_dir.join("uvx")).unwrap(), b"fake uvx");
        assert!(!install_dir.join("uv-extracting").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_git_runtime_stage_plan_matches_pinned_portable_git_assets() {
        let root = std::env::temp_dir().join(format!(
            "hermes-git-runtime-plan-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");

        let x64 = windows_git_runtime_stage_plan(&hermes_home, "x64").unwrap();

        assert_eq!(x64.tag, "v2.54.0.windows.1");
        assert_eq!(x64.version, "2.54.0");
        assert_eq!(x64.archive_name, "PortableGit-2.54.0-64-bit.7z.exe");
        assert_eq!(
            x64.download_url,
            concat!(
                "https://github.com/git-for-windows/git/releases/download/",
                "v2.54.0.windows.1/PortableGit-2.54.0-64-bit.7z.exe"
            )
        );
        assert_eq!(x64.install_dir, hermes_home.join("git"));
        assert_eq!(x64.git_exe, hermes_home.join("git").join("cmd").join("git.exe"));
        assert_eq!(x64.bash_exe, hermes_home.join("git").join("bin").join("bash.exe"));
        assert_eq!(x64.is_zip, false);

        let arm = windows_git_runtime_stage_plan(&hermes_home, "arm64").unwrap();
        assert_eq!(arm.archive_name, "PortableGit-2.54.0-arm64.7z.exe");
        let x86 = windows_git_runtime_stage_plan(&hermes_home, "x86").unwrap();
        assert_eq!(x86.archive_name, "MinGit-2.54.0-32-bit.zip");
        assert_eq!(x86.is_zip, true);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_git_install_command_plan_matches_shell_fallbacks() {
        let debian = unix_git_install_command_plan("linux", "debian", false, true, false)
            .expect("Debian with sudo should install Git");
        assert_eq!(
            debian,
            vec![
                UnixGitInstallCommandPlan {
                    program: "sudo".to_string(),
                    args: vec![
                        "env".to_string(),
                        "DEBIAN_FRONTEND=noninteractive".to_string(),
                        "apt-get".to_string(),
                        "update".to_string(),
                        "-qq".to_string(),
                    ],
                },
                UnixGitInstallCommandPlan {
                    program: "sudo".to_string(),
                    args: vec![
                        "env".to_string(),
                        "DEBIAN_FRONTEND=noninteractive".to_string(),
                        "apt-get".to_string(),
                        "install".to_string(),
                        "-y".to_string(),
                        "-qq".to_string(),
                        "git".to_string(),
                    ],
                },
            ]
        );

        let debian_root = unix_git_install_command_plan("linux", "ubuntu", true, false, false)
            .expect("root Ubuntu should install Git without sudo");
        assert_eq!(debian_root[0].program, "apt-get");
        assert_eq!(debian_root[0].args, vec!["update".to_string(), "-qq".to_string()]);
        assert_eq!(debian_root[1].program, "apt-get");

        let fedora = unix_git_install_command_plan("linux", "fedora", true, false, false)
            .expect("Fedora should install Git through dnf");
        assert_eq!(fedora[0].program, "dnf");
        assert_eq!(fedora[0].args, vec!["install", "-y", "git"]);

        let arch = unix_git_install_command_plan("linux", "arch", true, false, false)
            .expect("Arch should install Git through pacman");
        assert_eq!(arch[0].program, "pacman");
        assert_eq!(arch[0].args, vec!["-S", "--noconfirm", "git"]);

        let macos = unix_git_install_command_plan("macos", "", false, false, true)
            .expect("macOS with Homebrew should install Git through brew");
        assert_eq!(macos[0].program, "brew");
        assert_eq!(macos[0].args, vec!["install", "git"]);

        let termux = unix_git_install_command_plan("android", "termux", false, false, false)
            .expect("Termux should install Git through pkg");
        assert_eq!(termux[0].program, "pkg");
        assert_eq!(termux[0].args, vec!["install", "-y", "git"]);

        assert!(unix_git_install_command_plan("linux", "opensuse", false, false, false).is_err());
    }

    #[test]
    fn unix_system_package_install_command_plan_matches_shell_recovery() {
        let debian = unix_system_package_install_command_plan(
            "linux",
            "debian",
            &["ffmpeg"],
            false,
            true,
            false,
        )
        .expect("Debian with sudo should install ffmpeg");
        assert_eq!(
            debian,
            vec![UnixPackageInstallCommandPlan {
                program: "sudo".to_string(),
                args: vec![
                    "env".to_string(),
                    "DEBIAN_FRONTEND=noninteractive".to_string(),
                    "NEEDRESTART_MODE=a".to_string(),
                    "apt-get".to_string(),
                    "install".to_string(),
                    "-y".to_string(),
                    "-qq".to_string(),
                    "ffmpeg".to_string(),
                ],
            }]
        );

        let debian_root =
            unix_system_package_install_command_plan("linux", "ubuntu", &["ffmpeg"], true, false, false)
                .expect("root Ubuntu should install ffmpeg without sudo");
        assert_eq!(debian_root[0].program, "apt-get");
        assert_eq!(debian_root[0].args, vec!["install", "-y", "-qq", "ffmpeg"]);

        let fedora =
            unix_system_package_install_command_plan("linux", "fedora", &["ffmpeg"], true, false, false)
                .expect("Fedora should install ffmpeg through dnf");
        assert_eq!(fedora[0].program, "dnf");
        assert_eq!(fedora[0].args, vec!["install", "-y", "ffmpeg"]);

        let arch =
            unix_system_package_install_command_plan("linux", "arch", &["ffmpeg"], true, false, false)
                .expect("Arch should install ffmpeg through pacman");
        assert_eq!(arch[0].program, "pacman");
        assert_eq!(arch[0].args, vec!["-S", "--noconfirm", "ffmpeg"]);

        let macos =
            unix_system_package_install_command_plan("macos", "", &["ffmpeg"], false, false, true)
                .expect("macOS with Homebrew should install ffmpeg through brew");
        assert_eq!(macos[0].program, "brew");
        assert_eq!(macos[0].args, vec!["install", "ffmpeg"]);

        let termux =
            unix_system_package_install_command_plan("android", "termux", &["ffmpeg"], false, false, false)
                .expect("Termux should install ffmpeg through pkg");
        assert_eq!(termux[0].program, "pkg");
        assert_eq!(termux[0].args, vec!["install", "-y", "ffmpeg"]);

        assert!(unix_system_package_install_command_plan(
            "linux",
            "opensuse",
            &["ffmpeg"],
            false,
            false,
            false,
        )
        .is_err());
    }

    #[test]
    fn windows_system_package_install_command_plan_matches_shell_recovery() {
        let local_app_data = PathBuf::from("C:/Users/alice/AppData/Local");
        let commands = windows_system_package_install_command_plan(
            &["ffmpeg"],
            true,
            true,
            true,
            Some(local_app_data.as_path()),
        )
        .expect("Windows package managers should plan ffmpeg recovery");

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].program, "winget");
        assert_eq!(
            commands[0].args,
            vec![
                "install",
                "--exact",
                "--id",
                "Gyan.FFmpeg",
                "--source",
                "winget",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]
        );
        assert_eq!(
            commands[0].path_after_install,
            Some(PathBuf::from("C:/Users/alice/AppData/Local/Microsoft/WinGet/Links"))
        );
        assert_eq!(commands[1].program, "choco");
        assert_eq!(commands[1].args, vec!["install", "ffmpeg", "-y"]);
        assert_eq!(commands[2].program, "scoop");
        assert_eq!(commands[2].args, vec!["install", "ffmpeg"]);

        let no_manager = windows_system_package_install_command_plan(
            &["ffmpeg"],
            false,
            false,
            false,
            None,
        );
        assert!(no_manager.is_err());
    }

    #[test]
    fn linux_distro_ids_include_id_like_families() {
        let ids = linux_distro_ids_from_os_release(
            "NAME=Example Linux\nID=example\nID_LIKE=\"debian ubuntu\"\n",
        );

        assert_eq!(ids, vec!["example", "debian", "ubuntu"]);
    }

    #[test]
    fn linux_distro_id_like_unlocks_native_package_recovery() {
        let debian_like = unix_system_package_install_command_plan(
            "linux",
            "example debian",
            &["ffmpeg"],
            true,
            false,
            false,
        )
        .expect("ID_LIKE=debian should use apt recovery");
        assert_eq!(debian_like[0].program, "apt-get");

        let fedora_like = playwright_install_plan("linux", "nobara fedora", true, false)
            .expect("ID_LIKE=fedora should use dnf recovery");
        assert_eq!(fedora_like.system_deps, "dnf");
    }

    #[test]
    fn unix_package_command_failure_includes_process_output() {
        let root = std::env::temp_dir().join(format!(
            "hermes-package-output-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        #[cfg(target_os = "windows")]
        let command = {
            let command = root.join("fake-package-manager.cmd");
            std::fs::write(
                &command,
                "@echo off\r\necho package manager said no 1>&2\r\nexit /b 7\r\n",
            )
            .unwrap();
            command
        };

        #[cfg(not(target_os = "windows"))]
        let command = {
            let command = root.join("fake-package-manager.sh");
            std::fs::write(
                &command,
                "#!/usr/bin/env sh\necho 'package manager said no' >&2\nexit 7\n",
            )
            .unwrap();
            make_executable(&command).unwrap();
            command
        };

        let err = run_unix_system_package_install_command(&UnixPackageInstallCommandPlan {
            program: command.display().to_string(),
            args: Vec::new(),
        })
        .unwrap_err();

        assert!(err.to_string().contains("package manager said no"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_runtime_stage_plan_prefers_managed_uv() {
        let root = std::env::temp_dir().join(format!("hermes-python-plan-{}", std::process::id()));
        let hermes_home = root.join("home");
        let path_tools = root.join("tools");
        std::fs::create_dir_all(hermes_home.join("bin")).unwrap();
        std::fs::create_dir_all(&path_tools).unwrap();
        std::fs::write(hermes_home.join("bin").join("uv.exe"), b"managed uv").unwrap();
        std::fs::write(path_tools.join("uv.exe"), b"path uv").unwrap();

        let plan = python_runtime_stage_plan_for_layout(
            &hermes_home,
            &hermes_home.join("hermes-agent"),
            &path_tools,
            ".EXE",
        )
        .unwrap();

        assert_eq!(plan.uv, hermes_home.join("bin").join("uv.exe"));
        assert_eq!(plan.uv_cache_dir, hermes_home.join("uv-cache"));
        assert_eq!(plan.python_install_dir, hermes_home.join("python"));
        assert_eq!(plan.python_bin_dir, hermes_home.join("bin"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_runtime_dirs_use_shared_paths_for_fhs_layout() {
        let hermes_home = PathBuf::from("/root/.hermes");

        let fhs_dirs = python_runtime_dirs_for_layout(
            &hermes_home,
            &PathBuf::from("/usr/local/lib/hermes-agent")
        );
        let user_dirs = python_runtime_dirs_for_layout(&hermes_home, &hermes_home.join("hermes-agent"));

        assert_eq!(fhs_dirs.install_dir, PathBuf::from("/usr/local/share/uv/python"));
        assert_eq!(fhs_dirs.bin_dir, PathBuf::from("/usr/local/share/uv/bin"));
        assert_eq!(user_dirs.install_dir, hermes_home.join("python"));
        assert_eq!(user_dirs.bin_dir, hermes_home.join("bin"));
    }

    #[test]
    fn python_venv_stage_plan_uses_managed_python_dirs() {
        let root = std::env::temp_dir().join(format!("hermes-python-venv-plan-{}", std::process::id()));
        let hermes_home = root.join("home");
        let install_root = hermes_home.join("hermes-agent");
        std::fs::create_dir_all(hermes_home.join("bin")).unwrap();
        std::fs::create_dir_all(&install_root).unwrap();
        std::fs::write(hermes_home.join("bin").join("uv.exe"), b"managed uv").unwrap();

        let plan = python_venv_stage_plan(&install_root, &hermes_home, "", ".EXE").unwrap();

        assert_eq!(plan.uv_cache_dir, hermes_home.join("uv-cache"));
        assert_eq!(plan.python_install_dir, hermes_home.join("python"));
        assert_eq!(plan.python_bin_dir, hermes_home.join("bin"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn platform_sdks_skip_result_skips_only_when_no_platform_tokens_are_configured() {
        let root = std::env::temp_dir().join(format!(
            "hermes-platform-sdks-skip-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        std::fs::create_dir_all(&hermes_home).unwrap();

        let platform_sdks = stage_info(
            "platform-sdks",
            "Installing messaging platform SDKs",
            "finalize",
            false,
        );
        let missing_env = platform_sdks_skip_result(&platform_sdks, &hermes_home).unwrap();

        assert_eq!(missing_env.stage, "platform-sdks");
        assert_eq!(missing_env.ok, true);
        assert_eq!(missing_env.skipped, true);
        assert_eq!(
            missing_env.reason.as_deref(),
            Some("no messaging platform tokens configured")
        );

        std::fs::write(
            hermes_home.join(".env"),
            "TELEGRAM_BOT_TOKEN=your-token-here\nDISCORD_BOT_TOKEN=\n",
        )
        .unwrap();
        assert!(platform_sdks_skip_result(&platform_sdks, &hermes_home).is_some());

        std::fs::write(hermes_home.join(".env"), "TELEGRAM_BOT_TOKEN=abc123\n").unwrap();
        assert!(platform_sdks_skip_result(&platform_sdks, &hermes_home).is_none());

        let config = stage_info(
            "config-templates",
            "Writing configuration templates",
            "finalize",
            false,
        );
        assert!(platform_sdks_skip_result(&config, &hermes_home).is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn platform_sdk_requirements_map_real_tokens_to_imports_and_specs() {
        let env = concat!(
            "# TELEGRAM_BOT_TOKEN=ignored\n",
            "TELEGRAM_BOT_TOKEN=abc123\n",
            "DISCORD_BOT_TOKEN=your-token-here\n",
            "SLACK_BOT_TOKEN=xoxb-test\n",
            "WHATSAPP_ENABLED=false\n",
            "SLACK_APP_TOKEN=\n",
        );

        let requirements = platform_sdk_requirements_from_env(env);
        let names = requirements
            .iter()
            .map(|sdk| (sdk.env_var, sdk.import_name, sdk.pip_spec))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                (
                    "TELEGRAM_BOT_TOKEN",
                    "telegram",
                    "python-telegram-bot[webhooks]>=22.6,<23",
                ),
                ("SLACK_BOT_TOKEN", "slack_sdk", "slack-sdk>=3.27.0,<4"),
            ]
        );
    }

    #[test]
    fn platform_sdk_requirements_ignore_disabled_token_values() {
        let env = concat!(
            "TELEGRAM_BOT_TOKEN=false\n",
            "DISCORD_BOT_TOKEN=0\n",
            "SLACK_BOT_TOKEN=no\n",
            "SLACK_APP_TOKEN=off\n",
            "WHATSAPP_ENABLED=null\n",
        );

        assert!(platform_sdk_requirements_from_env(env).is_empty());
    }

    #[test]
    fn platform_sdk_stage_plan_uses_venv_python_and_configured_requirements() {
        let root = std::env::temp_dir().join(format!("hermes-platform-plan-{}", std::process::id()));
        let hermes_home = root.join("home");
        let install_root = hermes_home.join("hermes-agent");
        let venv_python = venv_python_path(&install_root.join("venv"));
        let checkout_wheelhouse = install_root.join("resources").join("wheelhouse");
        let resource_wheelhouse = root.join("tauri-resources").join("wheelhouse");
        std::fs::create_dir_all(venv_python.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&checkout_wheelhouse).unwrap();
        std::fs::create_dir_all(&resource_wheelhouse).unwrap();
        std::fs::write(&venv_python, b"python").unwrap();
        std::fs::write(
            checkout_wheelhouse.join("checkout-0.1-py3-none-any.whl"),
            b"wheel",
        )
        .unwrap();
        std::fs::write(
            resource_wheelhouse.join("resource-0.1-py3-none-any.whl"),
            b"wheel",
        )
        .unwrap();
        std::fs::create_dir_all(&hermes_home).unwrap();
        std::fs::write(hermes_home.join(".env"), "WHATSAPP_ENABLED=true\n").unwrap();

        let plan =
            platform_sdk_stage_plan(&hermes_home, &install_root, Some(&resource_wheelhouse)).unwrap();

        assert_eq!(plan.python, venv_python);
        assert_eq!(plan.pip_cache_dir, hermes_home.join("pip-cache"));
        assert_eq!(plan.wheelhouse_dir, Some(resource_wheelhouse));
        assert_eq!(plan.requirements.len(), 1);
        assert_eq!(plan.requirements[0].import_name, "qrcode");
        assert_eq!(plan.requirements[0].pip_spec, "qrcode>=7.0,<8");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn platform_sdk_install_commands_include_uv_pip_fallback() {
        let plan = PlatformSdkStagePlan {
            python: PathBuf::from("/opt/hermes/venv/bin/python"),
            uv: Some(PathBuf::from("/opt/hermes/bin/uv")),
            pip_cache_dir: PathBuf::from("/opt/hermes/pip-cache"),
            uv_cache_dir: PathBuf::from("/opt/hermes/uv-cache"),
            wheelhouse_dir: Some(PathBuf::from("/opt/hermes/hermes-agent/resources/wheelhouse")),
            requirements: Vec::new(),
        };
        let sdk = PlatformSdkRequirement {
            env_var: "SLACK_BOT_TOKEN",
            import_name: "slack_sdk",
            pip_spec: "slack-sdk>=3.27.0,<4",
        };

        let commands = platform_sdk_install_commands(&plan, sdk);

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].method, "wheelhouse");
        assert_eq!(commands[0].program, plan.python);
        assert_eq!(
            commands[0].args,
            vec![
                "-m",
                "pip",
                "install",
                "--no-index",
                "--find-links",
                "/opt/hermes/hermes-agent/resources/wheelhouse",
                "slack-sdk>=3.27.0,<4",
            ]
        );
        assert_eq!(
            commands[0].env,
            vec![("PIP_CACHE_DIR".to_string(), PathBuf::from("/opt/hermes/pip-cache"))]
        );
        assert_eq!(commands[1].method, "pip");
        assert_eq!(commands[1].program, plan.python);
        assert_eq!(
            commands[1].args,
            vec!["-m", "pip", "install", "slack-sdk>=3.27.0,<4"]
        );
        assert_eq!(
            commands[1].env,
            vec![("PIP_CACHE_DIR".to_string(), PathBuf::from("/opt/hermes/pip-cache"))]
        );
        assert_eq!(commands[2].method, "uv");
        assert_eq!(commands[2].program, PathBuf::from("/opt/hermes/bin/uv"));
        assert_eq!(
            commands[2].args,
            vec![
                "pip",
                "install",
                "--python",
                "/opt/hermes/venv/bin/python",
                "slack-sdk>=3.27.0,<4"
            ]
        );
        assert_eq!(
            commands[2].env,
            vec![("UV_CACHE_DIR".to_string(), PathBuf::from("/opt/hermes/uv-cache"))]
        );
    }

    #[test]
    fn write_bootstrap_marker_uses_pin_and_default_branch_without_bom() {
        let root = std::env::temp_dir().join(format!(
            "hermes-marker-test-{}",
            std::process::id()
        ));
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(&install_root).unwrap();

        let marker = write_bootstrap_marker(&install_root, Some("abcdef123"), None).unwrap();
        let marker_path = install_root.join(".hermes-bootstrap-complete");
        let bytes = std::fs::read(&marker_path).unwrap();

        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(marker["schemaVersion"], 1);
        assert_eq!(marker["pinnedCommit"], "abcdef123");
        assert_eq!(marker["pinnedBranch"], "main");
        assert!(marker["completedAt"].as_str().unwrap().ends_with('Z'));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_bootstrap_marker_rejects_missing_install_root() {
        let root = std::env::temp_dir().join(format!(
            "hermes-marker-missing-{}",
            std::process::id()
        ));

        let err = write_bootstrap_marker(&root.join("hermes-agent"), Some("abcdef123"), None)
            .unwrap_err();

        assert!(err.to_string().contains("install root does not exist"));
    }

    #[test]
    fn configure_templates_preserves_user_files_and_copies_skill_fallback() {
        let root = std::env::temp_dir().join(format!(
            "hermes-config-test-{}",
            std::process::id()
        ));
        let hermes_home = root.join("home");
        let install_root = root.join("hermes-agent");
        std::fs::create_dir_all(install_root.join("skills").join("demo")).unwrap();
        std::fs::write(install_root.join(".env.example"), "TOKEN=\n").unwrap();
        std::fs::write(install_root.join("cli-config.yaml.example"), "model: test\n").unwrap();
        std::fs::write(install_root.join("skills").join("demo").join("SKILL.md"), "# Demo\n")
            .unwrap();
        std::fs::create_dir_all(&hermes_home).unwrap();
        std::fs::write(hermes_home.join(".env"), "USER=1\n").unwrap();

        let report = configure_templates(&hermes_home, &install_root).unwrap();

        assert_eq!(report["envCreated"], false);
        assert_eq!(std::fs::read_to_string(hermes_home.join(".env")).unwrap(), "USER=1\n");
        assert_eq!(
            std::fs::read_to_string(hermes_home.join("config.yaml")).unwrap(),
            "model: test\n"
        );
        assert!(hermes_home.join("cron").is_dir());
        assert!(hermes_home.join("sessions").is_dir());
        assert!(hermes_home.join("skills").join("demo").join("SKILL.md").exists());
        let soul_bytes = std::fs::read(hermes_home.join("SOUL.md")).unwrap();
        assert!(!soul_bytes.starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(report["skillsSync"], "copied");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn skills_sync_env_sets_hermes_home_for_python_child() {
        let hermes_home = PathBuf::from("C:/Users/example/AppData/Local/hermes");
        let env = skills_sync_env(&hermes_home);

        assert_eq!(env.0, "HERMES_HOME");
        assert_eq!(env.1, hermes_home);
    }

    #[test]
    fn windows_path_stage_plan_reports_path_and_hermes_home_changes() {
        let hermes_home = PathBuf::from("C:/Users/example/AppData/Local/hermes");
        let install_root = hermes_home.join("hermes-agent");

        let report = windows_path_stage_plan(
            &hermes_home,
            &install_root,
            Some("C:/Windows/System32".to_string()),
            Some("C:/old/hermes".to_string()),
        );

        assert_eq!(
            report["hermesBin"],
            install_root.join("venv").join("Scripts").display().to_string()
        );
        assert_eq!(report["pathChanged"], true);
        assert_eq!(report["hermesHomeChanged"], true);
        assert_eq!(report["applied"], false);
    }

    #[test]
    fn unix_profile_path_uses_fish_config_for_fish_shell() {
        let home = PathBuf::from("/home/user");

        assert_eq!(
            default_unix_profile_path_for(&home, "/usr/bin/fish").unwrap(),
            home.join(".config").join("fish").join("config.fish")
        );
        assert_eq!(
            default_unix_profile_path_for(&home, "/bin/zsh").unwrap(),
            home.join(".zshrc")
        );
        assert_eq!(
            default_unix_profile_path_for(&home, "/bin/bash").unwrap(),
            home.join(".bashrc")
        );
    }

    #[test]
    fn unix_command_link_dir_uses_system_bin_for_fhs_layout() {
        let hermes_home = PathBuf::from("/root/.hermes");

        assert_eq!(
            unix_command_link_dir_for_layout(
                &hermes_home,
                &PathBuf::from("/usr/local/lib/hermes-agent")
            ),
            PathBuf::from("/usr/local/bin")
        );
        assert_eq!(
            unix_command_link_dir_for_layout(&hermes_home, &hermes_home.join("hermes-agent")),
            hermes_home.join("bin")
        );
    }

    #[test]
    fn unix_path_stage_writes_managed_profile_block() {
        let root = std::env::temp_dir().join(format!("hermes-unix-path-{}", std::process::id()));
        let home = root.join("home");
        let install_root = root.join("hermes-agent");
        let profile = home.join(".profile");
        let hermes_entry = install_root.join("venv").join("bin").join("hermes");
        std::fs::create_dir_all(hermes_entry.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&hermes_entry, "#!/bin/sh\n").unwrap();
        std::fs::write(&profile, "alias ll='ls -la'\n").unwrap();

        let report =
            configure_unix_path_stage_with_profile(&home, &install_root, &profile, None).unwrap();

        let launcher = home.join("bin").join("hermes");
        let launcher_text = std::fs::read_to_string(&launcher).unwrap();
        assert!(launcher_text.contains(&format!("exec \"{}\" \"$@\"", hermes_entry.display())));
        let text = std::fs::read_to_string(&profile).unwrap();
        assert!(text.contains("alias ll='ls -la'"));
        assert!(text.contains("Hermes Agent PATH"));
        assert!(text.contains(&install_root.join("venv").join("bin").display().to_string()));
        assert!(text.contains(&home.join("bin").display().to_string()));
        assert_eq!(report["profilePath"], profile.display().to_string());
        assert_eq!(report["profileChanged"], true);
        assert_eq!(report["pathChanged"], true);
        assert_eq!(
            report["hermesBin"],
            install_root.join("venv").join("bin").display().to_string()
        );
        assert_eq!(report["launcherPath"], launcher.display().to_string());
        assert_eq!(report["launcherChanged"], true);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unix_path_stage_writes_multiple_profile_blocks() {
        let root = std::env::temp_dir().join(format!(
            "hermes-unix-path-multi-{}",
            std::process::id()
        ));
        let home = root.join("home");
        let install_root = root.join("hermes-agent");
        let bashrc = home.join(".bashrc");
        let profile = home.join(".profile");
        let hermes_entry = install_root.join("venv").join("bin").join("hermes");
        std::fs::create_dir_all(hermes_entry.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&hermes_entry, "#!/bin/sh\n").unwrap();
        std::fs::write(&bashrc, "alias ll='ls -la'\n").unwrap();
        std::fs::write(&profile, "export EDITOR=vim\n").unwrap();

        let report = configure_unix_path_stage_with_profiles(
            &home,
            &install_root,
            &[bashrc.clone(), profile.clone()],
            None,
        )
        .unwrap();

        for profile_path in [&bashrc, &profile] {
            let text = std::fs::read_to_string(profile_path).unwrap();
            assert!(text.contains("Hermes Agent PATH"));
            assert!(text.contains(&install_root.join("venv").join("bin").display().to_string()));
            assert!(text.contains(&home.join("bin").display().to_string()));
        }
        assert_eq!(
            report["profilePaths"],
            serde_json::json!([
                bashrc.display().to_string(),
                profile.display().to_string()
            ])
        );
        assert_eq!(report["profileChanged"], true);
        assert_eq!(report["applied"], true);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_method_stamp_writes_git_for_update_compatibility() {
        let root = std::env::temp_dir().join(format!("hermes-install-method-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        let report = write_install_method_stamp(&root).unwrap();

        assert_eq!(std::fs::read_to_string(root.join(".install_method")).unwrap(), "git\n");
        assert_eq!(report["installMethod"], "git");
        assert_eq!(report["stampPath"], root.join(".install_method").display().to_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_venv_stage_plan_targets_install_root_and_prefers_managed_uv() {
        let root = std::env::temp_dir().join(format!("hermes-venv-plan-{}", std::process::id()));
        let hermes_home = root.join("home");
        let install_root = hermes_home.join("hermes-agent");
        let path_tools = root.join("tools");
        std::fs::create_dir_all(hermes_home.join("bin")).unwrap();
        std::fs::create_dir_all(&install_root).unwrap();
        std::fs::create_dir_all(&path_tools).unwrap();
        std::fs::write(hermes_home.join("bin").join("uv.exe"), b"managed uv").unwrap();
        std::fs::write(path_tools.join("uv.exe"), b"path uv").unwrap();

        let plan = python_venv_stage_plan(&install_root, &hermes_home, &path_tools, ".EXE")
            .unwrap();

        assert_eq!(plan.uv, hermes_home.join("bin").join("uv.exe"));
        assert_eq!(plan.venv, install_root.join("venv"));
        assert_eq!(plan.cwd, install_root);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependencies_stage_plan_requires_lock_and_targets_venv() {
        let root = std::env::temp_dir().join(format!("hermes-deps-plan-{}", std::process::id()));
        let hermes_home = root.join("home");
        let install_root = hermes_home.join("hermes-agent");
        let path_tools = root.join("tools");
        std::fs::create_dir_all(hermes_home.join("bin")).unwrap();
        std::fs::create_dir_all(&install_root).unwrap();
        std::fs::create_dir_all(&path_tools).unwrap();
        std::fs::write(hermes_home.join("bin").join("uv.exe"), b"managed uv").unwrap();

        let missing_lock =
            python_dependencies_stage_plan(&install_root, &hermes_home, &path_tools, ".EXE")
                .unwrap_err();
        assert!(missing_lock.to_string().contains("uv.lock"));

        std::fs::write(install_root.join("uv.lock"), b"lock").unwrap();
        let plan =
            python_dependencies_stage_plan(&install_root, &hermes_home, &path_tools, ".EXE")
                .unwrap();

        assert_eq!(plan.uv, hermes_home.join("bin").join("uv.exe"));
        assert_eq!(plan.cwd, install_root);
        assert_eq!(plan.venv, plan.cwd.join("venv"));
        assert_eq!(plan.python, venv_python_path(&plan.venv));
        assert_eq!(plan.uv_cache_dir, hermes_home.join("uv-cache"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependency_install_tiers_preserve_script_fallback_order() {
        let tiers = python_dependency_install_tiers_for_pyproject("", &[]);

        assert_eq!(tiers.len(), 4);
        assert_eq!(tiers[0].name, "hash-verified (uv.lock)");
        assert_eq!(tiers[0].args, vec!["sync", "--extra", "all", "--locked"]);
        assert_eq!(tiers[1].name, "all");
        assert_eq!(tiers[1].args, vec!["pip", "install", "-e", ".[all]"]);
        assert_eq!(tiers[2].name, "all minus known-broken (none)");
        assert_eq!(tiers[2].args, vec!["pip", "install", "-e", ".[all]"]);
        assert_eq!(tiers[3].name, "core only (no extras)");
        assert_eq!(tiers[3].args, vec!["pip", "install", "-e", "."]);
    }

    #[test]
    fn python_dependency_install_tiers_prefer_local_wheelhouse_when_present() {
        let root = std::env::temp_dir().join(format!(
            "hermes-wheelhouse-tier-{}",
            std::process::id()
        ));
        let wheelhouse = root.join("resources").join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        std::fs::write(wheelhouse.join("demo-0.1-py3-none-any.whl"), b"wheel").unwrap();
        std::fs::write(root.join("pyproject.toml"), b"").unwrap();

        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&root, None);

        assert_eq!(tiers[0].name, "local wheelhouse (all)");
        assert_eq!(
            tiers[0].args,
            vec![
                "pip".to_string(),
                "install".to_string(),
                "--no-index".to_string(),
                "--find-links".to_string(),
                wheelhouse.display().to_string(),
                "-e".to_string(),
                ".[all]".to_string(),
            ]
        );
        assert_eq!(tiers[1].name, "hash-verified (uv.lock)");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependency_install_tiers_prefer_resource_wheelhouse_over_checkout() {
        let root = std::env::temp_dir().join(format!(
            "hermes-resource-wheelhouse-tier-{}",
            std::process::id()
        ));
        let checkout = root.join("checkout");
        let checkout_wheelhouse = checkout.join("resources").join("wheelhouse");
        let resource_wheelhouse = root.join("tauri-resources").join("wheelhouse");
        std::fs::create_dir_all(&checkout_wheelhouse).unwrap();
        std::fs::create_dir_all(&resource_wheelhouse).unwrap();
        std::fs::write(
            checkout_wheelhouse.join("checkout-0.1-py3-none-any.whl"),
            b"wheel",
        )
        .unwrap();
        std::fs::write(
            resource_wheelhouse.join("resource-0.1-py3-none-any.whl"),
            b"wheel",
        )
        .unwrap();
        std::fs::write(checkout.join("pyproject.toml"), b"").unwrap();

        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(
            &checkout,
            Some(&resource_wheelhouse),
        );

        assert_eq!(tiers[0].name, "local wheelhouse (all)");
        assert_eq!(
            tiers[0].args[4],
            resource_wheelhouse.display().to_string()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependency_install_tiers_skip_manifest_mismatched_wheelhouse() {
        let root = std::env::temp_dir().join(format!(
            "hermes-wheelhouse-manifest-tier-{}",
            std::process::id()
        ));
        let wheelhouse = root.join("resources").join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        std::fs::write(root.join("pyproject.toml"), b"project").unwrap();
        let source_sha = crate::artifact::sha256_hex(b"project");
        std::fs::write(wheelhouse.join("demo-0.1-py3-none-any.whl"), b"wheel").unwrap();
        std::fs::write(
            wheelhouse.join("wheelhouse-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "sourceFiles": [
    {{
      "path": "pyproject.toml",
      "sha256": "{source_sha}"
    }}
  ],
  "wheels": [
    {{
      "arch": "x64",
      "platform": "windows",
      "python": "cp311",
      "name": "demo-0.1-py3-none-any.whl",
      "sizeBytes": 5,
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
    }}
  ]
}}
"#,
            ),
        )
        .unwrap();

        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&root, None);

        assert_eq!(tiers[0].name, "hash-verified (uv.lock)");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependency_install_tiers_validate_wheelhouse_source_files() {
        let root = std::env::temp_dir().join(format!(
            "hermes-wheelhouse-source-tier-{}",
            std::process::id()
        ));
        let wheelhouse = root.join("resources").join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        std::fs::write(root.join("pyproject.toml"), b"project").unwrap();
        let wheel = wheelhouse.join("demo-0.1-py3-none-any.whl");
        std::fs::write(&wheel, b"wheel").unwrap();
        let source_sha = crate::artifact::sha256_hex(b"project");
        let wheel_sha = crate::artifact::sha256_hex(b"wheel");
        let platform = current_wheelhouse_platform();
        let arch = current_wheelhouse_arch().unwrap_or("x64");
        std::fs::write(
            wheelhouse.join("wheelhouse-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "sourceFiles": [
    {{
      "path": "pyproject.toml",
      "sha256": "{source_sha}"
    }}
  ],
  "wheels": [
    {{
      "arch": "{arch}",
      "platform": "{platform}",
      "python": "cp311",
      "name": "demo-0.1-py3-none-any.whl",
      "sizeBytes": 5,
      "sha256": "{wheel_sha}"
    }}
  ]
}}
"#,
            ),
        )
        .unwrap();

        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&root, None);
        assert_eq!(tiers[0].name, "local wheelhouse (all)");

        std::fs::write(root.join("pyproject.toml"), b"changed").unwrap();
        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&root, None);
        assert_eq!(tiers[0].name, "hash-verified (uv.lock)");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_dependency_install_tiers_skip_wrong_target_wheelhouse() {
        let root = std::env::temp_dir().join(format!(
            "hermes-wheelhouse-target-tier-{}",
            std::process::id()
        ));
        let wheelhouse = root.join("resources").join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        std::fs::write(root.join("pyproject.toml"), b"project").unwrap();
        let source_sha = crate::artifact::sha256_hex(b"project");
        let wheel = wheelhouse.join("demo-0.1-py3-none-any.whl");
        std::fs::write(&wheel, b"wheel").unwrap();
        let wrong_platform = if cfg!(target_os = "windows") {
            "linux"
        } else {
            "windows"
        };
        let wrong_arch = if cfg!(target_arch = "x86_64") {
            "arm64"
        } else {
            "x64"
        };
        std::fs::write(
            wheelhouse.join("wheelhouse-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "sourceFiles": [
    {{
      "path": "pyproject.toml",
      "sha256": "{source_sha}"
    }}
  ],
  "wheels": [
    {{
      "arch": "{wrong_arch}",
      "platform": "{wrong_platform}",
      "python": "cp311",
      "name": "demo-0.1-py3-none-any.whl",
      "sizeBytes": 5,
      "sha256": "{}"
    }}
  ]
}}
"#,
                crate::artifact::sha256_hex(b"wheel")
            ),
        )
        .unwrap();

        let tiers = python_dependency_install_tiers_for_cwd_with_wheelhouse(&root, None);

        assert_eq!(tiers[0].name, "hash-verified (uv.lock)");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn python_known_broken_extra_tier_filters_pyproject_all_members() {
        let pyproject = r#"
[project.optional-dependencies]
all = [
  "hermes-agent[cron]",
  "hermes-agent[web]",
  "not-hermes[ignored]",
  "hermes-agent[youtube]",
]
"#;

        let tiers = python_dependency_install_tiers_for_pyproject(pyproject, &["web"]);

        assert_eq!(tiers[2].name, "all minus known-broken (web)");
        assert_eq!(tiers[2].args, vec!["pip", "install", "-e", ".[cron,youtube]"]);
    }

    #[test]
    fn repository_archive_spec_prefers_commit_and_keeps_branch() {
        let spec = repository_archive_spec(Some("abcdef123"), Some("main"));

        assert_eq!(spec.owner, "NousResearch");
        assert_eq!(spec.repo, "hermes-agent");
        assert_eq!(spec.commit.as_deref(), Some("abcdef123"));
        assert_eq!(spec.branch.as_deref(), Some("main"));
    }

    #[test]
    fn repository_archive_spec_defaults_to_main_without_pin() {
        let spec = repository_archive_spec(None, None);

        assert_eq!(spec.commit, None);
        assert_eq!(spec.branch.as_deref(), Some("main"));
        assert_eq!(
            spec.github_zip_url().unwrap(),
            "https://github.com/NousResearch/hermes-agent/archive/main.zip"
        );
    }
}
