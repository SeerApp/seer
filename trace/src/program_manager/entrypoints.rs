//! Purpose: resolve DWARF data into entrypoint lookups.

use crate::{
    dwarf::{manager::DwarfManager, source_die::SourceDieTrace},
    entrypoint_lookup::EntrypointLookup,
    errors::IrrecoverableError,
    path_resolver::PathResolver,
    seer_debug,
    sources::Sources,
    target_reader::Target,
};

pub fn get_entrypoint(
    target: &Target,
    path_resolver: PathResolver,
) -> Result<Option<EntrypointLookup>, IrrecoverableError> {
    let mut dwarf_manager = DwarfManager::new();

    let Some(dwarf_dir) = target.dwarf.as_ref() else {
        return Ok(None);
    };
    dwarf_manager.set_dwarf_section(dwarf_dir)?;

    seer_debug!("Fetching source files");
    let source_files = dwarf_manager.get_all_source_files(&path_resolver);
    seer_debug!("Found source files\n\t{:?}", source_files);
    let sources = Sources::new(path_resolver, source_files);

    seer_debug!("About to search for {} DWARF source(s)...", sources.len());
    seer_debug!("Building lookup for {}", target.base);
    let Some(dwarf) = dwarf_manager.get_dwarf() else {
        return Ok(None);
    };
    let source_die_trace = SourceDieTrace::new(&dwarf, &sources);

    let sizes = source_die_trace.sizes();

    seer_debug!(
        "Assembled Source Die Trace for program {} with {} traces {} parents and {} die ranges",
        target.base,
        sizes.0,
        sizes.1,
        sizes.2
    );

    let entrypoint_lookup: EntrypointLookup = source_die_trace.into();

    Ok(Some(entrypoint_lookup))
}
