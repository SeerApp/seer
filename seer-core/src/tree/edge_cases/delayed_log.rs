use crate::tree::{nodes::{EntrypointChildren, TreeEntrypoint}};

pub struct DelayedLogEdgeCase {
    memorised_entrypoint_tree: Option<TreeEntrypoint<EntrypointChildren>>,
    last_known_entrypoint: Option<TreeEntrypoint<EntrypointChildren>>,
}

impl DelayedLogEdgeCase {
    pub fn new() -> Self {
        Self { memorised_entrypoint_tree: None, last_known_entrypoint: None }
    }

    pub fn met_hook(&mut self, memorised_entrypoint_tree: &TreeEntrypoint<EntrypointChildren>) {
        self.memorised_entrypoint_tree = Some(memorised_entrypoint_tree.clone());
    }

    pub fn lke_hook(&mut self, last_known_entrypoint: &TreeEntrypoint<EntrypointChildren>) {
        self.last_known_entrypoint = Some(last_known_entrypoint.clone());
    }

    pub fn log_hook(&mut self, barrel: &mut Option<TreeEntrypoint<EntrypointChildren>>) {
        if let Some(memorised_entrypoint_tree) = self.memorised_entrypoint_tree.take() {
            if let Some(last_known_entrypoint) = self.last_known_entrypoint.take() {
                if memorised_entrypoint_tree.is_superset_of(&last_known_entrypoint) {
                    *barrel = Some(memorised_entrypoint_tree);
                }
            }
        }
    }
}