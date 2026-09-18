use seer_interface::GuestAccountBackdoor;
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::tree::nodes::account::TreeAccount;

#[derive(Clone, Copy, Debug)]
pub struct MirrorAccountHeader {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub executable: bool,
}

pub struct UnsafeAccountBackdoor {
    accounts: Vec<(Pubkey, AccountSharedData)>,
    mirror_ptr: Option<*const dyn GuestAccountBackdoor>,
}

impl UnsafeAccountBackdoor {
    pub unsafe fn new(bd: &dyn GuestAccountBackdoor) -> Self {
        Self {
            accounts: bd.get_accounts(),
            mirror_ptr: Some(std::ptr::from_ref(bd) as *const dyn GuestAccountBackdoor),
        }
    }

    /// `step_order` is stamped on each returned [`TreeAccount`] (single source of truth for the
    /// trace; `InvokeContext::account_diff` forwards it unchanged).
    pub fn check_diffs(&mut self, step_order: u64) -> Vec<TreeAccount> {
        let mirror_ptr = self
            .mirror_ptr
            .expect("UnsafeAccountBackdoor has been cleared");
        let mirror = unsafe { &*mirror_ptr };

        let accounts = mirror.get_accounts();
        let num_accounts = accounts.len();
        let mut changed_accounts = Vec::new();

        for index in 0..num_accounts {
            if accounts[index].0 != self.accounts[index].0 {
                panic!("Account pubkey different");
            }

            if accounts[index].1 != self.accounts[index].1 {
                changed_accounts.push(TreeAccount {
                    step_order,
                    key: accounts[index].0,
                    before: self.accounts[index].1.clone().into(),
                    after: accounts[index].1.clone().into(),
                });

                self.accounts[index] = accounts[index].clone();
            }
        }

        changed_accounts
    }

    pub fn live_accounts(&self) -> Vec<(Pubkey, AccountSharedData)> {
        let mirror_ptr = self
            .mirror_ptr
            .expect("UnsafeAccountBackdoor has been cleared");
        unsafe { &*mirror_ptr }.get_accounts()
    }

    pub fn clear(&mut self) {
        self.mirror_ptr = None;
    }
}
