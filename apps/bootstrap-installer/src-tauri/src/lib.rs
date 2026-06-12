//! Hermes Setup — Tauri entrypoint.
//!
//! Spawns a single window pointed at the React frontend (apps/bootstrap-installer/src/).
//! All install-time work lives in `bootstrap.rs` and is invoked through the Tauri
//! commands registered at the bottom of `run()`.
//!
//! The Windows-subsystem strip lives on the binary crate (src/main.rs), not
//! here — a crate-level attribute on a lib doesn't propagate to the linker
//! flags of the executable that consumes it.

pub mod artifact;
mod bootstrap;
mod events;
mod install_script;
mod orchestrator;
mod paths;
mod powershell;
pub mod repo_archive;
mod update;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

const BOOTSTRAP_TOOLS_MANIFEST: &str = "bootstrap-tools-manifest.json";
const ALLOWED_BOOTSTRAP_TOOLS_METADATA: [&str; 2] = [".gitignore", "README.md"];
const WHEELHOUSE_MANIFEST: &str = "wheelhouse-manifest.json";
const ALLOWED_WHEELHOUSE_METADATA: [&str; 2] = [".gitignore", "README.md"];

/// Machine-readable report emitted by the no-UI bootstrap installer self-check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BootstrapSelfCheckReport {
    pub ok: bool,
    pub commit: Option<String>,
    pub branch: Option<String>,
    pub embedded_scripts: Vec<install_script::BundledScriptResource>,
    pub bootstrap_tools_archives: Option<usize>,
    pub python_wheelhouse_wheels: Option<usize>,
    pub errors: Vec<String>,
}

/// How the installer was invoked. Resolved once from the process args in
/// `run()` and exposed to the frontend via `get_mode` so it can route to the
/// install flow (first-run onboarding) or the update flow (driven by the
/// desktop app handing off via `Hermes-Setup.exe --update`).
///
/// Bare launch (double-click, first-run) => Install.
/// `--update` (spawned by the desktop's "Update" button) => Update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Install,
    Update,
}

impl AppMode {
    /// Resolve the mode from an argument iterator. Anything containing the
    /// `--update` flag selects Update; otherwise Install. Kept arg-iterator
    /// generic (not reading `std::env` directly) so it's unit-testable.
    pub fn from_args<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for a in args {
            if a.as_ref() == "--update" {
                return AppMode::Update;
            }
        }
        AppMode::Install
    }
}

/// Returns true when the args request a forced installer UI (repair/reinstall)
/// via `--reinstall` or `--repair`, which overrides the macOS launcher
/// fast-path so a broken install can be repaired. Arg-iterator generic so it's
/// unit-testable, mirroring `AppMode::from_args`. Independent of mode selection:
/// these flags never flip Install<->Update.
pub fn force_setup_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|a| a.as_ref() == "--reinstall" || a.as_ref() == "--repair")
}

/// Build a no-UI self-check report for release-package smoke tests.
pub fn bootstrap_self_check_report(
    commit: Option<&str>,
    branch: Option<&str>,
    bootstrap_tools_dir: Option<&Path>,
    wheelhouse_dir: Option<&Path>,
    wheelhouse_platform: Option<&str>,
) -> BootstrapSelfCheckReport {
    let embedded_scripts = install_script::bundled_script_manifest();
    let mut errors = Vec::new();
    let mut bootstrap_tools_archives = None;
    let mut python_wheelhouse_wheels = None;
    if commit.map(|value| value.trim().is_empty()).unwrap_or(true) {
        errors.push("installer was built without a commit pin".to_string());
    }
    if embedded_scripts.len() < 2 {
        errors.push("installer is missing embedded install scripts".to_string());
    }
    for script in &embedded_scripts {
        if script.size_bytes == 0 {
            errors.push(format!(
                "embedded install script is empty: {}",
                script.filename
            ));
        }
        if script.sha256.len() != 64 || !script.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
            errors.push(format!(
                "embedded install script has invalid sha256: {}",
                script.filename
            ));
        }
    }
    if let Some(dir) = bootstrap_tools_dir {
        match validate_bootstrap_tools_for_self_check(dir) {
            Ok(count) => bootstrap_tools_archives = Some(count),
            Err(err) => errors.push(err),
        }
    }
    if let Some(dir) = wheelhouse_dir {
        match validate_wheelhouse_for_self_check(dir, wheelhouse_platform) {
            Ok(count) => python_wheelhouse_wheels = Some(count),
            Err(err) => errors.push(err),
        }
    }
    BootstrapSelfCheckReport {
        ok: errors.is_empty(),
        commit: commit.map(str::to_string),
        branch: branch.map(str::to_string),
        embedded_scripts,
        bootstrap_tools_archives,
        python_wheelhouse_wheels,
        errors,
    }
}

fn expected_self_check_commit<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        if let Some(value) = arg.strip_prefix("--self-check-expect-commit=") {
            return Some(value.to_string());
        }
        if arg == "--self-check-expect-commit" {
            return iter.next().map(|value| value.as_ref().to_string());
        }
    }
    None
}

fn self_check_bootstrap_tools_dir<I, S>(args: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        if let Some(value) = arg.strip_prefix("--self-check-bootstrap-tools=") {
            return Some(PathBuf::from(value));
        }
        if arg == "--self-check-bootstrap-tools" {
            return iter.next().map(|value| PathBuf::from(value.as_ref()));
        }
    }
    None
}

fn self_check_wheelhouse_dir<I, S>(args: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        if let Some(value) = arg.strip_prefix("--self-check-wheelhouse=") {
            return Some(PathBuf::from(value));
        }
        if arg == "--self-check-wheelhouse" {
            return iter.next().map(|value| PathBuf::from(value.as_ref()));
        }
    }
    None
}

fn self_check_wheelhouse_platform<I, S>(args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        if let Some(value) = arg.strip_prefix("--self-check-wheelhouse-platform=") {
            return Some(value.to_string());
        }
        if arg == "--self-check-wheelhouse-platform" {
            return iter.next().map(|value| value.as_ref().to_string());
        }
    }
    None
}

fn validate_bootstrap_tools_for_self_check(dir: &Path) -> Result<usize, String> {
    let manifest_path = dir.join(BOOTSTRAP_TOOLS_MANIFEST);
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("reading bootstrap tools manifest failed: {err}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| format!("parsing bootstrap tools manifest failed: {err}"))?;
    if manifest.get("schemaVersion").and_then(|value| value.as_u64()) != Some(1) {
        return Err("bootstrap tools manifest has unsupported schema".to_string());
    }
    let archives = manifest
        .get("archives")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "bootstrap tools manifest has no archives".to_string())?;
    if archives.is_empty() {
        return Err("bootstrap tools manifest has no archives".to_string());
    }

    let mut expected = std::collections::BTreeSet::from([BOOTSTRAP_TOOLS_MANIFEST.to_string()]);
    for name in ALLOWED_BOOTSTRAP_TOOLS_METADATA {
        expected.insert(name.to_string());
    }
    for archive in archives {
        let name = archive
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "bootstrap tools manifest archive is missing name".to_string())?;
        if !bootstrap_tool_name_is_plain_file(name) {
            return Err(format!("bootstrap tools manifest archive has unsafe name: {name}"));
        }
        if !expected.insert(name.to_string()) {
            return Err(format!("duplicate archive in bootstrap tools manifest: {name}"));
        }
        let arch = archive
            .get("arch")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("bootstrap tool archive is missing arch: {name}"))?;
        if arch.trim().is_empty() {
            return Err(format!("bootstrap tool archive is missing arch: {name}"));
        }
        let platform = archive
            .get("platform")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("bootstrap tool archive is missing platform: {name}"))?;
        if !matches!(platform, "windows" | "linux" | "macos") {
            return Err(format!("bootstrap tool archive is missing platform: {name}"));
        }
        if let Some((expected_platform, expected_arch)) = bootstrap_tool_archive_target(name) {
            if platform != expected_platform || arch != expected_arch {
                return Err(format!("bootstrap tool archive target mismatch: {name}"));
            }
        }
        let url = archive
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("bootstrap tool archive is missing url: {name}"))?;
        if !url.starts_with("https://") {
            return Err(format!("bootstrap tool archive has invalid url: {name}"));
        }
        let path = dir.join(name);
        let bytes = std::fs::read(&path)
            .map_err(|err| format!("reading bootstrap tool archive failed: {name}: {err}"))?;
        let expected_size = archive
            .get("sizeBytes")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| format!("bootstrap tool archive is missing sizeBytes: {name}"))?;
        if bytes.len() as u64 != expected_size {
            return Err(format!(
                "bootstrap tool archive size mismatch: {name}: expected {expected_size}, got {}",
                bytes.len()
            ));
        }
        let expected_sha256 = archive
            .get("sha256")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("bootstrap tool archive is missing sha256: {name}"))?;
        if expected_sha256.len() != 64
            || !expected_sha256.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(format!("bootstrap tool archive has invalid sha256: {name}"));
        }
        let actual_sha256 = crate::artifact::sha256_hex(&bytes);
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(format!(
                concat!(
                    "bootstrap tool archive checksum mismatch: {name}: ",
                    "expected {expected_sha256}, got {actual_sha256}"
                ),
                name = name,
                expected_sha256 = expected_sha256,
                actual_sha256 = actual_sha256
            ));
        }
    }

    for entry in std::fs::read_dir(dir)
        .map_err(|err| format!("reading bootstrap tools directory failed: {err}"))?
    {
        let entry = entry.map_err(|err| format!("reading bootstrap tools entry failed: {err}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !expected.contains(&name) {
            return Err(format!("unmanifested bootstrap tool payload: {name}"));
        }
        if !entry.path().is_file() {
            return Err(format!("bootstrap tool payload is not a file: {name}"));
        }
    }
    Ok(archives.len())
}

fn validate_wheelhouse_for_self_check(
    dir: &Path,
    expected_platform: Option<&str>,
) -> Result<usize, String> {
    let manifest_path = dir.join(WHEELHOUSE_MANIFEST);
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("reading wheelhouse manifest failed: {err}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| format!("parsing wheelhouse manifest failed: {err}"))?;
    if manifest.get("schemaVersion").and_then(|value| value.as_u64()) != Some(1) {
        return Err("wheelhouse manifest has unsupported schema".to_string());
    }
    let wheels = manifest
        .get("wheels")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "wheelhouse manifest has no wheels".to_string())?;
    if wheels.is_empty() {
        return Err("wheelhouse manifest has no wheels".to_string());
    }

    let mut expected = std::collections::BTreeSet::from([WHEELHOUSE_MANIFEST.to_string()]);
    for name in ALLOWED_WHEELHOUSE_METADATA {
        expected.insert(name.to_string());
    }
    for wheel in wheels {
        let name = wheel
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "wheelhouse manifest wheel is missing name".to_string())?;
        if !wheel_name_is_plain_file(name) {
            return Err(format!("wheelhouse manifest wheel has unsafe name: {name}"));
        }
        if !expected.insert(name.to_string()) {
            return Err(format!("duplicate wheel in wheelhouse manifest: {name}"));
        }
        let arch = wheel
            .get("arch")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("wheelhouse wheel is missing arch: {name}"))?;
        if arch.trim().is_empty() {
            return Err(format!("wheelhouse wheel is missing arch: {name}"));
        }
        let platform = wheel
            .get("platform")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("wheelhouse wheel is missing platform: {name}"))?;
        if !matches!(platform, "windows" | "linux" | "macos") {
            return Err(format!("wheelhouse wheel is missing platform: {name}"));
        }
        if let Some(expected) = expected_platform {
            if platform != expected {
                return Err(format!(
                    "unexpected wheelhouse platform for {name}: expected {expected}, got {platform}"
                ));
            }
        }
        let python = wheel
            .get("python")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("wheelhouse wheel is missing python tag: {name}"))?;
        if !python.starts_with("cp") || python.len() <= 2 {
            return Err(format!("wheelhouse wheel has invalid python tag: {name}"));
        }
        let path = dir.join(name);
        let bytes = std::fs::read(&path)
            .map_err(|err| format!("reading wheelhouse wheel failed: {name}: {err}"))?;
        let expected_size = wheel
            .get("sizeBytes")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| format!("wheelhouse wheel is missing sizeBytes: {name}"))?;
        if bytes.len() as u64 != expected_size {
            return Err(format!(
                "wheelhouse wheel size mismatch: {name}: expected {expected_size}, got {}",
                bytes.len()
            ));
        }
        let expected_sha256 = wheel
            .get("sha256")
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("wheelhouse wheel is missing sha256: {name}"))?;
        if expected_sha256.len() != 64
            || !expected_sha256.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(format!("wheelhouse wheel has invalid sha256: {name}"));
        }
        let actual_sha256 = crate::artifact::sha256_hex(&bytes);
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(format!(
                concat!(
                    "wheelhouse wheel checksum mismatch: {name}: ",
                    "expected {expected_sha256}, got {actual_sha256}"
                ),
                name = name,
                expected_sha256 = expected_sha256,
                actual_sha256 = actual_sha256
            ));
        }
    }

    for entry in std::fs::read_dir(dir)
        .map_err(|err| format!("reading wheelhouse directory failed: {err}"))?
    {
        let entry = entry.map_err(|err| format!("reading wheelhouse entry failed: {err}"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !expected.contains(&name) {
            return Err(format!("unmanifested wheelhouse payload: {name}"));
        }
        if !entry.path().is_file() {
            return Err(format!("wheelhouse payload is not a file: {name}"));
        }
    }
    Ok(wheels.len())
}

fn bootstrap_tool_archive_target(name: &str) -> Option<(&'static str, &'static str)> {
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
            return Some((platform, arch));
        }
    }
    match name {
        "uv-x86_64-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-x86_64-pc-windows-msvc.zip"
        | "PortableGit-2.54.0-64-bit.7z.exe" => Some(("windows", "x64")),
        "uv-aarch64-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-aarch64-pc-windows-msvc.zip"
        | "PortableGit-2.54.0-arm64.7z.exe" => Some(("windows", "arm64")),
        "uv-i686-pc-windows-msvc.zip"
        | "ripgrep-15.1.0-i686-pc-windows-msvc.zip"
        | "MinGit-2.54.0-32-bit.zip" => Some(("windows", "x86")),
        "uv-x86_64-unknown-linux-gnu.tar.gz"
        | "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz" => Some(("linux", "x64")),
        "uv-aarch64-unknown-linux-gnu.tar.gz"
        | "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz" => Some(("linux", "arm64")),
        "uv-x86_64-apple-darwin.tar.gz"
        | "ripgrep-15.1.0-x86_64-apple-darwin.tar.gz" => Some(("macos", "x64")),
        "uv-aarch64-apple-darwin.tar.gz"
        | "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz" => Some(("macos", "arm64")),
        _ => None,
    }
}

fn bootstrap_tool_name_is_plain_file(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn wheel_name_is_plain_file(name: &str) -> bool {
    bootstrap_tool_name_is_plain_file(name) && name.ends_with(".whl")
}

fn write_self_check_and_exit(args: &[String]) {
    let mut report = bootstrap_self_check_report(
        option_env!("BUILD_PIN_COMMIT"),
        option_env!("BUILD_PIN_BRANCH"),
        self_check_bootstrap_tools_dir(args).as_deref(),
        self_check_wheelhouse_dir(args).as_deref(),
        self_check_wheelhouse_platform(args).as_deref(),
    );
    if let Some(expected) = expected_self_check_commit(args) {
        if report.commit.as_deref() != Some(expected.as_str()) {
            report.ok = false;
            report.errors.push(format!(
                "commit pin mismatch: expected {}, got {:?}",
                expected, report.commit
            ));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("self-check report serializes")
    );
    std::process::exit(if report.ok { 0 } else { 1 });
}

/// Process-wide install state, shared across Tauri commands.
///
/// The bootstrap is a one-shot, single-tenant process — we only need one
/// of these per window. `Arc<Mutex<...>>` lets command handlers grab it
/// without lifetime gymnastics.
pub struct AppState {
    pub bootstrap: Mutex<Option<bootstrap::BootstrapHandle>>,
    /// How this process was launched (install vs update). Immutable for the
    /// lifetime of the process; read by the `get_mode` command.
    pub mode: AppMode,
}

impl AppState {
    fn new(mode: AppMode) -> Self {
        Self {
            bootstrap: Mutex::new(None),
            mode,
        }
    }
}

/// Frontend → Rust: which flow should the UI render?
#[tauri::command]
fn get_mode(state: tauri::State<'_, Arc<AppState>>) -> AppMode {
    state.mode
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--self-check") {
        write_self_check_and_exit(&args);
    }

    // Tracing → bootstrap-installer.log under HERMES_HOME/logs/ so install
    // failures leave a trail for support. Console output also goes here in
    // debug builds.
    let _guard = paths::init_logging();

    let mode = AppMode::from_args(args.iter());
    // Escape hatch: `--reinstall`/`--repair` forces the installer UI even when
    // Hermes is already installed, so users can re-run setup to repair a broken
    // install instead of the launcher fast path silently relaunching the app.
    let force_setup = force_setup_from_args(args.iter());
    tracing::info!(?mode, force_setup, "Hermes installer starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::new(AppState::new(mode)))
        .setup(move |app| {
            use tauri::Manager;
            // Launcher fast path (macOS only): a bare ("Install") launch when
            // Hermes is already installed should NOT show the installer or
            // rebuild — it should just open the app, so the /Applications
            // "Hermes" doubles as a normal launcher (first run installs, every
            // later run launches instantly). The window is kept hidden until
            // here via `"visible": false` so this path never flashes a window.
            //
            // Gated to macOS deliberately: on Windows/Linux the installer keeps
            // its existing behavior (Windows users relaunch via the Start
            // Menu/Desktop "Hermes" shortcuts that install.ps1 creates, and a
            // reliable detached relaunch there needs the DETACHED_PROCESS +
            // startup-grace handling used by launch_hermes_desktop — out of
            // scope here). So this is a pure no-op on non-macOS.
            //
            // `--reinstall`/`--repair` opts out so a broken install can be
            // repaired by re-running setup instead of launching the bad app.
            if cfg!(target_os = "macos") && mode == AppMode::Install && !force_setup {
                let install_root = paths::hermes_home().join("hermes-agent");
                if bootstrap::hermes_is_installed(&install_root) {
                    match bootstrap::spawn_installed_desktop(&install_root) {
                        Ok(()) => {
                            // Brief grace so the spawned app is registered
                            // before we exit (mirrors launch_hermes_desktop).
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            tracing::info!(
                                "hermes already installed — relaunched desktop; exiting installer"
                            );
                            app.handle().exit(0);
                            return Ok(());
                        }
                        Err(err) => {
                            tracing::warn!(
                                ?err,
                                "relaunch of installed desktop failed; showing installer UI"
                            );
                        }
                    }
                }
            }
            // First run / repair install, or Update mode: reveal the UI.
            match app.get_webview_window("main") {
                Some(win) => {
                    if let Err(err) = win.show() {
                        tracing::error!(?err, "failed to show main installer window");
                    }
                }
                None => {
                    tracing::error!(
                        "main installer window not found; installer UI will not appear"
                    );
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Mode (install vs update)
            get_mode,
            // Bootstrap lifecycle
            bootstrap::start_bootstrap,
            bootstrap::cancel_bootstrap,
            bootstrap::get_bootstrap_status,
            // Update lifecycle
            update::start_update,
            // Hand-off
            bootstrap::launch_hermes_desktop,
            // Diagnostics
            paths::get_log_path,
            paths::get_hermes_home,
            paths::open_log_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hermes Setup");
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_self_check_report, expected_self_check_commit, force_setup_from_args,
        self_check_wheelhouse_dir, self_check_wheelhouse_platform, AppMode,
    };
    use std::path::PathBuf;

    fn unique_tmp_dir(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hermes-bootstrap-self-check-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn bare_args_are_install() {
        assert_eq!(AppMode::from_args(Vec::<String>::new()), AppMode::Install);
        assert_eq!(AppMode::from_args(["--foo", "bar"]), AppMode::Install);
    }

    #[test]
    fn update_flag_selects_update() {
        assert_eq!(AppMode::from_args(["--update"]), AppMode::Update);
        assert_eq!(
            AppMode::from_args(["--something", "--update", "--else"]),
            AppMode::Update
        );
    }

    #[test]
    fn reinstall_and_repair_flags_force_setup() {
        assert!(force_setup_from_args(["--reinstall"]));
        assert!(force_setup_from_args(["--repair"]));
        assert!(force_setup_from_args(["--foo", "--repair", "--bar"]));
    }

    #[test]
    fn bare_or_unrelated_args_do_not_force_setup() {
        assert!(!force_setup_from_args(Vec::<String>::new()));
        assert!(!force_setup_from_args(["--foo", "bar"]));
        // --update must not be mistaken for a force-setup flag.
        assert!(!force_setup_from_args(["--update"]));
    }

    #[test]
    fn force_setup_flags_do_not_affect_mode_selection() {
        // The repair flags must never flip Install<->Update.
        assert_eq!(AppMode::from_args(["--reinstall"]), AppMode::Install);
        assert_eq!(AppMode::from_args(["--repair"]), AppMode::Install);
        assert_eq!(
            AppMode::from_args(["--update", "--reinstall"]),
            AppMode::Update
        );
    }

    #[test]
    fn self_check_requires_commit_pin_and_embedded_scripts() {
        let missing_commit = bootstrap_self_check_report(None, Some("main"), None, None, None);
        assert!(!missing_commit.ok);
        assert!(missing_commit
            .errors
            .iter()
            .any(|err| err.contains("commit pin")));

        let report =
            bootstrap_self_check_report(Some("abcdef1234567890"), Some("main"), None, None, None);
        assert!(report.ok, "{:?}", report.errors);
        assert_eq!(report.commit.as_deref(), Some("abcdef1234567890"));
        assert!(report.embedded_scripts.len() >= 2);
        assert!(report
            .embedded_scripts
            .iter()
            .all(|script| script.size_bytes > 0));
        assert!(report
            .embedded_scripts
            .iter()
            .all(|script| script.sha256.len() == 64));
    }

    #[test]
    fn self_check_validates_bootstrap_tools_manifest_dir() {
        let root = unique_tmp_dir("tools");
        let tools = root.join("bootstrap-tools");
        std::fs::create_dir_all(&tools).unwrap();
        let archive = tools.join("uv-x86_64-pc-windows-msvc.zip");
        std::fs::write(&archive, b"uv archive").unwrap();
        let sha256 = crate::artifact::sha256_hex(b"uv archive");
        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "arch": "x64",
      "platform": "windows",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "https://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();

        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(report.ok, "{:?}", report.errors);
        assert_eq!(report.bootstrap_tools_archives, Some(1));

        std::fs::write(tools.join("rogue.zip"), b"rogue").unwrap();
        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("unmanifested bootstrap tool payload")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn self_check_validates_python_wheelhouse_manifest_dir() {
        let root = unique_tmp_dir("wheelhouse");
        let wheelhouse = root.join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        let wheel = wheelhouse.join("demo-0.1-py3-none-any.whl");
        std::fs::write(&wheel, b"wheel bytes").unwrap();
        let sha256 = crate::artifact::sha256_hex(b"wheel bytes");
        std::fs::write(
            wheelhouse.join("wheelhouse-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "wheels": [
    {{
      "arch": "x64",
      "platform": "windows",
      "python": "cp311",
      "name": "demo-0.1-py3-none-any.whl",
      "sizeBytes": 11,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();

        let report = bootstrap_self_check_report(
            Some("abcdef1234567890"),
            Some("main"),
            None,
            Some(&wheelhouse),
            None,
        );
        assert!(report.ok, "{:?}", report.errors);
        assert_eq!(report.python_wheelhouse_wheels, Some(1));

        std::fs::write(wheelhouse.join("rogue-0.1-py3-none-any.whl"), b"rogue").unwrap();
        let report = bootstrap_self_check_report(
            Some("abcdef1234567890"),
            Some("main"),
            None,
            Some(&wheelhouse),
            None,
        );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("unmanifested wheelhouse payload")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn self_check_rejects_bootstrap_tools_manifest_audit_gaps() {
        let root = unique_tmp_dir("tools-audit");
        let tools = root.join("bootstrap-tools");
        std::fs::create_dir_all(&tools).unwrap();
        let archive = tools.join("uv-x86_64-pc-windows-msvc.zip");
        std::fs::write(&archive, b"uv archive").unwrap();
        let sha256 = crate::artifact::sha256_hex(b"uv archive");
        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();

        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("missing arch")));

        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "arch": "x64",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "https://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();
        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("missing platform")));

        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "arch": "x64",
      "platform": "linux",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "https://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();
        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("target mismatch")));

        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "arch": "x64",
      "platform": "windows",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "http://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();
        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("invalid url")));

        std::fs::write(
            tools.join("bootstrap-tools-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "archives": [
    {{
      "arch": "x64",
      "platform": "windows",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "https://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }},
    {{
      "arch": "x64",
      "platform": "windows",
      "name": "uv-x86_64-pc-windows-msvc.zip",
      "url": "https://example.invalid/uv.zip",
      "sizeBytes": 10,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();
        let report =
            bootstrap_self_check_report(
                Some("abcdef1234567890"),
                Some("main"),
                Some(&tools),
                None,
                None,
            );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("duplicate archive")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn self_check_expect_commit_parses_space_or_equals_forms() {
        assert_eq!(
            expected_self_check_commit(["--self-check", "--self-check-expect-commit", "abc"]),
            Some("abc".to_string())
        );
        assert_eq!(
            expected_self_check_commit(["--self-check-expect-commit=def"]),
            Some("def".to_string())
        );
        assert_eq!(expected_self_check_commit(["--self-check"]), None);
    }

    #[test]
    fn self_check_wheelhouse_dir_parses_space_or_equals_forms() {
        assert_eq!(
            self_check_wheelhouse_dir(["--self-check", "--self-check-wheelhouse", "wheels"]),
            Some(PathBuf::from("wheels"))
        );
        assert_eq!(
            self_check_wheelhouse_dir(["--self-check-wheelhouse=wheels2"]),
            Some(PathBuf::from("wheels2"))
        );
        assert_eq!(self_check_wheelhouse_dir(["--self-check"]), None);
    }

    #[test]
    fn self_check_wheelhouse_platform_rejects_wrong_release_payload() {
        let root = unique_tmp_dir("wheelhouse-platform");
        let wheelhouse = root.join("wheelhouse");
        std::fs::create_dir_all(&wheelhouse).unwrap();
        let wheel = wheelhouse.join("demo-0.1-py3-none-any.whl");
        std::fs::write(&wheel, b"wheel bytes").unwrap();
        let sha256 = crate::artifact::sha256_hex(b"wheel bytes");
        std::fs::write(
            wheelhouse.join("wheelhouse-manifest.json"),
            format!(
                r#"{{
  "schemaVersion": 1,
  "wheels": [
    {{
      "arch": "x64",
      "platform": "windows",
      "python": "cp311",
      "name": "demo-0.1-py3-none-any.whl",
      "sizeBytes": 11,
      "sha256": "{sha256}"
    }}
  ]
}}
"#
            ),
        )
        .unwrap();

        let report = bootstrap_self_check_report(
            Some("abcdef1234567890"),
            Some("main"),
            None,
            Some(&wheelhouse),
            Some("linux"),
        );
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|err| err.contains("unexpected wheelhouse platform")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn self_check_wheelhouse_platform_parses_space_or_equals_forms() {
        assert_eq!(
            self_check_wheelhouse_platform([
                "--self-check",
                "--self-check-wheelhouse-platform",
                "windows"
            ]),
            Some("windows".to_string())
        );
        assert_eq!(
            self_check_wheelhouse_platform(["--self-check-wheelhouse-platform=linux"]),
            Some("linux".to_string())
        );
        assert_eq!(self_check_wheelhouse_platform(["--self-check"]), None);
    }
}
