use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;

pub const AUTH_FAILED: &str = "Authentication failed. Create an API key at seer.run, then run `seer login` or set SEER_API_KEY.";

pub fn login_command(api_key: Option<String>) -> Result<()> {
    match api_key {
        Some(key) => store_api_key(&key),
        None => {
            print!("Enter your Seer API key: ");
            io::stdout().flush()?;
            store_api_key(&rpassword::read_password()?)
        }
    }
}

pub fn load_api_key() -> Result<String> {
    if let Ok(key) = std::env::var("SEER_API_KEY") {
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok(key);
        }
    }
    let path = api_key_path()?;
    let key = fs::read_to_string(&path)
        .map_err(|_| anyhow::anyhow!(AUTH_FAILED))?
        .trim()
        .to_string();
    if key.is_empty() {
        bail!(AUTH_FAILED);
    }
    Ok(key)
}

fn store_api_key(api_key: &str) -> Result<()> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        bail!(AUTH_FAILED);
    }
    let path = api_key_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, api_key)?;
    println!("API key saved to {}", path.display());
    Ok(())
}

fn api_key_path() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "seer", "seer").context("config directory")?;
    Ok(proj.config_dir().join("cli").join("api_key"))
}
