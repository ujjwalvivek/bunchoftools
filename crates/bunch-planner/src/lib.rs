use std::error::Error;
use std::fmt;

use bunch_core::{
    CapabilityDescriptor, CapabilitySet, ExecutionBindings, ExecutorKind, Operation, OutputPolicy,
    Recipe, StepSource, ValidationIssue,
};
use serde::{Deserialize, Serialize};

pub const PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlannedStep {
    pub id: String,
    pub inputs: Vec<StepSource>,
    pub operation: Operation,
    pub capability: CapabilityDescriptor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub schema_version: u32,
    pub recipe_name: String,
    pub executor: ExecutorKind,
    pub bindings: ExecutionBindings,
    pub steps: Vec<PlannedStep>,
    pub output: OutputPolicy,
    pub output_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanningError {
    InvalidRecipe(Vec<ValidationIssue>),
    MissingBinding(String),
    EmptyBinding(String),
    EmptyOutputDirectory,
    MissingCapability {
        operation: String,
        executor: ExecutorKind,
    },
}

impl fmt::Display for PlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecipe(issues) => {
                write!(formatter, "recipe has {} validation issue(s)", issues.len())
            }
            Self::MissingBinding(slot) => write!(formatter, "input slot {slot} is not bound"),
            Self::EmptyBinding(slot) => write!(formatter, "input slot {slot} has an empty binding"),
            Self::EmptyOutputDirectory => write!(formatter, "output directory is empty"),
            Self::MissingCapability {
                operation,
                executor,
            } => write!(
                formatter,
                "operation {operation} is unavailable on executor {executor:?}"
            ),
        }
    }
}

impl Error for PlanningError {}

#[derive(Clone, Copy, Debug, Default)]
pub struct Planner;

impl Planner {
    pub fn plan(
        self,
        recipe: &Recipe,
        bindings: ExecutionBindings,
        capabilities: &CapabilitySet,
        executor: ExecutorKind,
    ) -> Result<ExecutionPlan, PlanningError> {
        let issues = recipe.validate();
        if !issues.is_empty() {
            return Err(PlanningError::InvalidRecipe(issues));
        }
        for input in &recipe.inputs {
            let binding = bindings
                .inputs
                .get(&input.id)
                .ok_or_else(|| PlanningError::MissingBinding(input.id.clone()))?;
            if binding.locator.trim().is_empty() || binding.display_name.trim().is_empty() {
                return Err(PlanningError::EmptyBinding(input.id.clone()));
            }
        }
        if bindings.output_directory.trim().is_empty() {
            return Err(PlanningError::EmptyOutputDirectory);
        }

        let mut steps = Vec::with_capacity(recipe.steps.len());
        for step in &recipe.steps {
            let operation = step.operation.kind();
            let capability = capabilities
                .select(operation, executor)
                .ok_or_else(|| PlanningError::MissingCapability {
                    operation: operation.as_str().to_owned(),
                    executor,
                })?
                .clone();
            steps.push(PlannedStep {
                id: step.id.clone(),
                inputs: step.inputs.clone(),
                operation: step.operation.clone(),
                capability,
            });
        }

        let primary = bindings
            .inputs
            .get(&recipe.inputs[0].id)
            .expect("required binding was validated");
        let stem = portable_stem(&primary.display_name);
        let output_name = recipe.output.name_template.replace("{stem}", stem);

        Ok(ExecutionPlan {
            schema_version: PLAN_SCHEMA_VERSION,
            recipe_name: recipe.name.clone(),
            executor,
            bindings,
            steps,
            output: recipe.output.clone(),
            output_name,
        })
    }
}

fn portable_stem(display_name: &str) -> &str {
    let name = display_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(display_name);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bunch_core::{AdapterIdentity, AssetBinding, CapabilityDescriptor, OperationKind};

    use super::*;

    fn capabilities() -> CapabilitySet {
        CapabilitySet::new(vec![CapabilityDescriptor {
            operation: OperationKind::FoundationSimulate,
            executor: ExecutorKind::Native,
            adapter: AdapterIdentity {
                id: "foundation.fake".to_owned(),
                version: "1".to_owned(),
                engine_name: "fake".to_owned(),
                engine_version: "1".to_owned(),
            },
            supports_progress: true,
            supports_cancellation: true,
        }])
    }

    fn bindings() -> ExecutionBindings {
        ExecutionBindings {
            inputs: BTreeMap::from([(
                "source".to_owned(),
                AssetBinding {
                    locator: r"F:\project\screen.final.png".to_owned(),
                    display_name: "screen.final.png".to_owned(),
                },
            )]),
            output_directory: r"F:\project".to_owned(),
        }
    }

    #[test]
    fn planning_is_deterministic_and_uses_portable_stem() {
        let recipe = Recipe::foundation_demo();
        let first = Planner
            .plan(&recipe, bindings(), &capabilities(), ExecutorKind::Native)
            .expect("plan");
        let second = Planner
            .plan(&recipe, bindings(), &capabilities(), ExecutorKind::Native)
            .expect("plan");
        assert_eq!(first, second);
        assert_eq!(first.output_name, "screen.final.prepared");
    }

    #[test]
    fn missing_binding_and_capability_fail_before_execution() {
        let recipe = Recipe::foundation_demo();
        assert!(matches!(
            Planner.plan(
                &recipe,
                ExecutionBindings::default(),
                &capabilities(),
                ExecutorKind::Native,
            ),
            Err(PlanningError::MissingBinding(_))
        ));
        assert!(matches!(
            Planner.plan(
                &recipe,
                bindings(),
                &CapabilitySet::default(),
                ExecutorKind::Native,
            ),
            Err(PlanningError::MissingCapability { .. })
        ));
    }
}
