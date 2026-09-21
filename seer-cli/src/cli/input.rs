use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Context, Result};

pub(crate) const INPUT_HELP: &str = "File path, @path, or the value itself";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathOrValue(String);

impl FromStr for PathOrValue {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

impl PathOrValue {
    pub fn load_bytes(&self) -> Result<Vec<u8>> {
        let arg = self.0.as_str();
        if let Some(path) = arg.strip_prefix('@') {
            if path.is_empty() {
                bail!("empty path after @");
            }
            return std::fs::read(path).with_context(|| format!("read {path}"));
        }
        let path = Path::new(arg);
        if path.is_file() {
            return std::fs::read(path).with_context(|| format!("read {}", path.display()));
        }
        Ok(arg.as_bytes().to_vec())
    }

    pub fn load_text(&self) -> Result<String> {
        let bytes = self.load_bytes()?;
        String::from_utf8(bytes).context("input is not valid UTF-8")
    }
}
