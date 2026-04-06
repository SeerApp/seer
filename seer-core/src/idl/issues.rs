use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Program identity for a loaded IDL (logged once per [`IdlIssues`], not repeated on every issue).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub struct IdlProgramContext {
    pub program_name: String,
    pub program_address: String,
}

impl IdlProgramContext {
    pub fn new(program_name: impl Into<String>, program_address: impl Into<String>) -> Self {
        Self {
            program_name: program_name.into(),
            program_address: program_address.into(),
        }
    }

    /// Placeholder when no program metadata is available (e.g. Anchor IDL without a declared address).
    pub fn unknown() -> Self {
        Self {
            program_name: "unknown".into(),
            program_address: "unknown".into(),
        }
    }
}

/// Where in the IDL (or decode) an issue applies — use real instruction/account/type names and paths.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub enum IdlLocation {
    /// Static IDL: instruction definition (`path`: argument name, then nested field/variant steps).
    SchemaInstruction {
        instruction: String,
        path: Vec<String>,
    },
    SchemaAccount {
        account: String,
        path: Vec<String>,
    },
    SchemaDefinedType {
        type_name: String,
        path: Vec<String>,
    },
    /// Decoding full instruction payload after a match.
    RuntimeInstruction {
        instruction: String,
        path: Vec<String>,
    },
    RuntimeAccount {
        account: String,
        path: Vec<String>,
    },
    /// Partial decode at discriminator offset only.
    DiscriminatorInstruction {
        instruction: String,
        path: Vec<String>,
    },
    DiscriminatorAccount {
        account: String,
        path: Vec<String>,
    },
}

impl fmt::Display for IdlLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdlLocation::SchemaInstruction { instruction, path } => {
                write!(f, "instruction `{instruction}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (IDL definition)")
            }
            IdlLocation::SchemaAccount { account, path } => {
                write!(f, "account `{account}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (IDL definition)")
            }
            IdlLocation::SchemaDefinedType { type_name, path } => {
                write!(f, "defined type `{type_name}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (IDL definition)")
            }
            IdlLocation::RuntimeInstruction { instruction, path } => {
                write!(f, "instruction `{instruction}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (decoding instruction data)")
            }
            IdlLocation::RuntimeAccount { account, path } => {
                write!(f, "account `{account}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (decoding account data)")
            }
            IdlLocation::DiscriminatorInstruction { instruction, path } => {
                write!(f, "instruction `{instruction}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (discriminator probe)")
            }
            IdlLocation::DiscriminatorAccount { account, path } => {
                write!(f, "account `{account}`")?;
                if !path.is_empty() {
                    write!(f, " → {}", path.join(" → "))?;
                }
                write!(f, " (discriminator probe)")
            }
        }
    }
}

/// One logical IDL issue we record at most once (and warn once). Program identity is on [`IdlIssues::context`], not here.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord, Error)]
pub enum IdlIssue {
    #[error("Skipping account `{account}`: multiple discriminators are not supported")]
    AccountNonUnaryDiscriminator { account: String },

    #[error("Skipping instruction `{instruction}`: multiple discriminators are not supported")]
    InstructionNonUnaryDiscriminator { instruction: String },

    #[error("Defined type link `{link_name}` has no matching `definedTypes` entry ({at})")]
    MissingDefinedTypeLink {
        link_name: String,
        at: IdlLocation,
    },

    #[error("Invalid layout: `bytes` or `string` is not last in the sequence and has no length ({at})")]
    InvalidLayoutBytesOrStringWithoutLength { at: IdlLocation },

    #[error("Invalid layout: `remainderOption` must be the last field in the sequence ({at})")]
    InvalidLayoutRemainderOptionNotLast { at: IdlLocation },

    #[error("Unsupported `sentinel` type ({at})")]
    UnsupportedSentinelType { at: IdlLocation },

    #[error("Unsupported `zeroableOption` type ({at})")]
    UnsupportedZeroableOptionType { at: IdlLocation },

    #[error("Insufficient bytes at {at}: need {needed} byte(s) for {operation}, {remaining} byte(s) remain in the buffer")]
    InsufficientBytes {
        at: IdlLocation,
        operation: String,
        needed: usize,
        remaining: usize,
    },

    #[error("Cursor offset out of buffer bounds at {at}")]
    CursorOffsetOutOfBounds { at: IdlLocation },

    #[error("Expected a value but decoded nothing at {at}: {detail}")]
    DecodeResidualInNonOptionalContext {
        at: IdlLocation,
        detail: String,
    },

    #[error("Enum discriminant did not match any variant at {at} (raw={raw_discriminant})")]
    EnumDiscriminantUnresolved {
        at: IdlLocation,
        raw_discriminant: String,
    },

    #[error("Prefixed collection count is not a valid non-negative length at {at}")]
    PrefixedCountInvalid { at: IdlLocation },

    #[error("Remainder collection decode did not advance the cursor at {at}")]
    RemainderDecodeCursorRegression { at: IdlLocation },

    #[error("Option discriminant must be 0 or 1 at {at} (got `{prefix}`)")]
    OptionDiscriminantInvalid {
        at: IdlLocation,
        prefix: String,
    },

    #[error("Size prefix is not a valid length at {at} (raw `{raw}`)")]
    SizePrefixInvalid {
        at: IdlLocation,
        raw: String,
    },

    #[error("Bytes literal in IDL failed to decode ({encoding}) at {at}")]
    BytesLiteralDecodeFailed {
        at: IdlLocation,
        encoding: String,
    },
}

#[derive(Debug)]
pub struct IdlIssues {
    pub context: IdlProgramContext,
    seen: HashSet<IdlIssue>,
}

impl IdlIssues {
    pub fn new(context: IdlProgramContext) -> Self {
        Self {
            context,
            seen: HashSet::new(),
        }
    }

    /// For callers without program metadata (e.g. Anchor placeholder).
    pub fn placeholder() -> Self {
        Self::new(IdlProgramContext::unknown())
    }

    pub fn has(&self, issue: &IdlIssue) -> bool {
        self.seen.contains(issue)
    }

    pub fn note(&mut self, issue: IdlIssue) {
        if self.seen.insert(issue.clone()) {
            crate::seer_warn!(
                "IDL {} ({}): {}",
                self.context.program_name,
                self.context.program_address,
                issue
            );
        }
    }

    pub fn note_account_non_unary_discriminator(
        &mut self,
        account: String,
        count: usize,
    ) {
        let issue = IdlIssue::AccountNonUnaryDiscriminator {
            account,
        };
        if self.seen.insert(issue.clone()) {
            crate::seer_warn!(
                "IDL {} ({}): {} (found {count} discriminators)",
                self.context.program_name,
                self.context.program_address,
                issue
            );
        }
    }

    pub fn note_instruction_non_unary_discriminator(
        &mut self,
        instruction: String,
        count: usize,
    ) {
        let issue = IdlIssue::InstructionNonUnaryDiscriminator { instruction };
        if self.seen.insert(issue.clone()) {
            crate::seer_warn!(
                "IDL {} ({}): {} (found {count} discriminators)",
                self.context.program_name,
                self.context.program_address,
                issue
            );
        }
    }

    pub fn sorted_issues(&self) -> Vec<IdlIssue> {
        let mut v: Vec<_> = self.seen.iter().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{init_seer_logger, SeerLogger};

    fn test_ctx() -> IdlProgramContext {
        IdlProgramContext::new("myProgram", "BKPmYX2xSgis75tMCm5fqpWdQyoJxhEW2EQ7EBnrngDj")
    }

    #[test]
    fn note_account_non_unary_discriminator_inserts_once() {
        init_seer_logger(SeerLogger::from_env());
        let mut issues = IdlIssues::new(test_ctx());
        issues.note_account_non_unary_discriminator("myAccount".into(), 2);
        issues.note_account_non_unary_discriminator("myAccount".into(), 3);
        assert_eq!(issues.sorted_issues().len(), 1);
    }

    #[test]
    fn note_instruction_non_unary_discriminator_inserts_once() {
        init_seer_logger(SeerLogger::from_env());
        let mut issues = IdlIssues::new(test_ctx());
        issues.note_instruction_non_unary_discriminator("myIx".into(), 2);
        issues.note_instruction_non_unary_discriminator("myIx".into(), 3);
        assert_eq!(issues.sorted_issues().len(), 1);
    }

    #[test]
    fn note_generic_dedupes() {
        init_seer_logger(SeerLogger::from_env());
        let mut issues = IdlIssues::new(test_ctx());
        let at = IdlLocation::SchemaInstruction {
            instruction: "x".into(),
            path: vec![],
        };
        issues.note(IdlIssue::PrefixedCountInvalid { at: at.clone() });
        issues.note(IdlIssue::PrefixedCountInvalid { at });
        assert_eq!(issues.sorted_issues().len(), 1);
    }
}
