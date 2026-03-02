use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use serde::Deserialize;
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use std::{collections::HashMap, path::{Path, PathBuf}};
use anyhow::Result;

#[derive(Clone)]
pub struct StorageClient {
    s3_client: S3Client,
    bucket: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct SourceInfo {
    url: String,
    commit: String,
}

#[derive(Deserialize)]
pub struct ProgramMeta {
    pub project_path: String,
    pub executable: String,
    pub artifacts: HashMap<PathBuf, String>,
    #[allow(dead_code)]
    pub source: SourceInfo,
}

impl StorageClient {
    pub fn new(
        s3_client: S3Client,
        bucket: String,
    ) -> Self {
        Self {
            s3_client,
            bucket,
        }
    }

    pub async fn get_program_meta(&self, hash: &str) -> Result<Option<ProgramMeta>, StorageError> {
        let path = program_meta_path(hash);

        let response = match self
            .s3_client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(output) => output,
            Err(e) => {
                let service_err = e.into_service_error();
                if service_err.is_no_such_key() {
                    return Ok(None);
                }
                return Err(StorageError::ProgramMetaNotFound);
            }
        };

        let bytes = response
            .body
            .collect()
            .await
            .map_err(|e| StorageError::DownloadError(e.to_string()))?
            .into_bytes();

        match serde_json::from_slice::<ProgramMeta>(&bytes) {
            Ok(meta) => Ok(Some(meta)),
            Err(_) => Ok(None),
        }
    }

    pub async fn create_program_dir(&self, meta: &ProgramMeta) -> Result<TempDir> {
        let tmp_dir = tempfile::tempdir()?;
        let root = tmp_dir.path();

        for (rel_path, hash) in &meta.artifacts {
            let rel = sanitize_relative_path(&rel_path)?;
            let dst_path = root.join(&rel);

            if let Some(parent) = dst_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            let obj = self
                .s3_client
                .get_object()
                .bucket(&self.bucket)
                .key(hash)
                .send()
                .await?;

            write_bytestream_to_path(obj.body, &dst_path).await?;
        }

        Ok(tmp_dir)
    }

    pub async fn get_program_elf(&self, hash: &str) -> Result<Vec<u8>, StorageError> {
        let key = self.artifact_path(hash);

        let response = self
            .s3_client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                let service_err = e.into_service_error();
                if service_err.is_no_such_key() {
                    StorageError::ArtifactNotFound(key)
                } else {
                    StorageError::DownloadError(format!("S3 get failed: {:?}", service_err))
                }
            })?;

        let bytes = response
            .body
            .collect()
            .await
            .map_err(|e| {
                StorageError::DownloadError(format!("Failed to read artifact body: {:?}", e))
            })?
            .into_bytes()
            .to_vec();

        Ok(bytes)
    }

    pub fn artifact_path(&self, file_hash: &str) -> String {
        format!("artifacts/{}", file_hash)
    }
}

pub(crate) fn program_meta_path(hash: &str) -> String {
    format!("programs/{}/meta.json", hash)
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Download error: {0}")]
    DownloadError(String),

    #[error("Program meta not found")]
    ProgramMetaNotFound,

    #[error("Artifact not found: {0}")]
    ArtifactNotFound(String),
}

async fn write_bytestream_to_path(body: ByteStream, dst_path: &Path) -> Result<()> {
    let mut file = tokio::fs::File::create(dst_path).await?;
    let mut stream = body.into_async_read();
    tokio::io::copy(&mut stream, &mut file).await?;
    file.flush().await?;
    Ok(())
}

fn sanitize_relative_path(p: &Path) -> Result<PathBuf> {
    use std::path::Component;

    if p.is_absolute() {
        anyhow::bail!("artifact path must be relative: {}", p.display());
    }

    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("invalid artifact path component in {}", p.display());
            }
        }
    }

    if out.as_os_str().is_empty() {
        anyhow::bail!("empty artifact path");
    }

    Ok(out)
}