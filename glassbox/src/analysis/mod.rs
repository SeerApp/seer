//! Finished replay dump and how it is written.
//!
//! Path conditions and load defs were already rewritten at pack / ALU / branch
//! time. This module serializes the canonical JSON report. CLI hide / view
//! flags do not apply to that file.

mod filter;
mod formula;
mod options;
mod report;

use std::io::{self, Write};

pub use options::{DisplayOptions, HideDataLenChildren, HideMode, HideNumAccountChildren};
pub use report::{
    LoadDefJson, Minted, NoiseJson, OriginJson, PathConditionJson, RelJson, Report, Skipped,
};

use crate::state::Ledger;

/// Finished replay: the ledger (already rewritten) plus skip counts.
/// Produced by consuming a [`crate::vm::Vm`]; the scratchpad itself is gone.
pub struct Analysis {
    pub ledger: Ledger,
    pub memory_cells: usize,
    pub text_bytes: usize,
    pub input_bytes: usize,
    pub skipped_tautologies: u64,
    /// Path conditions dropped because they involve text but no real input.
    pub skipped_text_noise: u64,
}

impl Analysis {
    pub fn write(
        &self,
        out: &mut impl Write,
        run: i64,
        ix: i64,
        steps_applied: usize,
        missing_disasm: usize,
    ) -> io::Result<()> {
        let report = report::build(
            run,
            ix,
            steps_applied,
            missing_disasm,
            self.text_bytes,
            self.input_bytes,
            self.memory_cells,
            self.skipped_tautologies,
            self.skipped_text_noise,
            &self.ledger.path_conditions,
            &self.ledger.load_defs,
        );
        serde_json::to_writer_pretty(&mut *out, &report).map_err(io::Error::other)?;
        writeln!(out)?;
        Ok(())
    }
}
