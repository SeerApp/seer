use std::collections::HashMap;

use gimli::{
    AttributeValue, DW_AT_abstract_origin, DW_AT_call_file, DW_AT_call_line, DW_AT_decl_file,
    DW_AT_decl_line, DW_AT_specification, DW_TAG_formal_parameter, DW_TAG_inlined_subroutine,
    DW_TAG_local_variable, DW_TAG_subprogram, DW_TAG_variable, DebugInfoOffset,
    DebuggingInformationEntry, Dwarf, EntriesTreeNode, Reader, ReaderOffset, Unit, UnitOffset,
};
use serde::{Deserialize, Serialize};

use crate::{
    binary_lookup_tree::{LookupInterval, LookupNode},
    entrypoint_lookup::EntrypointLookup,
    sources::Sources,
    tree::loc::Loc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SourceDieType {
    Fn,
    Var,
}

struct SourceDieLocHolder {
    pub decl: Option<Loc>,
    pub call: Option<Loc>,
    pub signature: Option<String>,
}

impl SourceDieLocHolder {
    pub fn new() -> Self {
        Self {
            decl: None,
            call: None,
            signature: None,
        }
    }

    pub fn build<R: Reader>(
        sources: &Sources,
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        die: &DebuggingInformationEntry<R>,
        holder: &mut Self,
    ) {
        if holder.signature.is_none() {
            holder.signature = Loc::get_linkage_name(dwarf, die);
        }

        if holder.decl.is_none() {
            holder.decl = Loc::new(sources, dwarf, unit, die, DW_AT_decl_file, DW_AT_decl_line)
                .ok()
                .unwrap();
        }

        if holder.call.is_none() {
            holder.call = Loc::new(sources, dwarf, unit, die, DW_AT_call_file, DW_AT_call_line)
                .ok()
                .unwrap();
        }

        if holder.signature.is_none() || holder.decl.is_none() || holder.call.is_none() {
            for attr_kind in [DW_AT_abstract_origin, DW_AT_specification] {
                if let Some(attr) = die.attr(attr_kind).ok().unwrap() {
                    match attr.value() {
                        AttributeValue::UnitRef(offset) => {
                            SourceDieLocHolder::build(
                                sources,
                                dwarf,
                                unit,
                                &unit.entry(offset).ok().unwrap(),
                                holder,
                            );
                        }
                        AttributeValue::DebugInfoRef(debug_info_offset) => {
                            let (target_unit, unit_offset) =
                                find_unit_by_offset(dwarf, debug_info_offset).ok().unwrap();
                            let target_die = target_unit.entry(unit_offset).ok().unwrap();
                            SourceDieLocHolder::build(
                                sources,
                                dwarf,
                                &target_unit,
                                &target_die,
                                holder,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn find_unit_by_offset<R: Reader>(
    dwarf: &Dwarf<R>,
    offset: DebugInfoOffset<R::Offset>,
) -> Result<(Unit<R>, UnitOffset<R::Offset>), gimli::Error> {
    let mut units = dwarf.units();

    while let Some(header) = units.next()? {
        let unit_start = header.offset().as_debug_info_offset().unwrap();

        if offset.0 >= unit_start.0 {
            let relative_offset = offset.0 - unit_start.0;
            if relative_offset < header.length_including_self() {
                let unit_offset = UnitOffset(relative_offset);
                let unit = dwarf.unit(header)?;
                return Ok((unit, unit_offset));
            }
        }
    }

    Err(gimli::Error::NoEntryAtGivenOffset)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceDieLoc {
    pub decl: Option<Loc>,
    pub call: Option<Loc>,
    pub signature: String,
}

impl SourceDieLoc {
    pub fn new<R: Reader>(
        sources: &Sources,
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        die: &DebuggingInformationEntry<R>,
    ) -> Option<Self> {
        let mut holder = SourceDieLocHolder::new();

        SourceDieLocHolder::build(sources, dwarf, unit, die, &mut holder);

        if let Some(signature) = holder.signature {
            if holder.decl.is_some() || holder.call.is_some() {
                return Some(Self {
                    decl: holder.decl,
                    call: holder.call,
                    signature,
                });
            }
        }

        None
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceDie {
    pub loc: SourceDieLoc,
    pub source_type: SourceDieType,
}

impl SourceDie {
    #[allow(non_upper_case_globals)]
    pub fn new<R: Reader>(
        sources: &Sources,
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        die: &DebuggingInformationEntry<R>,
    ) -> Option<Self> {
        if let Some(loc) = SourceDieLoc::new(sources, dwarf, unit, die) {
            let source_type = matches!(die.tag(), DW_TAG_subprogram | DW_TAG_inlined_subroutine)
                .then(|| SourceDieType::Fn)
                .unwrap_or(SourceDieType::Var);

            Some(Self { loc, source_type })
        } else {
            None
        }
    }
}

/// Not combined into a single Vec<LookupInterval<SourceDie>>
/// for better readability during debugging.
#[derive(Debug, Serialize, Deserialize)]
pub struct SourceDieTrace {
    trace: HashMap<u64, SourceDie>,
    parents: HashMap<u64, u64>,
    source_die_ranges: Vec<LookupInterval<u64>>,
}

impl SourceDieTrace {
    pub fn new<R: Reader>(dwarf: &Dwarf<R>, sources: &Sources) -> Self {
        let mut source_die_trace = SourceDieTrace {
            trace: HashMap::new(),
            parents: HashMap::new(),
            source_die_ranges: Vec::new(),
        };

        let mut units = dwarf.units();

        while let Some(unit_header) = units.next().ok().unwrap() {
            let unit = dwarf.unit(unit_header.clone()).ok().unwrap();
            let mut tree = unit.entries_tree(None).ok().unwrap();
            let root = tree.root().ok().unwrap();

            source_die_trace.build(sources, dwarf, &unit, root, None, 0);
        }

        source_die_trace
    }

    #[allow(non_upper_case_globals)]
    fn build<R: Reader>(
        &mut self,
        sources: &Sources,
        dwarf: &Dwarf<R>,
        unit: &Unit<R>,
        entries_tree_node: EntriesTreeNode<R>,
        parent_offset: Option<u64>,
        depth: u32,
    ) {
        let die = entries_tree_node.entry();
        let unit_offset = die.offset();
        let header_offset = unit.header.offset().as_debug_info_offset().unwrap().0;

        let absolute_offset = header_offset + unit_offset.0;
        let offset = absolute_offset.into_u64();
        let tag = die.tag();

        let mut next_parent_offset = parent_offset;

        if matches!(
            tag,
            DW_TAG_inlined_subroutine
                | DW_TAG_subprogram
                | DW_TAG_variable
                | DW_TAG_formal_parameter
                | DW_TAG_local_variable
        ) {
            if let Some(source_die) = SourceDie::new(sources, dwarf, unit, die) {
                self.trace.insert(offset, source_die);

                if let Some(p) = parent_offset {
                    self.parents.insert(offset, p);
                }

                let mut ranges = dwarf.die_ranges(unit, die).ok().unwrap();
                while let Some(range) = ranges.next().ok().unwrap() {
                    self.source_die_ranges.push(LookupInterval {
                        begin: range.begin,
                        end: range.end,
                        depth,
                        data: offset,
                    });
                }

                next_parent_offset = Some(offset);
            }
        }

        let mut children = entries_tree_node.children();
        while let Some(child) = children.next().ok().unwrap() {
            self.build(sources, dwarf, unit, child, next_parent_offset, depth + 1);
        }
    }

    pub fn sizes(&self) -> (usize, usize, usize) {
        (
            self.trace.len(),
            self.parents.len(),
            self.source_die_ranges.len(),
        )
    }
}

impl From<SourceDieTrace> for EntrypointLookup {
    fn from(value: SourceDieTrace) -> Self {
        EntrypointLookup {
            lookup: *LookupNode::<u64>::build(value.source_die_ranges)
                .expect("Failed to build lookup for call trace"),
            parents: value.parents,
            sources: value.trace,
        }
    }
}
