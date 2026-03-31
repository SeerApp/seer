use std::{
    collections::{hash_map::Entry, HashMap},
    str::FromStr,
};

use codama_nodes::RootNode;
use solana_pubkey::Pubkey;

use crate::{idl::lookup::IdlLookup, program_manager::program_manager::ProgramInfo};


const KNOWN_PROGRAMS: [(&str, &str); 1] = [
    ("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA", include_str!("token_program.json"))
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
    KNOWN_PROGRAMS
        .iter()
        .map(|(key_str, idl_json)| get_known_program(key_str, idl_json))
        .collect()
}

fn get_known_program(key_str: &str, idl_json: &str) -> (Pubkey, IdlLookup) {
    (
        Pubkey::from_str(key_str).unwrap(),
        IdlLookup::new(
            serde_json::from_str::<RootNode>(idl_json)
                .expect("Known program must have valid embedded IDL JSON"),
        ),
    )
}

#[cfg(test)]
mod test {
    use crate::program_manager::known_programs::{get_known_programs, KNOWN_PROGRAMS};

    #[test]
    fn test_known_programs() {
        assert_eq!(get_known_programs().len(), KNOWN_PROGRAMS.len());
    }
}
