use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_pubkey::Pubkey;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateAccount {
    #[serde(serialize_with = "u64_str::serialize", deserialize_with = "u64_str::deserialize")]
    pub lamports: u64,
    #[serde(with = "hex32")]
    pub data: [u8; 32],
    pub owner: Pubkey,
    pub executable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "hex32_opt")]
    pub executable_data: Option<[u8; 32]>,
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

mod hex32_opt {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(h) => hex32::serialize(h, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(s) => {
                let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
                Ok(Some(bytes.try_into().map_err(|_| {
                    serde::de::Error::custom("sha256 hash must be 32 bytes")
                })?))
            }
        }
    }
}
