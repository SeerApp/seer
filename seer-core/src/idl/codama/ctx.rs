use crate::idl::{IdlIssue, IdlIssues, IdlLocation};

/// What kind of payload is being decoded (drives [`IdlLocation`]).
#[derive(Clone, Debug)]
pub enum ParseSite {
    /// Full instruction body after match.
    RuntimeInstruction { name: String },
    RuntimeAccount { name: String },
    /// Discriminator probe (partial buffer at offset).
    DiscriminatorInstruction { name: String },
    DiscriminatorAccount { name: String },
}

/// Mutable context threaded through Codama IDL parsing (instruction/account data).
pub struct CodamaParseCtx<'a> {
    pub issues: &'a mut IdlIssues,
    pub site: ParseSite,
    /// Nested path: argument or field names, tuple indices, etc.
    pub path: Vec<String>,
}

impl<'a> CodamaParseCtx<'a> {
    pub fn current_location(&self) -> IdlLocation {
        match &self.site {
            ParseSite::RuntimeInstruction { name } => IdlLocation::RuntimeInstruction {
                instruction: name.clone(),
                path: self.path.clone(),
            },
            ParseSite::RuntimeAccount { name } => IdlLocation::RuntimeAccount {
                account: name.clone(),
                path: self.path.clone(),
            },
            ParseSite::DiscriminatorInstruction { name } => IdlLocation::DiscriminatorInstruction {
                instruction: name.clone(),
                path: self.path.clone(),
            },
            ParseSite::DiscriminatorAccount { name } => IdlLocation::DiscriminatorAccount {
                account: name.clone(),
                path: self.path.clone(),
            },
        }
    }

    pub fn push_path(&mut self, segment: impl Into<String>) {
        self.path.push(segment.into());
    }

    pub fn pop_path(&mut self) {
        let _ = self.path.pop();
    }

    pub fn note_insufficient_bytes(
        &mut self,
        needed: usize,
        remaining: usize,
        operation: impl Into<String>,
    ) {
        self.issues.note(IdlIssue::InsufficientBytes {
            at: self.current_location(),
            operation: operation.into(),
            needed,
            remaining,
        });
    }

    pub fn note_cursor_offset_oob(&mut self) {
        self.issues.note(IdlIssue::CursorOffsetOutOfBounds {
            at: self.current_location(),
        });
    }

    pub fn note_decode_residual(&mut self, detail: impl Into<String>) {
        self.issues.note(IdlIssue::DecodeResidualInNonOptionalContext {
            at: self.current_location(),
            detail: detail.into(),
        });
    }
}
