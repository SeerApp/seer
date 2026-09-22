use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_pubkey::Pubkey;
use storage::Blob;

use crate::report::Report;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateAccount {
    #[serde(
        serialize_with = "u64_str::serialize",
        deserialize_with = "u64_str::deserialize"
    )]
    pub lamports: u64,
    #[serde(with = "hex32")]
    pub data: [u8; 32],
    #[serde(with = "b58")]
    pub owner: Pubkey,
    pub executable: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateAccounts(#[serde(with = "pubkey_map")] pub BTreeMap<Pubkey, StateAccount>);

impl StateAccounts {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self).context("state accounts JSON")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("state accounts JSON")
    }

    pub fn get_data(&self, blob: &Blob, pubkey: &Pubkey) -> std::result::Result<Vec<u8>, Report> {
        let Some(account) = self.0.get(pubkey) else {
            return Err(Report {
                missing_accounts: vec![*pubkey],
                ..Report::default()
            });
        };
        blob.read(&account.data).map_err(|_| Report {
            missing_data: vec![*pubkey],
            ..Report::default()
        })
    }

    pub fn get_executable_account(
        &self,
        blob: &Blob,
        pubkey: &Pubkey,
    ) -> std::result::Result<Option<Pubkey>, Report> {
        let data = self.get_data(blob, pubkey)?;
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

    pub fn verify(&self, blob: &Blob) -> std::result::Result<(), Report> {
        let mut report = Report::default();
        for (key, account) in &self.0 {
            match self.get_executable_account(blob, key) {
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

mod b58 {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Pubkey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Pubkey, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

mod pubkey_map {
    use super::*;
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use std::fmt;

    pub fn serialize<S: Serializer>(
        map: &BTreeMap<Pubkey, StateAccount>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            out.serialize_entry(&k.to_string(), v)?;
        }
        out.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<Pubkey, StateAccount>, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BTreeMap<Pubkey, StateAccount>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("map of base58 pubkeys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut map = BTreeMap::new();
                while let Some((k, v)) = a.next_entry::<String, StateAccount>()? {
                    map.insert(k.parse().map_err(serde::de::Error::custom)?, v);
                }
                Ok(map)
            }
        }
        d.deserialize_map(V)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn json_uses_base58_pubkeys() {
        let json = br#"{"11111111111111111111111111111111":{"lamports":"1","data":"0000000000000000000000000000000000000000000000000000000000000000","owner":"11111111111111111111111111111111","executable":false}}"#;
        let state = StateAccounts::from_bytes(json).unwrap();
        assert_eq!(state.to_bytes().unwrap(), json);
    }
}
