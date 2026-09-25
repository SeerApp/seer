mod blobs;
mod db;
mod home;

pub use blobs::Blob;
pub use db::{Db, ProgramChunk, RunRow};
pub use home::default_root;

use anyhow::Result;
use std::path::Path;

pub struct Storage {
    pub blob: Blob,
    pub db: Db,
}

impl Storage {
    pub fn open_at(root: impl AsRef<Path>) -> Result<Self> {
        let root = home::ensure(root.as_ref().to_path_buf())?;
        Ok(Self {
            blob: Blob::open(root.join("blob"))?,
            db: Db::open(root.join("db"))?,
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
    fn blob_and_run_walk() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let hash = storage.blob.store(b"hello").unwrap();
        assert_eq!(storage.blob.store(b"hello").unwrap(), hash);
        storage.db.insert_simulation(&hash, &hash).unwrap();
        let id1 = storage
            .db
            .insert_run(&hash, &hash, "{}", None, "[]", "")
            .unwrap();
        storage.db.finish_run(id1, None).unwrap();
        let id2 = storage
            .db
            .insert_run(&hash, &hash, r#"{"slot":1}"#, Some(id1), "[]", "from:1")
            .unwrap();
        storage.db.finish_run(id2, Some("boom")).unwrap();

        let runs = storage.db.list_runs().unwrap();
        let mut rows = runs.iter();
        let a = rows.next().unwrap();
        assert_eq!(a.id, id1);
        assert_eq!(a.environment, "{}");
        assert_eq!(a.parent_id, None);
        let b = rows.next().unwrap();
        assert_eq!(b.id, id2);
        assert_eq!(b.parent_id, Some(id1));
        assert_eq!(b.error.as_deref(), Some("boom"));
        assert!(rows.next().is_none());
        assert_eq!(
            storage.db.get_run(id2).unwrap().error.as_deref(),
            Some("boom")
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn empty_has_no_runs() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        assert!(storage.db.list_runs().unwrap().is_empty());
        assert!(storage.db.get_run(1).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn query_simulation() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let hash = storage.blob.store(b"tx").unwrap();
        storage.db.insert_simulation(&hash, &hash).unwrap();
        let rows = storage
            .db
            .query("SELECT COUNT(*) AS n FROM simulation")
            .unwrap();
        assert!(rows.contains("\"n\": 1"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn run_row_and_sig_lookup() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let hash = storage.blob.store(b"tx").unwrap();
        let sig = [7u8; 64];
        let network = "mainnet";
        assert!(storage
            .db
            .insert_historical(&hash, &hash, network, r#"{"slot":9}"#, &sig)
            .is_err());
        storage.db.insert_simulation(&hash, &hash).unwrap();
        storage
            .db
            .insert_historical(&hash, &hash, network, r#"{"slot":9}"#, &sig)
            .unwrap();
        storage
            .db
            .insert_historical(&hash, &hash, network, r#"{"slot":9}"#, &sig)
            .unwrap();
        let id = storage
            .db
            .insert_run(&hash, &hash, "{}", None, "[]", "sig:x")
            .unwrap();
        storage.db.finish_run(id, None).unwrap();
        let child = storage
            .db
            .insert_run(
                &hash,
                &hash,
                r#"{"slot":1}"#,
                Some(id),
                r#"[{"account":"11111111111111111111111111111111","lamports":0}]"#,
                "from:1",
            )
            .unwrap();
        storage.db.finish_run(child, Some("boom")).unwrap();
        let got = storage.db.get_run(child).unwrap();
        assert_eq!(got.parent_id, Some(id));
        assert_eq!(got.source, "from:1");
        assert_eq!(got.error.as_deref(), Some("boom"));
        assert_eq!(storage.db.list_runs().unwrap().len(), 2);
        assert_eq!(
            storage.db.lookup_sig(&sig, Some(network)).unwrap(),
            Some((hash, hash, r#"{"slot":9}"#.into()))
        );
        assert_eq!(
            storage.db.lookup_sig(&sig, None).unwrap(),
            Some((hash, hash, r#"{"slot":9}"#.into()))
        );
        assert!(storage
            .db
            .lookup_sig(&sig, Some("devnet"))
            .unwrap()
            .is_none());
        assert!(storage.db.lookup_sig(&[8u8; 64], None).unwrap().is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn program_chunk_rows_are_transactional_and_lazy() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let elf = storage.blob.store(b"elf").unwrap();
        storage.db.insert_program(&elf).unwrap();
        let a = storage.blob.store(b"chunk-a").unwrap();
        let b = storage.blob.store(b"chunk-b").unwrap();
        storage
            .db
            .insert_program_disasm(
                &elf,
                &[
                    crate::ProgramChunk {
                        start_pc: 64,
                        end_pc: 8072,
                        blob_hash: a,
                    },
                    crate::ProgramChunk {
                        start_pc: 8080,
                        end_pc: 16000,
                        blob_hash: b,
                    },
                ],
            )
            .unwrap();
        storage
            .db
            .insert_program_disasm(
                &elf,
                &[crate::ProgramChunk {
                    start_pc: 0,
                    end_pc: 1,
                    blob_hash: a,
                }],
            )
            .unwrap();
        let rows = storage.db.program_disasm_chunks(&elf).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].start_pc, 64);
        assert_eq!(rows[1].start_pc, 8080);
        assert!(storage.db.program_lifted_chunks(&elf).unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn glassbox_upsert_replaces_hash() {
        let root = tmp();
        let storage = Storage::open_at(&root).unwrap();
        let hash = storage.blob.store(b"tx").unwrap();
        storage.db.insert_simulation(&hash, &hash).unwrap();
        let id = storage
            .db
            .insert_run(&hash, &hash, "{}", None, "[]", "")
            .unwrap();
        storage.db.insert_run_ix(id, 0).unwrap();
        let a = storage.blob.store(b"report-a").unwrap();
        let b = storage.blob.store(b"report-b").unwrap();
        storage.db.upsert_glassbox(id, 0, &a).unwrap();
        assert_eq!(storage.db.glassbox_blob_hash(id, 0).unwrap(), Some(a));
        storage.db.upsert_glassbox(id, 0, &b).unwrap();
        assert_eq!(storage.db.glassbox_blob_hash(id, 0).unwrap(), Some(b));
        std::fs::remove_dir_all(root).ok();
    }
}
