//! Staged file commits via temp write + same-directory `rename` (no fsync / crash durability).

use std::{
    fs::{self, create_dir_all},
    io::Write,
    path::Path,
};

use serde::Serialize;

/// Stages each write as a hidden temp file in the destination's parent directory, then publishes
/// with `rename` so source and target stay on the same filesystem.
///
/// Owned by [`crate::contexts::seer::SeerContext`] (shared with the disasm worker via `Arc<Mutex<_>>`).
pub struct AtomicFileWriter;

impl AtomicFileWriter {
    pub fn new() -> Self {
        Self
    }

    pub fn write_bytes(&self, dest: &Path, bytes: &[u8], overwrite: bool) {
        if !overwrite && dest.exists() {
            return;
        }

        let parent = dest
            .parent()
            .expect("destination path must have a parent directory for atomic write");
        create_dir_all(parent).expect("create parent directory for atomic write");

        let mut staging_file = tempfile::Builder::new()
            .prefix(".seer-write-")
            .tempfile_in(parent)
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
