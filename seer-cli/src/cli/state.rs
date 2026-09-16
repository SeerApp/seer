use anyhow::{Context, Result};
use serde_json::Value;

use super::input::PathOrValue;

#[derive(Clone, Debug, PartialEq)]
pub struct StateObject {
    pub json: Value,
}

impl StateObject {
    pub(crate) fn parse(input: &PathOrValue) -> Result<Self> {
        let text = input.load_text()?;
        let json = serde_json::from_str(text.trim()).context("state object JSON")?;
        Ok(Self { json })
    }
}
