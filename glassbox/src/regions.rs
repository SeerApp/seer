//! Solana SBF virtual memory region bases (byte addresses).

pub const TEXT_BASE: u64 = 0x1_0000_0000;
pub const TEXT_END: u64 = 0x2_0000_0000;

pub const INPUT_BASE: u64 = 0x4_0000_0000;
pub const INPUT_END: u64 = 0x5_0000_0000;

/// Decimal width for region-relative byte symbol suffixes (`n_…`, `t_…`).
pub const REGION_OFFSET_WIDTH: usize = 10;

#[inline]
pub fn is_text(addr: u64) -> bool {
    (TEXT_BASE..TEXT_END).contains(&addr)
}

#[inline]
pub fn is_input(addr: u64) -> bool {
    (INPUT_BASE..INPUT_END).contains(&addr)
}

#[inline]
pub fn is_interesting(addr: u64) -> bool {
    is_text(addr) || is_input(addr)
}

#[inline]
pub fn text_offset(addr: u64) -> u64 {
    debug_assert!(is_text(addr));
    addr - TEXT_BASE
}

#[inline]
pub fn input_offset(addr: u64) -> u64 {
    debug_assert!(is_input(addr));
    addr - INPUT_BASE
}

/// Symbol name for a text-region byte at `addr` (e.g. `t_0000035230`).
pub fn text_symbol_name(addr: u64) -> String {
    format!(
        "t_{:0width$}",
        text_offset(addr),
        width = REGION_OFFSET_WIDTH
    )
}

/// Symbol name for an input-region byte at `addr` (e.g. `n_0000020856`).
pub fn input_symbol_name(addr: u64) -> String {
    format!(
        "n_{:0width$}",
        input_offset(addr),
        width = REGION_OFFSET_WIDTH
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_names_use_region_relative_decimal_offsets() {
        assert_eq!(text_symbol_name(TEXT_BASE), "t_0000000000");
        assert_eq!(text_symbol_name(TEXT_BASE + 0x5602e), "t_0000352302");
        assert_eq!(input_symbol_name(INPUT_BASE), "n_0000000000");
        assert_eq!(input_symbol_name(INPUT_BASE + 0x5178), "n_0000020856");
    }
}
