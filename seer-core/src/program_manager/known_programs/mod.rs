use std::{
    collections::{hash_map::Entry, HashMap},
    str::FromStr,
};

use solana_pubkey::Pubkey;

use crate::{idl::IdlLookup, program_manager::program_manager::ProgramInfo};


const KNOWN_PROGRAMS: [(&str, &str); 7] = [
    ("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", include_str!("token_program.json")),
    ("11111111111111111111111111111111", include_str!("system_program.json")),
    ("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb", include_str!("token_2022_program.json")),
    ("Stake11111111111111111111111111111111111111", include_str!("stake_program.json")),
    ("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL", include_str!("spl_associated_token_account_program.json")),
    ("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr", include_str!("memo_program.json")),
    ("AddressLookupTab1e1111111111111111111111111", include_str!("adress_lookup_table_program.json"))
];

pub fn add_known_programs(inner: &mut HashMap<Pubkey, ProgramInfo>) {
    for (key, idl_lookup) in get_known_programs() {
        match inner.entry(key) {
            Entry::Occupied(mut existing) => existing.get_mut().set_idl_lookup(idl_lookup),
            Entry::Vacant(vacant) => {
                vacant.insert(ProgramInfo::with_idl_lookup(idl_lookup));
            }
        }
    }
}

pub fn get_known_programs() -> Vec<(Pubkey, IdlLookup)> {
    build_idl_lookups(&KNOWN_PROGRAMS)
}

pub fn build_idl_lookups(entries: &[(&str, &str)]) -> Vec<(Pubkey, IdlLookup)> {
    entries
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

#[cfg(test)]
mod test {
    use crate::program_manager::known_programs::{get_known_programs, KNOWN_PROGRAMS};

    #[test]
    fn test_known_programs() {
        assert_eq!(get_known_programs().len(), KNOWN_PROGRAMS.len());
    }
}
