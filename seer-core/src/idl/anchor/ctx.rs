use crate::idl::{IdlIssue, IdlIssues, IdlLocation};

/// What Anchor payload is being decoded (drives [`IdlLocation`]).
#[derive(Clone, Debug)]
pub enum AnchorParseSite {
    RuntimeInstruction { name: String },
    RuntimeAccount { name: String },
}

/// Mutable context threaded through Anchor IDL instruction/account data parsing.
pub struct AnchorParseCtx<'a> {
    pub issues: &'a mut IdlIssues,
    pub site: AnchorParseSite,
    /// Nested path: argument or field names, tuple indices, etc.
    pub path: Vec<String>,
}

impl<'a> AnchorParseCtx<'a> {
    pub fn current_location(&self) -> IdlLocation {
        match &self.site {
            AnchorParseSite::RuntimeInstruction { name } => IdlLocation::RuntimeInstruction {
                instruction: name.clone(),
                path: self.path.clone(),
            },
            AnchorParseSite::RuntimeAccount { name } => IdlLocation::RuntimeAccount {
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

    pub fn note_decode_residual(&mut self, detail: impl Into<String>) {
        self.issues.note(IdlIssue::DecodeResidualInNonOptionalContext {
            at: self.current_location(),
            detail: detail.into(),
        });
    }
}
