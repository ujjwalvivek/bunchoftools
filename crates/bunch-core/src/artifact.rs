use serde::{Deserialize, Serialize};

use crate::ImageProbe;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub name: String,
    pub locator: String,
    pub media_type: String,
    pub byte_length: u64,
    pub adapter_id: String,
    pub engine_name: String,
    pub engine_version: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_properties: Option<ImageProbe>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_properties: Option<ImageProbe>,
}
