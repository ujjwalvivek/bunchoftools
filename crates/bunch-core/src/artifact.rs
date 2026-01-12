use serde::{Deserialize, Serialize};

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
}
