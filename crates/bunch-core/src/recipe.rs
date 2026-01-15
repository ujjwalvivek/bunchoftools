use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Operation, SimulateParameters};

pub const RECIPE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InputSlot {
    pub id: String,
    pub accepts: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", content = "id", rename_all = "kebab-case")]
pub enum StepSource {
    Input(String),
    Step(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecipeStep {
    pub id: String,
    pub inputs: Vec<StepSource>,
    pub operation: Operation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CollisionPolicy {
    Fail,
    Increment,
    Replace,
    Choose,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputPolicy {
    pub name_template: String,
    pub collision: CollisionPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub schema_version: u32,
    pub name: String,
    pub inputs: Vec<InputSlot>,
    pub steps: Vec<RecipeStep>,
    pub output: OutputPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationCode {
    UnsupportedSchema,
    EmptyName,
    InvalidIdentifier,
    DuplicateIdentifier,
    MissingInput,
    MissingStep,
    InvalidReference,
    InvalidParameter,
    InvalidOutput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub code: ValidationCode,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecipeLoadError {
    InvalidJson(String),
    MissingSchema,
    UnsupportedSchema { found: u64, supported: u32 },
    InvalidRecipe(Vec<ValidationIssue>),
}

impl fmt::Display for RecipeLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "invalid recipe JSON: {error}"),
            Self::MissingSchema => write!(formatter, "recipe is missing schema_version"),
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "recipe schema {found} is not supported; this build supports {supported}"
            ),
            Self::InvalidRecipe(issues) => {
                write!(formatter, "recipe has {} validation issue(s)", issues.len())
            }
        }
    }
}

impl Error for RecipeLoadError {}

impl Recipe {
    #[must_use]
    pub fn foundation_demo() -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            name: "Foundation job".to_owned(),
            inputs: vec![InputSlot {
                id: "source".to_owned(),
                accepts: vec!["*/*".to_owned()],
            }],
            steps: vec![RecipeStep {
                id: "prepare".to_owned(),
                inputs: vec![StepSource::Input("source".to_owned())],
                operation: Operation::FoundationSimulate(SimulateParameters {
                    duration_ms: 1_200,
                    output_label: "Prepared artifact".to_owned(),
                    fail_at_percent: None,
                }),
            }],
            output: OutputPolicy {
                name_template: "{stem}.prepared".to_owned(),
                collision: CollisionPolicy::Increment,
            },
        }
    }

    #[must_use]
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        if self.schema_version != RECIPE_SCHEMA_VERSION {
            push_issue(
                &mut issues,
                ValidationCode::UnsupportedSchema,
                "schema_version",
                format!(
                    "expected schema version {RECIPE_SCHEMA_VERSION}, found {}",
                    self.schema_version
                ),
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 128 {
            push_issue(
                &mut issues,
                ValidationCode::EmptyName,
                "name",
                "name must contain between 1 and 128 characters",
            );
        }

        let mut input_ids = HashSet::new();
        if self.inputs.is_empty() {
            push_issue(
                &mut issues,
                ValidationCode::MissingInput,
                "inputs",
                "at least one input slot is required",
            );
        }
        for (index, input) in self.inputs.iter().enumerate() {
            let path = format!("inputs[{index}].id");
            validate_id(&input.id, &path, &mut issues);
            if !input_ids.insert(input.id.as_str()) {
                push_issue(
                    &mut issues,
                    ValidationCode::DuplicateIdentifier,
                    path,
                    format!("duplicate input identifier {}", input.id),
                );
            }
            if input.accepts.is_empty() || input.accepts.iter().any(|kind| kind.trim().is_empty()) {
                push_issue(
                    &mut issues,
                    ValidationCode::InvalidParameter,
                    format!("inputs[{index}].accepts"),
                    "accepted media types must contain at least one non-empty value",
                );
            }
        }

        let mut step_ids = HashSet::new();
        if self.steps.is_empty() {
            push_issue(
                &mut issues,
                ValidationCode::MissingStep,
                "steps",
                "at least one operation step is required",
            );
        }
        for (index, step) in self.steps.iter().enumerate() {
            let path = format!("steps[{index}].id");
            validate_id(&step.id, &path, &mut issues);
            if !step_ids.insert(step.id.as_str()) {
                push_issue(
                    &mut issues,
                    ValidationCode::DuplicateIdentifier,
                    path,
                    format!("duplicate step identifier {}", step.id),
                );
            }
            if step.inputs.is_empty() {
                push_issue(
                    &mut issues,
                    ValidationCode::InvalidReference,
                    format!("steps[{index}].inputs"),
                    "each step requires at least one input",
                );
            }
            for (source_index, source) in step.inputs.iter().enumerate() {
                let valid = match source {
                    StepSource::Input(id) => input_ids.contains(id.as_str()),
                    StepSource::Step(id) => step_ids.contains(id.as_str()) && id != &step.id,
                };
                if !valid {
                    push_issue(
                        &mut issues,
                        ValidationCode::InvalidReference,
                        format!("steps[{index}].inputs[{source_index}]"),
                        format!("source {source:?} does not reference an available prior value"),
                    );
                }
            }
            validate_operation(&step.operation, index, &mut issues);
        }

        let output = self.output.name_template.trim();
        if output.is_empty()
            || output.chars().count() > 240
            || output.contains('/')
            || output.contains('\\')
            || !output.contains("{stem}")
        {
            push_issue(
                &mut issues,
                ValidationCode::InvalidOutput,
                "output.name_template",
                "output name must contain {stem}, contain no path separator, and fit within 240 characters",
            );
        }

        issues
    }

    pub fn to_pretty_json(&self) -> Result<String, RecipeLoadError> {
        let issues = self.validate();
        if !issues.is_empty() {
            return Err(RecipeLoadError::InvalidRecipe(issues));
        }
        serde_json::to_string_pretty(self)
            .map_err(|error| RecipeLoadError::InvalidJson(error.to_string()))
    }
}

pub fn parse_recipe_json(source: &str) -> Result<Recipe, RecipeLoadError> {
    let value: Value = serde_json::from_str(source)
        .map_err(|error| RecipeLoadError::InvalidJson(error.to_string()))?;
    let version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or(RecipeLoadError::MissingSchema)?;
    if version != u64::from(RECIPE_SCHEMA_VERSION) {
        return Err(RecipeLoadError::UnsupportedSchema {
            found: version,
            supported: RECIPE_SCHEMA_VERSION,
        });
    }
    let recipe: Recipe = serde_json::from_value(value)
        .map_err(|error| RecipeLoadError::InvalidJson(error.to_string()))?;
    let issues = recipe.validate();
    if issues.is_empty() {
        Ok(recipe)
    } else {
        Err(RecipeLoadError::InvalidRecipe(issues))
    }
}

fn validate_id(value: &str, path: &str, issues: &mut Vec<ValidationIssue>) {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !valid {
        push_issue(
            issues,
            ValidationCode::InvalidIdentifier,
            path,
            "identifier must use 1-64 ASCII letters, digits, hyphens, or underscores",
        );
    }
}

fn validate_operation(operation: &Operation, index: usize, issues: &mut Vec<ValidationIssue>) {
    match operation {
        Operation::FoundationSimulate(parameters) => {
            if !(100..=60_000).contains(&parameters.duration_ms) {
                push_issue(
                    issues,
                    ValidationCode::InvalidParameter,
                    format!("steps[{index}].operation.parameters.duration_ms"),
                    "simulation duration must be between 100 and 60000 milliseconds",
                );
            }
            if parameters.output_label.trim().is_empty()
                || parameters.output_label.chars().count() > 128
            {
                push_issue(
                    issues,
                    ValidationCode::InvalidParameter,
                    format!("steps[{index}].operation.parameters.output_label"),
                    "output label must contain between 1 and 128 characters",
                );
            }
            if parameters
                .fail_at_percent
                .is_some_and(|percent| !(1..100).contains(&percent))
            {
                push_issue(
                    issues,
                    ValidationCode::InvalidParameter,
                    format!("steps[{index}].operation.parameters.fail_at_percent"),
                    "failure percentage must be between 1 and 99",
                );
            }
        }
    }
}

fn push_issue(
    issues: &mut Vec<ValidationIssue>,
    code: ValidationCode,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    issues.push(ValidationIssue {
        code,
        path: path.into(),
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundation_recipe_round_trips() {
        let recipe = Recipe::foundation_demo();
        let json = recipe.to_pretty_json().expect("valid recipe serializes");
        assert_eq!(parse_recipe_json(&json).expect("recipe parses"), recipe);
    }

    #[test]
    fn future_schema_is_rejected_before_deserialization() {
        let json = Recipe::foundation_demo()
            .to_pretty_json()
            .expect("valid recipe")
            .replace("\"schema_version\": 1", "\"schema_version\": 99");
        assert!(matches!(
            parse_recipe_json(&json),
            Err(RecipeLoadError::UnsupportedSchema { found: 99, .. })
        ));
    }

    #[test]
    fn forward_step_reference_is_rejected() {
        let mut recipe = Recipe::foundation_demo();
        recipe.steps[0].inputs = vec![StepSource::Step("later".to_owned())];
        recipe.steps.push(RecipeStep {
            id: "later".to_owned(),
            inputs: vec![StepSource::Input("source".to_owned())],
            operation: recipe.steps[0].operation.clone(),
        });
        assert!(
            recipe
                .validate()
                .iter()
                .any(|issue| issue.code == ValidationCode::InvalidReference)
        );
    }

    #[test]
    fn unsafe_output_template_is_rejected() {
        let mut recipe = Recipe::foundation_demo();
        recipe.output.name_template = "../{stem}.prepared".to_owned();
        assert!(
            recipe
                .validate()
                .iter()
                .any(|issue| issue.code == ValidationCode::InvalidOutput)
        );
    }
}
