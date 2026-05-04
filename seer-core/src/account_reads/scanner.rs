use std::collections::HashMap;

use solana_pubkey::Pubkey;

use crate::{
    account_reads::{
        raw::{RawAccountRead, RawAccountReadKind},
        raw_to_view::raw_to_view,
        view::ViewAccountRead,
    },
    step_mirror::UnsafeAccountBackdoor,
    tree::nodes::account::{AccountSharedDataWrapper, TreeAccount},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSlot {
    Data,
    Lamports,
    DataLen,
    Key,
    Owner,
    Executable,
    RentEpoch,
}

#[derive(Clone, Debug)]
pub struct FieldRange {
    pub start: u64,
    pub end: u64,
    pub slot: FieldSlot,
}

impl FieldRange {
    pub fn matches(&self, start: u64, end: u64) -> bool {
        match self.slot {
            FieldSlot::Lamports | FieldSlot::DataLen | FieldSlot::RentEpoch => start == self.start,
            _ => start < self.end && end > self.start,
        }
    }
}

pub struct AccountVmLayout {
    pub original_data_len: usize,
    pub vm_data_addr: u64,
    pub vm_key_addr: u64,
    pub vm_lamports_addr: u64,
    pub vm_owner_addr: u64,
}

pub struct AccountFieldScanner {
    account_ranges: HashMap<Pubkey, Vec<FieldRange>>,
    accounts: HashMap<Pubkey, AccountSharedDataWrapper>,
    captured_reads: Vec<RawAccountRead>,
}

impl AccountFieldScanner {
    pub fn new(uab: &UnsafeAccountBackdoor) -> Self {
        Self {
            account_ranges: HashMap::new(),
            accounts: uab.clone_accounts(),
            captured_reads: vec![],
        }
    }

    pub fn update_account(&mut self, acc: &TreeAccount) {
        self.accounts.insert(acc.key, acc.after.clone());
    }

    pub fn capture_vm_layout(
        &mut self,
        layouts: &[AccountVmLayout],
        keys: Vec<Pubkey>,
        data_growth: u64,
    ) {
        debug_assert_eq!(layouts.len(), keys.len());

        for (k, l) in keys.iter().zip(layouts) {
            let ranges = get_ranges_for_account(l, data_growth);
            self.account_ranges.insert(k.clone(), ranges);
        }
    }

    pub fn capture_read(&mut self, step_order: u64, vm_addr: u64, width: u64) {
        let end = vm_addr.saturating_add(width);
        if width == 0 || end <= vm_addr {
            return;
        }

        for (k, ranges) in &self.account_ranges {
            for r in ranges {
                if r.matches(vm_addr, end) {
                    let account = self.accounts.get(k).expect("Read uninstantiated account");
                    let read_kind = match r.slot {
                        FieldSlot::Data => RawAccountReadKind::Data {
                            offset: vm_addr.saturating_sub(r.start) as usize,
                            bytes_width: width as usize,
                            bytes: account.data().to_vec(),
                        },
                        FieldSlot::Lamports => RawAccountReadKind::Lamports {
                            lamports: account.lamports(),
                        },
                        FieldSlot::DataLen => RawAccountReadKind::DataLen {
                            len: account.data().len() as u64,
                        },
                        FieldSlot::Key => RawAccountReadKind::Key {
                            offset: vm_addr.saturating_sub(r.start) as usize,
                            bytes_width: width as usize,
                            bytes: k.to_bytes().to_vec(),
                        },
                        FieldSlot::Owner => RawAccountReadKind::Owner {
                            offset: vm_addr.saturating_sub(r.start) as usize,
                            bytes_width: width as usize,
                            bytes: account.owner().to_bytes().to_vec(),
                        },
                        FieldSlot::Executable => RawAccountReadKind::Executable {
                            byte: u8::from(account.executable()),
                        },
                        FieldSlot::RentEpoch => RawAccountReadKind::RentEpoch {
                            rent_epoch: account.rent_epoch(),
                        },
                    };

                    self.captured_reads.push(RawAccountRead {
                        step_order,
                        key: k.clone(),
                        read_kind,
                        owner: account.owner(),
                    });

                    return;
                }
            }
        }
    }

    pub fn drain_reads(&mut self) -> Vec<ViewAccountRead> {
        let raw_reads = self.captured_reads.drain(..).collect();
        raw_to_view(raw_reads)
    }
}

pub fn get_ranges_for_account(m: &AccountVmLayout, data_growth: u64) -> Vec<FieldRange> {
    let mut out = Vec::new();
    let data_end = m
        .vm_data_addr
        .saturating_add(m.original_data_len as u64)
        .saturating_add(data_growth);

    out.push(FieldRange {
        start: m.vm_data_addr,
        end: data_end,
        slot: FieldSlot::Data,
    });

    let lamports_end = m.vm_lamports_addr.saturating_add(8);
    out.push(FieldRange {
        start: m.vm_lamports_addr,
        end: lamports_end,
        slot: FieldSlot::Lamports,
    });

    if lamports_end < m.vm_data_addr {
        out.push(FieldRange {
            start: lamports_end,
            end: m.vm_data_addr,
            slot: FieldSlot::DataLen,
        });
    }

    out.push(FieldRange {
        start: m.vm_key_addr,
        end: m.vm_key_addr.saturating_add(32),
        slot: FieldSlot::Key,
    });

    out.push(FieldRange {
        start: m.vm_owner_addr,
        end: m.vm_owner_addr.saturating_add(32),
        slot: FieldSlot::Owner,
    });

    let exe_start = m.vm_owner_addr.saturating_add(32);
    out.push(FieldRange {
        start: exe_start,
        end: exe_start.saturating_add(1),
        slot: FieldSlot::Executable,
    });
    let rent_start = exe_start.saturating_add(1);
    out.push(FieldRange {
        start: rent_start,
        end: rent_start.saturating_add(8),
        slot: FieldSlot::RentEpoch,
    });

    out
}
