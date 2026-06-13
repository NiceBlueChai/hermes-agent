//! Command-line entrypoint for the Hermes install manager.

use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;
use std::{env, fs};

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

/// Manage Hermes runtime installation resources.
#[derive(Debug, Parser)]
#[command(name = "hermes-manager")]
#[command(about = "Hermes install, repair, and uninstall manager")]
struct Cli {
    /// Override Hermes home for tests or isolated installs.
    #[arg(long)]
    hermes_home: Option<PathBuf>,

    /// Optional bundled manifest path to validate.
    #[arg(long)]
    manifest: Option<PathBuf>,

    /// Emit machine-readable JSON for cleanup commands.
    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the manager version.
    Version,
    /// Print resolved manager paths.
    Doctor,
    /// Create manager state and initial install metadata.
    InstallMetadata,
    /// Remove paths recorded in the installed-files manifest.
    UninstallLite {
        /// Report paths that would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Also remove Hermes Start Menu/Desktop shortcuts when supported.
        #[arg(long)]
        shortcuts: bool,
    },
    /// Remove runtime checkout state so launch can repair it.
    RepairClean {
        /// Report paths that would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove source-built desktop GUI artifacts while preserving the agent.
    UninstallGuiBuild {
        /// Report paths that would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Also remove the Electron desktop userData directory.
        #[arg(long)]
        user_data: bool,
        /// Also remove Linux desktop launcher entries.
        #[arg(long)]
        desktop_entries: bool,
    },
    /// Report native bootstrap bridge capabilities.
    BootstrapCapabilities,
    /// Report the native bootstrap bridge manifest.
    BootstrapManifest,
    /// Run one native bootstrap bridge stage.
    BootstrapStage {
        /// Stage name from `bootstrap-manifest`.
        stage: String,
        /// Override install root for path-related stages.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Current PATH value for planning or tests.
        #[arg(long)]
        current_path: Option<String>,
        /// Optional bundled Python wheelhouse directory.
        #[arg(long)]
        wheelhouse_dir: Option<PathBuf>,
        /// Optional bundled bootstrap-tools directory.
        #[arg(long)]
        bootstrap_tools_dir: Option<PathBuf>,
        /// Plan the stage without writing OS/user state.
        #[arg(long)]
        dry_run: bool,
        /// Pinned source commit for marker-producing stages.
        #[arg(long)]
        commit: Option<String>,
        /// Pinned source branch for marker-producing stages.
        #[arg(long)]
        branch: Option<String>,
    },
    /// Plan PATH changes needed to expose the Hermes command.
    PlanPath {
        /// Override install root; defaults to HERMES_HOME/hermes-agent.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Current PATH value to plan from; defaults to the process PATH.
        #[arg(long)]
        current_path: Option<String>,
        /// Plan using Windows PATH conventions.
        #[arg(long, conflicts_with = "unix")]
        windows: bool,
        /// Plan using Unix PATH conventions.
        #[arg(long, conflicts_with = "windows")]
        unix: bool,
    },
    /// Write an idempotent Hermes PATH block to a shell profile file.
    WriteProfileHint {
        /// Shell profile path to update.
        #[arg(long)]
        profile: PathBuf,
        /// Override install root; defaults to HERMES_HOME/hermes-agent.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Do not write the profile; only report what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// Write Hermes to the current user's Windows PATH.
    WriteUserPath {
        /// Override install root; defaults to HERMES_HOME/hermes-agent.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Current user PATH value to plan from; defaults to HKCU Environment Path.
        #[arg(long)]
        current_path: Option<String>,
        /// Do not write the registry; only report what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// Plan Start Menu and Desktop shortcuts for the packaged desktop app.
    PlanShortcuts {
        /// Packaged Hermes desktop executable.
        #[arg(long)]
        target_exe: Option<PathBuf>,
        /// Override install root; defaults to HERMES_HOME/hermes-agent.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Override Start Menu Programs directory.
        #[arg(long)]
        programs_dir: Option<PathBuf>,
        /// Override Desktop directory.
        #[arg(long)]
        desktop_dir: Option<PathBuf>,
    },
    /// Write Start Menu and Desktop shortcuts for the packaged desktop app.
    WriteShortcuts {
        /// Packaged Hermes desktop executable.
        #[arg(long)]
        target_exe: Option<PathBuf>,
        /// Override install root; defaults to HERMES_HOME/hermes-agent.
        #[arg(long)]
        install_root: Option<PathBuf>,
        /// Override Start Menu Programs directory.
        #[arg(long)]
        programs_dir: Option<PathBuf>,
        /// Override Desktop directory.
        #[arg(long)]
        desktop_dir: Option<PathBuf>,
        /// Do not write shortcuts; only report what would happen.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Serialize)]
struct CommandReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "dryRun")]
    dry_run: bool,
    paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProfileReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "dryRun")]
    dry_run: bool,
    profile: String,
    #[serde(rename = "hermesBin")]
    hermes_bin: String,
    changed: bool,
}

#[derive(Debug, Serialize)]
struct PathApplyReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "dryRun")]
    dry_run: bool,
    target: String,
    #[serde(rename = "hermesBin")]
    hermes_bin: String,
    changed: bool,
    applied: bool,
}

#[derive(Debug, Serialize)]
struct ShortcutApplyReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "dryRun")]
    dry_run: bool,
    applied: bool,
    shortcuts: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BootstrapCapabilitiesReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(rename = "canRunFullBootstrap")]
    can_run_full_bootstrap: bool,
    #[serde(rename = "supportedStages")]
    supported_stages: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct BootstrapStageDescriptor {
    name: &'static str,
    title: &'static str,
    category: &'static str,
    #[serde(rename = "needs_user_input")]
    needs_user_input: bool,
}

#[derive(Debug, Serialize)]
struct BootstrapManifestReport {
    ok: bool,
    command: &'static str,
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    protocol_version: u32,
    stages: Vec<BootstrapStageDescriptor>,
}

#[derive(Debug, Serialize)]
struct BootstrapStageReport {
    ok: bool,
    command: &'static str,
    stage: String,
    skipped: bool,
    reason: Option<String>,
    #[serde(rename = "duration_ms")]
    duration_ms: u128,
    #[serde(rename = "failureCategory", skip_serializing_if = "Option::is_none")]
    failure_category: Option<&'static str>,
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
    sha256: String,
}

const BASE_NATIVE_BOOTSTRAP_STAGES: [BootstrapStageDescriptor; 2] = [
    BootstrapStageDescriptor {
        name: "install-metadata",
        title: "Record install metadata",
        category: "finalize",
        needs_user_input: false,
    },
    BootstrapStageDescriptor {
        name: "bootstrap-marker",
        title: "Mark install complete",
        category: "finalize",
        needs_user_input: false,
    },
];

const WINDOWS_PATH_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "path",
    title: "Add Hermes to PATH",
    category: "finalize",
    needs_user_input: false,
};

const WINDOWS_CONFIG_TEMPLATES_BOOTSTRAP_STAGE: BootstrapStageDescriptor =
    BootstrapStageDescriptor {
        name: "config-templates",
        title: "Write configuration templates",
        category: "finalize",
        needs_user_input: false,
    };

const WINDOWS_PLATFORM_SDKS_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "platform-sdks",
    title: "Install messaging platform SDKs",
    category: "finalize",
    needs_user_input: false,
};

const WINDOWS_NODE_DEPS_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "node-deps",
    title: "Install Node.js dependencies",
    category: "install",
    needs_user_input: false,
};

const WINDOWS_SYSTEM_PACKAGES_BOOTSTRAP_STAGE: BootstrapStageDescriptor =
    BootstrapStageDescriptor {
        name: "system-packages",
        title: "Install ripgrep and ffmpeg",
        category: "prereqs",
        needs_user_input: false,
    };

const WINDOWS_NODE_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "node",
    title: "Detect Node.js",
    category: "prereqs",
    needs_user_input: false,
};

const WINDOWS_UV_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "uv",
    title: "Install uv package manager",
    category: "prereqs",
    needs_user_input: false,
};

const WINDOWS_GIT_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "git",
    title: "Install Git",
    category: "prereqs",
    needs_user_input: false,
};

const WINDOWS_PYTHON_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "python",
    title: "Verify Python 3.11",
    category: "prereqs",
    needs_user_input: false,
};

const WINDOWS_REPOSITORY_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "repository",
    title: "Clone Hermes repository",
    category: "install",
    needs_user_input: false,
};

const WINDOWS_VENV_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "venv",
    title: "Create Python virtual environment",
    category: "install",
    needs_user_input: false,
};

const WINDOWS_DEPENDENCIES_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "dependencies",
    title: "Install Python dependencies",
    category: "install",
    needs_user_input: false,
};

const WINDOWS_DESKTOP_BOOTSTRAP_STAGE: BootstrapStageDescriptor = BootstrapStageDescriptor {
    name: "desktop",
    title: "Build desktop app",
    category: "install",
    needs_user_input: false,
};

const WINDOWS_INTERACTIVE_BOOTSTRAP_STAGES: [BootstrapStageDescriptor; 2] = [
    BootstrapStageDescriptor {
        name: "configure",
        title: "Configure API keys and models",
        category: "post-install",
        needs_user_input: true,
    },
    BootstrapStageDescriptor {
        name: "gateway",
        title: "Start messaging gateway",
        category: "post-install",
        needs_user_input: true,
    },
];

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> hermes_manager::Result<()> {
    let cli = Cli::parse();
    let home = hermes_manager::paths::hermes_home(cli.hermes_home);

    match cli.command {
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Command::Doctor => {
            for line in hermes_manager::commands::doctor(&home) {
                println!("{line}");
            }
            if let Some(manifest_path) = cli.manifest.as_deref() {
                match hermes_manager::bundled_manifest::BundledManifest::read(manifest_path) {
                    Ok(manifest) => {
                        let manifest_root = manifest_path.parent().unwrap_or_else(|| ".".as_ref());
                        manifest.verify_resources(manifest_root)?;
                        println!("bundled_manifest=ok");
                        println!(
                            "bundled_manifest_hermes_version={}",
                            manifest.hermes_version
                        );
                        println!("bundled_manifest_resources=ok");
                    }
                    Err(err) => {
                        eprintln!("bundled_manifest=error: {err}");
                        std::process::exit(2);
                    }
                }
            }
        }
        Command::InstallMetadata => {
            hermes_manager::commands::install_metadata(&home)?;
            println!("install_metadata=ok");
        }
        Command::UninstallLite { dry_run, shortcuts } => {
            let mut paths = if dry_run {
                hermes_manager::commands::uninstall_lite_plan(&home)?
            } else {
                hermes_manager::commands::uninstall_lite(&home)?
            };
            if shortcuts {
                let plans = resolve_shortcut_plans(&home, None, None, None, None);
                let shortcut_paths = if dry_run {
                    hermes_manager::platform::existing_shortcut_paths(&plans)
                } else {
                    hermes_manager::platform::remove_windows_shortcuts(&plans)?
                };
                paths.extend(shortcut_paths.iter().map(|path| path.display().to_string()));
            }
            if cli.json {
                print_json_report(CommandReport {
                    ok: true,
                    command: "uninstall-lite",
                    dry_run,
                    paths,
                })?;
            } else {
                let prefix = if dry_run { "would_remove" } else { "removed" };
                for path in paths {
                    println!("{prefix}={path}");
                }
                println!("uninstall_lite=ok");
            }
        }
        Command::RepairClean { dry_run } => {
            let paths = if dry_run {
                hermes_manager::commands::repair_clean_plan(&home)?
            } else {
                hermes_manager::commands::repair_clean(&home)?
            };
            if cli.json {
                print_json_report(CommandReport {
                    ok: true,
                    command: "repair-clean",
                    dry_run,
                    paths,
                })?;
            } else {
                let prefix = if dry_run { "would_remove" } else { "removed" };
                for path in paths {
                    println!("{prefix}={path}");
                }
                println!("repair_clean=ok");
            }
        }
        Command::UninstallGuiBuild {
            dry_run,
            user_data,
            desktop_entries,
        } => {
            let paths = if dry_run {
                if user_data && desktop_entries {
                    hermes_manager::commands::uninstall_gui_build_plan_with_gui_state(&home)?
                } else if user_data {
                    hermes_manager::commands::uninstall_gui_build_plan_with_user_data(&home)?
                } else if desktop_entries {
                    hermes_manager::commands::uninstall_gui_build_plan_with_desktop_entries(&home)?
                } else {
                    hermes_manager::commands::uninstall_gui_build_plan(&home)?
                }
            } else if user_data && desktop_entries {
                hermes_manager::commands::uninstall_gui_build_with_gui_state(&home)?
            } else if user_data {
                hermes_manager::commands::uninstall_gui_build_with_user_data(&home)?
            } else if desktop_entries {
                hermes_manager::commands::uninstall_gui_build_with_desktop_entries(&home)?
            } else {
                hermes_manager::commands::uninstall_gui_build(&home)?
            };
            if cli.json {
                print_json_report(CommandReport {
                    ok: true,
                    command: "uninstall-gui-build",
                    dry_run,
                    paths,
                })?;
            } else {
                let prefix = if dry_run { "would_remove" } else { "removed" };
                for path in paths {
                    println!("{prefix}={path}");
                }
                println!("uninstall_gui_build=ok");
            }
        }
        Command::BootstrapCapabilities => {
            let report = BootstrapCapabilitiesReport {
                ok: true,
                command: "bootstrap-capabilities",
                schema_version: 1,
                can_run_full_bootstrap: false,
                supported_stages: native_bootstrap_stage_names(),
            };
            if cli.json {
                print_json(&report)?;
            } else {
                println!("bootstrap_capabilities=ok");
                println!("schema_version={}", report.schema_version);
                println!("can_run_full_bootstrap={}", report.can_run_full_bootstrap);
                println!("supported_stages={}", report.supported_stages.join(","));
            }
        }
        Command::BootstrapManifest => {
            let report = BootstrapManifestReport {
                ok: true,
                command: "bootstrap-manifest",
                schema_version: 1,
                protocol_version: 1,
                stages: native_bootstrap_stages(),
            };
            if cli.json {
                print_json(&report)?;
            } else {
                println!("bootstrap_manifest=ok");
                println!("protocol_version={}", report.protocol_version);
                for stage in report.stages {
                    println!("stage={}", stage.name);
                }
            }
        }
        Command::BootstrapStage {
            stage,
            install_root,
            current_path,
            wheelhouse_dir,
            bootstrap_tools_dir,
            dry_run,
            commit,
            branch,
        } => {
            let report = run_native_bootstrap_stage(
                &home,
                &stage,
                NativeBootstrapStageOptions {
                    install_root,
                    current_path,
                    wheelhouse_dir,
                    bootstrap_tools_dir,
                    dry_run,
                    commit: commit.as_deref(),
                    branch: branch.as_deref(),
                },
            );
            let ok = report.ok;
            let failure_category = report.failure_category;
            if cli.json {
                print_json(&report)?;
            } else if ok {
                println!("bootstrap_stage=ok");
                println!("stage={stage}");
            } else {
                println!("bootstrap_stage=error");
                println!("stage={stage}");
                if let Some(reason) = &report.reason {
                    println!("reason={reason}");
                }
            }
            if !ok {
                std::process::exit(if failure_category == Some("unknown-stage") {
                    2
                } else {
                    1
                });
            }
        }
        Command::PlanPath {
            install_root,
            current_path,
            windows,
            unix,
        } => {
            let install_root =
                install_root.unwrap_or_else(|| hermes_manager::paths::agent_root(&home));
            let current_path = current_path.or_else(|| std::env::var("PATH").ok());
            let use_windows = if windows {
                true
            } else if unix {
                false
            } else {
                cfg!(target_os = "windows")
            };
            let plan = if use_windows {
                hermes_manager::platform::plan_path_update(&install_root, current_path, true)
            } else {
                hermes_manager::platform::plan_path_update_with_extra_entries(
                    &install_root,
                    &[home.join("bin")],
                    current_path,
                    false,
                )
            };
            if cli.json {
                let text = serde_json::to_string_pretty(&plan).map_err(|err| {
                    hermes_manager::ManagerError::InvalidManifest(err.to_string())
                })?;
                println!("{text}");
            } else {
                println!("hermes_bin={}", plan.hermes_bin.display());
                println!("path_changed={}", plan.changed);
                println!("next_path={}", plan.next_path);
                if !use_windows {
                    println!(
                        "profile_hint={}",
                        hermes_manager::platform::shell_profile_hint(&plan)
                    );
                }
            }
        }
        Command::WriteProfileHint {
            profile,
            install_root,
            dry_run,
        } => {
            let install_root =
                install_root.unwrap_or_else(|| hermes_manager::paths::agent_root(&home));
            let current_path = std::env::var("PATH").ok();
            let plan = hermes_manager::platform::plan_path_update_with_extra_entries(
                &install_root,
                &[home.join("bin")],
                current_path,
                false,
            );
            if !dry_run {
                hermes_manager::platform::write_shell_profile_update(&profile, &plan)?;
            }
            if cli.json {
                let text = serde_json::to_string_pretty(&ProfileReport {
                    ok: true,
                    command: "write-profile-hint",
                    dry_run,
                    profile: profile.display().to_string(),
                    hermes_bin: plan.hermes_bin.display().to_string(),
                    changed: true,
                })
                .map_err(|err| hermes_manager::ManagerError::InvalidManifest(err.to_string()))?;
                println!("{text}");
            } else {
                let action = if dry_run { "would_update" } else { "updated" };
                println!("{action}={}", profile.display());
                println!(
                    "profile_hint={}",
                    hermes_manager::platform::shell_profile_hint(&plan)
                );
            }
        }
        Command::WriteUserPath {
            install_root,
            current_path,
            dry_run,
        } => {
            let install_root =
                install_root.unwrap_or_else(|| hermes_manager::paths::agent_root(&home));
            let current_path = match current_path {
                Some(value) => Some(value),
                None => hermes_manager::platform::read_windows_user_path()?,
            };
            let plan =
                hermes_manager::platform::plan_path_update(&install_root, current_path, true);
            let applied = if dry_run {
                false
            } else {
                hermes_manager::platform::write_windows_user_path_update(&plan)?
            };
            if cli.json {
                let text = serde_json::to_string_pretty(&PathApplyReport {
                    ok: true,
                    command: "write-user-path",
                    dry_run,
                    target: "user".to_string(),
                    hermes_bin: plan.hermes_bin.display().to_string(),
                    changed: plan.changed,
                    applied,
                })
                .map_err(|err| hermes_manager::ManagerError::InvalidManifest(err.to_string()))?;
                println!("{text}");
            } else {
                let action = if dry_run {
                    "would_update_user_path"
                } else if applied {
                    "updated_user_path"
                } else {
                    "user_path_unchanged"
                };
                println!("{action}=Path");
                println!("hermes_bin={}", plan.hermes_bin.display());
            }
        }
        Command::PlanShortcuts {
            target_exe,
            install_root,
            programs_dir,
            desktop_dir,
        } => {
            let plans =
                resolve_shortcut_plans(&home, target_exe, install_root, programs_dir, desktop_dir);
            if cli.json {
                let text = serde_json::to_string_pretty(&plans).map_err(|err| {
                    hermes_manager::ManagerError::InvalidManifest(err.to_string())
                })?;
                println!("{text}");
            } else {
                for plan in plans {
                    println!("shortcut={}", plan.path.display());
                    println!("target={}", plan.target.display());
                }
            }
        }
        Command::WriteShortcuts {
            target_exe,
            install_root,
            programs_dir,
            desktop_dir,
            dry_run,
        } => {
            let plans =
                resolve_shortcut_plans(&home, target_exe, install_root, programs_dir, desktop_dir);
            if !dry_run {
                hermes_manager::platform::write_windows_shortcuts(&plans)?;
            }
            if cli.json {
                let text = serde_json::to_string_pretty(&ShortcutApplyReport {
                    ok: true,
                    command: "write-shortcuts",
                    dry_run,
                    applied: !dry_run,
                    shortcuts: plans
                        .iter()
                        .map(|plan| plan.path.display().to_string())
                        .collect(),
                })
                .map_err(|err| hermes_manager::ManagerError::InvalidManifest(err.to_string()))?;
                println!("{text}");
            } else {
                let prefix = if dry_run { "would_create" } else { "created" };
                for plan in plans {
                    println!("{prefix}={}", plan.path.display());
                }
            }
        }
    }

    Ok(())
}

fn resolve_shortcut_plans(
    home: &std::path::Path,
    target_exe: Option<PathBuf>,
    install_root: Option<PathBuf>,
    programs_dir: Option<PathBuf>,
    desktop_dir: Option<PathBuf>,
) -> Vec<hermes_manager::platform::ShortcutPlan> {
    let install_root = install_root.unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    let target_exe = target_exe.unwrap_or_else(|| {
        install_root
            .join("apps")
            .join("desktop")
            .join("release")
            .join("win-unpacked")
            .join("Hermes.exe")
    });
    let programs_dir = programs_dir.unwrap_or_else(default_windows_programs_dir);
    let desktop_dir = desktop_dir.unwrap_or_else(default_windows_desktop_dir);
    let icon_exists = target_exe
        .parent()
        .map(|parent| parent.join("resources").join("icon.ico").is_file())
        .unwrap_or(false);
    hermes_manager::platform::plan_windows_shortcuts(
        &target_exe,
        &programs_dir,
        &desktop_dir,
        icon_exists,
    )
}

fn default_windows_programs_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
}

fn default_windows_desktop_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Desktop")
}

fn print_json_report(report: CommandReport) -> hermes_manager::Result<()> {
    print_json(&report)
}

fn print_json<T: Serialize>(report: &T) -> hermes_manager::Result<()> {
    let text = serde_json::to_string_pretty(report)
        .map_err(|err| hermes_manager::ManagerError::InvalidManifest(err.to_string()))?;
    println!("{text}");
    Ok(())
}

fn native_bootstrap_stage_names() -> Vec<&'static str> {
    native_bootstrap_stages()
        .into_iter()
        .map(|stage| stage.name)
        .collect()
}

struct NativeBootstrapStageOptions<'a> {
    install_root: Option<PathBuf>,
    current_path: Option<String>,
    wheelhouse_dir: Option<PathBuf>,
    bootstrap_tools_dir: Option<PathBuf>,
    dry_run: bool,
    commit: Option<&'a str>,
    branch: Option<&'a str>,
}

fn run_native_bootstrap_stage(
    home: &std::path::Path,
    stage: &str,
    options: NativeBootstrapStageOptions<'_>,
) -> BootstrapStageReport {
    let started_at = Instant::now();
    let result = match stage {
        "install-metadata" => hermes_manager::commands::install_metadata(home)
            .map(|()| (false, None))
            .map_err(|err| ("stage-failed", err.to_string())),
        "bootstrap-marker" => {
            hermes_manager::commands::write_bootstrap_marker(home, options.commit, options.branch)
                .map(|path| (path.is_none(), None))
                .map_err(|err| ("stage-failed", err.to_string()))
        }
        "path" => run_native_path_stage(home, options).map(|skipped| (skipped, None)),
        "uv" => run_native_uv_stage(home, options),
        "git" => run_native_git_stage(home, options),
        "python" => run_native_python_stage(options),
        "repository" => run_native_repository_stage(home, options),
        "venv" => run_native_venv_stage(home, options),
        "dependencies" | "python-deps" => run_native_dependencies_stage(home, options),
        "node" => run_native_node_stage(home, options),
        "system-packages" => run_native_system_packages_stage(options),
        "config-templates" => {
            run_native_config_templates_stage(home, options).map(|skipped| (skipped, None))
        }
        "node-deps" => run_native_node_deps_stage(options),
        "desktop" => run_native_desktop_stage(home, options),
        "platform-sdks" => run_native_platform_sdks_stage(home, options),
        "configure" | "gateway" => run_native_interactive_skip_stage(stage),
        other => Err((
            "unknown-stage",
            format!("unknown native bootstrap stage: {other}"),
        )),
    };
    match result {
        Ok((skipped, reason)) => BootstrapStageReport {
            ok: true,
            command: "bootstrap-stage",
            stage: stage.to_string(),
            skipped,
            reason,
            duration_ms: started_at.elapsed().as_millis(),
            failure_category: None,
        },
        Err((failure_category, reason)) => BootstrapStageReport {
            ok: false,
            command: "bootstrap-stage",
            stage: stage.to_string(),
            skipped: false,
            reason: Some(reason),
            duration_ms: started_at.elapsed().as_millis(),
            failure_category: Some(failure_category),
        },
    }
}

fn native_bootstrap_stages() -> Vec<BootstrapStageDescriptor> {
    let mut stages = BASE_NATIVE_BOOTSTRAP_STAGES.to_vec();
    if cfg!(target_os = "windows") {
        stages.push(WINDOWS_UV_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_GIT_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_PYTHON_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_REPOSITORY_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_VENV_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_DEPENDENCIES_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_NODE_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_SYSTEM_PACKAGES_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_NODE_DEPS_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_DESKTOP_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_PATH_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_CONFIG_TEMPLATES_BOOTSTRAP_STAGE);
        stages.push(WINDOWS_PLATFORM_SDKS_BOOTSTRAP_STAGE);
        stages.extend_from_slice(&WINDOWS_INTERACTIVE_BOOTSTRAP_STAGES);
    }
    stages
}

fn run_native_path_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<bool, (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native path stage is only complete on Windows".to_string(),
        ));
    }

    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    let current_path = match options.current_path {
        Some(value) => Some(value),
        None => hermes_manager::platform::read_windows_user_path()
            .map_err(|err| ("stage-failed", err.to_string()))?,
    };
    let plan = hermes_manager::platform::plan_path_update(&install_root, current_path, true);
    if !options.dry_run {
        hermes_manager::platform::write_windows_user_path_update(&plan)
            .map_err(|err| ("stage-failed", err.to_string()))?;
        hermes_manager::platform::write_windows_user_env_var(
            "HERMES_HOME",
            &home.display().to_string(),
        )
        .map_err(|err| ("stage-failed", err.to_string()))?;
    }
    Ok(false)
}

fn run_native_uv_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native uv probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let uv = Some(home.join("bin").join("uv.exe"))
        .filter(|path| path.is_file())
        .or_else(|| windows_path_command(&path_text, "uv"));
    let Some(uv) = uv else {
        return Err((
            "fallback-to-script",
            "uv missing; script installs managed uv".to_string(),
        ));
    };
    let output = ProcessCommand::new(&uv)
        .arg("--version")
        .output()
        .map_err(|err| ("stage-failed", err.to_string()))?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        return Ok((
            true,
            Some(format!("{version} already available; uv stage skipped")),
        ));
    }
    Err((
        "fallback-to-script",
        "uv exists but did not run successfully; script reinstalls managed uv".to_string(),
    ))
}

fn run_native_git_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native Git probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let git = windows_path_command(&path_text, "git").or_else(|| {
        [
            home.join("git").join("cmd").join("git.exe"),
            home.join("git").join("bin").join("git.exe"),
            home.join("git").join("mingw64").join("bin").join("git.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file())
    });
    let Some(git) = git else {
        return Err((
            "fallback-to-script",
            "Git missing; script installs managed PortableGit".to_string(),
        ));
    };
    let output = ProcessCommand::new(&git)
        .arg("--version")
        .output()
        .map_err(|err| ("stage-failed", err.to_string()))?;
    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok((
            true,
            Some(format!("{version} already available; git stage skipped")),
        ));
    }
    Err((
        "fallback-to-script",
        "Git exists but did not run successfully; script installs managed PortableGit".to_string(),
    ))
}

fn run_native_python_stage(
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native Python probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let Some(python) = windows_path_command(&path_text, "python") else {
        return Err((
            "fallback-to-script",
            "Python missing; script uses uv to find or install Python 3.11".to_string(),
        ));
    };
    let output = ProcessCommand::new(&python)
        .arg("--version")
        .output()
        .map_err(|err| ("stage-failed", err.to_string()))?;
    let version = command_version_text(&output);
    if output.status.success() && python_version_is_supported(&version) {
        return Ok((
            true,
            Some(format!("{version} already available; python stage skipped")),
        ));
    }
    Err((
        "fallback-to-script",
        format!("{version} missing or unsupported; script uses uv to install Python 3.11"),
    ))
}

fn run_native_repository_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native repository probe is only complete on Windows".to_string(),
        ));
    }
    let Some(expected_commit) = options.commit else {
        return Err((
            "fallback-to-script",
            "repository stage needs a pinned commit; script handles branch and tag installs"
                .to_string(),
        ));
    };
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    if !install_root.join(".git").exists() {
        return Err((
            "fallback-to-script",
            "repository checkout missing; script clones or downloads source".to_string(),
        ));
    }
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&install_root)
        .output()
        .map_err(|err| ("fallback-to-script", err.to_string()))?;
    let current_commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && current_commit.eq_ignore_ascii_case(expected_commit) {
        return Ok((
            true,
            Some(format!(
                "repository already at pinned commit {current_commit}"
            )),
        ));
    }
    Err((
        "fallback-to-script",
        "repository checkout missing or not at pinned commit; script updates source".to_string(),
    ))
}

fn run_native_venv_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native venv stage is only complete on Windows".to_string(),
        ));
    }
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    if !install_root.is_dir() {
        return Err((
            "fallback-to-script",
            "install root missing; script creates the repository before venv".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let Some(uv) = windows_uv_command(home, &path_text) else {
        return Err((
            "fallback-to-script",
            "uv missing; script creates the virtual environment after installing uv".to_string(),
        ));
    };
    let venv = install_root.join("venv");
    if options.dry_run {
        return Ok((
            false,
            Some(format!(
                "venv stage would run {} in {}",
                uv.display(),
                install_root.display()
            )),
        ));
    }
    if venv.exists() {
        fs::remove_dir_all(&venv).map_err(|err| ("stage-failed", err.to_string()))?;
    }
    let status = ProcessCommand::new(&uv)
        .args(["venv", "venv", "--python", "3.11"])
        .current_dir(&install_root)
        .env("UV_CACHE_DIR", home.join("uv-cache"))
        .env("UV_PYTHON_INSTALL_DIR", home.join("python"))
        .env("UV_PYTHON_BIN_DIR", home.join("bin"))
        .status()
        .map_err(|err| ("fallback-to-script", err.to_string()))?;
    if !status.success() {
        return Err((
            "fallback-to-script",
            format!(
                "uv venv failed with exit {:?}; script creates the virtual environment",
                status.code()
            ),
        ));
    }
    let python = venv.join("Scripts").join("python.exe");
    if !python.is_file() {
        return Err((
            "fallback-to-script",
            format!(
                "uv venv completed but Python was missing at {}; script verifies venv output",
                python.display()
            ),
        ));
    }
    Ok((
        false,
        Some(format!("created virtual environment at {}", venv.display())),
    ))
}

fn run_native_dependencies_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native dependency stage is only complete on Windows".to_string(),
        ));
    }
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    if !install_root.is_dir() {
        return Err((
            "fallback-to-script",
            "install root missing; script creates the repository before dependencies".to_string(),
        ));
    }
    if !install_root.join("uv.lock").is_file() {
        return Err((
            "fallback-to-script",
            "uv.lock missing; script handles dependency installation for this checkout".to_string(),
        ));
    }
    let Some(python) = venv_python_command(&install_root) else {
        return Err((
            "fallback-to-script",
            "venv Python missing; script recreates the virtual environment before dependencies"
                .to_string(),
        ));
    };
    let path_text = windows_stage_path(options.current_path)?;
    let Some(uv) = windows_uv_command(home, &path_text) else {
        return Err((
            "fallback-to-script",
            "uv missing; script installs uv before dependencies".to_string(),
        ));
    };
    if options.dry_run {
        return Ok((
            false,
            Some(format!(
                "dependencies stage would run {} in {}",
                uv.display(),
                install_root.display()
            )),
        ));
    }

    let tiers = dependency_install_tiers(&install_root, options.wheelhouse_dir.as_deref());
    let mut last_exit = None;
    for (tier_name, args) in tiers {
        let status = ProcessCommand::new(&uv)
            .args(args.iter().map(String::as_str))
            .current_dir(&install_root)
            .env("UV_PROJECT_ENVIRONMENT", install_root.join("venv"))
            .env("UV_CACHE_DIR", home.join("uv-cache"))
            .env("UV_PYTHON_INSTALL_DIR", home.join("python"))
            .env("UV_PYTHON_BIN_DIR", home.join("bin"))
            .status()
            .map_err(|err| ("fallback-to-script", err.to_string()))?;
        if status.success() {
            let baseline = ProcessCommand::new(&python)
                .args(["-c", "import dotenv, openai, rich, prompt_toolkit"])
                .status()
                .map_err(|err| ("fallback-to-script", err.to_string()))?;
            if baseline.success() {
                return Ok((
                    false,
                    Some(format!("Python dependencies installed using {tier_name}")),
                ));
            }
            return Err((
                "fallback-to-script",
                format!(
                    "baseline imports failed after {tier_name} with exit {:?}; script verifies dependencies",
                    baseline.code()
                ),
            ));
        }
        last_exit = status.code();
    }
    Err((
        "fallback-to-script",
        format!(
            "native dependency install failed; last tier exited {:?}; script installs dependencies",
            last_exit
        ),
    ))
}

fn run_native_node_deps_stage(
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native node dependency probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    if windows_path_command(&path_text, "npm").is_none() {
        return Ok((
            true,
            Some("npm not available; Node.js dependencies skipped".to_string()),
        ));
    }
    Err((
        "fallback-to-script",
        "npm is available; script installs Node.js dependencies".to_string(),
    ))
}

fn run_native_desktop_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native desktop stage is only complete on Windows".to_string(),
        ));
    }
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    if !install_root.is_dir() {
        return Err((
            "fallback-to-script",
            "install root missing; script prepares the repository before desktop build".to_string(),
        ));
    }
    let desktop_dir = install_root.join("apps").join("desktop");
    if !desktop_dir.join("package.json").is_file() {
        return Err((
            "fallback-to-script",
            "apps/desktop package missing; script decides whether to skip desktop build"
                .to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let Some(npm) = windows_npm_command(home, &path_text) else {
        return Err((
            "fallback-to-script",
            "npm missing; script verifies Node.js before desktop build".to_string(),
        ));
    };
    if options.dry_run {
        return Ok((
            false,
            Some(format!(
                "desktop stage would run {} in {}",
                npm.display(),
                install_root.display()
            )),
        ));
    }

    let npm_cache = home.join("npm-cache");
    let electron_cache = home.join("electron-cache");
    fs::create_dir_all(&npm_cache).map_err(|err| ("stage-failed", err.to_string()))?;
    fs::create_dir_all(&electron_cache).map_err(|err| ("stage-failed", err.to_string()))?;
    restore_bundled_windows_cache_archive(
        home,
        options.bootstrap_tools_dir.as_deref(),
        "npm-cache",
    )?;
    restore_bundled_windows_cache_archive(
        home,
        options.bootstrap_tools_dir.as_deref(),
        "electron-cache",
    )?;

    let ci_status = run_windows_npm_command(
        &npm,
        ["ci", "--prefer-offline", "--no-audit", "--fund=false"],
        &install_root,
        &npm_cache,
        &electron_cache,
    )?;
    if !ci_status.success() {
        let install_status = run_windows_npm_command(
            &npm,
            ["install", "--prefer-offline", "--no-audit", "--fund=false"],
            &install_root,
            &npm_cache,
            &electron_cache,
        )?;
        if !install_status.success() {
            return Err((
                "fallback-to-script",
                format!(
                    "desktop workspace npm install failed with exit {:?}; script preserves full npm diagnostics",
                    install_status.code()
                ),
            ));
        }
    }

    let pack_status = run_windows_npm_command(
        &npm,
        ["run", "pack"],
        &desktop_dir,
        &npm_cache,
        &electron_cache,
    )?;
    if !pack_status.success() {
        return Err((
            "fallback-to-script",
            format!(
                "desktop pack failed with exit {:?}; script retries Electron cache recovery",
                pack_status.code()
            ),
        ));
    }
    let Some(desktop_exe) = windows_desktop_exe(&desktop_dir) else {
        return Err((
            "fallback-to-script",
            "desktop build completed but no Hermes.exe was found; script verifies build output"
                .to_string(),
        ));
    };
    Ok((
        false,
        Some(format!("desktop app built at {}", desktop_exe.display())),
    ))
}

fn run_native_node_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native Node.js probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let node = windows_path_command(&path_text, "node")
        .or_else(|| Some(home.join("node").join("node.exe")).filter(|path| path.is_file()));
    let Some(node) = node else {
        return Err((
            "fallback-to-script",
            "Node.js missing; script installs managed Node.js".to_string(),
        ));
    };
    let output = ProcessCommand::new(&node)
        .arg("--version")
        .output()
        .map_err(|err| ("stage-failed", err.to_string()))?;
    let version = command_version_text(&output);
    if output.status.success() && node_version_is_supported(&version) {
        return Ok((
            true,
            Some(format!(
                "Node.js {version} already available; node stage skipped"
            )),
        ));
    }
    Err((
        "fallback-to-script",
        format!("Node.js {version} missing or unsupported; script installs managed Node.js"),
    ))
}

fn run_native_system_packages_stage(
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native system package probe is only complete on Windows".to_string(),
        ));
    }
    let path_text = windows_stage_path(options.current_path)?;
    let has_ripgrep = windows_path_command(&path_text, "rg").is_some();
    let has_ffmpeg = windows_path_command(&path_text, "ffmpeg").is_some();
    if has_ripgrep && has_ffmpeg {
        return Ok((
            true,
            Some("ripgrep and ffmpeg already available; system package stage skipped".to_string()),
        ));
    }
    Err((
        "fallback-to-script",
        "ripgrep or ffmpeg missing; script installs system packages".to_string(),
    ))
}

fn run_native_config_templates_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<bool, (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native config-template stage is only complete on Windows".to_string(),
        ));
    }
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    hermes_manager::commands::write_config_templates(home, &install_root)
        .map(|()| false)
        .map_err(|err| ("stage-failed", err.to_string()))
}

fn run_native_platform_sdks_stage(
    home: &std::path::Path,
    options: NativeBootstrapStageOptions<'_>,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            "native platform SDK probe is only complete on Windows".to_string(),
        ));
    }
    let install_root = options
        .install_root
        .unwrap_or_else(|| hermes_manager::paths::agent_root(home));
    let python = install_root.join("venv").join("Scripts").join("python.exe");
    if !python.is_file() {
        return Ok((
            true,
            Some("venv Python missing; platform SDK verification skipped".to_string()),
        ));
    }
    let env_path = home.join(".env");
    if !env_path.is_file() {
        return Ok((
            true,
            Some("no .env file; no messaging platform SDKs required".to_string()),
        ));
    }
    let env_text =
        fs::read_to_string(&env_path).map_err(|err| ("stage-failed", err.to_string()))?;
    if !has_configured_platform_sdk_token(&env_text) {
        return Ok((
            true,
            Some(
                "no configured messaging platform tokens; platform SDK verification skipped"
                    .to_string(),
            ),
        ));
    }
    Err((
        "fallback-to-script",
        "messaging platform tokens found; script verifies and installs SDKs".to_string(),
    ))
}

fn has_configured_platform_sdk_token(env_text: &str) -> bool {
    const TOKEN_NAMES: [&str; 5] = [
        "TELEGRAM_BOT_TOKEN",
        "DISCORD_BOT_TOKEN",
        "SLACK_BOT_TOKEN",
        "SLACK_APP_TOKEN",
        "WHATSAPP_ENABLED",
    ];
    env_text.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.contains("your-token-here") {
            return false;
        }
        TOKEN_NAMES.iter().any(|name| {
            trimmed
                .strip_prefix(&format!("{name}="))
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
    })
}

fn windows_stage_path(
    override_path: Option<String>,
) -> std::result::Result<String, (&'static str, String)> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    let mut parts = Vec::new();
    if let Ok(path) = env::var("PATH") {
        parts.push(path);
    }
    if let Some(path) = hermes_manager::platform::read_windows_user_path()
        .map_err(|err| ("stage-failed", err.to_string()))?
    {
        parts.push(path);
    }
    if let Some(path) = hermes_manager::platform::read_windows_machine_path()
        .map_err(|err| ("stage-failed", err.to_string()))?
    {
        parts.push(path);
    }
    Ok(parts.join(";"))
}

fn venv_python_command(install_root: &std::path::Path) -> Option<PathBuf> {
    [
        install_root.join("venv").join("Scripts").join("python.exe"),
        install_root.join("venv").join("Scripts").join("python.cmd"),
        install_root.join("venv").join("Scripts").join("python.bat"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn dependency_install_tiers(
    install_root: &std::path::Path,
    wheelhouse_dir: Option<&std::path::Path>,
) -> Vec<(String, Vec<String>)> {
    let mut tiers = Vec::new();
    let checkout_wheelhouse = install_root.join("resources").join("wheelhouse");
    let wheelhouse = wheelhouse_dir
        .filter(|path| wheelhouse_has_wheels(path))
        .map(Path::to_path_buf)
        .or_else(|| wheelhouse_has_wheels(&checkout_wheelhouse).then_some(checkout_wheelhouse));
    if let Some(wheelhouse) = wheelhouse {
        tiers.push((
            "local wheelhouse (all)".to_string(),
            vec![
                "pip".to_string(),
                "install".to_string(),
                "--no-index".to_string(),
                "--find-links".to_string(),
                wheelhouse.display().to_string(),
                "-e".to_string(),
                ".[all]".to_string(),
            ],
        ));
    }
    tiers.push((
        "hash-verified (uv.lock)".to_string(),
        vec![
            "sync".to_string(),
            "--extra".to_string(),
            "all".to_string(),
            "--locked".to_string(),
        ],
    ));
    tiers.push((
        "all".to_string(),
        vec![
            "pip".to_string(),
            "install".to_string(),
            "-e".to_string(),
            ".[all]".to_string(),
        ],
    ));
    tiers
}

fn wheelhouse_has_wheels(path: &std::path::Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .is_some_and(|extension| extension == std::ffi::OsStr::new("whl"))
    })
}

fn windows_uv_command(home: &std::path::Path, path_text: &str) -> Option<PathBuf> {
    [
        home.join("bin").join("uv.exe"),
        home.join("bin").join("uv.cmd"),
        home.join("bin").join("uv.bat"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .or_else(|| windows_path_command(path_text, "uv"))
}

fn windows_npm_command(home: &std::path::Path, path_text: &str) -> Option<PathBuf> {
    [
        home.join("node").join("npm.cmd"),
        home.join("node").join("npm.exe"),
        home.join("node").join("npm.bat"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .or_else(|| windows_path_command(path_text, "npm"))
}

fn restore_bundled_windows_cache_archive(
    home: &std::path::Path,
    bootstrap_tools_dir: Option<&std::path::Path>,
    cache_name: &str,
) -> std::result::Result<Option<PathBuf>, (&'static str, String)> {
    let Some(bootstrap_tools_dir) = bootstrap_tools_dir else {
        return Ok(None);
    };
    let Some(arch) = windows_cache_arch() else {
        return Ok(None);
    };
    let archive_name = format!("{cache_name}-windows-{arch}.zip");
    let archive = bootstrap_tools_dir.join(&archive_name);
    if !archive.is_file() {
        return Ok(None);
    }
    verify_bootstrap_tools_archive(bootstrap_tools_dir, &archive_name, &archive)?;
    let install_dir = home.join(cache_name);
    extract_windows_cache_zip(&archive, &install_dir, cache_name)?;
    Ok(Some(archive))
}

fn verify_bootstrap_tools_archive(
    bootstrap_tools_dir: &std::path::Path,
    archive_name: &str,
    archive_path: &std::path::Path,
) -> std::result::Result<(), (&'static str, String)> {
    let manifest_path = bootstrap_tools_dir.join("bootstrap-tools-manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|err| ("fallback-to-script", err.to_string()))?;
    let manifest: BootstrapToolsManifest = serde_json::from_str(&manifest_text)
        .map_err(|err| ("fallback-to-script", err.to_string()))?;
    if manifest.schema_version != 1 {
        return Err((
            "fallback-to-script",
            format!(
                "unsupported bootstrap tools manifest schema: {}",
                manifest.schema_version
            ),
        ));
    }
    let Some(record) = manifest
        .archives
        .iter()
        .find(|record| record.name == archive_name)
    else {
        return Err((
            "fallback-to-script",
            format!("bootstrap tools manifest does not own {archive_name}"),
        ));
    };
    if record.sha256.len() != 64 || !record.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err((
            "fallback-to-script",
            format!("bootstrap tools manifest has invalid sha256 for {archive_name}"),
        ));
    }
    let actual = sha256_file(archive_path).map_err(|err| ("fallback-to-script", err))?;
    if !actual.eq_ignore_ascii_case(&record.sha256) {
        return Err((
            "fallback-to-script",
            format!("bootstrap tools checksum mismatch for {archive_name}"),
        ));
    }
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> std::result::Result<String, String> {
    use sha2::Digest;

    let mut file = fs::File::open(path).map_err(|err| err.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_windows_cache_zip(
    archive: &std::path::Path,
    install_dir: &std::path::Path,
    cache_root_name: &str,
) -> std::result::Result<(), (&'static str, String)> {
    let parent = install_dir.parent().ok_or_else(|| {
        (
            "stage-failed",
            format!(
                "cache install path has no parent: {}",
                install_dir.display()
            ),
        )
    })?;
    let tmp_dir = parent.join(format!("{cache_root_name}-extracting"));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).map_err(|err| ("stage-failed", err.to_string()))?;
    }
    fs::create_dir_all(&tmp_dir).map_err(|err| ("stage-failed", err.to_string()))?;

    let file = fs::File::open(archive).map_err(|err| ("fallback-to-script", err.to_string()))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|err| ("fallback-to-script", err.to_string()))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|err| ("fallback-to-script", err.to_string()))?;
        let Some(enclosed_name) = entry.enclosed_name() else {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err((
                "fallback-to-script",
                format!("unsafe ZIP entry in {}", archive.display()),
            ));
        };
        if enclosed_name.as_os_str().is_empty() {
            let _ = fs::remove_dir_all(&tmp_dir);
            return Err((
                "fallback-to-script",
                format!("blank ZIP entry in {}", archive.display()),
            ));
        }
        let output = tmp_dir.join(enclosed_name);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|err| ("stage-failed", err.to_string()))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|err| ("stage-failed", err.to_string()))?;
        }
        let mut out = fs::File::create(&output).map_err(|err| ("stage-failed", err.to_string()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|err| ("fallback-to-script", err.to_string()))?;
    }

    let extracted_root = tmp_dir.join(cache_root_name);
    let source_dir = if extracted_root.is_dir() {
        extracted_root
    } else {
        tmp_dir.clone()
    };
    if install_dir.exists() {
        fs::remove_dir_all(install_dir).map_err(|err| ("stage-failed", err.to_string()))?;
    }
    fs::rename(&source_dir, install_dir).map_err(|err| ("stage-failed", err.to_string()))?;
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).map_err(|err| ("stage-failed", err.to_string()))?;
    }
    Ok(())
}

fn windows_cache_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x64"),
        "aarch64" => Some("arm64"),
        "x86" => Some("x86"),
        _ => None,
    }
}

fn run_windows_npm_command<I, S>(
    npm: &std::path::Path,
    args: I,
    cwd: &std::path::Path,
    npm_cache: &std::path::Path,
    electron_cache: &std::path::Path,
) -> std::result::Result<std::process::ExitStatus, (&'static str, String)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    ProcessCommand::new(npm)
        .args(args)
        .current_dir(cwd)
        .env("npm_config_cache", npm_cache)
        .env("electron_config_cache", electron_cache)
        .env("ELECTRON_CACHE", electron_cache)
        .env("ELECTRON_BUILDER_CACHE", electron_cache)
        .env("CSC_IDENTITY_AUTO_DISCOVERY", "false")
        .env("WIN_CSC_LINK", "")
        .env("WIN_CSC_KEY_PASSWORD", "")
        .status()
        .map_err(|err| ("fallback-to-script", err.to_string()))
}

fn windows_desktop_exe(desktop_dir: &std::path::Path) -> Option<PathBuf> {
    [
        desktop_dir
            .join("release")
            .join("win-unpacked")
            .join("Hermes.exe"),
        desktop_dir
            .join("release")
            .join("win-arm64-unpacked")
            .join("Hermes.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn windows_path_command(path_text: &str, command_name: &str) -> Option<PathBuf> {
    let path_ext = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let extensions: Vec<String> = path_ext
        .split(';')
        .filter(|ext| !ext.trim().is_empty())
        .map(|ext| ext.trim().to_string())
        .collect();
    for raw_dir in path_text.split(';') {
        let dir = raw_dir.trim().trim_matches('"');
        if dir.is_empty() {
            continue;
        }
        let direct = Path::new(dir).join(command_name);
        if direct.is_file() {
            return Some(direct);
        }
        for ext in &extensions {
            let candidate = Path::new(dir).join(format!("{command_name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn node_version_is_supported(version: &str) -> bool {
    let version = version.trim().trim_start_matches('v');
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    match (major, minor) {
        (Some(major), Some(_)) if major > 22 => true,
        (Some(22), Some(minor)) => minor >= 12,
        (Some(21), _) => false,
        (Some(20), Some(minor)) => minor >= 19,
        _ => false,
    }
}

fn python_version_is_supported(version: &str) -> bool {
    let version = version
        .trim()
        .strip_prefix("Python ")
        .unwrap_or(version.trim());
    let mut parts = version.split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    matches!((major, minor), (Some(3), Some(11)))
}

fn command_version_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn run_native_interactive_skip_stage(
    stage: &str,
) -> std::result::Result<(bool, Option<String>), (&'static str, String)> {
    if !cfg!(target_os = "windows") {
        return Err((
            "fallback-to-script",
            format!("native interactive stage skip is only complete on Windows: {stage}"),
        ));
    }
    Ok((
        true,
        Some("skipped by native bridge for non-interactive desktop bootstrap".to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_report_serializes_machine_readable_cleanup_result() {
        let report = CommandReport {
            ok: true,
            command: "uninstall-lite",
            dry_run: true,
            paths: vec!["/tmp/hermes/hermes-agent".to_string()],
        };

        let value = serde_json::to_value(report).expect("report should serialize");

        assert_eq!(value["ok"], true);
        assert_eq!(value["command"], "uninstall-lite");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["paths"][0], "/tmp/hermes/hermes-agent");
    }

    #[test]
    fn uninstall_lite_parses_shortcuts_flag() {
        let cli = Cli::try_parse_from(["hermes-manager", "uninstall-lite", "--shortcuts"])
            .expect("shortcut cleanup flag should parse");

        match cli.command {
            Command::UninstallLite { shortcuts, .. } => assert!(shortcuts),
            _ => panic!("expected uninstall-lite command"),
        }
    }

    #[test]
    fn uninstall_gui_build_parses_dry_run_flag() {
        let cli = Cli::try_parse_from(["hermes-manager", "uninstall-gui-build", "--dry-run"])
            .expect("GUI build cleanup flag should parse");

        match cli.command {
            Command::UninstallGuiBuild { dry_run, .. } => assert!(dry_run),
            _ => panic!("expected uninstall-gui-build command"),
        }
    }

    #[test]
    fn uninstall_gui_build_parses_user_data_flag() {
        let cli = Cli::try_parse_from(["hermes-manager", "uninstall-gui-build", "--user-data"])
            .expect("GUI userData cleanup flag should parse");

        match cli.command {
            Command::UninstallGuiBuild { user_data, .. } => assert!(user_data),
            _ => panic!("expected uninstall-gui-build command"),
        }
    }

    #[test]
    fn uninstall_gui_build_parses_desktop_entries_flag() {
        let cli =
            Cli::try_parse_from(["hermes-manager", "uninstall-gui-build", "--desktop-entries"])
                .expect("GUI desktop entry cleanup flag should parse");

        match cli.command {
            Command::UninstallGuiBuild {
                desktop_entries, ..
            } => assert!(desktop_entries),
            _ => panic!("expected uninstall-gui-build command"),
        }
    }

    #[test]
    fn path_apply_report_serializes_machine_readable_result() {
        let report = PathApplyReport {
            ok: true,
            command: "write-user-path",
            dry_run: true,
            target: "user".to_string(),
            hermes_bin: "C:/Users/example/hermes/hermes-agent/venv/Scripts".to_string(),
            changed: true,
            applied: false,
        };

        let value = serde_json::to_value(report).expect("report should serialize");

        assert_eq!(value["ok"], true);
        assert_eq!(value["command"], "write-user-path");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["target"], "user");
        assert_eq!(
            value["hermesBin"],
            "C:/Users/example/hermes/hermes-agent/venv/Scripts"
        );
        assert_eq!(value["changed"], true);
        assert_eq!(value["applied"], false);
    }

    #[test]
    fn default_shortcut_dirs_follow_windows_user_locations() {
        let programs = default_windows_programs_dir();
        let desktop = default_windows_desktop_dir();

        assert!(programs.ends_with("Microsoft/Windows/Start Menu/Programs"));
        assert!(desktop.ends_with("Desktop"));
    }

    #[test]
    fn shortcut_apply_report_serializes_machine_readable_result() {
        let report = ShortcutApplyReport {
            ok: true,
            command: "write-shortcuts",
            dry_run: true,
            applied: false,
            shortcuts: vec!["C:/Users/example/Desktop/Hermes.lnk".to_string()],
        };

        let value = serde_json::to_value(report).expect("report should serialize");

        assert_eq!(value["ok"], true);
        assert_eq!(value["command"], "write-shortcuts");
        assert_eq!(value["dryRun"], true);
        assert_eq!(value["applied"], false);
        assert_eq!(value["shortcuts"][0], "C:/Users/example/Desktop/Hermes.lnk");
    }
}
