use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use solana_instruction::error::InstructionError;
use solana_pubkey::Pubkey;

use crate::{
    atomic_file_writer::AtomicFileWriter,
    tree::nodes::account::TreeAccount,
};

#[derive(Serialize, Deserialize, Debug)]
pub enum ExecutionEvent {
    StartProgram(Pubkey),
    EndProgram(Option<InstructionError>),
    Step(u64),
    Log(String),
    AccountDiff(TreeAccount),
}

pub struct Analysis {
    pub sig: String,
    pub instruction: u8,
    pub events: Vec<ExecutionEvent>,
    pub read_only: bool,
}

impl Analysis {
    pub fn new(sig: String, instruction: u8) -> Self {
        Self {
            sig,
            instruction,
            events: vec![],
            read_only: false,
        }
    }

    pub fn save(self, file_writer: &AtomicFileWriter) {
        let data = serde_json::to_string_pretty(&self.events).ok().unwrap();
        let filename = format!("analysis_{}_{}", self.sig, self.instruction);
        file_writer.save_loose_file(&data, &filename, "json", false, false);
    }

    pub fn load(folder: &PathBuf, sig: String, instruction: u8) -> Self {
        let path = folder.join(format!("analysis_{}_{}.json", sig, instruction));

        let events = if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                serde_json::from_str::<Vec<ExecutionEvent>>(&content).unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Self {
            sig,
            instruction,
            events,
            read_only: true,
        }
    }

    fn is_writable(&self) {
        if self.read_only {
            panic!("Cannot mutate read only Analysis struct");
        }
    }

    pub fn start_program(&mut self, program_address: Pubkey) {
        self.is_writable();

        self.events
            .push(ExecutionEvent::StartProgram(program_address));
    }

    pub fn end_program(&mut self, err: Option<InstructionError>) {
        self.is_writable();

        self.events.push(ExecutionEvent::EndProgram(err));
    }

    pub fn step(&mut self, i: u64) {
        self.is_writable();

        self.events.push(ExecutionEvent::Step(i));
    }

    pub fn log(&mut self, message: &str) {
        self.is_writable();

        self.events.push(ExecutionEvent::Log(message.to_string()));
    }

    pub fn account_diff(&mut self, data: TreeAccount) {
        self.is_writable();

        self.events.push(ExecutionEvent::AccountDiff(data));
    }
}
