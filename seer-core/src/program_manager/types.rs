//! Purpose: define program manager structural data types.

use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
};

use solana_pubkey::Pubkey;

use crate::{entrypoint_lookup::EntrypointLookup, idl::IdlLookup};

pub struct ProgramInfo {
    pub(super) entrypoint_lookup: Option<Arc<EntrypointLookup>>,
    pub(super) idl_lookup: Option<Arc<IdlLookup>>,
}

impl ProgramInfo {
    pub fn new(entrypoint_lookup: Option<EntrypointLookup>, idl_lookup: Option<IdlLookup>) -> Self {
        Self {
            entrypoint_lookup: entrypoint_lookup.map(Arc::new),
            idl_lookup: idl_lookup.map(Arc::new),
        }
    }

    pub fn with_idl_lookup(idl_lookup: IdlLookup) -> Self {
        Self {
            entrypoint_lookup: None,
            idl_lookup: Some(Arc::new(idl_lookup)),
        }
    }

    pub fn set_idl_lookup(&mut self, idl_lookup: IdlLookup) {
        self.idl_lookup = Some(Arc::new(idl_lookup));
    }
}

/// Collects ProgramInfo from 4 sources:
/// 1. Known hardcoded (immutable) program IDLs at Seer initialization.
/// 2. The targets provided by the user at Seer initialization.
/// 3. External programs from the Seer storage lazily during execution.
/// 4. Mainnet programs (IDLs only) from the Solana RPC as fallback lazily during execution.
pub struct GlobalProgramContext {
    pub(super) inner: Mutex<HashMap<Pubkey, ProgramInfo>>,
    pub(super) disasm_requests_tx: mpsc::Sender<Pubkey>,
    pub(super) disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
    pub(super) network_rpc_url: Option<String>,
    pub(super) idl_fetch_failed: Mutex<HashMap<Pubkey, ()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DisasmStatus {
    Pending,
    Succeeded,
    Failed,
}

pub(super) struct RpcAccountInfo {
    pub(super) owner: String,
    pub(super) executable: bool,
    pub(super) data: Vec<u8>,
}

