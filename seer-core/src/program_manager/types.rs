//! Purpose: define program manager structural data types.

use std::{
    collections::HashMap,
    sync::{mpsc, Arc, Mutex},
};

use solana_pubkey::Pubkey;

use crate::entrypoint_lookup::EntrypointLookup;

pub struct ProgramInfo {
    pub(super) entrypoint_lookup: Option<Arc<EntrypointLookup>>,
}

impl ProgramInfo {
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(entrypoint_lookup: Option<EntrypointLookup>) -> Self {
        Self {
            entrypoint_lookup: entrypoint_lookup.map(Arc::new),
        }
    }
}

/// Collects ProgramInfo from local targets at Seer initialization.
pub struct GlobalProgramContext {
    pub(super) inner: Mutex<HashMap<Pubkey, ProgramInfo>>,
    pub(super) disasm_requests_tx: mpsc::Sender<Pubkey>,
    pub(super) disasm_status: Arc<Mutex<HashMap<Pubkey, DisasmStatus>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DisasmStatus {
    Pending,
    Succeeded,
    Failed,
}
