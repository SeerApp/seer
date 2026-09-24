use std::path::PathBuf;

use gimli::{
    Attribute, AttributeValue, DW_AT_linkage_name, DebuggingInformationEntry, DwAt, Dwarf, Reader,
    Result, Unit,
};
use serde::{Deserialize, Serialize};

use crate::{sources::Sources, tree::demangle::demangle};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Loc {
    pub file: PathBuf,
    pub line: u64,
}

impl Loc {
    pub fn new<R: Reader>(
        sources: &Sources,
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        die: &DebuggingInformationEntry<R>,
        dw_file: DwAt,
        dw_line: DwAt,
    ) -> Result<Option<Self>> {
        let maybe_file = if let Some(attr) = die.attr(dw_file)? {
            Loc::get_file_path(dwarf, unit, attr)?
        } else {
            None
        };

        if let Some(file) = maybe_file {
            let line = Loc::get_line(
                die.attr(dw_line)?
                    .expect("dw_line not present on a DIE with DW_AT_decl_file"),
            )
            .expect("Failed to get line on a DIE with existing file");

            if sources.is_valid_source(&file, line) {
                return Ok(Some(Self {
                    file: sources
                        .path_resolver
                        .dwarf_path_to_relative_path(&file)
                        .unwrap(),
                    line,
                }));
            }
        }

        Ok(None)
    }

    fn get_file_path<R: Reader>(
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        attr: Attribute<R>,
    ) -> Result<Option<PathBuf>> {
        if let AttributeValue::FileIndex(idx) = attr.value() {
            if let Some(line_program) = &unit.line_program {
                let header = line_program.header();
                if let Some(file_entry) = header.file(idx) {
                    let attr_string = dwarf.attr_string(unit, file_entry.path_name())?;
                    let mut path = PathBuf::from(attr_string.to_string_lossy()?.into_owned());

                    if let Some(dir_attr) = header.directory(file_entry.directory_index()) {
                        let attr_string = dwarf.attr_string(unit, dir_attr)?;
                        let dir_path = PathBuf::from(attr_string.to_string_lossy()?.into_owned());
                        path = dir_path.join(path);
                    }

                    if path.is_relative() {
                        if let Some(comp_dir_attr) = unit.comp_dir.as_ref() {
                            let comp_dir = comp_dir_attr.to_string_lossy()?.into_owned();
                            path = PathBuf::from(comp_dir).join(path);
                        }
                    }

                    return Ok(Some(path));
                }
            }
        }
        Ok(None)
    }

    pub fn get_linkage_name<R: Reader>(
        dwarf: &Dwarf<R>,
        die: &DebuggingInformationEntry<R>,
    ) -> Option<String> {
        if let Some(attr) = die.attr(DW_AT_linkage_name).ok().unwrap() {
            match attr.value() {
                gimli::AttributeValue::String(s) => {
                    Some(s.to_string_lossy().ok().unwrap().into_owned())
                }
                gimli::AttributeValue::DebugStrRef(off) => {
                    let s = dwarf.debug_str.get_str(off).ok().unwrap();
                    Some(s.to_string_lossy().ok().unwrap().into_owned())
                }
                gimli::AttributeValue::DebugLineStrRef(off) => {
                    let s = dwarf.debug_line_str.get_str(off).unwrap();
                    Some(s.to_string_lossy().ok().unwrap().into_owned())
                }
                _ => None,
            }
            .map(|m| Some(demangle(&m)))
            .unwrap_or(None)
        } else {
            None
        }
    }

    fn get_line<R: Reader>(attr: Attribute<R>) -> Option<u64> {
        if let AttributeValue::Udata(line) = attr.value() {
            Some(line)
        } else {
            None
        }
    }
}
