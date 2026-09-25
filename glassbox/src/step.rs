/// One decoded SBPF execution step (input-agnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub order: u64,
    pub pc: u64,
    /// PC of the following step, if any (used to decide whether a branch was taken).
    pub next_pc: Option<u64>,
    pub disasm: String,
    /// Concrete registers before the instruction.
    pub pre_regs: [u64; 11],
    /// Concrete registers after the instruction.
    pub post_regs: [u64; 11],
}
