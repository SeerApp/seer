use anyhow::{anyhow, Context, Result};

use super::input::PathOrValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha256Hash(pub [u8; 32]);

impl Sha256Hash {
    pub(crate) fn parse(input: &PathOrValue) -> Result<Self> {
        let text = input.load_text()?;
        let bytes = hex::decode(text.trim()).context("sha256 hash must be hex")?;
        let hash: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("sha256 hash must be 32 bytes (64 hex chars)"))?;
        Ok(Self(hash))
    }
}
