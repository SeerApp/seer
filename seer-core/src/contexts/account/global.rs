use seer_interface::GuestAccountBackdoor;
use solana_pubkey::Pubkey;

use crate::{
    account_reads::{
        scanner::{AccountFieldScanner, AccountVmLayout},
        view::{ViewAccountRead, ViewAccountReadKind},
    },
    program_manager::types::GlobalProgramContext,
    step_mirror::UnsafeAccountBackdoor,
    sysvar_accounts::parse_sysvar_account,
    tree::nodes::account::TreeAccount,
};

pub struct GlobalAccountContext {
    unsafe_account_backdoor: Option<UnsafeAccountBackdoor>,
    account_field_scanner: Option<AccountFieldScanner>,
}

impl GlobalAccountContext {
    pub fn new() -> Self {
        Self {
            unsafe_account_backdoor: None,
            account_field_scanner: None,
        }
    }

    pub fn open_account_backdoor_idempotent(&mut self, bd: &dyn GuestAccountBackdoor) {
        if self.unsafe_account_backdoor.is_none() {
            let uab = unsafe { UnsafeAccountBackdoor::new(bd) };
            self.account_field_scanner = Some(AccountFieldScanner::new(&uab));
            self.unsafe_account_backdoor = Some(uab);
        }
    }

    pub fn close_account_backdoor_idempotent(&mut self) {
        if let Some(mut uab) = self.unsafe_account_backdoor.take() {
            uab.clear();
            self.account_field_scanner = None;
        }
    }

    pub fn get_changed_accounts(&mut self, step_order: u64) -> Vec<TreeAccount> {
        if let Some(uab) = self.unsafe_account_backdoor.as_mut() {
            let afs = self
                .account_field_scanner
                .as_mut()
                .expect("AFS not initiated with UAB");
            let diffs = uab.check_diffs(step_order);

            for diff in &diffs {
                afs.update_account(diff);
            }

            diffs
        } else {
            vec![]
        }
    }

    pub fn capture_vm_layout(
        &mut self,
        layouts: &[AccountVmLayout],
        keys: Vec<Pubkey>,
        data_growth: u64,
    ) {
        self.account_field_scanner
            .as_mut()
            .expect("Capturing vm layout before AFS")
            .capture_vm_layout(layouts, keys, data_growth);
    }

    pub fn capture_account_read(&mut self, step_order: u64, vm_addr: u64, width: u64) {
        self.account_field_scanner
            .as_mut()
            .expect("Capturing account read before AFS")
            .capture_read(step_order, vm_addr, width);
    }

    pub fn drain_parsed_view_accounts(
        &mut self,
        program_context: &GlobalProgramContext,
        receiver_program: Pubkey,
    ) -> Vec<ViewAccountRead> {
        let mut view_reads = self
            .account_field_scanner
            .as_mut()
            .expect("Draining accounts before AFS")
            .drain_reads();

        for view_read in &mut view_reads {
            match &mut view_read.read_kind {
                ViewAccountReadKind::ReadData {
                    bytes,
                    reads: _,
                    parsed,
                    parsed_byte_offsets,
                } => {
                    if parsed.is_some() {
                        continue;
                    }

                    let key = view_read.key;

                    let mut parsed_out = parse_sysvar_account(&key, bytes.as_slice());

                    if parsed_out.is_none() {
                        if let Some(idl_lookup) = program_context.get_idl_lookup(&receiver_program)
                        {
                            parsed_out = GlobalProgramContext::parse_account_with_idl(
                                &idl_lookup,
                                bytes.as_slice(),
                            );
                        }
                    }

                    if let Some(result) = parsed_out {
                        *parsed_byte_offsets = result.parsed_byte_offsets;
                        *parsed = Some(result.parsed);
                    }
                }
                _ => {}
            }
        }

        view_reads
    }
}
