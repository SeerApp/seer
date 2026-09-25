//! Produce and recognize input / load-temp Z3 const names.
//!
//! Constructors are the on-the-wire spelling. [`parse`] is the only recognizer
//! hide/mint share.

/// Account-header provision flags (`signer` / `writable` / `executable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccFlag {
    Signer,
    Writable,
    Executable,
}

impl AccFlag {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signer => "signer",
            Self::Writable => "writable",
            Self::Executable => "executable",
        }
    }

    fn from_suffix(suffix: &str) -> Option<Self> {
        match suffix {
            "signer" => Some(Self::Signer),
            "writable" => Some(Self::Writable),
            "executable" => Some(Self::Executable),
            _ => None,
        }
    }
}

/// Families hide and mint classify. Other ABI names still use the constructors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed {
    NumAccountsByte,
    NumAccountsWord,
    NumAccountsChild,
    AccDataLenByte { acc: u32 },
    AccDataLenWord { acc: u32 },
    AccDataLenChild { acc: u32 },
    AccFlagByte { acc: u32, flag: AccFlag },
    AccFlagWord { acc: u32, flag: AccFlag },
    AccFlagChild { acc: u32, flag: AccFlag },
}

pub fn parse(name: &str) -> Option<Parsed> {
    if let Some(rest) = name.strip_prefix("n_num_accounts_") {
        return trailing_digits(rest).map(|_| Parsed::NumAccountsByte);
    }
    if name == "w_num_accounts" {
        return Some(Parsed::NumAccountsWord);
    }
    if let Some(rest) = name.strip_prefix("w_num_accounts_") {
        return trailing_digits(rest).map(|_| Parsed::NumAccountsChild);
    }
    if let Some(rest) = name.strip_prefix("n_acc") {
        let (acc, rest) = take_u32_prefix(rest)?;
        let rest = rest.strip_prefix('_')?;
        if let Some(flag) = AccFlag::from_suffix(rest) {
            return Some(Parsed::AccFlagByte { acc, flag });
        }
        if let Some(b) = rest.strip_prefix("data_len_") {
            return trailing_digits(b).map(|_| Parsed::AccDataLenByte { acc });
        }
        return None;
    }
    if let Some(rest) = name.strip_prefix("w_acc") {
        let (acc, rest) = take_u32_prefix(rest)?;
        let rest = rest.strip_prefix('_')?;
        if let Some(after) = rest.strip_prefix("data_len") {
            return match after {
                "" => Some(Parsed::AccDataLenWord { acc }),
                _ => after
                    .strip_prefix('_')
                    .and_then(trailing_digits)
                    .map(|_| Parsed::AccDataLenChild { acc }),
            };
        }
        for flag in [AccFlag::Signer, AccFlag::Writable, AccFlag::Executable] {
            if let Some(after) = rest.strip_prefix(flag.as_str()) {
                return match after {
                    "" => Some(Parsed::AccFlagWord { acc, flag }),
                    _ => after
                        .strip_prefix('_')
                        .and_then(trailing_digits)
                        .map(|_| Parsed::AccFlagChild { acc, flag }),
                };
            }
        }
    }
    None
}

pub fn is_num_accounts_byte(name: &str) -> bool {
    matches!(parse(name), Some(Parsed::NumAccountsByte))
}

pub fn is_num_accounts_child_name(name: &str) -> bool {
    matches!(parse(name), Some(Parsed::NumAccountsChild))
}

pub fn data_len_child_account(name: &str) -> Option<u32> {
    match parse(name) {
        Some(Parsed::AccDataLenChild { acc }) => Some(acc),
        _ => None,
    }
}

pub fn data_len_input_account(name: &str) -> Option<u32> {
    match parse(name) {
        Some(Parsed::AccDataLenByte { acc }) => Some(acc),
        _ => None,
    }
}

pub fn acc_flag_input(name: &str) -> Option<(u32, AccFlag)> {
    match parse(name) {
        Some(Parsed::AccFlagByte { acc, flag }) => Some((acc, flag)),
        _ => None,
    }
}

pub fn num_accounts_byte(i: u64) -> String {
    format!("n_num_accounts_{i:02}")
}

pub fn acc_dup(index: u32) -> String {
    format!("n_acc{index}_dup")
}

pub fn acc_flag_byte(index: u32, flag: AccFlag) -> String {
    format!("n_acc{index}_{}", flag.as_str())
}

pub fn acc_orig_data_len_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_orig_data_len_{i}")
}

pub fn acc_pubkey_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_pubkey_{i:02}")
}

pub fn acc_owner_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_owner_{i:02}")
}

pub fn acc_lamports_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_lamports_{i}")
}

pub fn acc_data_len_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_data_len_{i}")
}

pub fn acc_data_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_data_{i:04}")
}

pub fn acc_realloc_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_realloc_{i:04}")
}

pub fn acc_align_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_align_{i}")
}

pub fn acc_rent_epoch_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_rent_epoch_{i}")
}

pub fn acc_dup_pad_byte(index: u32, i: u64) -> String {
    format!("n_acc{index}_dup_pad_{i}")
}

pub fn ix_data_len_byte(i: u64) -> String {
    format!("n_ix_data_len_{i}")
}

pub fn ix_data_byte(i: u64) -> String {
    format!("n_ix_data_{i:04}")
}

pub fn program_id_byte(i: u64) -> String {
    format!("n_program_id_{i:02}")
}

pub fn num_accounts_word() -> String {
    "w_num_accounts".into()
}

pub fn num_accounts_child(k: u64) -> String {
    format!("w_num_accounts_{k}")
}

pub fn acc_data_len_word(index: u32) -> String {
    format!("w_acc{index}_data_len")
}

pub fn acc_data_len_child(index: u32, k: u64) -> String {
    format!("w_acc{index}_data_len_{k}")
}

pub fn acc_lamports_word(index: u32) -> String {
    format!("w_acc{index}_lamports")
}

pub fn acc_rent_epoch_word(index: u32) -> String {
    format!("w_acc{index}_rent_epoch")
}

pub fn acc_orig_data_len_word(index: u32) -> String {
    format!("w_acc{index}_orig_data_len")
}

pub fn acc_pubkey_word(index: u32, qword: u64) -> String {
    format!("w_acc{index}_pubkey_{qword}")
}

pub fn acc_owner_word(index: u32, qword: u64) -> String {
    format!("w_acc{index}_owner_{qword}")
}

pub fn acc_data_word(index: u32, off: u64) -> String {
    format!("w_acc{index}_data_{off:04}")
}

pub fn acc_flag_word(index: u32, flag: AccFlag) -> String {
    format!("w_acc{index}_{}", flag.as_str())
}

pub fn acc_flag_child(index: u32, flag: AccFlag, k: u64) -> String {
    format!("{}_{k}", acc_flag_word(index, flag))
}

pub fn ix_data_len_word() -> String {
    "w_ix_data_len".into()
}

pub fn ix_disc_word() -> String {
    "w_ix_disc".into()
}

pub fn ix_data_word(off: u64) -> String {
    format!("w_ix_data_{off:04}")
}

pub fn program_id_word(qword: u64) -> String {
    format!("w_program_id_{qword}")
}

pub fn generic_word(k: u64) -> String {
    format!("w_{k}")
}

fn take_u32_prefix(s: &str) -> Option<(u32, &str)> {
    let n = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let (num, rest) = s.split_at(n);
    Some((num.parse().ok()?, rest))
}

fn trailing_digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_num_accounts_parent_vs_child() {
        assert_eq!(parse("w_num_accounts"), Some(Parsed::NumAccountsWord));
        assert_eq!(parse("w_num_accounts_0"), Some(Parsed::NumAccountsChild));
        assert_eq!(parse("w_num_accounts_12"), Some(Parsed::NumAccountsChild));
        assert_eq!(parse("n_num_accounts_00"), Some(Parsed::NumAccountsByte));
        assert_eq!(parse("w_0"), None);
    }

    #[test]
    fn parse_data_len_parent_vs_child_vs_data() {
        assert_eq!(
            parse("w_acc0_data_len"),
            Some(Parsed::AccDataLenWord { acc: 0 })
        );
        assert_eq!(
            parse("w_acc0_data_len_0"),
            Some(Parsed::AccDataLenChild { acc: 0 })
        );
        assert_eq!(
            parse("w_acc12_data_len_3"),
            Some(Parsed::AccDataLenChild { acc: 12 })
        );
        assert_eq!(
            parse("n_acc0_data_len_0"),
            Some(Parsed::AccDataLenByte { acc: 0 })
        );
        assert_eq!(parse("n_acc0_dup"), None);
        assert_eq!(parse("w_acc0_data_0000"), None);
        assert_eq!(parse("n_acc0_data_0000"), None);
        assert_eq!(parse("n_acc0_orig_data_len_0"), None);
    }

    #[test]
    fn parse_acc_flags_byte_word_child() {
        assert_eq!(
            parse("n_acc0_signer"),
            Some(Parsed::AccFlagByte {
                acc: 0,
                flag: AccFlag::Signer
            })
        );
        assert_eq!(
            parse("n_acc12_writable"),
            Some(Parsed::AccFlagByte {
                acc: 12,
                flag: AccFlag::Writable
            })
        );
        assert_eq!(
            parse("w_acc7_executable"),
            Some(Parsed::AccFlagWord {
                acc: 7,
                flag: AccFlag::Executable
            })
        );
        assert_eq!(
            parse("w_acc7_executable_0"),
            Some(Parsed::AccFlagChild {
                acc: 7,
                flag: AccFlag::Executable
            })
        );
        assert_eq!(
            parse("w_acc0_writable"),
            Some(Parsed::AccFlagWord {
                acc: 0,
                flag: AccFlag::Writable
            })
        );
        assert_eq!(acc_flag_input("n_acc0_data_len_0"), None);
        assert_eq!(acc_flag_input("w_acc0_writable"), None);
    }

    #[test]
    fn constructors_roundtrip_hide_families() {
        assert_eq!(parse(&num_accounts_byte(0)), Some(Parsed::NumAccountsByte));
        assert_eq!(parse(&num_accounts_word()), Some(Parsed::NumAccountsWord));
        assert_eq!(
            parse(&num_accounts_child(0)),
            Some(Parsed::NumAccountsChild)
        );
        assert_eq!(
            parse(&acc_data_len_byte(0, 0)),
            Some(Parsed::AccDataLenByte { acc: 0 })
        );
        assert_eq!(
            parse(&acc_data_len_word(0)),
            Some(Parsed::AccDataLenWord { acc: 0 })
        );
        assert_eq!(
            parse(&acc_data_len_child(12, 3)),
            Some(Parsed::AccDataLenChild { acc: 12 })
        );
        assert_eq!(
            parse(&acc_flag_byte(1, AccFlag::Signer)),
            Some(Parsed::AccFlagByte {
                acc: 1,
                flag: AccFlag::Signer
            })
        );
        assert_eq!(
            parse(&acc_flag_word(1, AccFlag::Signer)),
            Some(Parsed::AccFlagWord {
                acc: 1,
                flag: AccFlag::Signer
            })
        );
        assert_eq!(
            parse(&acc_flag_child(7, AccFlag::Executable, 0)),
            Some(Parsed::AccFlagChild {
                acc: 7,
                flag: AccFlag::Executable
            })
        );
    }

    #[test]
    fn constructors_match_abi_spellings() {
        assert_eq!(num_accounts_byte(0), "n_num_accounts_00");
        assert_eq!(num_accounts_byte(7), "n_num_accounts_07");
        assert_eq!(num_accounts_word(), "w_num_accounts");
        assert_eq!(acc_dup(0), "n_acc0_dup");
        assert_eq!(acc_flag_byte(0, AccFlag::Signer), "n_acc0_signer");
        assert_eq!(acc_pubkey_byte(0, 0), "n_acc0_pubkey_00");
        assert_eq!(acc_data_len_word(0), "w_acc0_data_len");
        assert_eq!(acc_data_byte(2, 0), "n_acc2_data_0000");
        assert_eq!(acc_dup_pad_byte(1, 1), "n_acc1_dup_pad_1");
        assert_eq!(ix_data_len_word(), "w_ix_data_len");
        assert_eq!(ix_disc_word(), "w_ix_disc");
        assert_eq!(program_id_word(0), "w_program_id_0");
    }
}
