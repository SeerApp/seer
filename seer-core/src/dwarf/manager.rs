use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::PathBuf,
};

use gimli::{Dwarf, DwarfSections, EndianSlice, Reader, RunTimeEndian, SectionId};
use object::{Object, ObjectSection};
use path_clean::PathClean;
use solana_keypair::read_keypair_file;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::path_resolver::PathResolver;

pub struct DwarfManager {
    sections: HashMap<Pubkey, DwarfSections<Vec<u8>>>,
}

impl DwarfManager {
    pub fn new(target_deploy_dir: &PathBuf) -> Self {
        let mut manager = DwarfManager {
            sections: HashMap::new(),
        };

        manager.set_dwarf_sections(target_deploy_dir);

        manager
    }

    pub fn get_dwarf(&self, program_address: &Pubkey) -> Option<Dwarf<impl Reader + use<'_>>> {
        if self.sections.contains_key(program_address) {
            return Some(
                self.sections[&program_address]
                    .borrow(|bytes| EndianSlice::new(bytes, RunTimeEndian::Little))
                    .into(),
            );
        }

        None
    }

    pub fn get_pubkeys(&self) -> Vec<&Pubkey> {
        self.sections.keys().collect()
    }

    pub fn contains(&self, program_address: &Pubkey) -> bool {
        self.sections.contains_key(program_address)
    }

    pub fn get_all_source_files(&self, path_resolver: &PathResolver) -> HashSet<PathBuf> {
        let mut all_source_files = HashSet::new();

        for (program_address, _) in &self.sections {
            all_source_files.extend(self.get_source_files(path_resolver, program_address));
        }

        all_source_files
    }

    pub fn get_source_files(
        &self,
        path_resolver: &PathResolver,
        program_address: &Pubkey,
    ) -> HashSet<PathBuf> {
        let dwarf = self
            .get_dwarf(program_address)
            .expect("Failed to fetch dwarf for program");

        let mut units = dwarf.units();

        let Some(unit_header) = units.next().ok().unwrap() else {
            return HashSet::new();
        };

        let unit = dwarf.unit(unit_header).ok().unwrap();

        let Some(line_program) = unit.line_program.clone() else {
            return HashSet::new();
        };

        let header = line_program.header();
        let mut files = HashSet::new();

        for file_entry in header.file_names() {
            let file_name = match dwarf.attr_string(&unit, file_entry.path_name()) {
                Ok(name) => match name.to_string_lossy() {
                    Ok(s) => s.into_owned(),
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            let file_path = PathBuf::from(&file_name);

            let resolved_path = if file_path.is_absolute() {
                file_path.clean()
            } else {
                let dir_path = match file_entry.directory(header) {
                    Some(dir_attr) => match dwarf.attr_string(&unit, dir_attr) {
                        Ok(dir) => match dir.to_string_lossy() {
                            Ok(s) => PathBuf::from(s.as_ref()).clean(),
                            Err(_) => continue,
                        },
                        Err(_) => continue,
                    },
                    None => PathBuf::new(),
                };

                dir_path.join(file_path).clean()
            };

            let runtime_path = if resolved_path.is_absolute() {
                if let Some(runtime_path) = path_resolver
                    .dwarf_path_to_runtime_path(&resolved_path)
                    .ok()
                {
                    runtime_path
                } else {
                    continue;
                }
            } else {
                path_resolver.relative_path_to_runtime_path(&resolved_path)
            };

            if runtime_path.is_file()
                && runtime_path.extension().and_then(|e| e.to_str()) == Some("rs")
                && runtime_path.starts_with(path_resolver.runtime_dir())
            {
                files.insert(runtime_path.clean());
            }
        }

        files
    }

    fn set_dwarf_sections(&mut self, target_deploy_dir: &PathBuf) {
        let mut bases: HashSet<String> = HashSet::new();

        let entries = match fs::read_dir(target_deploy_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };

            if let Some(base) = file_name.strip_suffix("-keypair.json") {
                bases.insert(base.to_string());
            } else if let Some(base) = file_name.strip_suffix("-pubkey.json") {
                bases.insert(base.to_string());
            } else if let Some(base) = file_name.strip_suffix(".so") {
                bases.insert(base.to_string());
            } else if let Some(base) = file_name.strip_suffix(".debug") {
                bases.insert(base.to_string());
            }
        }

        for base in bases {
            let keypair_path = target_deploy_dir.join(format!("{base}-keypair.json"));
            let pubkey_path = target_deploy_dir.join(format!("{base}-pubkey.json"));
            let dwarf_path = target_deploy_dir.join(format!("{base}.debug"));

            let pubkey = if keypair_path.exists() {
                let keypair = read_keypair_file(&keypair_path).unwrap_or_else(|e| {
                    panic!("Failed to read keypair `{}`: {e}", keypair_path.display())
                });
                keypair.pubkey()
            } else if pubkey_path.exists() {
                let pubkey_str: String =
                    serde_json::from_str(&fs::read_to_string(&pubkey_path).unwrap_or_else(|e| {
                        panic!(
                            "Failed to read pubkey file `{}`: {e}",
                            pubkey_path.display()
                        )
                    }))
                    .unwrap_or_else(|e| {
                        panic!(
                            "Failed to parse pubkey file `{}`: {e}",
                            pubkey_path.display()
                        )
                    });
                pubkey_str.parse().unwrap_or_else(|e| {
                    panic!("Failed to parse pubkey string `{}`: {e}", pubkey_str)
                })
            } else {
                panic!(
                    "Program `{base}` is missing both keypair and pubkey file (expected `{}` or `{}`)",
                    keypair_path.display(),
                    pubkey_path.display()
                );
            };

            if !dwarf_path.exists() {
                panic!(
                    "Program `{base}` is missing DWARF file: {}",
                    dwarf_path.display()
                );
            }

            self.set_dwarf_section(pubkey, &dwarf_path);
        }
    }

    fn set_dwarf_section(&mut self, program_address: Pubkey, path: &PathBuf) {
        let data = fs::read(path).expect("Failed to read DWARF path");
        let obj = object::File::parse(&*data).expect("Failed to parse DWARF data");

        let sections = DwarfSections::load(|id: SectionId| -> io::Result<Vec<u8>> {
            match obj.section_by_name(id.name()) {
                Some(s) => Ok(s
                    .uncompressed_data()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                    .into_owned()),
                None => Ok(Vec::new()),
            }
        })
        .expect("Failed to parse DWARF sections");

        self.sections.insert(program_address, sections);
    }
}
