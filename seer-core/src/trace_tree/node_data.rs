use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

use crate::{
    source_die_trace::{SourceDie, SourceDieType},
    trace_tree::loc::Loc,
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum NodeData {
    // branch
    Invoke(InvokeData),
    Entrypoint(EntrypointData),
    FnCall(FnCallData),
    // leaf
    Log(LogData),
    Line(LineData),
    Error(ErrorData),
    Account(AccountData),
}

impl NodeData {
    pub fn from_first(source: SourceDie) -> Option<Self> {
        match source.source_type {
            SourceDieType::Fn => source
                .loc
                .decl
                .map(|d| {
                    Some(NodeData::Entrypoint(EntrypointData {
                        signature: source.loc.signature,
                        loc: d,
                    }))
                })
                .unwrap_or(None),
            SourceDieType::Var => None,
        }
    }

    pub fn from(source: SourceDie) -> Option<Self> {
        match source.source_type {
            SourceDieType::Fn => {
                source.loc.call.map(|c| {
                    Some(NodeData::FnCall(FnCallData {
                        signature: source.loc.signature,
                        loc: c,
                    }))
                })
            }
            .unwrap_or(None),
            SourceDieType::Var => source
                .loc
                .decl
                .map(|d| Some(NodeData::Line(LineData { loc: d })))
                .unwrap_or(None),
        }
    }

    pub fn is_leaf(&self) -> bool {
        matches!(
            self,
            NodeData::Log(_) | NodeData::Line(_) | NodeData::Account(_)
        )
    }

    pub fn is_branch(&self) -> bool {
        matches!(
            self,
            NodeData::Invoke(_) | NodeData::Entrypoint(_) | NodeData::FnCall(_)
        )
    }

    pub fn is_invoke_parent(&self) -> bool {
        matches!(
            self,
            NodeData::Invoke(_) | NodeData::Entrypoint(_) | NodeData::Line(_)
        )
    }
}

// branch

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct InvokeData {
    #[serde_as(as = "DisplayFromStr")]
    sender: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    receiver: Pubkey,
}

impl InvokeData {
    pub fn new(sender: Pubkey, receiver: Pubkey) -> Self {
        Self { sender, receiver }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EntrypointData {
    // corresponds to first trace DIE with decl loc
    pub signature: String,
    pub loc: Loc,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FnCallData {
    // corresponds to subsequent trace DIE with call locs
    pub signature: String,
    pub loc: Loc,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LineData {
    pub loc: Loc,
}

// leaf

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct LogData {
    msg: String,
}

impl LogData {
    pub fn new(msg: &str) -> Self {
        Self {
            msg: msg.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ErrorData {
    msg: String,
}

impl ErrorData {
    pub fn new(msg: String) -> Self {
        Self { msg }
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct AccountData {
    #[serde_as(as = "DisplayFromStr")]
    pub key: Pubkey,
    pub before: AccountSharedDataWrapper,
    pub after: AccountSharedDataWrapper,
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
}

impl From<AccountSharedData> for AccountSharedDataWrapper {
    fn from(value: AccountSharedData) -> Self {
        AccountSharedDataWrapper {
            lamports: value.lamports(),
            data: value.data().to_vec(),
            owner: *value.owner(),
            executable: value.executable(),
            rent_epoch: value.rent_epoch(),
        }
    }
}
