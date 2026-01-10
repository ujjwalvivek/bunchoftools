//! Portable product contracts for Bunch of Tools.
//!
//! This crate deliberately excludes UI frameworks, native process execution,
//! and target-specific filesystem behavior.

mod artifact;
mod binding;
mod capability;
mod operation;
mod recipe;

pub use artifact::ArtifactManifest;
pub use binding::{AssetBinding, ExecutionBindings};
pub use capability::{AdapterIdentity, CapabilityDescriptor, CapabilitySet, ExecutorKind};
pub use operation::{Operation, OperationKind, SimulateParameters};
pub use recipe::{
    CollisionPolicy, InputSlot, OutputPolicy, RECIPE_SCHEMA_VERSION, Recipe, RecipeLoadError,
    RecipeStep, StepSource, ValidationCode, ValidationIssue, parse_recipe_json,
};
