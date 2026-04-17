use std::collections::{HashMap, VecDeque};

use crate::{
    binary_lookup_tree::LookupNode,
    dwarf::source_die::{SourceDie, SourceDieType},
    tree::nodes::{EntrypointChildren, TreeEntrypoint},
};

pub struct EntrypointLookup {
    pub lookup: LookupNode<u64>,          // index of DIE-correlated indexes
    pub parents: HashMap<u64, u64>,       // mapping from children to parents
    pub sources: HashMap<u64, SourceDie>, // mapping from indexes to valid sources
}

impl EntrypointLookup {
    pub fn get_entrypoint(
        &self,
        instruction: u64,
        step_order: u64,
    ) -> Option<TreeEntrypoint<EntrypointChildren>> {
        let mut source_die_trace: VecDeque<SourceDie> = VecDeque::new();

        if let Some(deepest_index) = self.lookup.search_deepest(&instruction) {
            let mut index = deepest_index.data;

            loop {
                if let Some(s) = self.sources.get(&index) {
                    source_die_trace.push_front(s.clone());
                }

                match self.parents.get(&index) {
                    Some(parent) => index = *parent,
                    None => break,
                }
            }
        }

        let maybe_entrypoint = 'find_entrypoint: {
            while let Some(source_die) = source_die_trace.pop_front() {
                match source_die.source_type {
                    SourceDieType::Fn => {
                        if let Some(d) = source_die.loc.decl {
                            break 'find_entrypoint Some(TreeEntrypoint {
                                step_order,
                                instruction,
                                signature: source_die.loc.signature,
                                loc: d,
                                children: vec![],
                            });
                        }
                    }
                    _ => {}
                }
            }
            None
        };

        maybe_entrypoint.map(|mut entrypoint| {
            entrypoint.push_call_trace(instruction, source_die_trace);
            entrypoint
        })
    }
}
