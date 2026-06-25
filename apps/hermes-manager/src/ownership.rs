//! Ownership and safety checks before deleting Hermes-managed paths.

use std::fs;
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
    if hermes_home.as_os_str().is_empty() {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }

    let lexical_home = normalize(hermes_home);
    let hermes_home = fs::canonicalize(hermes_home)
        .map_err(|source| ManagerError::io(hermes_home.to_path_buf(), source))?;
    if is_filesystem_root(&hermes_home) {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }

    if normalize(candidate) == lexical_home {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }

    let candidate_real = canonical_candidate_boundary(candidate)?;
    if candidate_real.exists && candidate_real.path == hermes_home {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }
    if !candidate_real.path.starts_with(&hermes_home) {
        return Err(ManagerError::UnsafePath(candidate.to_path_buf()));
    }
    Ok(())
}

struct CandidateBoundary {
    path: PathBuf,
    exists: bool,
}

fn canonical_candidate_boundary(candidate: &Path) -> Result<CandidateBoundary> {
    match fs::canonicalize(candidate) {
        Ok(path) => Ok(CandidateBoundary { path, exists: true }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            canonical_existing_parent(candidate).map(|path| CandidateBoundary {
                path,
                exists: false,
            })
        }
        Err(source) => Err(ManagerError::io(candidate.to_path_buf(), source)),
    }
}

fn canonical_existing_parent(path: &Path) -> Result<PathBuf> {
    let mut parent = path.parent();
    while let Some(current) = parent {
        if current.as_os_str().is_empty() {
            break;
        }
        match fs::canonicalize(current) {
            Ok(path) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                parent = current.parent();
            }
            Err(source) => return Err(ManagerError::io(current.to_path_buf(), source)),
        }
    }

    Err(ManagerError::UnsafePath(path.to_path_buf()))
}

fn is_filesystem_root(path: &Path) -> bool {
    path.parent().is_none()
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut normal_depth = 0usize;
    let mut anchored = false;

    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                normalized.push(component.as_os_str());
                normal_depth = 0;
                anchored = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if normal_depth > 0 {
                    normalized.pop();
                    normal_depth -= 1;
                } else if !anchored {
                    normalized.push(component.as_os_str());
                }
            }
            Component::Normal(part) => {
                normalized.push(part);
                normal_depth += 1;
            }
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn accepts_child_path() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("hermes");
        let candidate = root.join("bin").join("rg");
        fs::create_dir_all(candidate.parent().expect("candidate should have parent"))
            .expect("candidate parent should be created");
        fs::write(&candidate, "").expect("candidate should be created");

        assert!(is_inside_root(&root, &candidate));
        ensure_safe_to_delete(&root, &candidate).expect("child should be safe");
    }

    #[test]
    fn accepts_missing_child_path() {
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("hermes");
        let candidate = root.join("bin").join("missing-rg");
        fs::create_dir_all(candidate.parent().expect("candidate should have parent"))
            .expect("candidate parent should be created");

        assert!(is_inside_root(&root, &candidate));
        ensure_safe_to_delete(&root, &candidate).expect("missing child should be safe");
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
        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("hermes");
        fs::create_dir_all(&root).expect("root should be created");

        assert!(ensure_safe_to_delete(&root, &root).is_err());
    }

    #[test]
    fn rejects_sibling_prefix_confusion() {
        let root = Path::new("/tmp/hermes");
        let candidate = Path::new("/tmp/hermes2/bin/rg");
        assert!(!is_inside_root(root, candidate));
        assert!(ensure_safe_to_delete(root, candidate).is_err());
    }

    #[test]
    fn rejects_empty_hermes_home() {
        assert!(ensure_safe_to_delete(Path::new(""), Path::new("bin/rg")).is_err());
    }

    #[test]
    fn rejects_filesystem_root_hermes_home() {
        let root = Path::new(std::path::MAIN_SEPARATOR_STR);
        let candidate = root.join("tmp");
        assert!(ensure_safe_to_delete(root, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_ancestor_escape() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("hermes");
        let outside = temp.path().join("outside");
        let link = root.join("link_to_outside");
        let candidate = link.join("file");
        fs::create_dir_all(&root).expect("root should be created");
        fs::create_dir_all(&outside).expect("outside should be created");
        fs::write(outside.join("file"), "").expect("outside file should be created");
        symlink(&outside, &link).expect("symlink should be created");

        assert!(is_inside_root(&root, &candidate));
        assert!(ensure_safe_to_delete(&root, &candidate).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_home_root_through_symlink_alias() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let real_home = temp.path().join("real-home");
        let alias_home = temp.path().join("alias-home");
        fs::create_dir_all(&real_home).expect("real home should be created");
        symlink(&real_home, &alias_home).expect("symlink should be created");

        assert!(ensure_safe_to_delete(&alias_home, &real_home).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_symlink_ancestor_escape_when_supported() {
        use std::os::windows::fs::symlink_dir;

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let root = temp.path().join("hermes");
        let outside = temp.path().join("outside");
        let link = root.join("link_to_outside");
        let candidate = link.join("file");
        fs::create_dir_all(&root).expect("root should be created");
        fs::create_dir_all(&outside).expect("outside should be created");
        fs::write(outside.join("file"), "").expect("outside file should be created");

        if symlink_dir(&outside, &link).is_err() {
            return;
        }

        assert!(is_inside_root(&root, &candidate));
        assert!(ensure_safe_to_delete(&root, &candidate).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_home_root_through_symlink_alias_when_supported() {
        use std::os::windows::fs::symlink_dir;

        let temp = tempfile::tempdir().expect("tempdir should be created");
        let real_home = temp.path().join("real-home");
        let alias_home = temp.path().join("alias-home");
        fs::create_dir_all(&real_home).expect("real home should be created");

        if symlink_dir(&real_home, &alias_home).is_err() {
            return;
        }

        assert!(ensure_safe_to_delete(&alias_home, &real_home).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_drive_relative_candidate() {
        let root = Path::new(r"C:\Users\tester\.hermes");
        let candidate = Path::new(r"C:Users\tester\.hermes\bin\rg.exe");
        assert!(!is_inside_root(root, candidate));
        assert!(ensure_safe_to_delete(root, candidate).is_err());
    }
}
