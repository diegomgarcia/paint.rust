use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

pub struct TestTree {
    root: PathBuf,
}

impl TestTree {
    pub fn new(label: &str) -> Self {
        let unique = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "paint-r xtask {label} {} {nanos} {unique}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create temporary test tree");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create test file parent");
        }
        fs::write(&path, contents).expect("write test file");
        path
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
