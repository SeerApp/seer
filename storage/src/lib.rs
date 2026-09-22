mod blobs;
mod db;
mod home;

pub use blobs::Blob;
pub use db::Db;
pub use home::default_root;

use anyhow::Result;
use std::path::Path;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Default,
    Compare,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Default => "default",
            Self::Compare => "compare",
        })
    }
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "default" => Ok(Self::Default),
            "compare" => Ok(Self::Compare),
            other => anyhow::bail!("storage mode must be default or compare, got {other}"),
        }
    }
}

pub struct Storage {
    pub blob: Blob,
    pub db: Db,
}

impl Storage {
    pub fn open_at(root: impl AsRef<Path>, mode: Mode) -> Result<Self> {
        let root = home::ensure(root.as_ref().to_path_buf())?;
        Ok(Self {
            blob: Blob::open(root.join("blob"), mode)?,
            db: Db::open(root.join("db"), mode)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "seer-storage-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn compare_blob_and_only_run() {
        let root = tmp();
        let (hash, id) = {
            let storage = Storage::open_at(&root, Mode::Default).unwrap();
            let hash = storage.blob.store(b"hello").unwrap();
            storage.db.insert_simulation(&hash, &hash).unwrap();
            let id = storage.db.insert_run(&hash, &hash, "{}").unwrap();
            storage.db.finish_run(id, None).unwrap();
            (hash, id)
        };
        let storage = Storage::open_at(&root, Mode::Compare).unwrap();
        assert_eq!(storage.blob.store(b"hello").unwrap(), hash);
        assert!(storage.blob.store(b"hello!").is_err());
        assert_eq!(storage.db.insert_run(&hash, &hash, "{}").unwrap(), id);
        assert!(storage
            .db
            .insert_run(&hash, &hash, r#"{"slot":1}"#)
            .is_err());
        storage.db.finish_run(id, None).unwrap();
        assert!(storage.db.finish_run(id, Some("nope")).is_err());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn compare_rejects_wrong_run_count() {
        let root = tmp();
        {
            let storage = Storage::open_at(&root, Mode::Compare).unwrap();
            assert!(storage.db.insert_run(&[1; 32], &[2; 32], "{}").is_err());
        }
        {
            let storage = Storage::open_at(&root, Mode::Default).unwrap();
            storage.db.insert_simulation(&[1; 32], &[2; 32]).unwrap();
            storage.db.insert_run(&[1; 32], &[2; 32], "{}").unwrap();
            storage.db.insert_run(&[1; 32], &[2; 32], "{}").unwrap();
        }
        let storage = Storage::open_at(&root, Mode::Compare).unwrap();
        assert!(storage.db.insert_run(&[1; 32], &[2; 32], "{}").is_err());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn print_state_and_query() {
        let root = tmp();
        let storage = Storage::open_at(&root, Mode::Default).unwrap();
        let hash = storage
            .blob
            .store(br#"{"11111111111111111111111111111111":{"lamports":"1","data":"00","owner":"11111111111111111111111111111111","executable":false}}"#)
            .unwrap();
        let printed = storage.blob.print_state(&hash).unwrap();
        assert!(printed.contains("11111111111111111111111111111111"));
        storage.db.insert_simulation(&hash, &hash).unwrap();
        let rows = storage
            .db
            .query("SELECT COUNT(*) AS n FROM simulation")
            .unwrap();
        assert!(rows.contains("\"n\": 1"));
        std::fs::remove_dir_all(root).ok();
    }
}
