use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_pubkey::Pubkey;

use crate::idl::{parsed_arg::ParsedArgByteOffset, types::ParsedAccount};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewDataRead {
    pub offset: usize,
    pub bytes_width: usize,
    pub step_order: u64,
    pub step_order_end: u64,
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ViewAccountReadKind {
    ReadKey {
        step_order: u64,
        step_order_end: u64,
    },
    ReadOwner {
        #[serde_as(as = "DisplayFromStr")]
        owner: Pubkey,
        step_order: u64,
        step_order_end: u64,
    },
    ReadLamports {
        lamports: u64,
        step_order: u64,
    },
    ReadDataLen {
        len: u64,
        step_order: u64,
    },
    ReadData {
        bytes: Vec<u8>,
        reads: Vec<ViewDataRead>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parsed: Option<ParsedAccount>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        parsed_byte_offsets: Vec<ParsedArgByteOffset>,
    },
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ViewAccountRead {
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub read_kind: ViewAccountReadKind,
}

impl ViewAccountRead {
    pub fn view_read_start_step(&self) -> u64 {
        match &self.read_kind {
            ViewAccountReadKind::ReadKey { step_order, .. } => *step_order,
            ViewAccountReadKind::ReadOwner { step_order, .. } => *step_order,
            ViewAccountReadKind::ReadLamports { step_order, .. } => *step_order,
            ViewAccountReadKind::ReadDataLen { step_order, .. } => *step_order,
            ViewAccountReadKind::ReadData { reads, .. } => {
                reads.iter().map(|r| r.step_order).min().unwrap_or(0)
            }
        }
    }
}
