//! Coverage gaps: work present in the trace that the interpreter did not model.
//!
//! These are recorded for later product decisions. They are not printed on
//! stdout (goldens stay stable). See [`format_coverage`].

use std::collections::BTreeMap;
use std::fmt;

use crate::step::Step;

/// Why a trace step (or a whole program run) was not interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SkipReason {
    /// `step.disasm` was empty (PC has no line in `programs/<id>/disasm`).
    EmptyDisasm,
    /// Disasm did not parse to a known SBPF op.
    UnknownOp,
    /// Syscall we do not model yet (hashes, return data, unrecognized name).
    UnhandledSyscall,
    /// Adapter could not load a program invocation (missing disasm dir, …).
    ProgramLoad,
}

impl SkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyDisasm => "empty_disasm",
            Self::UnknownOp => "unknown_op",
            Self::UnhandledSyscall => "unhandled_syscall",
            Self::ProgramLoad => "program_load",
        }
    }
}

/// One coverage gap. `order` / `pc` are zero for [`SkipReason::ProgramLoad`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub reason: SkipReason,
    pub order: u64,
    pub pc: u64,
    pub detail: String,
}

impl Skip {
    pub fn empty_disasm(step: &Step) -> Self {
        Self {
            reason: SkipReason::EmptyDisasm,
            order: step.order,
            pc: step.pc,
            detail: String::new(),
        }
    }

    pub fn unknown_op(step: &Step) -> Self {
        Self {
            reason: SkipReason::UnknownOp,
            order: step.order,
            pc: step.pc,
            detail: step.disasm.clone(),
        }
    }

    pub fn unhandled_syscall(step: &Step, name: &str) -> Self {
        Self {
            reason: SkipReason::UnhandledSyscall,
            order: step.order,
            pc: step.pc,
            detail: name.to_string(),
        }
    }

    pub fn program_load(ix: u8, program_id: &str, err: impl fmt::Display) -> Self {
        Self {
            reason: SkipReason::ProgramLoad,
            order: 0,
            pc: 0,
            detail: format!("{ix}/{program_id}: {err}"),
        }
    }
}

/// Stderr-oriented summary. Stable key order; suitable to diff later.
pub fn format_coverage(skips: &[Skip]) -> String {
    let mut counts: BTreeMap<SkipReason, usize> = BTreeMap::new();
    let mut details: BTreeMap<SkipReason, BTreeMap<String, usize>> = BTreeMap::new();
    for skip in skips {
        *counts.entry(skip.reason).or_insert(0) += 1;
        if skip.detail.is_empty() {
            continue;
        }
        *details
            .entry(skip.reason)
            .or_default()
            .entry(skip.detail.clone())
            .or_insert(0) += 1;
    }

    let mut out = format!(
        "# coverage skips={} empty_disasm={} unknown_op={} unhandled_syscall={} program_load={}",
        skips.len(),
        counts.get(&SkipReason::EmptyDisasm).copied().unwrap_or(0),
        counts.get(&SkipReason::UnknownOp).copied().unwrap_or(0),
        counts
            .get(&SkipReason::UnhandledSyscall)
            .copied()
            .unwrap_or(0),
        counts.get(&SkipReason::ProgramLoad).copied().unwrap_or(0),
    );
    for reason in [
        SkipReason::UnknownOp,
        SkipReason::UnhandledSyscall,
        SkipReason::ProgramLoad,
    ] {
        let Some(map) = details.get(&reason) else {
            continue;
        };
        if map.is_empty() {
            continue;
        }
        out.push('\n');
        out.push_str(&format!("#   {}:", reason.as_str()));
        for (detail, n) in map {
            if *n == 1 {
                out.push_str(&format!(" {detail}"));
            } else {
                out.push_str(&format!(" {detail}×{n}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(disasm: &str) -> Step {
        Step {
            order: 7,
            pc: 0x100,
            next_pc: None,
            disasm: disasm.into(),
            pre_regs: [0; 11],
            post_regs: [0; 11],
        }
    }

    #[test]
    fn format_zero_skips() {
        assert_eq!(
            format_coverage(&[]),
            "# coverage skips=0 empty_disasm=0 unknown_op=0 unhandled_syscall=0 program_load=0"
        );
    }

    #[test]
    fn format_groups_details() {
        let skips = vec![
            Skip::unhandled_syscall(&step("syscall sol_sha256"), "sol_sha256"),
            Skip::unhandled_syscall(&step("syscall sol_sha256"), "sol_sha256"),
            Skip::unhandled_syscall(&step("syscall sol_keccak256"), "sol_keccak256"),
            Skip::unknown_op(&step("hor64 r1, r2")),
            Skip::program_load(1, "Tokenkeg", "disasm dir missing"),
            Skip::empty_disasm(&step("")),
        ];
        let s = format_coverage(&skips);
        assert!(s.starts_with(
            "# coverage skips=6 empty_disasm=1 unknown_op=1 unhandled_syscall=3 program_load=1"
        ));
        assert!(s.contains("#   unknown_op: hor64 r1, r2"));
        assert!(s.contains("sol_keccak256"));
        assert!(s.contains("sol_sha256×2"));
        assert!(s.contains("#   program_load: 1/Tokenkeg: disasm dir missing"));
    }
}
