use std::collections::{HashMap, VecDeque};

use crate::{
    binary_lookup_tree::LookupNode, source_die_trace::SourceDie, trace_tree::node_data::NodeData
};

pub struct CallTraceLookup {
    pub lookup: LookupNode<u64>,               // index of DIE-correlated indexes
    pub parents: HashMap<u64, u64>,            // mapping from children to parents
    pub sources: HashMap<u64, SourceDie>,      // mapping from indexes to valid sources
}

impl CallTraceLookup {
    pub fn get_call_trace(&self, i: u64) -> VecDeque<NodeData> {
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

        let mut call_trace: VecDeque<NodeData> = VecDeque::new();

        while source_die_trace.len() > 0 {
            let source_die = source_die_trace.pop_front().unwrap();

            if call_trace.is_empty() {
                NodeData::from_first(source_die).map(|n| {
                    call_trace.push_back(n);
                });
            } else {
                NodeData::from(source_die).map(|n| {
                    call_trace.push_back(n);
                });
            }
        }

        call_trace
    }
}
