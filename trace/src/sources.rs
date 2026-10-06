use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use crate::errors::IrrecoverableError;
use crate::path_resolver::PathResolver;

#[derive(Debug)]
pub struct ProgramInfo {
    pub attribute_lines: HashSet<u64>,
}

impl ProgramInfo {
    pub fn new(source_file: &Path) -> Self {
        let file = File::open(source_file).expect("Could not read Rust file");
        let reader = BufReader::new(file);

        ProgramInfo {
            attribute_lines: reader
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    let line = line.ok()?.trim().to_string();

                    if line.starts_with("#[") {
                        Some(i.saturating_add(1) as u64)
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct Sources {
    pub path_resolver: PathResolver,
    // Paths must exist in runtime.
    pub infos: HashMap<PathBuf, ProgramInfo>,
}

impl Sources {
    pub fn new(path_resolver: PathResolver, source_files: HashSet<PathBuf>) -> Self {
        let mut infos = HashMap::new();

        for sf in &source_files {
            infos.insert(sf.clone(), ProgramInfo::new(sf));
        }

        Self {
            path_resolver,
            infos,
        }
    }

    pub fn is_valid_source(&self, dwarf_file_path: &Path, file_line: u64) -> bool {
        let runtime_file_path = match self
            .path_resolver
            .dwarf_path_to_runtime_path(dwarf_file_path)
        {
            Ok(path) => path,
            Err(IrrecoverableError::DwarfPath { .. }) => return false,
            Err(err) => panic!("dwarf path resolution failed: {err}"),
        };
        self.infos
            .get(&runtime_file_path)
            .map(|f| !f.attribute_lines.contains(&file_line))
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }
}
