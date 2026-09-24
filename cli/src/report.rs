use std::fmt;

use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Report {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_accounts: Vec<Pubkey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_data: Vec<Pubkey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoherent: Vec<Pubkey>,
}

impl Report {
    pub fn merge(&mut self, other: Self) {
        self.missing_accounts.extend(other.missing_accounts);
        self.missing_data.extend(other.missing_data);
        self.incoherent.extend(other.incoherent);
    }

    pub fn is_empty(&self) -> bool {
        self.missing_accounts.is_empty()
            && self.missing_data.is_empty()
            && self.incoherent.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string_pretty(self) {
            Ok(s) => f.write_str(&s),
            Err(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Report {}
