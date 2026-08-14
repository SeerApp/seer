use serde::{Deserialize, Serialize};

/// Same JSON shape at run root (`seer/failure.json`) and per-tx (`seer/tx/<sig>/failure.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub version: u32,
    pub kind: FailureKind,
    pub code: String,
    pub message: String,
    pub component: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Internal,
    Execution,
}

impl Failure {
    pub fn internal(
        code: impl Into<String>,
        message: impl Into<String>,
        component: impl Into<String>,
    ) -> Self {
        Self {
            version: 1,
            kind: FailureKind::Internal,
            code: code.into(),
            message: message.into(),
            component: component.into(),
        }
    }

    pub fn execution(
        code: impl Into<String>,
        message: impl Into<String>,
        component: impl Into<String>,
    ) -> Self {
        Self {
            version: 1,
            kind: FailureKind::Execution,
            code: code.into(),
            message: message.into(),
            component: component.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, FailureKind};

    #[test]
    fn serializes_snake_case_kind() {
        let json = serde_json::to_string(&Failure::internal(
            "blockhash_not_found",
            "Blockhash not found",
            "validator",
        ))
        .unwrap();
        assert!(json.contains("\"kind\":\"internal\""));
        assert!(json.contains("\"code\":\"blockhash_not_found\""));

        let parsed: Failure = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kind, FailureKind::Internal);
    }
}
