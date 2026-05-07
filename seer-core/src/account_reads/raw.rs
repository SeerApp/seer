use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RawAccountReadKind {
    Key {
        offset: usize,
        #[serde(default)]
        bytes_width: usize,
        bytes: Vec<u8>,
    },
    Owner {
        offset: usize,
        #[serde(default)]
        bytes_width: usize,
        bytes: Vec<u8>,
    },
    Lamports {
        lamports: u64,
    },
    DataLen {
        len: u64,
    },
    RentEpoch {
        rent_epoch: u64,
    },
    Executable {
        byte: u8,
    },
    Data {
        offset: usize,
        #[serde(default)]
        bytes_width: usize,
        #[serde(default)]
        bytes: Vec<u8>,
    },
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawAccountRead {
    #[serde(default)]
    pub step_order: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub read_kind: RawAccountReadKind,
    #[serde(skip_serializing, skip_deserializing, default)]
    pub owner: Pubkey,
}

impl PartialEq for RawAccountRead {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.step_order == other.step_order
            && self.read_kind == other.read_kind
    }
}
