mod common;

use std::{fs, str::FromStr};

use seer_core::dwarf::manager::DwarfManager;
use solana_pubkey::Pubkey;

use crate::common::tests_fixtures_dir;

#[test]
fn test_read_pubkey() {
    let target_deploy_dir = tests_fixtures_dir().join("native/pubkey/target/deploy");

    let mut dwarf_manager = DwarfManager::new();
    dwarf_manager
        .set_dwarf_section(&target_deploy_dir.join("manager.debug"))
        .expect("load DWARF section");

    let pubkey_raw = fs::read_to_string(target_deploy_dir.join("manager-pubkey.json"))
        .expect("read manager-pubkey.json");
    let pubkey_str = pubkey_raw.trim().trim_matches('"');
    let expected_key = Pubkey::from_str(pubkey_str).expect("parse pubkey from fixture");

    assert_eq!(
        expected_key,
        Pubkey::from_str("28pGw7cozqAYyVeZsnGmYHhv7CD3YxQdzTXKS61Do4fD")
            .expect("parse expected pubkey constant")
    );

    let dwarf = dwarf_manager.get_dwarf().expect("dwarf after load");
    let mut units = dwarf.debug_info.units();
    assert!(
        units.next().is_ok(),
        "debug_info section should contain at least one unit"
    );
}
