//! Spellings for environmental Z3 consts and Solana syscall names.

mod names;
mod syscall;

pub use names::*;
pub use syscall::Syscall;

pub fn is_input(name: &str) -> bool {
    name.starts_with("n_")
}

pub fn is_text(name: &str) -> bool {
    name.starts_with("t_")
}

pub fn is_word(name: &str) -> bool {
    name.starts_with("w_")
}

pub fn is_syscall_uif(name: &str) -> bool {
    name.starts_with("uif_")
}

pub fn is_recovered_uif(name: &str) -> bool {
    matches!(
        name,
        "clz"
            | "ctz"
            | "popcnt"
            | "bswap"
            | "ror"
            | "rol"
            | "uitofp"
            | "sitofp"
            | "f64_mul"
            | "f64_exp"
            | "f64_sign"
            | "f64_mant"
            | "fptoui"
            | "fptosi"
            | "umul128_hi"
            | "umul128_lo"
            | "sipround0"
            | "sipround1"
            | "sipround2"
            | "sipround3"
            | "sipc0"
            | "sipc1"
            | "sipc2"
            | "sipc3"
            | "sipd0"
            | "sipd1"
            | "sipd2"
            | "sipd3"
            | "siphash13"
            | "siphash24"
    )
}

pub fn is_pack(name: &str) -> bool {
    name.starts_with("pack_")
}

pub fn is_env_ident(name: &str) -> bool {
    is_input(name) || is_text(name) || is_word(name) || is_syscall_uif(name) || is_pack(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_match_env_idents() {
        assert!(is_env_ident("n_acc0_dup"));
        assert!(is_env_ident("t_0000000001"));
        assert!(is_env_ident("w_num_accounts"));
        assert!(is_env_ident("uif_sol_get_rent_sysvar"));
        assert!(is_syscall_uif("uif_sol_get_rent_sysvar"));
        assert!(is_recovered_uif("f64_mul"));
        assert!(is_recovered_uif("ror"));
        assert!(is_recovered_uif("ctz"));
        assert!(is_recovered_uif("sipround0"));
        assert!(is_recovered_uif("siphash13"));
        assert!(!is_env_ident("f64_mul"));
        assert!(is_env_ident("pack_0"));
        assert!(!is_env_ident("bvadd"));
        assert!(!is_env_ident("x_acc0"));
    }
}
