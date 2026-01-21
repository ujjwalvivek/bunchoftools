use serde::{Deserialize, Serialize};

use crate::capabilities::FOUNDATION_ADAPTER_ID;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineStatus {
    Ready,
    Missing,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EngineDiagnostic {
    pub adapter_id: String,
    pub engine_name: String,
    pub engine_version: String,
    pub status: EngineStatus,
    pub detail: String,
}

#[must_use]
pub fn engine_diagnostics() -> Vec<EngineDiagnostic> {
    vec![EngineDiagnostic {
        adapter_id: FOUNDATION_ADAPTER_ID.to_owned(),
        engine_name: "Built-in foundation runner".to_owned(),
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        status: EngineStatus::Ready,
        detail: "M1 contract harness; no input files are modified".to_owned(),
    }]
}
