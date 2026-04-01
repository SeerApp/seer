use codama_nodes::RootNode;

pub struct CodamaIdlLookup {
    root_node: RootNode,
}

impl CodamaIdlLookup {
    pub fn from_json_str(idl_json: &str) -> Result<Self, serde_json::Error> {
        let root_node: RootNode = serde_json::from_str(idl_json)?;
        Ok(Self { root_node })
    }

    pub fn root_node(&self) -> &RootNode {
        &self.root_node
    }
}