use std::{collections::HashMap, path::PathBuf};

use solana_pubkey::Pubkey;

use crate::save::save;

#[derive(Debug)]
pub struct StepTrace {
    pub cache: HashMap<Pubkey, Vec<u64>>,
}

impl StepTrace {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn load(folder: PathBuf) -> Self {
        let mut cache = HashMap::new();
        
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let path = entry.path();

                if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(program_address) = filename.parse::<Pubkey>() {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(instructions) = serde_json::from_str::<Vec<u64>>(&content) {
                                    cache.insert(program_address, instructions);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Self { cache }
    }

    pub fn insert(&mut self, program_address: Pubkey, i: u64) {
        match self.cache.get_mut(&program_address) {
            Some(instructions) => {
                instructions.push(i);
            }
            None => {
                self.cache.insert(program_address, vec![i]);
            }
        }
    }

    pub fn save(self) {
        for (program, instructions) in self.cache {
            save(
                serde_json::to_string_pretty(&instructions).ok().unwrap(),
                format!("step_trace_{}", program),
                "json",
                true,
            );
        }
    }
}
