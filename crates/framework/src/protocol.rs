use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Failures shared by the command engine and extension adapters.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid definitions, arguments, or extension responses.
    #[error("{0}")]
    Invalid(String),
    /// An extension or host operation failed.
    #[error("{0}")]
    Operation(String),
    /// An explicit host grant is missing.
    #[error("permission denied: {0}")]
    Permission(String),
    /// The user cancelled execution.
    #[error("cancelled")]
    Cancelled,
    /// A supervised process exited unsuccessfully.
    #[error("process exited with status {0}")]
    ProcessExit(i32),
    /// Host input or output failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A JSON document was invalid.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Versioned input passed to a handler; answers are never included in reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Optional trusted-host event channel, excluded from the JSON ABI.
    #[serde(skip)]
    pub events: crate::EventSink,
    /// JSON ABI version.
    pub abi_version: u32,
    /// Canonical project directory selected by the host.
    pub project: PathBuf,
    /// Command arguments, separate from interactive answers.
    pub input: Value,
    /// Answers supplied by the host.
    pub answers: BTreeMap<String, Value>,
    /// Outputs of explicitly declared dependencies only.
    pub steps: BTreeMap<String, Value>,
    /// Outputs from preceding hooks on this step.
    pub hooks: BTreeMap<String, Value>,
    /// Host-owned capability grants, scoped to this installed workflow.
    #[serde(default)]
    pub capabilities: BTreeMap<String, Value>,
}

/// Supported host-rendered question types in ABI v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionKind {
    /// A nonempty text answer.
    Text,
    /// Explicit boolean consent; `--yes` can answer this kind only.
    Confirm,
    /// One of the exact values provided by an extension.
    Select {
        /// Allowed values, also used as display labels in M0.
        options: Vec<String>,
    },
    /// A set of distinct values from the provided list.
    MultiSelect {
        /// Allowed values and display labels.
        options: Vec<String>,
    },
}

/// A stable question identifier with host-owned interaction semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Stable identifier scoped to this workflow.
    pub id: String,
    /// User-facing prompt, optionally including a change preview.
    pub message: String,
    /// Required answer type.
    pub kind: QuestionKind,
}

impl Question {
    /// Check an answer without applying implicit defaults or coercion.
    ///
    /// # Errors
    /// Returns an error if the value does not satisfy this question.
    pub fn validate(&self, value: &Value) -> Result<(), Error> {
        let valid = match &self.kind {
            QuestionKind::Text => value.as_str().is_some_and(|s| !s.trim().is_empty()),
            QuestionKind::Confirm => value.is_boolean(),
            QuestionKind::Select { options } => value
                .as_str()
                .is_some_and(|s| options.iter().any(|o| o == s)),
            QuestionKind::MultiSelect { options } => value.as_array().is_some_and(|values| {
                let mut seen = std::collections::BTreeSet::new();
                values.iter().all(|value| {
                    value.as_str().is_some_and(|value| {
                        options.iter().any(|option| option == value) && seen.insert(value)
                    })
                })
            }),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Invalid(format!("invalid answer for {}", self.id)))
        }
    }
}

/// A handler either completes or yields a question for the interaction host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    /// Successful output, available to dependent steps.
    Complete {
        /// Serializable handler result.
        output: Value,
    },
    /// Pause without implicitly choosing an answer.
    NeedsInput {
        /// Question to render or return to an agent.
        question: Question,
    },
}

impl Outcome {
    /// Construct a successful result.
    #[must_use]
    pub fn complete(output: Value) -> Self {
        Self::Complete { output }
    }
}

/// Ordered extension handlers surrounding a step's primary handler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    /// Derive context before the primary handler.
    pub prepare: Vec<String>,
    /// Run checks before execution.
    pub before: Vec<String>,
    /// Validate the primary output, exposed as `hooks.result`.
    pub validate: Vec<String>,
    /// Enrich or check the completed result.
    pub after: Vec<String>,
}

/// A unit of work and its explicit data dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Unique identifier within the workflow.
    pub id: String,
    /// Registered native or Wasm handler identifier.
    pub handler: String,
    /// Step identifiers whose outputs this step consumes.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Optional extension hooks.
    #[serde(default)]
    pub hooks: Hooks,
}

impl Step {
    /// Construct an independent step with no hooks.
    #[must_use]
    pub fn new(id: impl Into<String>, handler: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            handler: handler.into(),
            needs: Vec::new(),
            hooks: Hooks::default(),
        }
    }
}

/// A bounded dependency graph shared by all command aliases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    /// Namespaced execution identifier, independent of aliases.
    pub id: String,
    /// Steps; dependencies determine execution order.
    pub steps: Vec<Step>,
}

/// Observable execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Every step completed.
    Complete,
    /// Execution requires an explicit answer.
    NeedsInput,
}

/// Machine-readable execution result, with no answer or environment dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    /// Stable event discriminator.
    pub r#type: String,
    /// Workflow identifier.
    pub workflow: String,
    /// Current execution state.
    pub status: Status,
    /// Completed step outputs.
    pub outputs: BTreeMap<String, Value>,
    /// Pending interaction, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<Question>,
}
