use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

use crate::idl::{types::ParsedAccount, IdlTreeParser};

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeAccount {
    #[serde(default)]
    pub step_order: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub before: AccountSharedDataWrapper,
    pub after: AccountSharedDataWrapper,
}

impl PartialEq for TreeAccount {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.before == other.before && self.after == other.after
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AccountSharedDataWrapper {
    lamports: u64,
    data: Vec<u8>,
    #[serde_as(as = "DisplayFromStr")]
    owner: Pubkey,
    executable: bool,
    rent_epoch: Epoch,
    parsed: Option<ParsedAccount>,
}

impl AccountSharedDataWrapper {
    pub fn lamports(&self) -> u64 {
        self.lamports
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn owner(&self) -> Pubkey {
        self.owner
    }

    pub fn executable(&self) -> bool {
        self.executable
    }

    pub fn rent_epoch(&self) -> Epoch {
        self.rent_epoch
    }

    pub fn set_parsed(&mut self, parsed: Option<ParsedAccount>) {
        self.parsed = parsed;
    }
}

impl From<AccountSharedData> for AccountSharedDataWrapper {
    fn from(value: AccountSharedData) -> Self {
        AccountSharedDataWrapper {
            lamports: value.lamports(),
            data: value.data().to_vec(),
            owner: *value.owner(),
            executable: value.executable(),
            rent_epoch: value.rent_epoch(),
            parsed: None,
        }
    }
}

impl TreeAccount {
    pub fn parse<T: IdlTreeParser>(&mut self, parser: &T) {
        self.before
            .set_parsed(parser.get_account(self.before.data()));
        self.after.set_parsed(parser.get_account(self.after.data()));
    }
}
