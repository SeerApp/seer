pub mod anchor;
pub mod codama;
pub mod cursor;
pub mod issues;
pub mod parsed_arg;
pub mod types;

pub use issues::{IdlIssue, IdlIssues, IdlLocation, IdlProgramContext};

use crate::errors::IrrecoverableError;
use crate::idl::anchor::AnchorIdlLookup;
use crate::idl::codama::CodamaIdlLookup;
use crate::idl::types::{ParsedAccount, ParsedInstruction};
use crate::target_reader::Target;
use crate::tree::nodes::{RootChildren, TreeRoot};
use solana_instruction_error::InstructionError;
use std::{fs, path::PathBuf};
use thiserror::Error;

pub trait IdlTreeParser {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction>;

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount>;

    fn get_error(&self, error: InstructionError) -> String;
}

pub(crate) fn display_error_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub enum IdlLookup {
    Anchor(AnchorIdlLookup),
    Codama(CodamaIdlLookup),
}

impl IdlTreeParser for IdlLookup {
    fn get_instruction(&self, data: &[u8]) -> Option<ParsedInstruction> {
        match self {
            Self::Anchor(anchor) => anchor.get_instruction(data),
            Self::Codama(codama) => codama.get_instruction(data),
        }
    }

    fn get_account(&self, data: &[u8]) -> Option<ParsedAccount> {
        match self {
            Self::Anchor(anchor) => anchor.get_account(data),
            Self::Codama(codama) => codama.get_account(data),
        }
    }

    fn get_error(&self, error: InstructionError) -> String {
        match self {
            Self::Anchor(anchor) => anchor.get_error(error),
            Self::Codama(codama) => codama.get_error(error),
        }
    }
}

#[derive(Debug, Error)]
pub enum IdlLoadError {
    #[error("{0}")]
    Irrecoverable(#[from] IrrecoverableError),
    #[error("{0}")]
    Warning(String),
}

impl IdlLookup {
    pub fn new(idl_json: &str, source_label: &str) -> Result<Self, IdlLoadError> {
        match AnchorIdlLookup::from_json_str(&idl_json) {
            Ok(anchor) => Ok(Self::Anchor(anchor)),
            Err(anchor_err) => match CodamaIdlLookup::from_json_str(&idl_json) {
                Ok(codama) => Ok(Self::Codama(codama)),
                Err(codama_err) => Err(IdlLoadError::Warning(format!(
                    "The provided IDL source \"{}\" could not be parsed into either Anchor v0.30.0+ or Codama IDL. \
                    Anchor parse error: {anchor_err}; Codama parse error: {codama_err}",
                    source_label
                ))),
            },
        }
    }

    pub fn new_from_path(idl_path: &PathBuf) -> Result<Self, IdlLoadError> {
        let idl_json = fs::read_to_string(idl_path).map_err(|err| {
            IdlLoadError::Irrecoverable(IrrecoverableError::IdlFileOpen {
                filename: idl_path.to_string_lossy().to_string(),
                detail: err.to_string(),
            })
        })?;

        IdlLookup::new(&idl_json, &idl_path.to_string_lossy())
    }

    pub fn new_from_target(target: &Target) -> Result<Self, IdlLoadError> {
        let idl = target
            .idl
            .as_ref()
            .ok_or_else(|| IrrecoverableError::TargetFileOpen {
                target: target.base.clone(),
            })?;
        IdlLookup::new_from_path(idl)
    }

    pub fn parse_tree(&self, tree: &mut TreeRoot<RootChildren>) {
        match self {
            Self::Anchor(anchor) => {
                tree.parse(anchor);
            }
            Self::Codama(codama) => {
                tree.parse(codama);
            }
        }
    }
}
