use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_endpoint_url: String,
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub s3_force_path_style: bool,
}

impl Config {
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        match envy::from_env::<Self>() {
            Ok(cfg) => Ok(Some(cfg)),
            Err(envy::Error::MissingValue(_)) => Ok(None),
            Err(e) => Err(ConfigError::from(e)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read environment configuration: {0}")]
    Envy(#[from] envy::Error),
}