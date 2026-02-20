use bunch_core::{
    AdapterIdentity, CapabilityDescriptor, CapabilitySet, ExecutorKind, OperationKind,
};

use crate::adapters::imagemagick::ImageMagick;

pub const FOUNDATION_ADAPTER_ID: &str = "foundation.fake";

#[must_use]
pub fn foundation_capabilities() -> CapabilitySet {
    CapabilitySet::new(vec![CapabilityDescriptor {
        operation: OperationKind::FoundationSimulate,
        executor: ExecutorKind::Native,
        adapter: AdapterIdentity {
            id: FOUNDATION_ADAPTER_ID.to_owned(),
            version: "1".to_owned(),
            engine_name: "Built-in foundation runner".to_owned(),
            engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        supports_progress: true,
        supports_cancellation: true,
    }])
}

#[must_use]
pub fn native_capabilities() -> CapabilitySet {
    let mut capabilities = foundation_capabilities().all().to_vec();
    if let Ok(engine) = ImageMagick::discover() {
        capabilities.push(engine.capability());
    }
    CapabilitySet::new(capabilities)
}
