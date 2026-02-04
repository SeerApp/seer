use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

#[derive(Debug)]
pub struct ProgramInfo {
    pub attribute_lines: HashSet<u64>,
}

impl ProgramInfo {
    pub fn new(source_file: &PathBuf) -> Self {
        let file = File::open(source_file).expect("Could not read Rust file");
        let reader = BufReader::new(file);

        ProgramInfo {
            attribute_lines: reader
                .lines()
                .enumerate()
                .filter_map(|(i, line)| {
                    let line = line.ok()?.trim().to_string();

                    if line.starts_with("#[") {
                        Some((i + 1) as u64)
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
    pub infos: HashMap<PathBuf, ProgramInfo>,
}

impl Sources {
    pub fn new(_: PathBuf, source_files: HashSet<PathBuf>) -> Self {
        let mut infos = HashMap::new();

        for sf in &source_files {
            infos.insert(sf.clone(), ProgramInfo::new(sf));
        }

        Self { infos }
    }

    pub fn in_sources(&self, file_path: &PathBuf) -> bool {
        self.infos.contains_key(file_path)
    }

    pub fn is_valid_source(&self, file_path: &PathBuf, file_line: u64) -> bool {
        self.infos
            .get(file_path)
            .map(|f| !f.attribute_lines.contains(&file_line))
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }
}
