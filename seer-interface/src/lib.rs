pub trait GuestMemory {
    fn read(&mut self, addr: u64, len: u64) -> Vec<u8>;
}