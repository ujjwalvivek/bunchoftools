use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum OperationKind {
    #[serde(rename = "foundation.simulate")]
    FoundationSimulate,
}

impl OperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FoundationSimulate => "foundation.simulate",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulateParameters {
    pub duration_ms: u64,
    pub output_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_at_percent: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "parameters")]
pub enum Operation {
    #[serde(rename = "foundation.simulate")]
    FoundationSimulate(SimulateParameters),
}

impl Operation {
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::FoundationSimulate(_) => OperationKind::FoundationSimulate,
        }
    }
}
