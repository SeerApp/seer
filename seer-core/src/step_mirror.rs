use seer_interface::GuestStepMirror;
use solana_account::AccountSharedData;

use crate::tree::nodes::TreeAccount;

pub struct StepMirror {
    accounts: Vec<AccountSharedData>,
    mirror_ptr: Option<*const dyn GuestStepMirror>,
}

impl StepMirror {
    pub unsafe fn new(mirror: &dyn GuestStepMirror) -> Self {
        let touched_flags = mirror.clone_flags().into_inner();

        let mut accounts = Vec::new();

        let num_accounts = touched_flags.len();
        for index in 0..num_accounts {
            if let Some(account) = mirror.get_account_at_index(index) {
                accounts.push(account);
            }
        }

        Self {
            accounts,
            mirror_ptr: Some(std::ptr::from_ref(mirror) as *const dyn GuestStepMirror),
        }
    }

    pub fn check_diffs(&mut self) -> Vec<TreeAccount> {
        let mirror_ptr = self.mirror_ptr.expect("StepMirror has been cleared");
        let mirror = unsafe { &*mirror_ptr };

        let current_flags = mirror.clone_flags();
        let current_flags_ref = current_flags.borrow();

        let mut changed_accounts = Vec::new();

        let num_accounts = current_flags_ref.len();
        for index in 0..num_accounts {
            let current_flag = current_flags_ref[index];
            if current_flag == true {
                if let Some((key, current_account)) = mirror.read_account_at_index(index) {
                    if let Some(old_account) = self.accounts.get(index) {
                        if current_account != *old_account {
                            changed_accounts.push(TreeAccount {
                                key,
                                before: old_account.clone().into(),
                                after: current_account.clone().into(),
                            });
                            self.accounts[index] = current_account;
                        }
                    }
                }
            }
        }

        changed_accounts
    }

    pub fn clear(&mut self) {
        self.mirror_ptr = None;
    }
}
