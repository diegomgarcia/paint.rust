use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The operation which failed during an atomic replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomicWriteStage {
    CreateTemporary,
    Write,
    Sync,
    Rename,
    SyncDirectory,
}

/// An atomic-output error, including whether replacement had already committed.
#[derive(Debug)]
pub struct AtomicWriteError {
    stage: AtomicWriteStage,
    path: PathBuf,
    source: io::Error,
    committed: bool,
}

impl AtomicWriteError {
    fn new(stage: AtomicWriteStage, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self {
            stage,
            path: path.into(),
            source,
            committed: false,
        }
    }

    pub fn stage(&self) -> AtomicWriteStage {
        self.stage
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn committed(&self) -> bool {
        self.committed
    }
}

impl fmt::Display for AtomicWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "atomic output failed during {:?} for {}: {}",
            self.stage,
            self.path.display(),
            self.source
        )?;
        if self.committed {
            f.write_str(" (the replacement had already committed)")?;
        }
        Ok(())
    }
}

impl std::error::Error for AtomicWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomicFault {
    Write,
    Sync,
    Rename,
}

/// Replaces a file by writing and syncing a unique temporary in the same directory.
///
/// A successful rename is the atomic visibility transition. This function does not
/// claim hardware durability, which depends on the filesystem and storage device.
pub fn atomic_write(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), AtomicWriteError> {
    atomic_write_with_fault(path.as_ref(), bytes, None)
}

fn atomic_write_with_fault(
    path: &Path,
    bytes: &[u8],
    fault: Option<AtomicFault>,
) -> Result<(), AtomicWriteError> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
    const MAX_COLLISIONS: usize = 128;

    let file_name = path.file_name().ok_or_else(|| {
        AtomicWriteError::new(
            AtomicWriteStage::CreateTemporary,
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "output must name a file"),
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));

    let mut temporary = None;
    for _ in 0..MAX_COLLISIONS {
        let nonce = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), nonce));
        let temporary_path = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
        {
            Ok(file) => {
                temporary = Some((temporary_path, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(AtomicWriteError::new(
                    AtomicWriteStage::CreateTemporary,
                    temporary_path,
                    error,
                ));
            }
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        AtomicWriteError::new(
            AtomicWriteStage::CreateTemporary,
            path,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate a unique temporary file",
            ),
        )
    })?;
    let mut guard = TemporaryGuard {
        path: temporary_path.clone(),
        armed: true,
    };

    file.write_all(bytes)
        .map_err(|error| AtomicWriteError::new(AtomicWriteStage::Write, &temporary_path, error))?;
    if fault == Some(AtomicFault::Write) {
        return Err(injected_fault(AtomicWriteStage::Write, &temporary_path));
    }
    file.sync_all()
        .map_err(|error| AtomicWriteError::new(AtomicWriteStage::Sync, &temporary_path, error))?;
    if fault == Some(AtomicFault::Sync) {
        return Err(injected_fault(AtomicWriteStage::Sync, &temporary_path));
    }
    if fault == Some(AtomicFault::Rename) {
        return Err(injected_fault(AtomicWriteStage::Rename, &temporary_path));
    }
    fs::rename(&temporary_path, path)
        .map_err(|error| AtomicWriteError::new(AtomicWriteStage::Rename, &temporary_path, error))?;
    guard.armed = false;

    #[cfg(unix)]
    {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| AtomicWriteError {
                stage: AtomicWriteStage::SyncDirectory,
                path: parent.to_path_buf(),
                source: error,
                committed: true,
            })?;
    }

    Ok(())
}

fn injected_fault(stage: AtomicWriteStage, path: &Path) -> AtomicWriteError {
    AtomicWriteError::new(
        stage,
        path,
        io::Error::other("injected atomic-output test fault"),
    )
}

struct TemporaryGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TemporaryGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_files(directory: &Path) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(directory)
            .expect("read temporary directory")
            .map(|entry| entry.expect("directory entry").path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn atomic_write_creates_exact_contents_without_temp_residue() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");

        atomic_write(&output, b"first").expect("initial atomic write");

        assert_eq!(fs::read(&output).expect("read output"), b"first");
        assert_eq!(temp_files(directory.path()), vec![output]);
    }

    #[test]
    fn atomic_write_replaces_existing_file_exactly() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");
        fs::write(&output, b"old contents which are longer").expect("seed output");

        atomic_write(&output, b"new").expect("replacement atomic write");

        assert_eq!(fs::read(&output).expect("read replacement"), b"new");
        assert_eq!(temp_files(directory.path()), vec![output]);
    }

    #[test]
    fn simulated_write_failure_preserves_prior_file_and_cleans_temp() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");
        fs::write(&output, b"prior").expect("seed output");

        let error = atomic_write_with_fault(&output, b"new", Some(AtomicFault::Write))
            .expect_err("injected write must fail");

        assert_eq!(error.stage(), AtomicWriteStage::Write);
        assert!(!error.committed());
        assert_eq!(fs::read(&output).expect("read prior output"), b"prior");
        assert_eq!(temp_files(directory.path()), vec![output]);
    }

    #[test]
    fn simulated_sync_failure_preserves_prior_file_and_cleans_temp() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");
        fs::write(&output, b"prior").expect("seed output");

        let error = atomic_write_with_fault(&output, b"new", Some(AtomicFault::Sync))
            .expect_err("injected sync must fail");

        assert_eq!(error.stage(), AtomicWriteStage::Sync);
        assert!(!error.committed());
        assert_eq!(fs::read(&output).expect("read prior output"), b"prior");
        assert_eq!(temp_files(directory.path()), vec![output]);
    }

    #[test]
    fn simulated_rename_failure_preserves_prior_file_and_cleans_temp() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("image.png");
        fs::write(&output, b"prior").expect("seed output");

        let error = atomic_write_with_fault(&output, b"new", Some(AtomicFault::Rename))
            .expect_err("injected rename must fail");

        assert_eq!(error.stage(), AtomicWriteStage::Rename);
        assert!(!error.committed());
        assert_eq!(fs::read(&output).expect("read prior output"), b"prior");
        assert_eq!(temp_files(directory.path()), vec![output]);
    }

    #[test]
    fn failed_initial_write_does_not_leave_an_output_file() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("new.png");

        atomic_write_with_fault(&output, b"new", Some(AtomicFault::Write))
            .expect_err("injected write must fail");

        assert!(!output.exists());
        assert!(temp_files(directory.path()).is_empty());
    }

    #[test]
    fn missing_parent_is_a_typed_create_error() {
        let directory = TempDir::new().expect("temporary directory");
        let output = directory.path().join("missing").join("image.png");

        let error = atomic_write(&output, b"data").expect_err("missing parent must fail");

        assert_eq!(error.stage(), AtomicWriteStage::CreateTemporary);
        assert!(!error.committed());
        assert!(!output.exists());
    }
}
