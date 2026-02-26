pub mod config;
mod storage;

use std::{collections::HashMap};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use solana_pubkey::Pubkey;
use tempfile::TempDir;

use crate::config::Config;
use crate::storage::{StorageClient};

pub struct ExternalProgramService {
    external_programs: HashMap<Pubkey, String>,
    storage: StorageClient,
}

impl ExternalProgramService {
    pub async fn new(config: Config) -> Self {
        dotenvy::dotenv().ok();

        let aws_config_loader = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_config::Region::new(config.s3_region.clone()))
        .endpoint_url(config.s3_endpoint_url.clone());

        let aws_config = aws_config_loader.load().await;
        let s3_config = aws_sdk_s3::config::Builder::from(&aws_config)
            .force_path_style(config.s3_force_path_style)
            .build();
        let s3_client = S3Client::from_conf(s3_config);
        let storage = StorageClient::new(
            s3_client,
            config.s3_bucket.clone(),
        );

        Self {
            external_programs: HashMap::new(),
            storage,
        }
    }

    pub async fn get_account(&self, pubkey: &Pubkey) -> Result<Option<(Vec<u8>, TempDir, String)>, anyhow::Error> {
        let Some(hash) = self.external_programs.get(pubkey) else {
            return Ok(None);
        };
    
        let Some(meta) = self.storage.get_program_meta(hash).await? else {
            return Ok(None);
        };

        let tmp_dir = self.storage.create_program_dir(&meta).await?;
        let elf = self.storage.get_program_elf(&meta.executable).await?;
    
        Ok(Some((elf, tmp_dir, meta.project_path)))
    } 
}