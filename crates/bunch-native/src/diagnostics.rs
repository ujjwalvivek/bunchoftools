use serde::{Deserialize, Serialize};

use crate::adapters::imagemagick::{ADAPTER_ID as IMAGEMAGICK_ADAPTER_ID, ImageMagick};
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
    let mut diagnostics = vec![EngineDiagnostic {
        adapter_id: FOUNDATION_ADAPTER_ID.to_owned(),
        engine_name: "Built-in foundation runner".to_owned(),
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        status: EngineStatus::Ready,
        detail: "M1 contract harness; no input files are modified".to_owned(),
    }];
    diagnostics.push(match ImageMagick::discover() {
        Ok(engine) => EngineDiagnostic {
            adapter_id: IMAGEMAGICK_ADAPTER_ID.to_owned(),
            engine_name: "ImageMagick".to_owned(),
            engine_version: engine.version().to_owned(),
            status: EngineStatus::Ready,
            detail: format!(
                "restricted M2 image adapter at {}",
                engine.executable().display()
            ),
        },
        Err(error) => EngineDiagnostic {
            adapter_id: IMAGEMAGICK_ADAPTER_ID.to_owned(),
            engine_name: "ImageMagick 7".to_owned(),
            engine_version: "unavailable".to_owned(),
            status: EngineStatus::Missing,
            detail: error,
        },
    });
    diagnostics
}
