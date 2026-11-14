//! Static discovery shared by native and sandboxed command frontends.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{ArgumentKind, CommandSpec, Question};

/// Optional author declarations; `None` means unknown, while an empty list declares none.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommandContract {
    /// Workflow input schema, distinct from the CLI's string/flag parsing schema.
    pub input_schema: Option<Value>,
    /// Declared handler output schema; discovery does not execute validation.
    pub output_schema: Option<Value>,
    /// Capability names required by the workflow, without host grant values.
    pub capabilities: Option<Vec<String>>,
    /// Potential effects declared by the author, not an execution authorization.
    pub effects: Option<Vec<String>>,
    /// Questions that may be asked; dynamic questions can remain undeclared.
    pub questions: Option<Vec<Question>>,
    /// Author-provided verification instructions, never run during discovery.
    pub verification: Option<Vec<String>>,
}

/// Versioned, read-only command catalog; it contains no inputs, answers, or grants.
#[derive(Debug, Clone, Serialize)]
pub struct CommandCatalog<'a> {
    /// Stable output discriminator.
    pub r#type: &'static str,
    /// Discovery protocol version, independent of the execution ABI.
    pub schema_version: u32,
    /// Registered aliases in deterministic path order.
    pub commands: Vec<CommandDescription<'a>>,
}

/// A registered alias and the known portion of its workflow contract.
#[derive(Debug, Clone, Serialize)]
pub struct CommandDescription<'a> {
    /// Original alias metadata, retained without a second parameter definition.
    #[serde(flatten)]
    pub command: &'a CommandSpec,
    /// Installed provider version, or null when the host has not declared one.
    pub provider_version: Option<&'a str>,
    /// Argument shapes derived from `ArgumentKind`; omitted options remain optional.
    pub cli_input_schema: Value,
    /// Optional workflow declarations; a missing contract serializes as null.
    pub contract: Option<&'a CommandContract>,
}

pub(crate) fn input_schema(spec: &CommandSpec) -> Value {
    let properties: BTreeMap<_, _> = spec
        .arguments
        .iter()
        .map(|arg| {
            let mut schema = match arg.kind {
                ArgumentKind::Text => json!({"type":"string"}),
                ArgumentKind::Flag => json!({"type":"boolean"}),
                ArgumentKind::List => {
                    json!({"type":"array","items":{"type":"string"},"minItems":1})
                }
            };
            schema["description"] = json!(arg.help);
            (arg.name.clone(), schema)
        })
        .collect();
    json!({"type":"object","properties":properties})
}
