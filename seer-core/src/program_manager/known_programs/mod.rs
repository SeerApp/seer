use std::{
    collections::{hash_map::Entry, HashMap},
    str::FromStr,
};

use codama_nodes::RootNode;
use solana_pubkey::Pubkey;

use crate::{idl::lookup::IdlLookup, program_manager::program_manager::ProgramInfo};


const KNOWN_PROGRAMS: [(&str, &str); 2] = [
    ("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", include_str!("token_program.json")),
    ("11111111111111111111111111111111", include_str!("system_program.json")),
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
            (
                Pubkey::from_str(key_str).unwrap(),
                IdlLookup::new(
                    serde_json::from_str::<RootNode>(idl_json)
                        .expect("Known program must have valid embedded IDL JSON"),
                ),
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
