use seer_interface::GuestMemory;

pub trait GuestFetch<T> {
    /// Human-readable DWARF type name, e.g. "solana_account_info::AccountInfo"
    const DWARF_NAME: &'static str;

    /// Size in bytes, if known statically
    fn size_of() -> usize;

    /// Fetches and reconstructs the type from guest memory at given address
    fn fetch<M: GuestMemory>(mem: &mut M, addr: u64) -> T
    where
        Self: Sized;
}

pub fn fetch<T, M: GuestMemory>(mem: &mut M, addr: u64, size: u64) -> T {
    let raw_bytes = mem.read(addr, size);

    let raw: T = unsafe {
        std::ptr::read_unaligned(raw_bytes.as_ptr() as *const T)
    };

    raw
}