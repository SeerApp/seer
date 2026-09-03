//! Staged file commits via `<dest>.tmp` in the same directory + `rename` (no fsync / crash durability).

use std::{
    fs::{self, create_dir_all, File},
    io::Write,
    path::Path,
};

use serde::Serialize;

use super::layout::staging_path_for;

/// Stages each write as `<filename>.tmp` beside the final path, then publishes with `rename`
/// so source and target stay on the same filesystem.
///
/// Owned by [`crate::contexts::seer::SeerContext`] (shared with the disasm worker via `Arc<Mutex<_>>`).
pub struct AtomicFileWriter;

impl AtomicFileWriter {
    pub fn new() -> Self {
        Self
    }

    /// Low-level commit. Prefer [`Self::replace_bytes`] / [`Self::write_bytes_if_missing`] at call sites.
    pub(crate) fn write_bytes(&self, dest: &Path, bytes: &[u8], overwrite: bool) {
        if !overwrite && dest.exists() {
            return;
        }

        let parent = dest
            .parent()
            .expect("destination path must have a parent directory for atomic write");
        create_dir_all(parent).expect("create parent directory for atomic write");

        let staging_path = staging_path_for(dest);
        {
            let mut staging_file =
                File::create(&staging_path).expect("create staging .tmp file for atomic write");
            staging_file
                .write_all(bytes)
                .expect("write staging .tmp file");
        }

        if let Err(e) = fs::rename(&staging_path, dest) {
            let _ = fs::remove_file(&staging_path);
            panic!(
                "atomic rename {} -> {}: {e}",
                staging_path.display(),
                dest.display()
            );
        }
    }

    pub(crate) fn write_json<T: Serialize + ?Sized>(
        &self,
        dest: &Path,
        value: &T,
        overwrite: bool,
    ) {
        let json = serde_json::to_string_pretty(value).expect("serialize json for atomic write");
        self.write_bytes(dest, json.as_bytes(), overwrite);
    }

    /// Replace `dest` if it already exists.
    pub fn replace_bytes(&self, dest: &Path, bytes: &[u8]) {
        self.write_bytes(dest, bytes, true);
    }

    /// Write only when `dest` is absent (no-op otherwise).
    pub fn write_bytes_if_missing(&self, dest: &Path, bytes: &[u8]) {
        self.write_bytes(dest, bytes, false);
    }

    pub(crate) fn replace_json<T: Serialize + ?Sized>(&self, dest: &Path, value: &T) {
        self.write_json(dest, value, true);
    }

    pub(crate) fn write_json_if_missing<T: Serialize + ?Sized>(&self, dest: &Path, value: &T) {
        self.write_json(dest, value, false);
    }
}

#[cfg(test)]
mod tests {
    use super::AtomicFileWriter;
    use std::fs;

    #[test]
    fn replace_bytes_publishes_via_rename() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("artifact.json");
        AtomicFileWriter::new().replace_bytes(&dest, br#"{"ok":true}"#);
        assert_eq!(fs::read(&dest).expect("read artifact"), br#"{"ok":true}"#);
        let staging = dir.path().join("artifact.json.tmp");
        assert!(!staging.exists());
    }
}
