//! Names for serialized Solana BPF input-region bytes (aligned ABI).
//!
//! Layout after `INPUT_BASE` (see Agave `serialize_parameters` /
//! `solana_program_entrypoint::deserialize`):
//!
//! ```text
//! u64 num_accounts
//! for each account:
//!   if unique (dup == 0xff):
//!     u8 dup, signer, writable, executable
//!     u32 orig_data_len
//!     [u8; 32] pubkey, owner
//!     u64 lamports, data_len
//!     [u8; data_len] data
//!     [u8; 10240] realloc pad
//!     align 8
//!     u64 rent_epoch
//!   else:
//!     u8 dup_index + 7 pad
//! u64 ix_data_len
//! [u8; ix_data_len] ix_data
//! [u8; 32] program_id
//! ```
//!
//! Account 0 is always unique. Later account starts depend on concrete
//! `data_len` / dup markers observed from the trace.

use crate::grammar::{self, AccFlag};

/// Marker written for a unique (non-duplicate) serialized account.
pub const NON_DUP_MARKER: u8 = 0xff;
/// `MAX_PERMITTED_DATA_INCREASE` — realloc slack after account data.
pub const MAX_PERMITTED_DATA_INCREASE: u64 = 10 * 1024;
/// `BPF_ALIGN_OF_U128` in current Agave (alignment after realloc pad).
pub const BPF_ALIGN: u64 = 8;
const MAX_ACCOUNTS: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    NumAccounts,
    AccDup { index: u32 },
    AccSigner { index: u32 },
    AccWritable { index: u32 },
    AccExecutable { index: u32 },
    AccOrigDataLen { index: u32 },
    AccPubkey { index: u32 },
    AccOwner { index: u32 },
    AccLamports { index: u32 },
    AccDataLen { index: u32 },
    AccData { index: u32 },
    AccRealloc { index: u32 },
    AccAlign { index: u32 },
    AccRentEpoch { index: u32 },
    AccDupPad { index: u32 },
    IxDataLen,
    IxData,
    ProgramId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLoc {
    pub field: InputField,
    pub offset_in_field: u64,
    pub field_len: u64,
}

enum AccWalk {
    Hit(FieldLoc),
    Skip(u64),
    Stuck,
}

pub fn byte_symbol(concrete: impl Fn(u64) -> Option<u8>, offset: u64) -> Option<String> {
    Some(byte_name(&classify(concrete, offset)?))
}

pub fn word_symbol(
    concrete: impl Fn(u64) -> Option<u8>,
    offset: u64,
    nbytes: usize,
) -> Option<String> {
    word_name(&classify(concrete, offset)?, nbytes)
}

/// Walk the serialized input layout using observed concrete bytes.
pub fn classify(concrete: impl Fn(u64) -> Option<u8>, offset: u64) -> Option<FieldLoc> {
    if offset < 8 {
        return Some(FieldLoc {
            field: InputField::NumAccounts,
            offset_in_field: offset,
            field_len: 8,
        });
    }

    let mut cursor = 8u64;
    let n_accounts = read_u64(&concrete, 0);
    let max_i = n_accounts
        .map(|n| n.min(MAX_ACCOUNTS as u64) as u32)
        .unwrap_or(MAX_ACCOUNTS);

    for i in 0..max_i {
        match skip_or_classify_account(&concrete, i, cursor, offset) {
            AccWalk::Hit(loc) => return Some(loc),
            AccWalk::Skip(next) => cursor = next,
            AccWalk::Stuck => return None,
        }
    }

    if n_accounts.is_none() {
        return None;
    }

    if offset < cursor + 8 {
        return Some(FieldLoc {
            field: InputField::IxDataLen,
            offset_in_field: offset - cursor,
            field_len: 8,
        });
    }
    let ix_len = read_u64(&concrete, cursor)?;
    cursor += 8;
    if offset < cursor + ix_len {
        return Some(FieldLoc {
            field: InputField::IxData,
            offset_in_field: offset - cursor,
            field_len: ix_len,
        });
    }
    cursor += ix_len;
    if offset < cursor + 32 {
        return Some(FieldLoc {
            field: InputField::ProgramId,
            offset_in_field: offset - cursor,
            field_len: 32,
        });
    }
    None
}

fn read_u64(concrete: &impl Fn(u64) -> Option<u8>, offset: u64) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..8u64 {
        let b = concrete(offset.wrapping_add(i))?;
        v |= (b as u64) << (8 * i);
    }
    Some(v)
}

fn skip_or_classify_account(
    concrete: &impl Fn(u64) -> Option<u8>,
    i: u32,
    start: u64,
    offset: u64,
) -> AccWalk {
    let dup = concrete(start);
    // Account 0 is always unique. An unobserved dup is treated as unique so
    // header fields (pubkey, data_len, …) still name; a later non-0xff
    // observe switches that slot to the 8-byte duplicate stub.
    let unique = i == 0 || dup.map(|d| d == NON_DUP_MARKER).unwrap_or(true);

    if !unique {
        if offset < start + 8 {
            if offset == start {
                return AccWalk::Hit(FieldLoc {
                    field: InputField::AccDup { index: i },
                    offset_in_field: 0,
                    field_len: 1,
                });
            }
            return AccWalk::Hit(FieldLoc {
                field: InputField::AccDupPad { index: i },
                offset_in_field: offset - start,
                field_len: 8,
            });
        }
        return AccWalk::Skip(start + 8);
    }

    if offset < start + 88 {
        return AccWalk::Hit(header_field(i, start, offset));
    }

    let Some(data_len) = read_u64(concrete, start + 80) else {
        return AccWalk::Stuck;
    };

    let data_start = start + 88;
    let data_end = data_start.saturating_add(data_len);
    if offset < data_end {
        return AccWalk::Hit(FieldLoc {
            field: InputField::AccData { index: i },
            offset_in_field: offset - data_start,
            field_len: data_len,
        });
    }

    let realloc_end = data_end.saturating_add(MAX_PERMITTED_DATA_INCREASE);
    if offset < realloc_end {
        return AccWalk::Hit(FieldLoc {
            field: InputField::AccRealloc { index: i },
            offset_in_field: offset - data_end,
            field_len: MAX_PERMITTED_DATA_INCREASE,
        });
    }

    let aligned = align_up(realloc_end, BPF_ALIGN);
    if offset < aligned {
        return AccWalk::Hit(FieldLoc {
            field: InputField::AccAlign { index: i },
            offset_in_field: offset - realloc_end,
            field_len: aligned - realloc_end,
        });
    }
    if offset < aligned + 8 {
        return AccWalk::Hit(FieldLoc {
            field: InputField::AccRentEpoch { index: i },
            offset_in_field: offset - aligned,
            field_len: 8,
        });
    }
    AccWalk::Skip(aligned + 8)
}

fn align_up(x: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    let mask = align - 1;
    x.wrapping_add(mask) & !mask
}

fn header_field(index: u32, start: u64, offset: u64) -> FieldLoc {
    let rel = offset - start;
    match rel {
        0 => FieldLoc {
            field: InputField::AccDup { index },
            offset_in_field: 0,
            field_len: 1,
        },
        1 => FieldLoc {
            field: InputField::AccSigner { index },
            offset_in_field: 0,
            field_len: 1,
        },
        2 => FieldLoc {
            field: InputField::AccWritable { index },
            offset_in_field: 0,
            field_len: 1,
        },
        3 => FieldLoc {
            field: InputField::AccExecutable { index },
            offset_in_field: 0,
            field_len: 1,
        },
        4..=7 => FieldLoc {
            field: InputField::AccOrigDataLen { index },
            offset_in_field: rel - 4,
            field_len: 4,
        },
        8..=39 => FieldLoc {
            field: InputField::AccPubkey { index },
            offset_in_field: rel - 8,
            field_len: 32,
        },
        40..=71 => FieldLoc {
            field: InputField::AccOwner { index },
            offset_in_field: rel - 40,
            field_len: 32,
        },
        72..=79 => FieldLoc {
            field: InputField::AccLamports { index },
            offset_in_field: rel - 72,
            field_len: 8,
        },
        80..=87 => FieldLoc {
            field: InputField::AccDataLen { index },
            offset_in_field: rel - 80,
            field_len: 8,
        },
        _ => unreachable!("header_field rel={rel} should be < 88"),
    }
}

fn byte_name(loc: &FieldLoc) -> String {
    let i = loc.offset_in_field;
    match loc.field {
        InputField::NumAccounts => grammar::num_accounts_byte(i),
        InputField::AccDup { index } => grammar::acc_dup(index),
        InputField::AccSigner { index } => grammar::acc_flag_byte(index, AccFlag::Signer),
        InputField::AccWritable { index } => grammar::acc_flag_byte(index, AccFlag::Writable),
        InputField::AccExecutable { index } => grammar::acc_flag_byte(index, AccFlag::Executable),
        InputField::AccOrigDataLen { index } => grammar::acc_orig_data_len_byte(index, i),
        InputField::AccPubkey { index } => grammar::acc_pubkey_byte(index, i),
        InputField::AccOwner { index } => grammar::acc_owner_byte(index, i),
        InputField::AccLamports { index } => grammar::acc_lamports_byte(index, i),
        InputField::AccDataLen { index } => grammar::acc_data_len_byte(index, i),
        InputField::AccData { index } => grammar::acc_data_byte(index, i),
        InputField::AccRealloc { index } => grammar::acc_realloc_byte(index, i),
        InputField::AccAlign { index } => grammar::acc_align_byte(index, i),
        InputField::AccRentEpoch { index } => grammar::acc_rent_epoch_byte(index, i),
        InputField::AccDupPad { index } => grammar::acc_dup_pad_byte(index, i),
        InputField::IxDataLen => grammar::ix_data_len_byte(i),
        InputField::IxData => grammar::ix_data_byte(i),
        InputField::ProgramId => grammar::program_id_byte(i),
    }
}

fn word_name(loc: &FieldLoc, nbytes: usize) -> Option<String> {
    let off = loc.offset_in_field;
    if off.saturating_add(nbytes as u64) > loc.field_len {
        return None;
    }
    match loc.field {
        InputField::NumAccounts if nbytes == 8 && off == 0 => Some(grammar::num_accounts_word()),
        InputField::AccDataLen { index } if nbytes == 8 && off == 0 => {
            Some(grammar::acc_data_len_word(index))
        }
        InputField::AccLamports { index } if nbytes == 8 && off == 0 => {
            Some(grammar::acc_lamports_word(index))
        }
        InputField::AccRentEpoch { index } if nbytes == 8 && off == 0 => {
            Some(grammar::acc_rent_epoch_word(index))
        }
        InputField::AccOrigDataLen { index } if nbytes == 4 && off == 0 => {
            Some(grammar::acc_orig_data_len_word(index))
        }
        InputField::AccPubkey { index } if nbytes == 8 && off % 8 == 0 => {
            Some(grammar::acc_pubkey_word(index, off / 8))
        }
        InputField::AccOwner { index } if nbytes == 8 && off % 8 == 0 => {
            Some(grammar::acc_owner_word(index, off / 8))
        }
        InputField::AccData { index } => Some(grammar::acc_data_word(index, off)),
        InputField::IxDataLen if nbytes == 8 && off == 0 => Some(grammar::ix_data_len_word()),
        InputField::IxData if nbytes == 8 && off == 0 => Some(grammar::ix_disc_word()),
        InputField::IxData => Some(grammar::ix_data_word(off)),
        InputField::ProgramId if nbytes == 8 && off % 8 == 0 => {
            Some(grammar::program_id_word(off / 8))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct Bytes(HashMap<u64, u8>);

    impl Bytes {
        fn new() -> Self {
            Self(HashMap::new())
        }

        fn observe(&mut self, offset: u64, nbytes: usize, value: u64) {
            for i in 0..nbytes {
                let b = ((value >> (8 * i)) & 0xff) as u8;
                self.0.insert(offset.wrapping_add(i as u64), b);
            }
        }

        fn byte_symbol(&self, offset: u64) -> Option<String> {
            byte_symbol(|o| self.0.get(&o).copied(), offset)
        }

        fn word_symbol(&self, offset: u64, nbytes: usize) -> Option<String> {
            word_symbol(|o| self.0.get(&o).copied(), offset, nbytes)
        }
    }

    #[test]
    fn account0_header_needs_no_observations() {
        let abi = Bytes::new();
        assert_eq!(abi.byte_symbol(0).as_deref(), Some("n_num_accounts_00"));
        assert_eq!(abi.byte_symbol(7).as_deref(), Some("n_num_accounts_07"));
        assert_eq!(abi.word_symbol(0, 8).as_deref(), Some("w_num_accounts"));
        assert_eq!(abi.byte_symbol(8).as_deref(), Some("n_acc0_dup"));
        assert_eq!(abi.byte_symbol(9).as_deref(), Some("n_acc0_signer"));
        assert_eq!(abi.byte_symbol(10).as_deref(), Some("n_acc0_writable"));
        assert_eq!(abi.byte_symbol(11).as_deref(), Some("n_acc0_executable"));
        assert_eq!(abi.byte_symbol(16).as_deref(), Some("n_acc0_pubkey_00"));
        assert_eq!(abi.byte_symbol(48).as_deref(), Some("n_acc0_owner_00"));
        assert_eq!(abi.word_symbol(80, 8).as_deref(), Some("w_acc0_lamports"));
        assert_eq!(abi.word_symbol(88, 8).as_deref(), Some("w_acc0_data_len"));
        assert_eq!(abi.word_symbol(16, 8).as_deref(), Some("w_acc0_pubkey_0"));
        assert_eq!(abi.word_symbol(24, 8).as_deref(), Some("w_acc0_pubkey_1"));
        assert!(abi.byte_symbol(10344).is_none());
        assert!(abi.word_symbol(8, 8).is_none());
    }

    #[test]
    fn later_accounts_follow_concrete_data_len() {
        let mut abi = Bytes::new();
        abi.observe(88, 8, 0);
        assert_eq!(abi.byte_symbol(10344).as_deref(), Some("n_acc1_dup"));
        assert_eq!(
            abi.word_symbol(10424, 8).as_deref(),
            Some("w_acc1_data_len")
        );

        abi.observe(10424, 8, 96);
        assert_eq!(
            abi.word_symbol(20856, 8).as_deref(),
            Some("w_acc2_data_len")
        );
        assert_eq!(abi.byte_symbol(20776).as_deref(), Some("n_acc2_dup"));
        assert_eq!(
            abi.word_symbol(20784, 8).as_deref(),
            Some("w_acc2_pubkey_0")
        );
        abi.observe(20856, 8, 8);
        assert_eq!(abi.byte_symbol(20864).as_deref(), Some("n_acc2_data_0000"));
        assert_eq!(
            abi.word_symbol(20864, 8).as_deref(),
            Some("w_acc2_data_0000")
        );
    }

    #[test]
    fn duplicate_account_is_eight_byte_stub() {
        let mut abi = Bytes::new();
        abi.observe(88, 8, 0);
        abi.observe(10344, 1, 0);
        assert_eq!(abi.byte_symbol(10344).as_deref(), Some("n_acc1_dup"));
        assert_eq!(abi.byte_symbol(10345).as_deref(), Some("n_acc1_dup_pad_1"));
        assert_eq!(abi.byte_symbol(10352).as_deref(), Some("n_acc2_dup"));
    }

    #[test]
    fn ix_data_after_known_account_count() {
        let mut abi = Bytes::new();
        abi.observe(0, 8, 1);
        abi.observe(88, 8, 0);
        let ix_len_off = 10344u64;
        abi.observe(ix_len_off, 8, 8);
        assert_eq!(
            abi.word_symbol(ix_len_off, 8).as_deref(),
            Some("w_ix_data_len")
        );
        assert_eq!(
            abi.word_symbol(ix_len_off + 8, 8).as_deref(),
            Some("w_ix_disc")
        );
        assert_eq!(
            abi.word_symbol(ix_len_off + 16, 8).as_deref(),
            Some("w_program_id_0")
        );
    }
}
