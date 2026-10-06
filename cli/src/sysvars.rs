use anyhow::{bail, Result};
#[cfg(test)]
use solana_account::Account;
use solana_pubkey::Pubkey;
use solana_sdk_ids::sysvar::{
    clock, epoch_rewards, epoch_schedule, last_restart_slot, rent, slot_hashes, stake_history,
};
use solana_sysvar::{
    clock::Clock, epoch_rewards::EpochRewards, epoch_schedule::EpochSchedule,
    last_restart_slot::LastRestartSlot, rent::Rent, slot_hashes::SlotHashes,
    stake_history::StakeHistory,
};
#[cfg(test)]
use std::collections::BTreeMap;

pub fn include(keys: &mut Vec<Pubkey>) {
    for key in keys_of() {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
}

pub fn require<'a>(get: impl Fn(&Pubkey) -> Option<&'a [u8]>) -> Result<()> {
    for key in keys_of() {
        let Some(data) = get(&key) else {
            bail!("missing sysvar {key}");
        };
        if !decodes(&key, data) {
            bail!("invalid sysvar {key}");
        }
    }
    Ok(())
}

fn keys_of() -> [Pubkey; 7] {
    [
        Pubkey::from(clock::id().to_bytes()),
        Pubkey::from(epoch_schedule::id().to_bytes()),
        Pubkey::from(rent::id().to_bytes()),
        Pubkey::from(epoch_rewards::id().to_bytes()),
        Pubkey::from(last_restart_slot::id().to_bytes()),
        Pubkey::from(stake_history::id().to_bytes()),
        Pubkey::from(slot_hashes::id().to_bytes()),
    ]
}

fn decodes(key: &Pubkey, data: &[u8]) -> bool {
    let keys = keys_of();
    if key == &keys[0] {
        bincode::deserialize::<Clock>(data).is_ok()
    } else if key == &keys[1] {
        bincode::deserialize::<EpochSchedule>(data).is_ok()
    } else if key == &keys[2] {
        bincode::deserialize::<Rent>(data).is_ok()
    } else if key == &keys[3] {
        bincode::deserialize::<EpochRewards>(data).is_ok()
    } else if key == &keys[4] {
        bincode::deserialize::<LastRestartSlot>(data).is_ok()
    } else if key == &keys[5] {
        bincode::deserialize::<StakeHistory>(data).is_ok()
    } else {
        wincode::deserialize::<SlotHashes>(data).is_ok()
    }
}

#[cfg(test)]
pub(crate) fn valid_for_test() -> BTreeMap<Pubkey, Option<Account>> {
    let keys = keys_of();
    let clock: Clock = Default::default();
    let epoch_schedule: EpochSchedule = Default::default();
    let rent: Rent = Default::default();
    let epoch_rewards: EpochRewards = Default::default();
    let last_restart_slot: LastRestartSlot = Default::default();
    let stake_history: StakeHistory = Default::default();
    let slot_hashes: SlotHashes = Default::default();
    let blobs = [
        bincode::serialize(&clock).unwrap(),
        bincode::serialize(&epoch_schedule).unwrap(),
        bincode::serialize(&rent).unwrap(),
        bincode::serialize(&epoch_rewards).unwrap(),
        bincode::serialize(&last_restart_slot).unwrap(),
        bincode::serialize(&stake_history).unwrap(),
        wincode::serialize(&slot_hashes).unwrap(),
    ];
    keys.into_iter()
        .zip(blobs)
        .map(|(pubkey, data)| {
            (
                pubkey,
                Some(Account {
                    lamports: 1,
                    data,
                    owner: Pubkey::from(solana_sdk_ids::sysvar::id().to_bytes()),
                    executable: false,
                    rent_epoch: 0,
                }),
            )
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    fn bytes<'a>(map: &'a BTreeMap<Pubkey, Option<Account>>, key: &Pubkey) -> Option<&'a [u8]> {
        map.get(key)
            .and_then(|slot| slot.as_ref())
            .map(|account| account.data.as_slice())
    }

    #[test]
    fn a_missing_sysvar_fails_and_the_seven_pass() {
        let full = valid_for_test();
        assert!(require(|key| bytes(&full, key)).is_ok());
        let mut missing = full.clone();
        let clock = keys_of()[0];
        missing.remove(&clock);
        let err = require(|key| bytes(&missing, key)).unwrap_err();
        assert!(err.to_string().contains("missing sysvar"), "{err}");
        let mut bad = valid_for_test();
        bad.get_mut(&clock).unwrap().as_mut().unwrap().data = vec![1, 2, 3];
        let err = require(|key| bytes(&bad, key)).unwrap_err();
        assert!(err.to_string().contains("invalid sysvar"), "{err}");
    }
}
