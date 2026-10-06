use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use anyhow::{Context, Result};
use object::{File as ObjectFile, Object, ObjectSection, ObjectSymbol, SymbolKind, SymbolSection};
use solana_program_runtime::solana_sbpf::ebpf;

/// `.text` section VMA (`sh_addr`) from the ELF file alone — same basis as llvm-objdump's address
/// column, without extending `solana-sbpf`.
pub(crate) fn text_section_vma_from_elf(elf_bytes: &[u8]) -> Result<u64> {
    let obj = ObjectFile::parse(elf_bytes).context("parse ELF for .text VMA")?;
    Ok(obj
        .section_by_name(".text")
        .map(|s| s.address())
        .unwrap_or(0))
}

/// Maps VMAs in `.text` to linker symbol names so `call` lines name real callees instead of
/// `solana-sbpf` placeholders like `function_<pc>`.
#[derive(Default)]
pub(crate) struct TextFnSymbols {
    /// Sorted, non-overlapping `[start, end)` spans with best name for that range.
    spans: Vec<(u64, u64, String)>,
}

fn prefer_symbol_name(a: &str, b: &str) -> String {
    let score = |s: &str| -> i32 {
        let mut sc = s.len() as i32;
        if s.contains("::") {
            sc = sc.saturating_add(100);
        }
        if s.starts_with('.') {
            sc = sc.saturating_sub(30);
        }
        sc
    };
    if score(b) > score(a) {
        b.to_string()
    } else {
        a.to_string()
    }
}

impl TextFnSymbols {
    pub(crate) fn build(elf_bytes: &[u8]) -> Result<Self> {
        let obj = ObjectFile::parse(elf_bytes).context("parse ELF for symbol index")?;
        let Some(text) = obj.section_by_name(".text") else {
            return Ok(Self::default());
        };
        let text_id = text.index();
        let text_vma = text.address();
        let text_size = text.size();
        let text_end = text_vma.saturating_add(text_size);
        let insn_sz = ebpf::INSN_SIZE as u64;

        let mut by_start: BTreeMap<u64, (u64, String)> = BTreeMap::new();

        let mut ingest = |sym: <ObjectFile as Object>::Symbol<'_>| {
            if sym.kind() != SymbolKind::Text && sym.kind() != SymbolKind::Unknown {
                return;
            }
            if sym.section() == SymbolSection::Undefined {
                return;
            }
            if sym.section_index() != Some(text_id) {
                return;
            }
            let Ok(name) = sym.name() else {
                return;
            };
            if name.is_empty() {
                return;
            }

            let mut vma = sym.address();
            if vma < text_vma && vma < text_size {
                vma = text_vma.saturating_add(vma);
            }
            if vma < text_vma || vma >= text_end {
                return;
            }

            let size = sym.size().max(insn_sz);

            match by_start.entry(vma) {
                Entry::Vacant(e) => {
                    e.insert((size, name.to_string()));
                }
                Entry::Occupied(mut e) => {
                    let (sz0, n0) = e.get_mut();
                    *n0 = prefer_symbol_name(n0, name);
                    *sz0 = (*sz0).max(size);
                }
            }
        };

        for sym in obj.symbols() {
            ingest(sym);
        }
        for sym in obj.dynamic_symbols() {
            ingest(sym);
        }

        let mut entries: Vec<(u64, u64, String)> = by_start
            .into_iter()
            .map(|(addr, (sz, name))| (addr, sz, name))
            .collect();
        entries.sort_by_key(|e| e.0);

        for i in 0..entries.len() {
            let next_start = entries
                .get(i.saturating_add(1))
                .map(|e| e.0)
                .unwrap_or(text_end);
            let (start, sz, _) = &entries[i];
            let own_end = start.saturating_add(*sz).max(start.saturating_add(insn_sz));
            let end = next_start.min(own_end).max(start.saturating_add(insn_sz));
            entries[i].1 = end.saturating_sub(*start);
        }

        let spans: Vec<(u64, u64, String)> = entries
            .into_iter()
            .map(|(start, span, name)| {
                let end = start.saturating_add(span).min(text_end);
                (start, end.max(start.saturating_add(insn_sz)), name)
            })
            .collect();

        Ok(Self { spans })
    }

    /// Resolve a VMA inside `.text` to a function symbol name, if covered by a span.
    pub(crate) fn resolve_vaddr(&self, vma: u64) -> Option<&str> {
        let i = self.spans.partition_point(|(start, _, _)| *start <= vma);
        if i == 0 {
            return None;
        }
        let (start, end, name) = &self.spans[i.saturating_sub(1)];
        if vma >= *start && vma < *end {
            Some(name.as_str())
        } else {
            None
        }
    }
}
