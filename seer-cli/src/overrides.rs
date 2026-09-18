use anyhow::{bail, Result};
use litesvm::LiteSVM;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_address::Address;
use solana_clock::Clock;
use solana_compute_budget::compute_budget::ComputeBudget;
use solana_hash::Hash;
use solana_pubkey::Pubkey;
use solana_slot_hashes::SlotHashes;
use solana_stake_interface::stake_history::{StakeHistory, StakeHistoryEntry};
#[allow(deprecated)]
use solana_sysvar::{
    fees::Fees,
    recent_blockhashes::{IterItem, RecentBlockhashes},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Airdrop {
    #[serde(with = "pubkey_b58")]
    pub address: Pubkey,
    pub lamports: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_unit_limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instruction_stack_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_instruction_trace_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_call_depth: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch_start_timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unix_timestamp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader_schedule_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_stake: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activating_stake: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deactivating_stake: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", with = "hash_b58")]
    pub slot_hash: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none", with = "hash_b58")]
    pub blockhash: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub airdrop: Vec<Airdrop>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sigverify: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub blockhash_check: bool,
}

impl Overrides {
    pub fn apply(&self) -> LiteSVM {
        let mut svm = LiteSVM::new()
            .with_sigverify(self.sigverify)
            .with_blockhash_check(self.blockhash_check);
        if let Some(budget) = self.compute_budget() {
            svm = svm.with_compute_budget(budget);
        }
        self.apply_sysvars(&mut svm);
        svm
    }

    pub fn airdrop(&self, svm: &mut LiteSVM) -> Result<()> {
        for Airdrop { address, lamports } in &self.airdrop {
            if let Err(failed) = svm.airdrop(&Address::from(address.to_bytes()), *lamports) {
                bail!("airdrop to {address}: {}", failed.err);
            }
        }
        Ok(())
    }

    fn compute_budget(&self) -> Option<ComputeBudget> {
        if self.compute_unit_limit.is_none()
            && self.max_instruction_stack_depth.is_none()
            && self.max_instruction_trace_length.is_none()
            && self.max_call_depth.is_none()
            && self.heap_size.is_none()
        {
            return None;
        }
        let mut budget = ComputeBudget::new_with_defaults(false, false);
        if let Some(v) = self.compute_unit_limit {
            budget.compute_unit_limit = v;
        }
        if let Some(v) = self.max_instruction_stack_depth {
            budget.max_instruction_stack_depth = v;
        }
        if let Some(v) = self.max_instruction_trace_length {
            budget.max_instruction_trace_length = v;
        }
        if let Some(v) = self.max_call_depth {
            budget.max_call_depth = v;
        }
        if let Some(v) = self.heap_size {
            budget.heap_size = v;
        }
        Some(budget)
    }

    fn apply_sysvars(&self, svm: &mut LiteSVM) {
        let mut clock = svm.get_sysvar::<Clock>();
        if let Some(v) = self.slot {
            clock.slot = v;
        }
        if let Some(v) = self.epoch {
            clock.epoch = v;
        }
        if let Some(v) = self.epoch_start_timestamp {
            clock.epoch_start_timestamp = v;
        }
        if let Some(v) = self.unix_timestamp {
            clock.unix_timestamp = v;
        }
        if let Some(v) = self.leader_schedule_epoch {
            clock.leader_schedule_epoch = v;
        }
        if self.slot.is_some()
            || self.epoch.is_some()
            || self.epoch_start_timestamp.is_some()
            || self.unix_timestamp.is_some()
            || self.leader_schedule_epoch.is_some()
        {
            svm.set_sysvar(&clock);
        }
        if self.effective_stake.is_some()
            || self.activating_stake.is_some()
            || self.deactivating_stake.is_some()
        {
            let mut history = svm.get_sysvar::<StakeHistory>();
            history.add(
                clock.epoch,
                StakeHistoryEntry {
                    effective: self.effective_stake.unwrap_or(0),
                    activating: self.activating_stake.unwrap_or(0),
                    deactivating: self.deactivating_stake.unwrap_or(0),
                },
            );
            svm.set_sysvar(&history);
        }
        if let Some(slot_hash) = self.slot_hash {
            svm.set_sysvar(&SlotHashes::new(&[(clock.slot, slot_hash.to_bytes().into())]));
        }
        if let Some(blockhash) = self.blockhash {
            #[allow(deprecated)]
            let fees = Fees::default();
            let hash = blockhash.to_bytes().into();
            #[allow(deprecated)]
            svm.set_sysvar(&RecentBlockhashes::from_iter([IterItem(
                0,
                &hash,
                fees.fee_calculator.lamports_per_signature,
            )]));
        }
    }
}

mod hash_b58 {
    use super::*;

    pub fn serialize<S: Serializer>(h: &Option<Hash>, s: S) -> Result<S::Ok, S::Error> {
        match h {
            Some(h) => s.serialize_some(&h.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Hash>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| s.parse().map_err(serde::de::Error::custom))
            .transpose()
    }
}

mod pubkey_b58 {
    use super::*;

    pub fn serialize<S: Serializer>(k: &Pubkey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&k.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Pubkey, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
