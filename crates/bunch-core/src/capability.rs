use serde::{Deserialize, Serialize};

use crate::OperationKind;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutorKind {
    Native,
    Server,
    Browser,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct AdapterIdentity {
    pub id: String,
    pub version: String,
    pub engine_name: String,
    pub engine_version: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub operation: OperationKind,
    pub executor: ExecutorKind,
    pub adapter: AdapterIdentity,
    pub supports_progress: bool,
    pub supports_cancellation: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: Vec<CapabilityDescriptor>,
}

impl CapabilitySet {
    #[must_use]
    pub fn new(mut capabilities: Vec<CapabilityDescriptor>) -> Self {
        capabilities.sort();
        capabilities.dedup();
        Self { capabilities }
    }

    #[must_use]
    pub fn all(&self) -> &[CapabilityDescriptor] {
        &self.capabilities
    }

    #[must_use]
    pub fn select(
        &self,
        operation: OperationKind,
        executor: ExecutorKind,
    ) -> Option<&CapabilityDescriptor> {
        self.capabilities
            .iter()
            .find(|capability| capability.operation == operation && capability.executor == executor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str) -> CapabilityDescriptor {
        CapabilityDescriptor {
            operation: OperationKind::FoundationSimulate,
            executor: ExecutorKind::Native,
            adapter: AdapterIdentity {
                id: id.to_owned(),
                version: "1".to_owned(),
                engine_name: "fixture".to_owned(),
                engine_version: "1".to_owned(),
            },
            supports_progress: true,
            supports_cancellation: true,
        }
    }

    #[test]
    fn capability_selection_is_deterministic() {
        let capabilities = CapabilitySet::new(vec![descriptor("z"), descriptor("a")]);
        let selected = capabilities
            .select(OperationKind::FoundationSimulate, ExecutorKind::Native)
            .expect("native capability");
        assert_eq!(selected.adapter.id, "a");
    }
}
