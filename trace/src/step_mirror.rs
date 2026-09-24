use hooks::GuestAccountBackdoor;
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
    /// # Safety
    /// `bd` must remain valid until `clear` (the matching `end_program` close).
    pub unsafe fn new(bd: &dyn GuestAccountBackdoor) -> Self {
        let ptr: *const dyn GuestAccountBackdoor = bd;
        Self {
            accounts: bd.get_accounts(),
            // SAFETY: caller keeps `bd` alive until `clear` (start_program through
            // close_account_backdoor). rustc 1.96 refuses the implicit lifetime-to-'static
            // trait-object pointer cast.
            mirror_ptr: Some(std::mem::transmute::<
                *const dyn GuestAccountBackdoor,
                *const (dyn GuestAccountBackdoor + 'static),
            >(ptr)),
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
        let mut changed_accounts = Vec::new();

        for (index, (key, account)) in accounts.iter().enumerate() {
            if *key != self.accounts[index].0 {
                panic!("Account pubkey different");
            }

            if *account != self.accounts[index].1 {
                changed_accounts.push(TreeAccount {
                    step_order,
                    key: *key,
                    before: self.accounts[index].1.clone().into(),
                    after: account.clone().into(),
                });

                self.accounts[index] = (*key, account.clone());
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
