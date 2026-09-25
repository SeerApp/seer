//! Solana syscall names we dispatch, emit UIFs for, or book as coverage gaps.

/// A syscall name that appears in traces. Unknown names stay [`None`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Syscall {
    Abort,
    Panic,
    Log,
    Log64,
    LogPubkey,
    LogComputeUnits,
    LogData,
    InvokeSignedRust,
    InvokeSignedC,
    Memcpy,
    Memmove,
    Memset,
    Memcmp,
    Sha256,
    Keccak256,
    Blake3,
    Sha512,
    Poseidon,
    CreateProgramAddress,
    TryFindProgramAddress,
    GetClockSysvar,
    GetRentSysvar,
    GetEpochScheduleSysvar,
    GetEpochRewardsSysvar,
    GetFeesSysvar,
    GetLastRestartSlot,
    GetSysvar,
    GetReturnData,
}

impl Syscall {
    /// Every variant. [`parse`] walks this so `as_str` is the only spelling.
    const ALL: &[Self] = &[
        Self::Abort,
        Self::Panic,
        Self::Log,
        Self::Log64,
        Self::LogPubkey,
        Self::LogComputeUnits,
        Self::LogData,
        Self::InvokeSignedRust,
        Self::InvokeSignedC,
        Self::Memcpy,
        Self::Memmove,
        Self::Memset,
        Self::Memcmp,
        Self::Sha256,
        Self::Keccak256,
        Self::Blake3,
        Self::Sha512,
        Self::Poseidon,
        Self::CreateProgramAddress,
        Self::TryFindProgramAddress,
        Self::GetClockSysvar,
        Self::GetRentSysvar,
        Self::GetEpochScheduleSysvar,
        Self::GetEpochRewardsSysvar,
        Self::GetFeesSysvar,
        Self::GetLastRestartSlot,
        Self::GetSysvar,
        Self::GetReturnData,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Abort => "abort",
            Self::Panic => "sol_panic_",
            Self::Log => "sol_log_",
            Self::Log64 => "sol_log_64_",
            Self::LogPubkey => "sol_log_pubkey",
            Self::LogComputeUnits => "sol_log_compute_units_",
            Self::LogData => "sol_log_data",
            Self::InvokeSignedRust => "sol_invoke_signed_rust",
            Self::InvokeSignedC => "sol_invoke_signed_c",
            Self::Memcpy => "sol_memcpy_",
            Self::Memmove => "sol_memmove_",
            Self::Memset => "sol_memset_",
            Self::Memcmp => "sol_memcmp_",
            Self::Sha256 => "sol_sha256",
            Self::Keccak256 => "sol_keccak256",
            Self::Blake3 => "sol_blake3",
            Self::Sha512 => "sol_sha512",
            Self::Poseidon => "sol_poseidon",
            Self::CreateProgramAddress => "sol_create_program_address",
            Self::TryFindProgramAddress => "sol_try_find_program_address",
            Self::GetClockSysvar => "sol_get_clock_sysvar",
            Self::GetRentSysvar => "sol_get_rent_sysvar",
            Self::GetEpochScheduleSysvar => "sol_get_epoch_schedule_sysvar",
            Self::GetEpochRewardsSysvar => "sol_get_epoch_rewards_sysvar",
            Self::GetFeesSysvar => "sol_get_fees_sysvar",
            Self::GetLastRestartSlot => "sol_get_last_restart_slot",
            Self::GetSysvar => "sol_get_sysvar",
            Self::GetReturnData => "sol_get_return_data",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.as_str() == name)
    }

    pub fn parse_uif(name: &str) -> Option<Self> {
        Self::parse(name.strip_prefix("uif_")?)
    }

    pub fn uif(self) -> String {
        format!("uif_{}", self.as_str())
    }

    pub fn sysvar_size(self) -> Option<u32> {
        match self {
            Self::GetClockSysvar => Some(40),
            Self::GetRentSysvar => Some(17),
            Self::GetEpochScheduleSysvar => Some(40),
            Self::GetEpochRewardsSysvar => Some(81),
            Self::GetFeesSysvar => Some(8),
            Self::GetLastRestartSlot => Some(8),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrips_as_str() {
        for &s in Syscall::ALL {
            assert_eq!(Syscall::parse(s.as_str()), Some(s), "{}", s.as_str());
            assert_eq!(Syscall::parse_uif(&s.uif()), Some(s));
        }
        assert_eq!(Syscall::parse("sol_log_"), Some(Syscall::Log));
        assert_eq!(
            Syscall::parse_uif("uif_sol_create_program_address"),
            Some(Syscall::CreateProgramAddress)
        );
        assert_eq!(Syscall::parse("sol_unknown"), None);
        assert_eq!(Syscall::GetRentSysvar.sysvar_size(), Some(17));
        assert_eq!(Syscall::Log.sysvar_size(), None);
    }

    #[test]
    fn all_covers_every_variant() {
        fn touch(s: Syscall) {
            match s {
                Syscall::Abort
                | Syscall::Panic
                | Syscall::Log
                | Syscall::Log64
                | Syscall::LogPubkey
                | Syscall::LogComputeUnits
                | Syscall::LogData
                | Syscall::InvokeSignedRust
                | Syscall::InvokeSignedC
                | Syscall::Memcpy
                | Syscall::Memmove
                | Syscall::Memset
                | Syscall::Memcmp
                | Syscall::Sha256
                | Syscall::Keccak256
                | Syscall::Blake3
                | Syscall::Sha512
                | Syscall::Poseidon
                | Syscall::CreateProgramAddress
                | Syscall::TryFindProgramAddress
                | Syscall::GetClockSysvar
                | Syscall::GetRentSysvar
                | Syscall::GetEpochScheduleSysvar
                | Syscall::GetEpochRewardsSysvar
                | Syscall::GetFeesSysvar
                | Syscall::GetLastRestartSlot
                | Syscall::GetSysvar
                | Syscall::GetReturnData => {}
            }
        }
        for &s in Syscall::ALL {
            touch(s);
        }
        assert_eq!(Syscall::ALL.len(), 28);
    }
}
