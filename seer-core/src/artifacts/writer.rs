//! Staged file commits via temp write + `rename` (no fsync / crash durability).

use std::{
    fs::{self, create_dir_all},
    io::Write,
    path::Path,
};

use serde::Serialize;
use tempfile::TempDir;

/// Stages each write under a process-local temp directory, then publishes with `rename`.
///
/// Owned by [`crate::contexts::seer::SeerContext`] (shared with the disasm worker via `Arc<Mutex<_>>`).
pub struct AtomicFileWriter {
    staging_dir: TempDir,
}

impl AtomicFileWriter {
    pub fn new() -> Self {
        let staging_dir = tempfile::Builder::new()
            .prefix(".seer-atomic-writes-")
            .tempdir_in(std::env::temp_dir())
            .expect("create seer atomic write staging directory");
        Self { staging_dir }
    }

    pub fn write_bytes(&self, dest: &Path, bytes: &[u8], overwrite: bool) {
        if !overwrite && dest.exists() {
            return;
        }
        if let Some(parent) = dest.parent() {
            create_dir_all(parent).expect("create parent directory for atomic write");
        }

        let mut staging_file = tempfile::Builder::new()
            .prefix(".seer-write-")
            .tempfile_in(self.staging_dir.path())
            .expect("create staging temp file for atomic write");
        staging_file
            .write_all(bytes)
            .expect("write staging temp file");
        let staging_path = staging_file.into_temp_path();

        if let Err(e) = fs::rename(&staging_path, dest) {
            let _ = fs::remove_file(&staging_path);
            panic!(
                "atomic rename {} -> {}: {e}",
                staging_path.display(),
                dest.display()
            );
        }
    }

    pub fn write_json<T: Serialize + ?Sized>(&self, dest: &Path, value: &T, overwrite: bool) {
        let json = serde_json::to_string_pretty(value).expect("serialize json for atomic write");
        self.write_bytes(dest, json.as_bytes(), overwrite);
    }
}
