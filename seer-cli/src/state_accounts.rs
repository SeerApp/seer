use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use storage::blobs::read_blob;

use crate::report::Report;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateAccount {
    #[serde(serialize_with = "u64_str::serialize", deserialize_with = "u64_str::deserialize")]
    pub lamports: u64,
    #[serde(with = "hex32")]
    pub data: [u8; 32],
    pub owner: Pubkey,
    pub executable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateAccounts(pub BTreeMap<Pubkey, StateAccount>);

impl StateAccounts {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("state accounts JSON")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("state accounts JSON")
    }

    pub fn get_data(&self, pubkey: &Pubkey) -> std::result::Result<Vec<u8>, Report> {
        let Some(account) = self.0.get(pubkey) else {
            return Err(Report {
                missing_accounts: vec![*pubkey],
                ..Report::default()
            });
        };
        read_blob(&account.data).map_err(|_| Report {
            missing_data: vec![*pubkey],
            ..Report::default()
        })
    }

    pub fn get_executable_account(&self, pubkey: &Pubkey) -> std::result::Result<Option<Pubkey>, Report> {
        let data = self.get_data(pubkey)?;
        let Some(account) = self.0.get(pubkey) else {
            return Err(Report {
                missing_accounts: vec![*pubkey],
                ..Report::default()
            });
        };
        if !solana_sdk_ids::bpf_loader_upgradeable::check_id(&account.owner) {
            return Ok(None);
        }
        match bincode::deserialize(&data) {
            Ok(UpgradeableLoaderState::Program {
                programdata_address,
            }) => Ok(Some(programdata_address)),
            Ok(_) => Ok(None),
            Err(_) => Err(Report {
                incoherent: vec![*pubkey],
                ..Report::default()
            }),
        }
    }

    pub fn verify(&self) -> std::result::Result<(), Report> {
        let mut report = Report::default();
        for (key, account) in &self.0 {
            match self.get_executable_account(key) {
                Err(e) => report.merge(e),
                Ok(executable_account) => {
                    if account.executable
                        && solana_sdk_ids::bpf_loader_upgradeable::check_id(&account.owner)
                            != executable_account.is_some()
                    {
                        report.incoherent.push(*key);
                    }
                    if let Some(programdata) = executable_account {
                        if !self.0.contains_key(&programdata) {
                            report.missing_accounts.push(programdata);
                        }
                    }
                }
            }
        }
        if report.is_empty() {
            Ok(())
        } else {
            Err(report)
        }
    }
}

mod u64_str {
    use super::*;

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

mod hex32 {
    use super::*;

    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let bytes = hex::decode(String::deserialize(d)?).map_err(serde::de::Error::custom)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("sha256 hash must be 32 bytes"))
    }
}
