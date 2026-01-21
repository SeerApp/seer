use std::collections::{HashMap, VecDeque};

use crate::{binary_lookup_tree::LookupNode, dwarf::source_die::SourceDie};

pub struct CallTraceLookup {
    pub lookup: LookupNode<u64>,          // index of DIE-correlated indexes
    pub parents: HashMap<u64, u64>,       // mapping from children to parents
    pub sources: HashMap<u64, SourceDie>, // mapping from indexes to valid sources
}

impl CallTraceLookup {
    pub fn get_call_trace(&self, i: u64) -> VecDeque<SourceDie> {
        let mut source_die_trace: VecDeque<SourceDie> = VecDeque::new();

        if let Some(deepest_index) = self.lookup.search_deepest(&i) {
            let mut tracing = true;
            let mut index = deepest_index.data;

            while tracing {
                if let Some(s) = self.sources.get(&index) {
                    source_die_trace.push_front(s.clone());
                }

                match self.parents.get(&index) {
                    Some(parent) => index = *parent,
                    None => tracing = false,
                }
            }
        }

        source_die_trace
    }
}
