use seer_interface::GuestAccountBackdoor;
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::{step_mirror::UnsafeAccountBackdoor, tree::nodes::account::TreeAccount};

pub struct GlobalAccountContext {
    unsafe_account_backdoor: Option<UnsafeAccountBackdoor>,
}

impl GlobalAccountContext {
    pub fn new() -> Self {
        Self {
            unsafe_account_backdoor: None,
        }
    }

    pub fn open_account_backdoor_idempotent(&mut self, bd: &dyn GuestAccountBackdoor) {
        if self.unsafe_account_backdoor.is_none() {
            let uab = unsafe { UnsafeAccountBackdoor::new(bd) };
            self.unsafe_account_backdoor = Some(uab);
        }
    }

    pub fn close_account_backdoor_idempotent(&mut self) {
        if let Some(mut uab) = self.unsafe_account_backdoor.take() {
            uab.clear();
        }
    }

    pub fn live_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        self.unsafe_account_backdoor
            .as_ref()
            .map(UnsafeAccountBackdoor::live_accounts)
            .unwrap_or_default()
    }

    pub fn get_changed_accounts(&mut self, step_order: u64) -> Vec<TreeAccount> {
        if let Some(uab) = self.unsafe_account_backdoor.as_mut() {
            uab.check_diffs(step_order)
        } else {
            vec![]
        }
    }
}
