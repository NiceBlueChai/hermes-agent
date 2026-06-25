<!--
文件意图：提供 Hermes Agent Rust 安装管理器第一阶段实现计划，约束后续编码步骤不删减现有功能。
-->

# Rust Install Manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first vertical slice of a Rust install manager that reduces install-time dependency friction while
preserving every existing Hermes feature path.

**Architecture:** Add a standalone Rust CLI crate under `apps/hermes-manager` that owns manifest parsing, Hermes path
resolution, installed-file tracking, repair cleanup, and safe uninstall primitives. Keep Python and Electron behavior
intact; later tasks wire the manager into desktop packaging and bootstrap fallback paths through explicit manifests.

**Tech Stack:** Rust 2021, `clap`, `serde`, `serde_json`, `thiserror`, existing Electron `extraResources`, existing
Python uninstall fallback.

---

## Scope Check

The design spec spans several release subsystems: Rust manager, offline wheelhouse, desktop bootstrap, and
cross-platform installer packaging. This plan implements the first testable slice only:

1. Standalone Rust manager crate.
2. Bundled manifest schema.
3. Installed-files manifest.
4. Safe lite uninstall and repair-clean primitives.
5. Desktop package staging of the manager binary as a resource.

Offline wheelhouse generation and full replacement of `bootstrap-runner.cjs` require separate follow-up plans after this
slice lands.

## File Structure

- Create `apps/hermes-manager/Cargo.toml`: standalone Rust crate manifest.
- Create `apps/hermes-manager/src/main.rs`: CLI entrypoint and subcommand routing.
- Create `apps/hermes-manager/src/lib.rs`: module exports for tests and reuse.
- Create `apps/hermes-manager/src/error.rs`: shared error type and result alias.
- Create `apps/hermes-manager/src/paths.rs`: cross-platform Hermes path resolution.
- Create `apps/hermes-manager/src/bundled_manifest.rs`: bundled runtime manifest schema and validation.
- Create `apps/hermes-manager/src/installed_manifest.rs`: installed-files manifest schema and atomic read/write.
- Create `apps/hermes-manager/src/ownership.rs`: path-boundary and ownership checks before deletion.
- Create `apps/hermes-manager/src/commands.rs`: `doctor`, `install`, `repair-clean`, and `uninstall-lite` command
  implementations.
- Create `apps/hermes-manager/fixtures/bundled-manifest.valid.json`: manifest fixture used by tests.
- Modify `apps/desktop/package.json`: stage the manager binary in `extraResources` after build.
- Modify `apps/desktop/scripts/stage-native-deps.cjs`: copy a prebuilt manager binary when present.
- Modify `apps/desktop/electron/desktop-uninstall.cjs`: prefer the Rust manager for lite cleanup, fall back to Python
  uninstall.
- Test with `cargo test --manifest-path apps/hermes-manager/Cargo.toml` and existing desktop node tests.

## Task 1: Add Rust Manager Crate Skeleton

**Files:**
- Create: `apps/hermes-manager/Cargo.toml`
- Create: `apps/hermes-manager/src/main.rs`
- Create: `apps/hermes-manager/src/lib.rs`
- Create: `apps/hermes-manager/src/error.rs`

- [ ] **Step 1: Write the crate manifest**

Create `apps/hermes-manager/Cargo.toml`:

```toml
# 文件意图：定义 Hermes Rust 安装管理器的独立构建单元。
[package]
name = "hermes-manager"
version = "0.1.0"
edition = "2021"
rust-version = "1.77"
description = "Hermes install, repair, and uninstall manager"
license = "MIT"

[[bin]]
name = "hermes-manager"
path = "src/main.rs"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"

[dev-dependencies]
tempfile = "3"

[profile.release]
panic = "abort"
codegen-units = 1
lto = true
opt-level = "s"
strip = true
```

- [ ] **Step 2: Write the initial shared error type**

Create `apps/hermes-manager/src/error.rs`:

```rust
//! Shared error type for the Hermes install manager.

use std::path::PathBuf;

/// Result alias used by manager modules.
pub type Result<T> = std::result::Result<T, ManagerError>;

/// Errors surfaced by install, repair, and uninstall management commands.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON error at {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unsafe path outside Hermes home: {0}")]
    UnsafePath(PathBuf),
}

impl ManagerError {
    /// Wrap an I/O error with the path being operated on.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Wrap a JSON error with the path being parsed.
    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }
}
```

- [ ] **Step 3: Write library exports**

Create `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod error;

pub use error::{ManagerError, Result};
```

- [ ] **Step 4: Write the CLI entrypoint**

Create `apps/hermes-manager/src/main.rs`:

```rust
//! Command-line entrypoint for the Hermes install manager.

use clap::{Parser, Subcommand};

/// Manage Hermes runtime installation resources.
#[derive(Debug, Parser)]
#[command(name = "hermes-manager")]
#[command(about = "Hermes install, repair, and uninstall manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the manager version.
    Version,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
    }
}
```

- [ ] **Step 5: Verify the crate compiles**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
```

Expected: command exits 0 with `0 passed; 0 failed`.

- [ ] **Step 6: Commit**

```bash
git add apps/hermes-manager/Cargo.toml
git add apps/hermes-manager/src/main.rs apps/hermes-manager/src/lib.rs apps/hermes-manager/src/error.rs
git commit -m "feat(manager): 添加 Rust 安装管理器骨架"
```

## Task 2: Add Cross-Platform Hermes Path Resolution

**Files:**
- Create: `apps/hermes-manager/src/paths.rs`
- Modify: `apps/hermes-manager/src/lib.rs`
- Modify: `apps/hermes-manager/src/main.rs`

- [ ] **Step 1: Write failing path tests and implementation**

Create `apps/hermes-manager/src/paths.rs`:

```rust
//! Cross-platform path helpers for Hermes-managed resources.

use std::path::PathBuf;

/// Environment variable that overrides the default Hermes home.
pub const HERMES_HOME_ENV: &str = "HERMES_HOME";

/// Directory name used under the operating-system home directory.
pub const HERMES_DIR_NAME: &str = ".hermes";

/// Resolve Hermes home from an explicit override or the process environment.
pub fn hermes_home(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit {
        return path;
    }

    if let Some(path) = std::env::var_os(HERMES_HOME_ENV) {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    default_hermes_home()
}

/// Return the default Hermes home for the current platform.
pub fn default_hermes_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("hermes");
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(HERMES_DIR_NAME);
    }

    PathBuf::from(HERMES_DIR_NAME)
}

/// Runtime source checkout directory managed by Hermes.
pub fn agent_root(hermes_home: &std::path::Path) -> PathBuf {
    hermes_home.join("hermes-agent")
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
    fn installed_manifest_lives_under_manager_state() {
        let home = PathBuf::from("/tmp/hermes");
        assert_eq!(
            installed_manifest_path(&home),
            PathBuf::from("/tmp/hermes/manager/installed-files.json")
        );
    }
}
```

- [ ] **Step 2: Export the module**

Modify `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod error;
pub mod paths;

pub use error::{ManagerError, Result};
```

- [ ] **Step 3: Add `doctor` CLI output for paths**

Replace `apps/hermes-manager/src/main.rs` with:

```rust
//! Command-line entrypoint for the Hermes install manager.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Manage Hermes runtime installation resources.
#[derive(Debug, Parser)]
#[command(name = "hermes-manager")]
#[command(about = "Hermes install, repair, and uninstall manager")]
struct Cli {
    /// Override Hermes home for tests or isolated installs.
    #[arg(long)]
    hermes_home: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the manager version.
    Version,
    /// Print resolved manager paths.
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Command::Doctor => {
            let home = hermes_manager::paths::hermes_home(cli.hermes_home);
            println!("hermes_home={}", home.display());
            println!("agent_root={}", hermes_manager::paths::agent_root(&home).display());
            println!(
                "installed_manifest={}",
                hermes_manager::paths::installed_manifest_path(&home).display()
            );
        }
    }
}
```

- [ ] **Step 4: Verify tests and CLI**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
cargo run --manifest-path apps/hermes-manager/Cargo.toml -- --hermes-home D:\tmp\hermes-manager doctor
```

Expected: tests pass; CLI prints `hermes_home=`, `agent_root=`, and `installed_manifest=`.

- [ ] **Step 5: Commit**

```bash
git add apps/hermes-manager/src/lib.rs apps/hermes-manager/src/main.rs apps/hermes-manager/src/paths.rs
git commit -m "feat(manager): 解析 Hermes 托管路径"
```

## Task 3: Add Bundled Runtime Manifest Schema

**Files:**
- Create: `apps/hermes-manager/src/bundled_manifest.rs`
- Create: `apps/hermes-manager/fixtures/bundled-manifest.valid.json`
- Modify: `apps/hermes-manager/src/lib.rs`
- Modify: `apps/hermes-manager/src/main.rs`

- [ ] **Step 1: Add a valid bundled manifest fixture**

Create `apps/hermes-manager/fixtures/bundled-manifest.valid.json`:

```json
{
  "schema_version": 1,
  "hermes_version": "0.16.0",
  "source_commit": "615ad9792",
  "resources": [
    {
      "kind": "agent_snapshot",
      "path": "resources/agent",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    {
      "kind": "core_wheelhouse",
      "path": "resources/wheelhouse",
      "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }
  ]
}
```

- [ ] **Step 2: Implement manifest parsing and validation**

Create `apps/hermes-manager/src/bundled_manifest.rs`:

```rust
//! Bundled runtime manifest schema and validation.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{ManagerError, Result};

/// Supported bundled manifest schema version.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Manifest describing resources shipped in a release package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundledManifest {
    pub schema_version: u32,
    pub hermes_version: String,
    pub source_commit: String,
    pub resources: Vec<BundledResource>,
}

/// A single bundled resource entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundledResource {
    pub kind: ResourceKind,
    pub path: String,
    pub sha256: String,
}

/// Resource types the first manager slice understands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    AgentSnapshot,
    CoreWheelhouse,
    PythonRuntime,
    Tool,
}

impl BundledManifest {
    /// Read a bundled manifest from JSON.
    pub fn read(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|err| ManagerError::io(path, err))?;
        let manifest: Self = serde_json::from_str(&text).map_err(|err| ManagerError::json(path, err))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate schema and required fields.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(ManagerError::InvalidManifest(format!(
                "schema_version {} is not supported",
                self.schema_version
            )));
        }
        if self.hermes_version.trim().is_empty() {
            return Err(ManagerError::InvalidManifest("hermes_version is empty".into()));
        }
        if self.source_commit.trim().is_empty() {
            return Err(ManagerError::InvalidManifest("source_commit is empty".into()));
        }
        if self.resources.is_empty() {
            return Err(ManagerError::InvalidManifest("resources is empty".into()));
        }
        for resource in &self.resources {
            if resource.path.trim().is_empty() {
                return Err(ManagerError::InvalidManifest("resource path is empty".into()));
            }
            if resource.sha256.len() != 64 || !resource.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Err(ManagerError::InvalidManifest(format!(
                    "resource {} has invalid sha256",
                    resource.path
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_valid_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/bundled-manifest.valid.json");
        let manifest = BundledManifest::read(&path).expect("fixture should parse");
        assert_eq!(manifest.schema_version, SUPPORTED_SCHEMA_VERSION);
        assert_eq!(manifest.resources.len(), 2);
        assert_eq!(manifest.resources[0].kind, ResourceKind::AgentSnapshot);
    }

    #[test]
    fn rejects_unsupported_schema() {
        let manifest = BundledManifest {
            schema_version: 99,
            hermes_version: "0.16.0".into(),
            source_commit: "615ad9792".into(),
            resources: vec![BundledResource {
                kind: ResourceKind::AgentSnapshot,
                path: "resources/agent".into(),
                sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }],
        };

        let err = manifest.validate().expect_err("schema should be rejected");
        assert!(err.to_string().contains("not supported"));
    }
}
```

- [ ] **Step 3: Export the module**

Modify `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod bundled_manifest;
pub mod error;
pub mod paths;

pub use error::{ManagerError, Result};
```

- [ ] **Step 4: Add manifest validation to `doctor`**

Modify `apps/hermes-manager/src/main.rs` so the `Cli` struct includes:

```rust
    /// Optional bundled manifest path to validate.
    #[arg(long)]
    manifest: Option<PathBuf>,
```

Then add this block inside the `Command::Doctor` arm after printing paths:

```rust
            if let Some(manifest_path) = cli.manifest.as_deref() {
                match hermes_manager::bundled_manifest::BundledManifest::read(manifest_path) {
                    Ok(manifest) => {
                        println!("bundled_manifest=ok");
                        println!("bundled_manifest_hermes_version={}", manifest.hermes_version);
                    }
                    Err(err) => {
                        eprintln!("bundled_manifest=error: {err}");
                        std::process::exit(2);
                    }
                }
            }
```

- [ ] **Step 5: Verify tests and manifest CLI**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
cargo run --manifest-path apps/hermes-manager/Cargo.toml -- `
  --manifest apps\hermes-manager\fixtures\bundled-manifest.valid.json doctor
```

Expected: tests pass; CLI prints `bundled_manifest=ok`.

- [ ] **Step 6: Commit**

```bash
git add apps/hermes-manager/src/bundled_manifest.rs apps/hermes-manager/src/lib.rs
git add apps/hermes-manager/src/main.rs apps/hermes-manager/fixtures/bundled-manifest.valid.json
git commit -m "feat(manager): 校验随包资源清单"
```

## Task 4: Add Installed-Files Manifest

**Files:**
- Create: `apps/hermes-manager/src/installed_manifest.rs`
- Modify: `apps/hermes-manager/src/lib.rs`

- [ ] **Step 1: Implement installed-files manifest read/write**

Create `apps/hermes-manager/src/installed_manifest.rs`:

```rust
//! Installed-files manifest for Hermes-managed resources.

use std::fs;
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
        let manifest: Self = serde_json::from_str(&text).map_err(|err| ManagerError::json(path, err))?;
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
        let text = serde_json::to_string_pretty(self).map_err(|err| ManagerError::json(path, err))?;
        fs::write(&tmp, text).map_err(|err| ManagerError::io(&tmp, err))?;
        fs::rename(&tmp, path).map_err(|err| ManagerError::io(path, err))?;
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
}
```

- [ ] **Step 2: Export the module**

Modify `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod bundled_manifest;
pub mod error;
pub mod installed_manifest;
pub mod paths;

pub use error::{ManagerError, Result};
```

- [ ] **Step 3: Verify tests**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
```

Expected: tests pass, including `write_and_read_round_trip`.

- [ ] **Step 4: Commit**

```bash
git add apps/hermes-manager/src/installed_manifest.rs apps/hermes-manager/src/lib.rs
git commit -m "feat(manager): 记录托管安装文件"
```

## Task 5: Add Safe Ownership Checks

**Files:**
- Create: `apps/hermes-manager/src/ownership.rs`
- Modify: `apps/hermes-manager/src/lib.rs`

- [ ] **Step 1: Implement safe path checks**

Create `apps/hermes-manager/src/ownership.rs`:

```rust
//! Ownership and safety checks before deleting Hermes-managed paths.

use std::path::{Component, Path, PathBuf};

use crate::{ManagerError, Result};

/// Return true when `candidate` is inside `root` after lexical normalization.
pub fn is_inside_root(root: &Path, candidate: &Path) -> bool {
    let root = normalize(root);
    let candidate = normalize(candidate);
    candidate == root || candidate.starts_with(root)
}

/// Ensure a path is safe to delete as a Hermes-managed path.
pub fn ensure_safe_to_delete(hermes_home: &Path, candidate: &Path) -> Result<()> {
    if !is_inside_root(hermes_home, candidate) {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }
    if normalize(hermes_home) == normalize(candidate) {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }
    Ok(())
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_child_path() {
        let root = Path::new("/tmp/hermes");
        let candidate = Path::new("/tmp/hermes/bin/rg");
        assert!(is_inside_root(root, candidate));
        ensure_safe_to_delete(root, candidate).expect("child should be safe");
    }

    #[test]
    fn rejects_parent_escape() {
        let root = Path::new("/tmp/hermes");
        let candidate = Path::new("/tmp/hermes/../other");
        assert!(!is_inside_root(root, candidate));
        assert!(ensure_safe_to_delete(root, candidate).is_err());
    }

    #[test]
    fn rejects_deleting_home_root() {
        let root = Path::new("/tmp/hermes");
        assert!(ensure_safe_to_delete(root, root).is_err());
    }
}
```

- [ ] **Step 2: Export the module**

Modify `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod bundled_manifest;
pub mod error;
pub mod installed_manifest;
pub mod ownership;
pub mod paths;

pub use error::{ManagerError, Result};
```

- [ ] **Step 3: Verify tests**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
```

Expected: tests pass, including `rejects_parent_escape` and `rejects_deleting_home_root`.

- [ ] **Step 4: Commit**

```bash
git add apps/hermes-manager/src/ownership.rs apps/hermes-manager/src/lib.rs
git commit -m "feat(manager): 防止卸载路径越界"
```

## Task 6: Implement Lite Uninstall and Repair-Clean

**Files:**
- Create: `apps/hermes-manager/src/commands.rs`
- Modify: `apps/hermes-manager/src/lib.rs`
- Modify: `apps/hermes-manager/src/main.rs`

- [ ] **Step 1: Implement command logic**

Create `apps/hermes-manager/src/commands.rs`:

```rust
//! Command implementations for the Hermes install manager.

use std::fs;
use std::path::Path;

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
    manifest.add_entry(paths::agent_root(hermes_home), InstalledKind::Directory);
    manifest.write_atomic(&manifest_path)
}

/// Remove managed runtime paths while preserving user data.
pub fn uninstall_lite(hermes_home: &Path) -> Result<Vec<String>> {
    let manifest_path = paths::installed_manifest_path(hermes_home);
    let manifest = InstalledManifest::read(&manifest_path)?;
    let mut removed = Vec::new();

    for entry in manifest.entries.iter().rev() {
        ensure_safe_to_delete(hermes_home, &entry.path)?;
        if !entry.path.exists() {
            continue;
        }
        match entry.kind {
            InstalledKind::File => {
                fs::remove_file(&entry.path).map_err(|err| ManagerError::io(&entry.path, err))?;
            }
            InstalledKind::Directory => {
                fs::remove_dir_all(&entry.path).map_err(|err| ManagerError::io(&entry.path, err))?;
            }
        }
        removed.push(entry.path.display().to_string());
    }

    Ok(removed)
}

/// Remove the runtime checkout and bootstrap marker so the next launch repairs it.
pub fn repair_clean(hermes_home: &Path) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    let agent_root = paths::agent_root(hermes_home);
    ensure_safe_to_delete(hermes_home, &agent_root)?;
    if agent_root.exists() {
        fs::remove_dir_all(&agent_root).map_err(|err| ManagerError::io(&agent_root, err))?;
        removed.push(agent_root.display().to_string());
    }

    let marker = hermes_home.join("hermes-agent").join(".hermes-bootstrap-complete");
    ensure_safe_to_delete(hermes_home, &marker)?;
    if marker.exists() {
        fs::remove_file(&marker).map_err(|err| ManagerError::io(&marker, err))?;
        removed.push(marker.display().to_string());
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installed_manifest::{InstalledKind, InstalledManifest};

    #[test]
    fn install_metadata_creates_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        install_metadata(&home).expect("install metadata");
        assert!(paths::installed_manifest_path(&home).exists());
    }

    #[test]
    fn uninstall_lite_removes_only_manifest_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let runtime = home.join("hermes-agent");
        let user_data = home.join("sessions");
        fs::create_dir_all(&runtime).expect("runtime");
        fs::create_dir_all(&user_data).expect("user data");

        let mut manifest = InstalledManifest::new(home.clone());
        manifest.add_entry(runtime.clone(), InstalledKind::Directory);
        manifest
            .write_atomic(&paths::installed_manifest_path(&home))
            .expect("manifest");

        let removed = uninstall_lite(&home).expect("uninstall lite");
        assert_eq!(removed, vec![runtime.display().to_string()]);
        assert!(!runtime.exists());
        assert!(user_data.exists());
    }
}
```

- [ ] **Step 2: Export commands**

Modify `apps/hermes-manager/src/lib.rs`:

```rust
//! Library modules for the Hermes install manager.

pub mod bundled_manifest;
pub mod commands;
pub mod error;
pub mod installed_manifest;
pub mod ownership;
pub mod paths;

pub use error::{ManagerError, Result};
```

- [ ] **Step 3: Wire CLI subcommands**

Replace `apps/hermes-manager/src/main.rs` with:

```rust
//! Command-line entrypoint for the Hermes install manager.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the manager version.
    Version,
    /// Print resolved manager paths and validate optional bundled manifest.
    Doctor,
    /// Create manager metadata.
    InstallMetadata,
    /// Remove managed runtime resources while keeping user data.
    UninstallLite,
    /// Remove runtime checkout so next launch can repair it.
    RepairClean,
}

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
                let manifest = hermes_manager::bundled_manifest::BundledManifest::read(manifest_path)?;
                println!("bundled_manifest=ok");
                println!("bundled_manifest_hermes_version={}", manifest.hermes_version);
            }
        }
        Command::InstallMetadata => {
            hermes_manager::commands::install_metadata(&home)?;
            println!("install_metadata=ok");
        }
        Command::UninstallLite => {
            for removed in hermes_manager::commands::uninstall_lite(&home)? {
                println!("removed={removed}");
            }
            println!("uninstall_lite=ok");
        }
        Command::RepairClean => {
            for removed in hermes_manager::commands::repair_clean(&home)? {
                println!("removed={removed}");
            }
            println!("repair_clean=ok");
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Verify tests and CLI**

Run:

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
$home = Join-Path $env:TEMP "hermes-manager-smoke"
cargo run --manifest-path apps/hermes-manager/Cargo.toml -- --hermes-home $home install-metadata
cargo run --manifest-path apps/hermes-manager/Cargo.toml -- --hermes-home $home doctor
```

Expected: tests pass; install metadata prints `install_metadata=ok`; doctor prints paths under the temp home.

- [ ] **Step 5: Commit**

```bash
git add apps/hermes-manager/src/commands.rs apps/hermes-manager/src/lib.rs apps/hermes-manager/src/main.rs
git commit -m "feat(manager): 支持轻量卸载和修复清理"
```

## Task 7: Stage Manager Binary in Desktop Packaging

**Files:**
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/scripts/stage-native-deps.cjs`

- [ ] **Step 1: Add manager resource to desktop package config**

In `apps/desktop/package.json`, append this `extraResources` entry after the existing `native-deps` entry:

```json
      {
        "from": "build/hermes-manager",
        "to": "hermes-manager"
      },
```

Keep existing entries for `install-stamp.json`, `native-deps`, and `icon.ico`.

- [ ] **Step 2: Stage a prebuilt manager binary when available**

Append this block near the end of `apps/desktop/scripts/stage-native-deps.cjs`, before the final completion path exits:

```javascript
const managerName = process.platform === 'win32' ? 'hermes-manager.exe' : 'hermes-manager'
const managerSource = path.join(REPO_ROOT, 'apps', 'hermes-manager', 'target', 'release', managerName)
const managerDestDir = path.join(APP_ROOT, 'build', 'hermes-manager')
const managerDest = path.join(managerDestDir, managerName)

if (fs.existsSync(managerSource)) {
  fs.mkdirSync(managerDestDir, { recursive: true })
  fs.copyFileSync(managerSource, managerDest)
  console.log(`[stage-native-deps] hermes-manager: copied ${path.relative(APP_ROOT, managerDest)}`)
} else {
  console.log(
    `[stage-native-deps] hermes-manager: ${path.relative(REPO_ROOT, managerSource)} not found; ` +
      'packaged app will use existing Python uninstall fallback'
  )
}
```

- [ ] **Step 3: Verify desktop staging tests**

Run:

```powershell
node apps/desktop/scripts/before-pack.test.cjs
npm --workspace apps/desktop run test:desktop:platforms
```

Expected: node tests exit 0. If workspace dependencies are missing, run the existing project install command first and
rerun the tests.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/package.json apps/desktop/scripts/stage-native-deps.cjs
git commit -m "build(desktop): 打包 Rust 安装管理器"
```

## Task 8: Prefer Manager for Desktop Lite Uninstall

**Files:**
- Modify: `apps/desktop/electron/desktop-uninstall.cjs`
- Modify: `apps/desktop/electron/desktop-uninstall.test.cjs`
- Modify: `apps/desktop/electron/main.cjs`

- [ ] **Step 1: Add manager path resolver tests**

In `apps/desktop/electron/desktop-uninstall.test.cjs`, add tests for a new exported `resolveHermesManagerPath` function:

```javascript
test('resolveHermesManagerPath returns packaged resource path on Windows', () => {
  const resourcesPath = 'C:\\Program Files\\Hermes\\resources'
  assert.equal(
    resolveHermesManagerPath(resourcesPath, 'win32'),
    'C:\\Program Files\\Hermes\\resources\\hermes-manager\\hermes-manager.exe'
  )
})

test('resolveHermesManagerPath returns packaged resource path on POSIX', () => {
  const resourcesPath = '/Applications/Hermes.app/Contents/Resources'
  assert.equal(
    resolveHermesManagerPath(resourcesPath, 'darwin'),
    '/Applications/Hermes.app/Contents/Resources/hermes-manager/hermes-manager'
  )
})
```

Also add `resolveHermesManagerPath` to the destructured import at the top of that test file.

- [ ] **Step 2: Export manager path resolver**

In `apps/desktop/electron/desktop-uninstall.cjs`, add:

```javascript
function resolveHermesManagerPath(resourcesPath, platform = process.platform) {
  if (!resourcesPath) return null
  const exe = platform === 'win32' ? 'hermes-manager.exe' : 'hermes-manager'
  return path.join(resourcesPath, 'hermes-manager', exe)
}
```

Include it in `module.exports`.

- [ ] **Step 3: Add manager command construction tests**

In `apps/desktop/electron/desktop-uninstall.test.cjs`, add:

```javascript
test('buildManagerCommandForMode uses manager only for lite mode when binary exists', () => {
  assert.deepEqual(
    buildManagerCommandForMode({
      mode: 'lite',
      managerPath: '/resources/hermes-manager/hermes-manager',
      hermesHome: '/tmp/hermes',
      managerExists: file => file === '/resources/hermes-manager/hermes-manager'
    }),
    {
      command: '/resources/hermes-manager/hermes-manager',
      args: ['--hermes-home', '/tmp/hermes', 'uninstall-lite']
    }
  )
})

test('buildManagerCommandForMode returns null for full/gui/missing manager', () => {
  assert.equal(
    buildManagerCommandForMode({
      mode: 'full',
      managerPath: '/resources/hermes-manager/hermes-manager',
      hermesHome: '/tmp/hermes',
      managerExists: () => true
    }),
    null
  )
  assert.equal(
    buildManagerCommandForMode({
      mode: 'gui',
      managerPath: '/resources/hermes-manager/hermes-manager',
      hermesHome: '/tmp/hermes',
      managerExists: () => true
    }),
    null
  )
  assert.equal(
    buildManagerCommandForMode({
      mode: 'lite',
      managerPath: '/resources/hermes-manager/hermes-manager',
      hermesHome: '/tmp/hermes',
      managerExists: () => false
    }),
    null
  )
})
```

- [ ] **Step 4: Implement manager path and command helpers**

In `apps/desktop/electron/desktop-uninstall.cjs`, add `fs` at the top:

```javascript
const fs = require('node:fs')
```

Add these helpers after `shouldRemoveAppBundle`:

```javascript
function resolveHermesManagerPath(resourcesPath, platform = process.platform) {
  if (!resourcesPath) return null
  const p = platform === 'win32' ? path.win32 : path.posix
  const exe = platform === 'win32' ? 'hermes-manager.exe' : 'hermes-manager'
  return p.join(resourcesPath, 'hermes-manager', exe)
}

function buildManagerCommandForMode({ mode, managerPath, hermesHome, managerExists = fs.existsSync }) {
  if (mode !== 'lite') return null
  if (!managerPath || !managerExists(managerPath)) return null
  const args = []
  if (hermesHome) {
    args.push('--hermes-home', hermesHome)
  }
  args.push('uninstall-lite')
  return { command: managerPath, args }
}
```

Export both functions in `module.exports`.

- [ ] **Step 5: Make cleanup scripts use optional manager command**

Change `buildPosixCleanupScript` signature to accept `managerCommand`:

```javascript
function buildPosixCleanupScript({
  desktopPid,
  pythonExe,
  pythonPath,
  agentRoot,
  uninstallArgs,
  appPath,
  hermesHome,
  managerCommand = null
}) {
```

Replace the Python uninstall line in `buildPosixCleanupScript` with:

```javascript
  if (managerCommand) {
    lines.push(
      `${q(managerCommand.command)} ${managerCommand.args.map(q).join(' ')} || ` +
        `${q(pythonExe)} ${uninstallArgs.map(q).join(' ')} || true`
    )
  } else {
    lines.push(`${q(pythonExe)} ${uninstallArgs.map(q).join(' ')} || true`)
  }
```

Change `buildWindowsCleanupScript` signature to accept `managerCommand`:

```javascript
function buildWindowsCleanupScript({
  desktopPid,
  pythonExe,
  pythonPath,
  agentRoot,
  uninstallArgs,
  appPath,
  hermesHome,
  managerCommand = null
}) {
```

Replace the Python uninstall line in `buildWindowsCleanupScript` with:

```javascript
  if (managerCommand) {
    lines.push(
      `${q(managerCommand.command)} ${managerCommand.args.map(q).join(' ')} || ` +
        `${q(pythonExe)} ${uninstallArgs.map(q).join(' ')}`
    )
  } else {
    lines.push(`${q(pythonExe)} ${uninstallArgs.map(q).join(' ')}`)
  }
```

Add tests that assert the manager command is preferred and the Python command remains as fallback:

```javascript
test('buildPosixCleanupScript prefers manager command with python fallback', () => {
  const script = buildPosixCleanupScript({
    desktopPid: 1,
    pythonExe: '/usr/bin/python3',
    pythonPath: '/home/x/.hermes/hermes-agent',
    agentRoot: '/home/x/.hermes/hermes-agent',
    uninstallArgs: ['-m', 'hermes_cli.uninstall', '--mode', 'lite'],
    appPath: null,
    hermesHome: '/home/x/.hermes',
    managerCommand: {
      command: '/resources/hermes-manager/hermes-manager',
      args: ['--hermes-home', '/home/x/.hermes', 'uninstall-lite']
    }
  })

  assert.match(
    script,
    /'\/resources\/hermes-manager\/hermes-manager' '--hermes-home' '\/home\/x\/\.hermes' 'uninstall-lite' \|\|/
  )
  assert.match(script, /'\/usr\/bin\/python3' '-m' 'hermes_cli\.uninstall' '--mode' 'lite'/)
})

test('buildWindowsCleanupScript prefers manager command with python fallback', () => {
  const script = buildWindowsCleanupScript({
    desktopPid: 1,
    pythonExe: 'C:\\Python313\\python.exe',
    pythonPath: 'C:\\hermes',
    agentRoot: 'C:\\hermes',
    uninstallArgs: ['-m', 'hermes_cli.uninstall', '--mode', 'lite'],
    appPath: null,
    hermesHome: 'C:\\Users\\x\\AppData\\Local\\hermes',
    managerCommand: {
      command: 'C:\\Hermes\\resources\\hermes-manager\\hermes-manager.exe',
      args: ['--hermes-home', 'C:\\Users\\x\\AppData\\Local\\hermes', 'uninstall-lite']
    }
  })

  const managerPattern = new RegExp(
    '"C:\\\\Hermes\\\\resources\\\\hermes-manager\\\\hermes-manager.exe" "--hermes-home" ' +
      '"C:\\\\Users\\\\x\\\\AppData\\\\Local\\\\hermes" "uninstall-lite" \\|\\|'
  )
  assert.match(script, managerPattern)
  assert.match(script, /"C:\\Python313\\python.exe" "-m" "hermes_cli\.uninstall" "--mode" "lite"/)
})
```

- [ ] **Step 6: Pass manager command from Electron main**

In `apps/desktop/electron/main.cjs`, add these imports from `desktop-uninstall.cjs`:

```javascript
  buildManagerCommandForMode,
  resolveHermesManagerPath,
```

Inside `runDesktopUninstall(mode)`, after `removeBundle` is assigned, add:

```javascript
  const managerCommand = buildManagerCommandForMode({
    mode,
    managerPath: resolveHermesManagerPath(process.resourcesPath, process.platform),
    hermesHome: HERMES_HOME
  })
```

Add `managerCommand` to `scriptArgs`:

```javascript
    managerCommand
```

This preserves the Python fallback because the cleanup script still runs `python -m hermes_cli.uninstall` if the manager
exits non-zero.

- [ ] **Step 7: Verify desktop uninstall tests**

Run:

```powershell
node --test apps/desktop/electron/desktop-uninstall.test.cjs
```

Expected: all desktop uninstall tests pass.

- [ ] **Step 8: Commit**

```bash
git add apps/desktop/electron/desktop-uninstall.cjs
git add apps/desktop/electron/desktop-uninstall.test.cjs apps/desktop/electron/main.cjs
git commit -m "feat(desktop): 优先使用 Rust 管理器轻量卸载"
```

## Task 9: Final Verification and Push

**Files:**
- No new files expected.

- [ ] **Step 1: Run Rust tests**

```powershell
cargo test --manifest-path apps/hermes-manager/Cargo.toml
```

Expected: all Rust tests pass.

- [ ] **Step 2: Run focused desktop tests**

```powershell
node --test apps/desktop/electron/desktop-uninstall.test.cjs apps/desktop/electron/bootstrap-platform.test.cjs
```

Expected: all selected Node tests pass.

- [ ] **Step 3: Run diff checks**

```powershell
git diff --check
git status --short
```

Expected: no whitespace errors; status shows only intentional files before final commit, then clean after commit.

- [ ] **Step 4: Push branch**

```bash
git push
```

Expected: branch updates on `fork/docs/rust-self-contained-release`.

## Self-Review

Spec coverage:

- Installation dependency friction is covered by Tasks 1-7.
- No feature deletion is covered by Python fallback in Task 8 and compatibility verification in Task 9.
- Safe uninstall is covered by Tasks 4-6 and the lite desktop fallback in Task 8.
- Desktop packaging integration is covered by Task 7.
- Offline wheelhouse is intentionally excluded from this first implementation slice and should get its own follow-up
  plan after the manager manifest exists.

Placeholder scan:

- The plan contains no placeholder markers or vague deferred implementation steps.
- Each code-changing task includes concrete file paths, code blocks, commands, and expected results.

Type consistency:

- The Rust crate consistently uses `ManagerError`, `Result`, `BundledManifest`, `InstalledManifest`, and
  `InstalledKind`.
- CLI subcommands use kebab-case generated by `clap`, so `InstallMetadata` is invoked as `install-metadata`,
  `UninstallLite` as `uninstall-lite`, and `RepairClean` as `repair-clean`.
