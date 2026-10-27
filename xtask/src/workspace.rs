use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// A canonicalized Cargo workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Validate an explicit workspace directory without searching its parents.
    pub fn at(path: &Path) -> Result<Self> {
        let root = fs::canonicalize(path).map_err(|error| {
            Error::new(format!(
                "cannot open workspace '{}': {error}",
                path.display()
            ))
        })?;
        if !root.is_dir() {
            return Err(Error::new(format!(
                "workspace '{}' is not a directory",
                root.display()
            )));
        }
        let manifest = root.join("Cargo.toml");
        let source = fs::read_to_string(&manifest).map_err(|error| {
            Error::new(format!(
                "cannot read workspace manifest '{}': {error}",
                manifest.display()
            ))
        })?;
        if !manifest_declares_workspace(&source) {
            return Err(Error::new(format!(
                "'{}' does not declare a [workspace]",
                manifest.display()
            )));
        }
        Ok(Self { root })
    }

    /// Find the nearest ancestor with a Cargo workspace manifest.
    pub fn discover_from(start: &Path) -> Result<Self> {
        let canonical = fs::canonicalize(start).map_err(|error| {
            Error::new(format!(
                "cannot begin workspace discovery at '{}': {error}",
                start.display()
            ))
        })?;
        let mut current = if canonical.is_file() {
            canonical.parent().map(Path::to_path_buf).ok_or_else(|| {
                Error::new(format!(
                    "'{}' has no parent directory for workspace discovery",
                    canonical.display()
                ))
            })?
        } else {
            canonical
        };

        loop {
            let manifest = current.join("Cargo.toml");
            if manifest.is_file() {
                let source = fs::read_to_string(&manifest).map_err(|error| {
                    Error::new(format!(
                        "cannot read candidate workspace manifest '{}': {error}",
                        manifest.display()
                    ))
                })?;
                if manifest_declares_workspace(&source) {
                    return Self::at(&current);
                }
            }
            if !current.pop() {
                break;
            }
        }

        Err(Error::new(format!(
            "no Cargo [workspace] found in '{}' or its ancestors",
            start.display()
        )))
    }

    /// Discover from the process directory, then from the compiled xtask manifest.
    pub fn discover() -> Result<Self> {
        let current = std::env::current_dir()
            .map_err(|error| Error::new(format!("cannot read current directory: {error}")))?;
        match Self::discover_from(&current) {
            Ok(workspace) => Ok(workspace),
            Err(current_error) => {
                let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
                let compiled_root = manifest_directory.parent().ok_or_else(|| {
                    Error::new("compiled xtask manifest path has no workspace parent")
                })?;
                Self::at(compiled_root).map_err(|compiled_error| {
                    Error::new(format!(
                        "{current_error}; compiled-workspace fallback also failed: {compiled_error}"
                    ))
                })
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn manifest_declares_workspace(source: &str) -> bool {
    source.lines().any(|line| {
        let without_comment = line.split('#').next().unwrap_or(line);
        without_comment.trim() == "[workspace]"
    })
}

#[cfg(test)]
mod tests {
    use super::manifest_declares_workspace;

    #[test]
    fn workspace_manifest_detection_ignores_comments_and_other_sections() {
        assert!(manifest_declares_workspace(
            "members = []\n[workspace] # root\n"
        ));
        assert!(!manifest_declares_workspace(
            "# [workspace]\n[workspace.package]\nversion = '1'\n"
        ));
        assert!(!manifest_declares_workspace(
            "[package]\nname = 'example'\n"
        ));
    }
}
