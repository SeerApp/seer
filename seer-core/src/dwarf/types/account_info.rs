use seer_interface::GuestMemory;
use serde::{Serialize, Deserialize};
use solana_pubkey::Pubkey;
use crate::dwarf::types::guest_fetch::{fetch, GuestFetch};

mod pubkey_as_base58 {
    use std::str::FromStr;

    use super::*;
    use serde::{Serializer, Deserializer};

    pub fn serialize<S>(key: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&key.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Pubkey::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountInfoFlat {
    #[serde(with = "pubkey_as_base58")]
    pub key: Pubkey,
    pub lamports: u64,
    pub data: Vec<u8>,
    #[serde(with = "pubkey_as_base58")]
    pub owner: Pubkey,
    pub rent_epoch: u64,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,
}

#[repr(C)]
pub struct AccountInfoRepr {
    pub key_ptr: u64,
    pub lamports_ptr: u64,
    pub data_ptr: u64,
    pub owner_ptr: u64,
    pub rent_epoch: u64,
    pub is_signer: bool,
    pub is_writable: bool,
    pub executable: bool,
}

impl<'a> GuestFetch<AccountInfoFlat> for AccountInfoRepr {
    const DWARF_NAME: &'static str = "&solana_account_info::AccountInfo";

    fn size_of() -> usize { std::mem::size_of::<Self>() }

    fn fetch<M: GuestMemory>(mem: &mut M, addr: u64) -> AccountInfoFlat {
        let raw: Self = fetch(mem, addr, Self::size_of() as u64);

        let lamports_rc_inner_ptr: u64 = raw.lamports_ptr;
        let lamports_refcell_ptr = lamports_rc_inner_ptr + 2 * 8;
        let lamports_ref_ptr: u64 = fetch(mem, lamports_refcell_ptr + 8, 8);
        let lamports: u64 = fetch(mem, lamports_ref_ptr, 8);

        let data_rc_inner_ptr: u64 = raw.data_ptr;
        let data_refcell_ptr = data_rc_inner_ptr + 2 * 8;
        let data_slice_ptr: u64 = fetch(mem, data_refcell_ptr + 8, 8);
        let data_slice_len: u64 = fetch(mem, data_refcell_ptr + 16, 8);

        let data_bytes: Vec<u8> = (0..data_slice_len)
            .map(|i| fetch(mem, data_slice_ptr + i, 1))
            .collect();

        #[allow(deprecated)]
        let account_info: AccountInfoFlat = AccountInfoFlat 
        { 
            key: fetch(mem, raw.key_ptr, std::mem::size_of::<Pubkey>() as u64), 
            lamports: lamports, 
            data: data_bytes, 
            owner: fetch(mem, raw.owner_ptr, std::mem::size_of::<Pubkey>() as u64), 
            rent_epoch: raw.rent_epoch, 
            is_signer: raw.is_signer, 
            is_writable: raw.is_writable, 
            executable: raw.executable,
        };

        account_info
    }
}