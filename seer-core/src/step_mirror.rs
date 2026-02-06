use seer_interface::GuestStepMirror;
use solana_account::AccountSharedData;
use solana_pubkey::Pubkey;

use crate::tree::nodes::TreeAccount;

pub struct StepMirror {
    accounts: Vec<(Pubkey, AccountSharedData)>,
    mirror_ptr: Option<*const dyn GuestStepMirror>,
}

impl StepMirror {
    pub unsafe fn new(mirror: &dyn GuestStepMirror) -> Self {
        Self {
            accounts: mirror.get_accounts(),
            mirror_ptr: Some(std::ptr::from_ref(mirror) as *const dyn GuestStepMirror),
        }
    }

    pub fn check_diffs(&mut self) -> Vec<TreeAccount> {
        let mirror_ptr = self.mirror_ptr.expect("StepMirror has been cleared");
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
                    key: accounts[index].0,
                    before: self.accounts[index].1.clone().into(),
                    after: accounts[index].1.clone().into(),
                });

                self.accounts[index] = accounts[index].clone();
            }
        }

        changed_accounts
    }

    pub fn clear(&mut self) {
        self.mirror_ptr = None;
    }
}
