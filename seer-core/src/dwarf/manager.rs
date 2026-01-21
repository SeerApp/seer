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

pub struct DwarfManager {
    sections: HashMap<Pubkey, DwarfSections<Vec<u8>>>,
}

impl DwarfManager {
    pub fn new(deploy_folder_root: PathBuf) -> Self {
        let mut manager = DwarfManager {
            sections: HashMap::new(),
        };

        manager.set_dwarf_sections(deploy_folder_root);

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

    pub fn get_all_source_files(&self, cwd: &PathBuf, source_project_root: &PathBuf) -> HashSet<PathBuf> {
        let mut all_source_files = HashSet::new();

        for (program_address, _) in &self.sections {
            all_source_files.extend(self.get_source_files(cwd, source_project_root, program_address));
        }

        all_source_files
    }
    pub fn get_source_files(
        &self,
        cwd: &PathBuf,
        source_project_root: &PathBuf,
        program_address: &Pubkey,
    ) -> HashSet<PathBuf> {
        let dwarf = self
            .get_dwarf(program_address)
            .expect("Failed to fetch dwarf for program");
    
        let mut units = dwarf.units();
        while let Some(header) = units.next().ok().unwrap() {
            let unit = dwarf.unit(header).ok().unwrap();
    
            if let Some(line_prog) = unit.line_program.clone() {
                let header = line_prog.header();
    
                for dir_attr in header.include_directories() {
                    let cow = dwarf.attr_string(&unit, dir_attr.clone()).ok().unwrap();
                    let dir_str = cow.to_string_lossy().ok().unwrap().into_owned();
    
                    let dir_path = PathBuf::from(&dir_str).clean();
    
                    let resolved = cwd.join(&dir_path).clean();
    
                    if resolved.exists() && resolved.is_dir() {
                        let relative_to_root = source_project_root.join(&dir_path).clean();
                        
                        let files: HashSet<PathBuf> = std::fs::read_dir(&resolved)
                            .ok()
                            .unwrap()
                            .filter_map(|entry| entry.ok())
                            .map(|entry| {
                                let filename = entry.file_name();
                                relative_to_root.join(filename)
                            })
                            .filter(|_| true)
                            .collect();
    
                        return files;
                    }
                }
            }
        }
    
        HashSet::new()
    }

    fn set_dwarf_sections(&mut self, deploy_folder_root: PathBuf) {
        let mut bases: HashSet<String> = HashSet::new();

        let entries = match fs::read_dir(&deploy_folder_root) {
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
            } else if let Some(base) = file_name.strip_suffix(".so") {
                bases.insert(base.to_string());
            } else if let Some(base) = file_name.strip_suffix(".debug") {
                bases.insert(base.to_string());
            }
        }

        for base in bases {
            let keypair_path = deploy_folder_root.join(format!("{base}-keypair.json"));
            let dwarf_path = deploy_folder_root.join(format!("{base}.debug"));

            if !keypair_path.exists() {
                panic!(
                    "Program `{base}` is missing keypair file: {}",
                    keypair_path.display()
                );
            }

            if !dwarf_path.exists() {
                panic!(
                    "Program `{base}` is missing DWARF file: {}",
                    dwarf_path.display()
                );
            }

            let keypair = read_keypair_file(&keypair_path).unwrap_or_else(|e| {
                panic!("Failed to read keypair `{}`: {e}", keypair_path.display())
            });

            let pubkey = keypair.pubkey();
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
