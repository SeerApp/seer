use seer_core::program_manager::known_programs::get_known_programs;
use seer_core::{init_seer_logger, SeerLogger};
use solana_pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Once;

static INIT_SEER_LOG: Once = Once::new();

fn ensure_seer_logger() {
    INIT_SEER_LOG.call_once(|| {
        init_seer_logger(SeerLogger::from_env());
    });
}

#[test]
fn known_program_idls_can_instantiate_idl_lookup() {
    ensure_seer_logger();
    let known_programs = get_known_programs();
    let expected_program_ids = [
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
        "11111111111111111111111111111111",
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
        "Stake11111111111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
        "AddressLookupTab1e1111111111111111111111111",
    ]
    .into_iter()
    .map(|id| Pubkey::from_str(id).expect("known program id must be valid pubkey"))
    .collect::<Vec<_>>();

    let actual_program_ids = known_programs.into_iter().map(|(id, _)| id).collect::<Vec<_>>();

    assert_eq!(
        actual_program_ids, expected_program_ids,
        "known programs and embedded IDLs should stay in sync",
    );
}
