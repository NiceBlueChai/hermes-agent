//! Installed-files manifest for Hermes-managed resources.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ManagerError, Result};

/// Installed manifest schema version.
pub const INSTALLED_SCHEMA_VERSION: u32 = 1;

/// Manifest recording files and directories created by the Rust manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledManifest {
    pub schema_version: u32,
    pub hermes_home: PathBuf,
    pub entries: Vec<InstalledEntry>,
}

/// A single managed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub path: PathBuf,
    pub kind: InstalledKind,
}

/// Managed path type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledKind {
    File,
    Directory,
}

impl InstalledManifest {
    /// Create an empty installed manifest for a Hermes home.
    pub fn new(hermes_home: PathBuf) -> Self {
        Self {
            schema_version: INSTALLED_SCHEMA_VERSION,
            hermes_home,
            entries: Vec::new(),
        }
    }

    /// Add a managed entry if it is not already present.
    pub fn add_entry(&mut self, path: PathBuf, kind: InstalledKind) {
        if self.entries.iter().any(|entry| entry.path == path) {
            return;
        }
        self.entries.push(InstalledEntry { path, kind });
    }

    /// Read an installed manifest.
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|err| ManagerError::io(path, err))?;
        let manifest: Self =
            serde_json::from_str(&text).map_err(|err| ManagerError::json(path, err))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Write the installed manifest atomically.
    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| ManagerError::io(parent, err))?;
        }
        let tmp = path.with_extension("json.tmp");
        let text =
            serde_json::to_string_pretty(self).map_err(|err| ManagerError::json(path, err))?;
        fs::write(&tmp, text).map_err(|err| ManagerError::io(&tmp, err))?;
        replace_file_with_backup(&tmp, path)?;
        Ok(())
    }

    /// Validate the manifest schema.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != INSTALLED_SCHEMA_VERSION {
            return Err(ManagerError::InvalidManifest(format!(
                "installed schema_version {} is not supported",
                self.schema_version
            )));
        }
        if self.hermes_home.as_os_str().is_empty() {
            return Err(ManagerError::InvalidManifest("hermes_home is empty".into()));
        }
        Ok(())
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension(
        match path.extension().and_then(|extension| extension.to_str()) {
            Some(extension) => format!("{extension}.bak"),
            None => String::from("bak"),
        },
    )
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ManagerError::io(path, err)),
    }
}

fn replace_file_with_backup(tmp: &Path, destination: &Path) -> Result<()> {
    let backup = backup_path(destination);
    let has_destination = match destination.try_exists() {
        Ok(exists) => exists,
        Err(err) => {
            let _ = fs::remove_file(tmp);
            return Err(ManagerError::io(destination, err));
        }
    };

    if has_destination {
        if let Err(err) = remove_file_if_exists(&backup) {
            let _ = fs::remove_file(tmp);
            return Err(err);
        }
        if let Err(err) = fs::rename(destination, &backup) {
            let _ = fs::remove_file(tmp);
            return Err(ManagerError::io(destination, err));
        }
    }

    if let Err(err) = fs::rename(tmp, destination) {
        if has_destination {
            let _ = fs::rename(&backup, destination);
        }
        let _ = fs::remove_file(tmp);
        return Err(ManagerError::io(destination, err));
    }

    if has_destination {
        remove_file_if_exists(&backup)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_entry_deduplicates_paths() {
        let mut manifest = InstalledManifest::new(PathBuf::from("/tmp/hermes"));
        manifest.add_entry(PathBuf::from("/tmp/hermes/bin/rg"), InstalledKind::File);
        manifest.add_entry(PathBuf::from("/tmp/hermes/bin/rg"), InstalledKind::File);
        assert_eq!(manifest.entries.len(), 1);
    }

    #[test]
    fn write_and_read_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("installed-files.json");
        let mut manifest = InstalledManifest::new(dir.path().join("home"));
        manifest.add_entry(dir.path().join("home/bin/rg"), InstalledKind::File);

        manifest.write_atomic(&path).expect("write");
        let read = InstalledManifest::read(&path).expect("read");

        assert_eq!(read, manifest);
    }

    #[test]
    fn write_atomic_replaces_existing_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("installed-files.json");
        let mut manifest = InstalledManifest::new(dir.path().join("home"));
        manifest.add_entry(dir.path().join("home/bin/rg"), InstalledKind::File);
        manifest.write_atomic(&path).expect("first write");

        manifest.add_entry(dir.path().join("home/bin/fd"), InstalledKind::File);
        manifest.write_atomic(&path).expect("second write");
        let read = InstalledManifest::read(&path).expect("read");

        assert_eq!(read, manifest);
    }

    #[test]
    fn write_atomic_removes_backup_after_replace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("installed-files.json");
        let mut manifest = InstalledManifest::new(dir.path().join("home"));
        manifest.add_entry(dir.path().join("home/bin/rg"), InstalledKind::File);
        manifest.write_atomic(&path).expect("first write");

        manifest.add_entry(dir.path().join("home/bin/fd"), InstalledKind::File);
        manifest.write_atomic(&path).expect("second write");

        assert!(!backup_path(&path).exists());
    }
}
