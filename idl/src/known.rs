//! Built-in immutable program IDL lookups.

use std::str::FromStr;

use once_cell::sync::Lazy;
use solana_pubkey::Pubkey;

use crate::IdlLookup;

/// Base58 address of the native Solana system program.
pub const SYSTEM_PROGRAM_ADDRESS: &str = "11111111111111111111111111111111";

/// Native system program id, parsed from [`SYSTEM_PROGRAM_ADDRESS`].
pub static SYSTEM_PROGRAM_PUBKEY: Lazy<Pubkey> =
    Lazy::new(|| Pubkey::from_str(SYSTEM_PROGRAM_ADDRESS).expect("system program id"));

const KNOWN_PROGRAMS: [(&str, &str); 8] = [
    (
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        include_str!("known/token_program.json"),
    ),
    (
        SYSTEM_PROGRAM_ADDRESS,
        include_str!("known/system_program.json"),
    ),
    (
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
        include_str!("known/token_2022_program.json"),
    ),
    (
        "Stake11111111111111111111111111111111111111",
        include_str!("known/stake_program.json"),
    ),
    (
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        include_str!("known/spl_associated_token_account_program.json"),
    ),
    (
        "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
        include_str!("known/memo_program.json"),
    ),
    (
        "AddressLookupTab1e1111111111111111111111111",
        include_str!("known/adress_lookup_table_program.json"),
    ),
    (
        "ComputeBudget111111111111111111111111111111",
        include_str!("known/compute_budget.json"),
    ),
];

pub fn get_known_programs() -> Vec<(Pubkey, IdlLookup)> {
    KNOWN_PROGRAMS
        .iter()
        .map(|(key_str, idl_json)| {
            let key = Pubkey::from_str(key_str).unwrap();
            (
                key,
                IdlLookup::new(idl_json, key_str)
                    .expect("Known program must have parseable embedded IDL JSON"),
            )
        })
        .collect()
}

pub fn builtin(program_id: &Pubkey) -> Option<IdlLookup> {
    get_known_programs()
        .into_iter()
        .find(|(id, _)| id == program_id)
        .map(|(_, lookup)| lookup)
}

#[cfg(test)]
mod test {
    use super::{get_known_programs, KNOWN_PROGRAMS};

    #[test]
    fn test_known_programs() {
        assert_eq!(get_known_programs().len(), KNOWN_PROGRAMS.len());
    }
}
