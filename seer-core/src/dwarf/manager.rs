use std::{collections::HashSet, fs, io, path::{Path, PathBuf}};

use gimli::{Dwarf, DwarfSections, EndianSlice, Reader, RunTimeEndian, SectionId};
use object::{Object, ObjectSection};
use path_clean::PathClean;

use crate::{errors::IrrecoverableError, path_resolver::PathResolver};

/// Keeps living references to sections, to avoid borrow issues.
pub struct DwarfManager {
    sections: Option<DwarfSections<Vec<u8>>>,
}

impl Default for DwarfManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DwarfManager {
    pub fn new() -> Self {
        Self { sections: None }
    }

    pub fn get_dwarf(&self) -> Option<Dwarf<impl Reader + use<'_>>> {
        let sections = self.sections.as_ref()?;
        Some(
            sections
                .borrow(|bytes| EndianSlice::new(bytes, RunTimeEndian::Little)),
        )
    }

    pub fn get_all_source_files(&self, path_resolver: &PathResolver) -> HashSet<PathBuf> {
        self.get_source_files(path_resolver)
    }

    pub fn get_source_files(&self, path_resolver: &PathResolver) -> HashSet<PathBuf> {
        let dwarf = self.get_dwarf().expect("Failed to fetch dwarf");

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
                if let Ok(runtime_path) = path_resolver
                    .dwarf_path_to_runtime_path(&resolved_path)
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

    pub fn set_dwarf_section(&mut self, path: &Path) -> Result<(), IrrecoverableError> {
        let data = fs::read(path).map_err(|e| IrrecoverableError::DwarfFileRead {
            filename: path.to_string_lossy().to_string(),
            detail: e.to_string(),
        })?;
        let obj = object::File::parse(&*data).map_err(|e| IrrecoverableError::DwarfFileParse {
            filename: path.to_string_lossy().to_string(),
            detail: e.to_string(),
        })?;

        let sections: DwarfSections<Vec<u8>> =
            DwarfSections::load(|id: SectionId| -> io::Result<Vec<u8>> {
                match obj.section_by_name(id.name()) {
                    Some(s) => Ok(s
                        .uncompressed_data()
                        .map_err(io::Error::other)?
                        .into_owned()),
                    None => Ok(Vec::new()),
                }
            })
            .map_err(|e| IrrecoverableError::DwarfFileParse {
                filename: path.to_string_lossy().to_string(),
                detail: e.to_string(),
            })?;

        self.sections = Some(sections);
        Ok(())
    }
}
