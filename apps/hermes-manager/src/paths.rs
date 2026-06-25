//! Cross-platform path helpers for Hermes-managed resources.

use std::ffi::OsString;
use std::path::PathBuf;

/// Environment variable that overrides the default Hermes home.
pub const HERMES_HOME_ENV: &str = "HERMES_HOME";

/// Directory name used under the operating-system home directory.
pub const HERMES_DIR_NAME: &str = ".hermes";

/// Resolve Hermes home from an explicit override or the process environment.
pub fn hermes_home(explicit: Option<PathBuf>) -> PathBuf {
    hermes_home_from_env(
        explicit,
        std::env::var_os(HERMES_HOME_ENV),
        std::env::var_os("LOCALAPPDATA"),
        os_home_dir(),
    )
}

fn hermes_home_from_env(
    explicit: Option<PathBuf>,
    env_home: Option<OsString>,
    local_app_data: Option<OsString>,
    home: Option<OsString>,
) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }

    if let Some(path) = env_home {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    default_hermes_home_from_env(local_app_data, home, cfg!(target_os = "windows"))
}

/// Return the default Hermes home for the current platform.
pub fn default_hermes_home() -> PathBuf {
    default_hermes_home_from_env(
        std::env::var_os("LOCALAPPDATA"),
        os_home_dir(),
        cfg!(target_os = "windows"),
    )
}

fn os_home_dir() -> Option<OsString> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut home = PathBuf::from(drive);
                home.push(path);
                Some(home.into_os_string())
            })
            .or_else(|| std::env::var_os("HOME"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME")
    }
}

fn default_hermes_home_from_env(
    local_app_data: Option<OsString>,
    home: Option<OsString>,
    is_windows: bool,
) -> PathBuf {
    if is_windows {
        if let Some(local_app_data) = local_app_data {
            if !local_app_data.is_empty() {
                return PathBuf::from(local_app_data).join("hermes");
            }
        }

        if let Some(home) = home {
            if !home.is_empty() {
                return PathBuf::from(home)
                    .join("AppData")
                    .join("Local")
                    .join("hermes");
            }
        }

        return PathBuf::from("AppData").join("Local").join("hermes");
    }

    if let Some(home) = home {
        if !home.is_empty() {
            return PathBuf::from(home).join(HERMES_DIR_NAME);
        }
    }

    PathBuf::from(HERMES_DIR_NAME)
}

/// Runtime source checkout directory managed by Hermes.
pub fn agent_root(hermes_home: &std::path::Path) -> PathBuf {
    hermes_home.join("hermes-agent")
}

/// Runtime directories that Hermes installers own and may recreate.
pub fn managed_runtime_roots(hermes_home: &std::path::Path) -> Vec<PathBuf> {
    vec![
        agent_root(hermes_home),
        hermes_home.join("bin"),
        hermes_home.join("uv-cache"),
        hermes_home.join("pip-cache"),
        hermes_home.join("npm-cache"),
        hermes_home.join("electron-cache"),
        hermes_home.join("playwright-browsers"),
        hermes_home.join("node"),
        hermes_home.join("python"),
        hermes_home.join("git"),
        hermes_home.join("gateway-service"),
        hermes_home.join("bootstrap-cache"),
    ]
}

/// Runtime files that Hermes installers own and may recreate.
pub fn managed_runtime_files(hermes_home: &std::path::Path) -> Vec<PathBuf> {
    let installer_name = if cfg!(target_os = "windows") {
        "hermes-setup.exe"
    } else {
        "hermes-setup"
    };
    vec![
        hermes_home.join(installer_name),
        hermes_home.join("desktop-build-stamp.json"),
    ]
}

/// Source-built desktop GUI directories that can be regenerated on demand.
pub fn source_gui_build_roots(hermes_home: &std::path::Path) -> Vec<PathBuf> {
    let agent_root = agent_root(hermes_home);
    let desktop_dir = agent_root.join("apps").join("desktop");
    vec![
        desktop_dir.join("dist"),
        desktop_dir.join("release"),
        desktop_dir.join("node_modules"),
        agent_root.join("node_modules"),
    ]
}

/// Source-built desktop GUI files that can be regenerated on demand.
pub fn source_gui_build_files(hermes_home: &std::path::Path) -> Vec<PathBuf> {
    vec![hermes_home.join("desktop-build-stamp.json")]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DesktopPlatform {
    Windows,
    Macos,
    Linux,
}

/// Return the Electron desktop userData directory when it can be resolved.
pub fn desktop_user_data_dir() -> Option<PathBuf> {
    desktop_user_data_dir_from_env(
        current_desktop_platform(),
        os_home_dir(),
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_CONFIG_HOME"),
    )
}

fn current_desktop_platform() -> DesktopPlatform {
    if cfg!(target_os = "windows") {
        DesktopPlatform::Windows
    } else if cfg!(target_os = "macos") {
        DesktopPlatform::Macos
    } else {
        DesktopPlatform::Linux
    }
}

fn desktop_user_data_dir_from_env(
    platform: DesktopPlatform,
    home: Option<OsString>,
    appdata: Option<OsString>,
    xdg_config_home: Option<OsString>,
) -> Option<PathBuf> {
    match platform {
        DesktopPlatform::Windows => appdata
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                home.filter(|value| !value.is_empty())
                    .map(|value| PathBuf::from(value).join("AppData").join("Roaming"))
            })
            .map(|base| base.join("Hermes")),
        DesktopPlatform::Macos => home
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .map(|base| {
                base.join("Library")
                    .join("Application Support")
                    .join("Hermes")
            }),
        DesktopPlatform::Linux => xdg_config_home
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                home.filter(|value| !value.is_empty())
                    .map(|value| PathBuf::from(value).join(".config"))
            })
            .map(|base| base.join("Hermes")),
    }
}

/// Return Linux desktop entry files for packaged Hermes launchers.
pub fn linux_desktop_entry_files() -> Vec<PathBuf> {
    linux_desktop_entry_files_from_env(
        current_desktop_platform(),
        os_home_dir(),
        std::env::var_os("XDG_DATA_HOME"),
    )
}

fn linux_desktop_entry_files_from_env(
    platform: DesktopPlatform,
    home: Option<OsString>,
    xdg_data_home: Option<OsString>,
) -> Vec<PathBuf> {
    if platform != DesktopPlatform::Linux {
        return Vec::new();
    }

    let Some(data_base) = xdg_data_home
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|value| !value.is_empty())
                .map(|value| PathBuf::from(value).join(".local").join("share"))
        })
    else {
        return Vec::new();
    };
    let applications_dir = data_base.join("applications");
    vec![
        applications_dir.join("hermes.desktop"),
        applications_dir.join("Hermes.desktop"),
    ]
}

/// Manager metadata directory.
pub fn manager_state_dir(hermes_home: &std::path::Path) -> PathBuf {
    hermes_home.join("manager")
}

/// Installed-files manifest path.
pub fn installed_manifest_path(hermes_home: &std::path::Path) -> PathBuf {
    manager_state_dir(hermes_home).join("installed-files.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_home_wins() {
        let home = hermes_home(Some(PathBuf::from("D:/tmp/hermes-test")));
        assert_eq!(home, PathBuf::from("D:/tmp/hermes-test"));
    }

    #[test]
    fn agent_root_is_under_hermes_home() {
        let home = PathBuf::from("/tmp/hermes");
        assert_eq!(agent_root(&home), PathBuf::from("/tmp/hermes/hermes-agent"));
    }

    #[test]
    fn managed_runtime_roots_are_under_hermes_home() {
        let home = PathBuf::from("/tmp/hermes");
        assert_eq!(
            managed_runtime_roots(&home),
            vec![
                PathBuf::from("/tmp/hermes/hermes-agent"),
                PathBuf::from("/tmp/hermes/bin"),
                PathBuf::from("/tmp/hermes/uv-cache"),
                PathBuf::from("/tmp/hermes/pip-cache"),
                PathBuf::from("/tmp/hermes/npm-cache"),
                PathBuf::from("/tmp/hermes/electron-cache"),
                PathBuf::from("/tmp/hermes/playwright-browsers"),
                PathBuf::from("/tmp/hermes/node"),
                PathBuf::from("/tmp/hermes/python"),
                PathBuf::from("/tmp/hermes/git"),
                PathBuf::from("/tmp/hermes/gateway-service"),
                PathBuf::from("/tmp/hermes/bootstrap-cache"),
            ]
        );
    }

    #[test]
    fn managed_runtime_files_are_under_hermes_home() {
        let home = PathBuf::from("/tmp/hermes");
        let installer_name = if cfg!(target_os = "windows") {
            "hermes-setup.exe"
        } else {
            "hermes-setup"
        };

        assert_eq!(
            managed_runtime_files(&home),
            vec![
                PathBuf::from("/tmp/hermes").join(installer_name),
                PathBuf::from("/tmp/hermes/desktop-build-stamp.json"),
            ]
        );
    }

    #[test]
    fn source_gui_build_artifacts_are_under_hermes_home() {
        let home = PathBuf::from("/tmp/hermes");

        assert_eq!(
            source_gui_build_roots(&home),
            vec![
                PathBuf::from("/tmp/hermes/hermes-agent/apps/desktop/dist"),
                PathBuf::from("/tmp/hermes/hermes-agent/apps/desktop/release"),
                PathBuf::from("/tmp/hermes/hermes-agent/apps/desktop/node_modules"),
                PathBuf::from("/tmp/hermes/hermes-agent/node_modules"),
            ]
        );
        assert_eq!(
            source_gui_build_files(&home),
            vec![PathBuf::from("/tmp/hermes/desktop-build-stamp.json")]
        );
    }

    #[test]
    fn desktop_user_data_dir_matches_electron_locations() {
        assert_eq!(
            desktop_user_data_dir_from_env(
                DesktopPlatform::Macos,
                Some("/Users/alice".into()),
                None,
                None,
            ),
            Some(PathBuf::from(
                "/Users/alice/Library/Application Support/Hermes"
            ))
        );
        assert_eq!(
            desktop_user_data_dir_from_env(
                DesktopPlatform::Windows,
                Some("C:/Users/alice".into()),
                Some("C:/Users/alice/AppData/Roaming".into()),
                None,
            ),
            Some(PathBuf::from("C:/Users/alice/AppData/Roaming/Hermes"))
        );
        assert_eq!(
            desktop_user_data_dir_from_env(
                DesktopPlatform::Linux,
                Some("/home/alice".into()),
                None,
                Some("/tmp/xdg-config".into()),
            ),
            Some(PathBuf::from("/tmp/xdg-config/Hermes"))
        );
    }

    #[test]
    fn linux_desktop_entry_files_match_python_gui_uninstall_locations() {
        assert_eq!(
            linux_desktop_entry_files_from_env(
                DesktopPlatform::Linux,
                Some("/home/alice".into()),
                Some("/tmp/xdg-data".into()),
            ),
            vec![
                PathBuf::from("/tmp/xdg-data/applications/hermes.desktop"),
                PathBuf::from("/tmp/xdg-data/applications/Hermes.desktop"),
            ]
        );
        assert_eq!(
            linux_desktop_entry_files_from_env(
                DesktopPlatform::Macos,
                Some("/Users/alice".into()),
                None,
            ),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn installed_manifest_lives_under_manager_state() {
        let home = PathBuf::from("/tmp/hermes");
        assert_eq!(
            installed_manifest_path(&home),
            PathBuf::from("/tmp/hermes/manager/installed-files.json")
        );
    }

    #[test]
    fn explicit_env_home_wins_before_default_home() {
        let home = hermes_home_from_env(
            None,
            Some("D:/env/hermes".into()),
            Some("C:/Users/alice/AppData/Local".into()),
            Some("C:/Users/alice".into()),
        );
        assert_eq!(home, PathBuf::from("D:/env/hermes"));
    }

    #[test]
    fn windows_local_app_data_wins_for_default_home() {
        let home = default_hermes_home_from_env(
            Some("C:/Users/alice/AppData/Local".into()),
            Some("C:/Users/alice".into()),
            true,
        );
        assert_eq!(home, PathBuf::from("C:/Users/alice/AppData/Local/hermes"));
    }

    #[test]
    fn windows_blank_local_app_data_falls_back_under_home_app_data_local() {
        let home =
            default_hermes_home_from_env(Some("".into()), Some("C:/Users/alice".into()), true);
        assert_eq!(home, PathBuf::from("C:/Users/alice/AppData/Local/hermes"));
    }

    #[test]
    fn unix_default_home_uses_dot_hermes() {
        let home = default_hermes_home_from_env(
            Some("/ignored/localappdata".into()),
            Some("/home/alice".into()),
            false,
        );
        assert_eq!(home, PathBuf::from("/home/alice/.hermes"));
    }
}
