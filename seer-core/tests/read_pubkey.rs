use std::str::FromStr;

use seer_core::{dwarf::manager::DwarfManager, get_cwd};
use solana_pubkey::Pubkey;

#[test]
fn test_read_pubkey() {
    let mut target_deploy_dir = get_cwd();
    target_deploy_dir.push("tests/fixtures/native/pubkey/target/deploy");

    let dwarf_manger = DwarfManager::new(&target_deploy_dir);

    let final_key = *dwarf_manger.get_pubkeys()[0];

    assert!(
        final_key
            == Pubkey::from_str("28pGw7cozqAYyVeZsnGmYHhv7CD3YxQdzTXKS61Do4fD")
                .ok()
                .unwrap()
    );

    let dwarf = dwarf_manger.get_dwarf(&final_key).unwrap();
    let mut units = dwarf.debug_info.units();
    assert!(
        units.next().is_ok(),
        "debug_info section should contain at least one unit"
    );
}
