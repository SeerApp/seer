//! Interpreter account-load tracing: classify guest loads against serialized account layout
//! and emit [`crate::tree::nodes::TreeAccountLoad`] nodes.

use std::cell::{Cell, RefCell};
use std::ptr;

use solana_pubkey::Pubkey;

use crate::{
    contexts::seer::SeerContext,
    tree::nodes::{TreeAccountLoad, TreeAccountLoadKind},
};

/// Snapshot of Agave `SerializedAccountMetadata` fields needed for classification.
#[derive(Clone, Copy, Debug)]
pub struct AccountReadMeta {
    pub original_data_len: usize,
    pub vm_data_addr: u64,
    pub vm_key_addr: u64,
    pub vm_lamports_addr: u64,
    pub vm_owner_addr: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldSlot {
    Data,
    Lamports,
    DataLen,
    Key,
    Owner,
    Executable,
    RentEpoch,
}

#[derive(Clone, Debug)]
struct FieldRange {
    start: u64,
    end: u64,
    slot: FieldSlot,
}

/// Per–instruction-account index, ordered ranges for classification.
#[derive(Clone, Debug)]
pub struct AccountReadMap {
    /// `ranges_per_account[i]` = ranges for instruction account index `i`.
    ranges_per_account: Vec<Vec<FieldRange>>,
    pub keys: Vec<Pubkey>,
    pub owners: Vec<Pubkey>,
    pub tx_account_index: Vec<u16>,
    pub data_growth: u64,
}

fn ranges_for_account(m: &AccountReadMeta, data_growth: u64) -> Vec<FieldRange> {
    let mut out = Vec::new();
    let data_end = m
        .vm_data_addr
        .saturating_add(m.original_data_len as u64)
        .saturating_add(data_growth);

    // Prefer data first in iteration order.
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

impl AccountReadMap {
    pub fn build(
        metas: &[AccountReadMeta],
        keys: Vec<Pubkey>,
        owners: Vec<Pubkey>,
        tx_account_index: Vec<u16>,
        data_growth: u64,
    ) -> Self {
        debug_assert_eq!(metas.len(), keys.len());
        debug_assert_eq!(metas.len(), owners.len());
        debug_assert_eq!(metas.len(), tx_account_index.len());
        let ranges_per_account = metas
            .iter()
            .map(|m| ranges_for_account(m, data_growth))
            .collect();
        Self {
            ranges_per_account,
            keys,
            owners,
            tx_account_index,
            data_growth,
        }
    }

    /// Returns `(instruction_account_index, field slot, data_offset_if_data)`.
    pub(crate) fn classify(
        &self,
        vm_addr: u64,
        len: u64,
    ) -> Option<(u16, FieldSlot, Option<usize>)> {
        let end = vm_addr.saturating_add(len);
        if len == 0 || end <= vm_addr {
            return None;
        }
        for (ix, ranges) in self.ranges_per_account.iter().enumerate() {
            for r in ranges {
                if vm_addr < r.end && end > r.start {
                    let data_off = match r.slot {
                        FieldSlot::Data => Some((vm_addr.saturating_sub(r.start)) as usize),
                        _ => None,
                    };
                    return Some((ix as u16, r.slot, data_off));
                }
            }
        }
        None
    }
}

struct ActiveReadCtx {
    map: AccountReadMap,
}

thread_local! {
    static ACTIVE_STACK: RefCell<Vec<ActiveReadCtx>> = RefCell::new(Vec::new());
}

thread_local! {
    static CURRENT_TRACE_ORDER: Cell<u64> = Cell::new(0);
}

pub fn trace_account_reads_enabled() -> bool {
    matches!(
        std::env::var("SEER_TRACE_ACCOUNT_READS").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

pub fn set_current_trace_order(order: u64) {
    CURRENT_TRACE_ORDER.set(order);
}

/// # Safety
/// `host_addr` must be a valid host pointer for `width` bytes (same contract as the VM load).
unsafe fn read_load_segment(host_addr: u64, width: u64) -> Vec<u8> {
    let n = width as usize;
    if n == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(host_addr as *const u8, n).to_vec()
}

fn read_u64_le_from_host(host_addr: u64, width: u64) -> u64 {
    let n = (width as usize).min(8);
    if n == 0 {
        return 0;
    }
    let mut buf = [0u8; 8];
    unsafe {
        ptr::copy_nonoverlapping(host_addr as *const u8, buf.as_mut_ptr(), n);
    }
    u64::from_le_bytes(buf)
}

fn build_read_kind(slot: FieldSlot, data_offset: Option<usize>, host_addr: u64, width: u64) -> TreeAccountLoadKind {
    let w = width as usize;
    match slot {
        FieldSlot::Data => TreeAccountLoadKind::Data {
            offset: data_offset.unwrap_or(0),
            bytes_width: w,
            bytes: unsafe { read_load_segment(host_addr, width) },
        },
        FieldSlot::Key => TreeAccountLoadKind::Key {
            bytes: unsafe { read_load_segment(host_addr, width) },
        },
        FieldSlot::Owner => TreeAccountLoadKind::Owner {
            bytes: unsafe { read_load_segment(host_addr, width) },
        },
        FieldSlot::Lamports => TreeAccountLoadKind::Lamports {
            lamports: read_u64_le_from_host(host_addr, width),
        },
        FieldSlot::DataLen => TreeAccountLoadKind::DataLen {
            len: read_u64_le_from_host(host_addr, width),
        },
        FieldSlot::RentEpoch => TreeAccountLoadKind::RentEpoch {
            rent_epoch: read_u64_le_from_host(host_addr, width),
        },
        FieldSlot::Executable => {
            let byte = if w == 0 {
                0
            } else {
                unsafe { (host_addr as *const u8).read_unaligned() }
            };
            TreeAccountLoadKind::Executable { byte }
        }
    }
}

/// # Safety
/// `host_addr` must be valid for `width` bytes (successful `MemoryMapping::load` host mapping).
pub unsafe fn push_active(map: AccountReadMap) {
    ACTIVE_STACK.with(|c| {
        c.borrow_mut().push(ActiveReadCtx { map });
    });
}

pub fn pop_active() {
    ACTIVE_STACK.with(|c| {
        c.borrow_mut().pop();
    });
}

/// Clears the entire stack (e.g. abnormal teardown); normal paths use [`pop_active`].
pub fn clear_stack() {
    ACTIVE_STACK.with(|c| {
        c.borrow_mut().clear();
    });
}

fn is_active() -> bool {
    trace_account_reads_enabled() && ACTIVE_STACK.with(|c| !c.borrow().is_empty())
}

#[cfg(test)]
fn test_active_stack_depth() -> usize {
    ACTIVE_STACK.with(|c| c.borrow().len())
}

/// `host_addr` is the resolved host pointer for this load (see `MemoryMapping::load`).
///
/// # Safety
/// `host_addr` must be valid for `width` bytes until this function returns.
pub unsafe fn on_guest_load(vm_addr: u64, width: u64, host_addr: u64) {
    if !is_active() {
        return;
    }
    let hit = ACTIVE_STACK.with(|c| {
        let b = c.borrow();
        let ctx = b.last()?;
        ctx.map.classify(vm_addr, width)
    });
    let Some((ix, slot, data_off)) = hit else {
        return;
    };

    let key = ACTIVE_STACK.with(|c| {
        let v = c.borrow();
        let ctx = v.last().expect("active");
        ctx.map.keys.get(ix as usize).copied().unwrap_or_default()
    });
    let owner_snapshot = ACTIVE_STACK.with(|c| {
        let v = c.borrow();
        let ctx = v.last().expect("active");
        ctx.map.owners.get(ix as usize).copied()
    });

    let read_kind = build_read_kind(slot, data_off, host_addr, width);

    let load = TreeAccountLoad {
        step_order: CURRENT_TRACE_ORDER.get(),
        key,
        read_kind,
        owner_snapshot,
    };

    crate::get(|seer: &mut SeerContext| {
        seer.raw_account_load(load);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;

    fn sample_meta() -> AccountReadMeta {
        AccountReadMeta {
            original_data_len: 4,
            vm_data_addr: 1000,
            vm_key_addr: 100,
            vm_lamports_addr: 132,
            vm_owner_addr: 200,
        }
    }

    #[test]
    fn classify_lamports_slot() {
        let m = sample_meta();
        let k = Pubkey::new_unique();
        let map = AccountReadMap::build(&[m], vec![k], vec![Pubkey::new_unique()], vec![0u16], 1024);
        let hit = map
            .classify(m.vm_lamports_addr, 8)
            .expect("lamports hit");
        assert_eq!(hit.0, 0);
        assert_eq!(hit.1, FieldSlot::Lamports);
        assert!(hit.2.is_none());
    }

    #[test]
    fn classify_key_slot() {
        let m = sample_meta();
        let k = Pubkey::new_unique();
        let map = AccountReadMap::build(&[m], vec![k], vec![Pubkey::new_unique()], vec![0u16], 1024);
        let hit = map.classify(m.vm_key_addr, 32).expect("key hit");
        assert_eq!(hit.1, FieldSlot::Key);
        assert!(hit.2.is_none());
    }

    #[test]
    fn classify_data_offset() {
        let m = sample_meta();
        let k = Pubkey::new_unique();
        let map = AccountReadMap::build(&[m], vec![k], vec![Pubkey::new_unique()], vec![0u16], 1024);
        let hit = map
            .classify(m.vm_data_addr.saturating_add(2), 1)
            .expect("data hit");
        assert_eq!(hit.1, FieldSlot::Data);
        assert_eq!(hit.2, Some(2));
    }

    #[test]
    fn duplicate_account_first_instruction_index_wins() {
        let m = sample_meta();
        let k = Pubkey::new_unique();
        let map = AccountReadMap::build(
            &[m.clone(), m],
            vec![k, k],
            vec![Pubkey::new_unique(), Pubkey::new_unique()],
            vec![0u16, 0u16],
            1024,
        );
        let hit = map
            .classify(m.vm_data_addr.saturating_add(1), 1)
            .expect("shared data");
        assert_eq!(hit.0, 0);
        assert_eq!(hit.1, FieldSlot::Data);
        assert_eq!(hit.2, Some(1));
    }

    #[test]
    fn push_pop_stack_depth() {
        clear_stack();
        let m = sample_meta();
        let k = Pubkey::new_unique();
        let map = AccountReadMap::build(&[m], vec![k], vec![Pubkey::new_unique()], vec![0u16], 1024);
        unsafe {
            push_active(map.clone());
            push_active(map);
        }
        assert_eq!(test_active_stack_depth(), 2);
        pop_active();
        assert_eq!(test_active_stack_depth(), 1);
        pop_active();
        assert_eq!(test_active_stack_depth(), 0);
        clear_stack();
    }
}
