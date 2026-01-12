use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AssetBinding {
    pub locator: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionBindings {
    pub inputs: BTreeMap<String, AssetBinding>,
    pub output_directory: String,
}
