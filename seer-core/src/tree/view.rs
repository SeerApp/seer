use core::panic;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use solana_account::{AccountSharedData, ReadableAccount};
use solana_program::clock::Epoch;
use solana_pubkey::Pubkey;

use crate::tree::{TreeNode, loc::Loc};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum ViewNode {
    // branch
    Invoke(InvokeData),
    Entrypoint(EntrypointData),
    FnCall(FnCallData),
    // leaf
    Log(LogData),
    Error(ErrorData),
    Account(AccountData),
}

impl TreeNode for ViewNode {
    fn root() -> Self {
        panic!("Must never call root on ViewNode");
    }

    fn is_leaf(&self) -> bool {
        panic!("Must never call is_leaf on ViewNode");
    }

    fn is_fn_call(&self) -> bool {
        panic!("Must never call is_fn_call on ViewNode");
    }

    fn can_push_to(&self, _: &super::Tree<Self>) -> bool {
        panic!("Must never call can_push_to on ViewNode");
    }
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct InvokeData {
    #[serde_as(as = "DisplayFromStr")]
    pub sender: Pubkey,
    #[serde_as(as = "DisplayFromStr")]
    pub receiver: Pubkey,
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

