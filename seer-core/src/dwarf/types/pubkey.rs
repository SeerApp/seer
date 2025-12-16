use seer_interface::GuestMemory;
use crate::dwarf::types::guest_fetch::{GuestFetch, fetch};
use solana_pubkey::Pubkey;

impl<'a> GuestFetch<Pubkey> for Pubkey {
    const DWARF_NAME: &'static str = "&solana_pubkey::Pubkey";

    fn size_of() -> usize {
        std::mem::size_of::<Self>()
    }

    fn fetch<M: GuestMemory>(mem: &mut M, addr: u64) -> Pubkey {
        fetch(mem, addr, Self::size_of() as u64)
    }
}