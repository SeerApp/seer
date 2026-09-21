use std::collections::BTreeMap;

use anyhow::Result;
use solana_account::Account;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;

use crate::network::get_multiple_accounts;

pub trait MergeFetched {
    fn merge_fetched(&mut self, keys: &[Pubkey], url: &str) -> Result<()>;
}

impl MergeFetched for BTreeMap<Pubkey, Account> {
    fn merge_fetched(&mut self, keys: &[Pubkey], url: &str) -> Result<()> {
        if keys.is_empty() {
            return Ok(());
        }
        for (key, account) in keys.iter().zip(get_multiple_accounts(url, keys)?) {
            self.insert(*key, account.unwrap_or_default());
        }
        Ok(())
    }
}

pub trait GetProgramdata {
    fn get_programdata(&self) -> Option<Pubkey>;
}

impl GetProgramdata for Account {
    fn get_programdata(&self) -> Option<Pubkey> {
        if !solana_sdk_ids::bpf_loader_upgradeable::check_id(&self.owner) {
            return None;
        }
        match bincode::deserialize(&self.data) {
            Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) => Some(Pubkey::from(programdata_address.to_bytes())),
            _ => None,
        }
    }
}
