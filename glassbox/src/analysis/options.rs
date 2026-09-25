//! CLI-facing display options and hide-mode filters.

use crate::ancestry::AccFlag;
use crate::path_condition::ConstraintHide;

/// How aggressively to suppress a family of derived words / constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HideMode {
    /// Include in constraints and load defs.
    #[default]
    Off,
    /// Omit matching load defs unless another shown word depends on them.
    Words,
    /// Omit path conditions that only involve this family.
    Constraints,
    /// Both [`Words`] and [`Constraints`].
    Full,
}

impl HideMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "words" => Some(Self::Words),
            "constraints" => Some(Self::Constraints),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn hide_words(self) -> bool {
        matches!(self, Self::Words | Self::Full)
    }

    pub fn hide_constraints(self) -> bool {
        matches!(self, Self::Constraints | Self::Full)
    }

    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Words => Some("words"),
            Self::Constraints => Some("constraints"),
            Self::Full => Some("full"),
        }
    }
}

/// Back-compat alias for [`HideMode`] (num-accounts allocator children).
pub type HideNumAccountChildren = HideMode;

/// Which `w_acc{i}_data_len_{j}` allocator temps to suppress (words + constraints).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HideDataLenChildren {
    #[default]
    Off,
    /// Every account index.
    All,
    /// Bit `i` set ⇒ hide pure children of `acc{i}_data_len` (indices 0–127).
    Indices(u128),
}

impl HideDataLenChildren {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value == "full" {
            return Some(Self::All);
        }
        if value.is_empty() {
            return None;
        }
        let mut mask = 0u128;
        for part in value.split(',') {
            let idx: u32 = part.trim().parse().ok()?;
            if idx >= 128 {
                return None;
            }
            mask |= 1u128 << idx;
        }
        if mask == 0 {
            return None;
        }
        Some(Self::Indices(mask))
    }

    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }

    pub fn includes(self, account: u32) -> bool {
        match self {
            Self::Off => false,
            Self::All => true,
            Self::Indices(mask) => account < 128 && (mask & (1u128 << account)) != 0,
        }
    }

    pub fn display(self) -> Option<String> {
        match self {
            Self::Off => None,
            Self::All => Some("full".into()),
            Self::Indices(mask) => {
                let idxs: Vec<String> = (0..128u32)
                    .filter(|i| (mask & (1u128 << i)) != 0)
                    .map(|i| i.to_string())
                    .collect();
                Some(idxs.join(","))
            }
        }
    }
}

/// Controls how formulas and load defs are rendered.
#[derive(Debug, Clone, Copy)]
pub struct DisplayOptions {
    /// When true, emit complete formulas with no depth/budget truncation.
    pub full: bool,
    /// When true, emit raw SMT-LIB instead of formal-logic notation.
    pub smt: bool,
    /// When true, list every minted load temp; otherwise only `w_*` referenced
    /// in printed path conditions (including transitive load-def dependencies).
    pub all_load_defs: bool,
    /// When true, print only path conditions for branches that were taken.
    pub taken_only: bool,
    /// When true, colour branch outcome labels (green taken, red not-taken).
    pub color: bool,
    /// Suppress `w_num_accounts_{i}` noise in words and/or constraints.
    pub hide_num_account_children: HideMode,
    /// Suppress pure `w_acc{i}_data_len_{j}` noise in words and constraints.
    pub hide_data_len_children: HideDataLenChildren,
    /// Suppress signer-byte words and/or provision checks.
    pub hide_signer: HideMode,
    /// Suppress writable-byte words and/or provision checks.
    pub hide_writable: HideMode,
    /// Suppress executable-byte words and/or provision checks.
    pub hide_executable: HideMode,
}

/// Combined display filters for path conditions and load defs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HideFilters {
    pub num_account_children: HideMode,
    pub data_len_children: HideDataLenChildren,
    pub signer: HideMode,
    pub writable: HideMode,
    pub executable: HideMode,
}

impl HideFilters {
    pub(crate) fn hide_flag_words(self, flag: AccFlag) -> bool {
        match flag {
            AccFlag::Signer => self.signer.hide_words(),
            AccFlag::Writable => self.writable.hide_words(),
            AccFlag::Executable => self.executable.hide_words(),
        }
    }

    pub(crate) fn any_flag_words(self) -> bool {
        self.signer.hide_words() || self.writable.hide_words() || self.executable.hide_words()
    }

    #[allow(dead_code)]
    pub(crate) fn constraint_hide(self) -> ConstraintHide {
        ConstraintHide {
            num_accounts_children: self.num_account_children.hide_constraints(),
            data_len_all: matches!(self.data_len_children, HideDataLenChildren::All),
            data_len_mask: match self.data_len_children {
                HideDataLenChildren::Indices(m) => m,
                _ => 0,
            },
            signer: self.signer.hide_constraints(),
            writable: self.writable.hide_constraints(),
            executable: self.executable.hide_constraints(),
        }
    }
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            full: false,
            smt: false,
            all_load_defs: false,
            taken_only: false,
            color: false,
            hide_num_account_children: HideMode::Off,
            hide_data_len_children: HideDataLenChildren::Off,
            hide_signer: HideMode::Off,
            hide_writable: HideMode::Off,
            hide_executable: HideMode::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hide_data_len_children_parse_full_and_indices() {
        assert_eq!(
            HideDataLenChildren::parse("full"),
            Some(HideDataLenChildren::All)
        );
        let idxs = HideDataLenChildren::parse("0,1,4,5").unwrap();
        assert!(idxs.includes(0));
        assert!(idxs.includes(1));
        assert!(!idxs.includes(2));
        assert!(idxs.includes(4));
        assert!(idxs.includes(5));
        assert_eq!(idxs.display().as_deref(), Some("0,1,4,5"));
        assert!(HideDataLenChildren::parse("128").is_none());
        assert!(HideDataLenChildren::parse("words").is_none());
        assert!(HideDataLenChildren::Off.is_off());
    }
}
