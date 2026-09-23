use serde::Serialize;

#[derive(Serialize, Clone)]
struct TxNotes {
    warnings: Vec<String>,
}

#[derive(Serialize, Clone)]
struct TxData {
    notes: TxNotes,
}

/// Completion marker for a simulated transaction. Errors live in `failure.json`.
#[derive(Serialize, Clone)]
pub struct TxMetadata {
    version: u32,
    data: TxData,
}

impl Default for TxMetadata {
    fn default() -> Self {
        Self {
            version: 1,
            data: TxData {
                notes: TxNotes { warnings: vec![] },
            },
        }
    }
}

impl TxMetadata {
    pub fn push_warning(&mut self, warning: impl Into<String>) {
        self.data.notes.warnings.push(warning.into());
    }
}

#[cfg(test)]
mod tests {
    use super::TxMetadata;

    #[test]
    fn default_has_no_warnings() {
        let meta = TxMetadata::default();
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["version"], 1);
        assert!(json["data"]["notes"]["warnings"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(json["data"].get("success").is_none());
        assert!(json["data"].get("output").is_none());
    }

    #[test]
    fn records_warnings() {
        let mut meta = TxMetadata::default();
        meta.push_warning("nonce hash did not match");
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(
            json["data"]["notes"]["warnings"][0],
            "nonce hash did not match"
        );
    }
}
