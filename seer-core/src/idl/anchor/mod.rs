use anchor_lang_idl_spec::Idl;

pub struct AnchorIdlLookup {
    idl: Idl,
}

impl AnchorIdlLookup {
    pub fn from_json_str(idl_json: &str) -> Result<Self, serde_json::Error> {
        let idl: Idl = serde_json::from_str(idl_json)?;
        Ok(Self { idl })
    }

    pub fn idl(&self) -> &Idl {
        &self.idl
    }
}