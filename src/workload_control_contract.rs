//! Frozen private wire mirror for the Workload Control contract in LioRael/lenso#530.
//!
//! Keep this module private and byte-compatible with the reviewed v1 contract until the
//! framework publishes the contract for normal registry consumption. The schema digest and
//! public HTTP conformance tests guard this temporary package boundary.

use std::{collections::BTreeSet, fmt::Write as _, num::NonZeroU32};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub(crate) const WORKLOAD_CONTROL_PROTOCOL: &str = "lenso.workload-control.v1";
pub(crate) const WORKLOAD_CONTROL_OBSERVE_PATH: &str = "/workload-control/v1/observe";
pub(crate) const WORKLOAD_CONTROL_OPERATIONS_PATH: &str = "/workload-control/v1/operations";
pub(crate) const WORKLOAD_CONTROL_OPERATION_PATH: &str =
    "/workload-control/v1/operations/{operationId}";
#[cfg(test)]
const WORKLOAD_CONTROL_SCHEMA_DIGEST: &str =
    "sha256:d3666bb1fd85576f9af4205dbcc70029acd81462678c47d2b315c40ef1a9161d";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "field names mirror the frozen framework contract"
)]
pub(crate) struct WorkloadReference {
    pub(crate) system_id: String,
    pub(crate) service_id: String,
    pub(crate) workload_id: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadControlCapability {
    Suspend,
    Resume,
    Restart,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadOperationalState {
    Running,
    Suspended,
    Transitioning,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadProtection {
    Controllable,
    ControlPlane,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WorkloadControlAction {
    Suspend,
    Resume,
    Restart,
    Scale {
        #[serde(rename = "targetCapacity")]
        target_capacity: NonZeroU32,
    },
}

impl<'de> Deserialize<'de> for WorkloadControlAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_workload_control_action(&value).map_err(de::Error::custom)
    }
}

fn parse_workload_control_action(value: &Value) -> Result<WorkloadControlAction, &'static str> {
    let object = value
        .as_object()
        .ok_or("Workload Control action must be an object")?;
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("Workload Control action requires kind")?;
    match kind {
        "suspend" if object.len() == 1 => Ok(WorkloadControlAction::Suspend),
        "resume" if object.len() == 1 => Ok(WorkloadControlAction::Resume),
        "restart" if object.len() == 1 => Ok(WorkloadControlAction::Restart),
        "scale" if object.len() == 2 => {
            let capacity = object
                .get("targetCapacity")
                .and_then(Value::as_u64)
                .and_then(|capacity| u32::try_from(capacity).ok())
                .and_then(NonZeroU32::new)
                .ok_or("Scale requires a positive targetCapacity")?;
            Ok(WorkloadControlAction::Scale {
                target_capacity: capacity,
            })
        }
        "suspend" | "resume" | "restart" | "scale" => {
            Err("Workload Control action contains unknown fields")
        }
        _ => Err("Workload Control action kind is unsupported"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadControlActorKind {
    Operator,
    Automation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadControlActor {
    pub(crate) kind: WorkloadControlActorKind,
    pub(crate) subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadMutationRequest {
    pub(crate) protocol: String,
    pub(crate) workload: WorkloadReference,
    pub(crate) action: WorkloadControlAction,
    pub(crate) observed_revision: String,
    pub(crate) idempotency_key: String,
    pub(crate) actor: WorkloadControlActor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadControlAuthorityDecision {
    Accepted,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadControlAuthority {
    pub(crate) adapter_id: String,
    pub(crate) decision: WorkloadControlAuthorityDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadOperationPhase {
    Accepted,
    Executing,
    Verifying,
    Succeeded,
    Failed,
    Denied,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkloadControlErrorCode {
    Unauthenticated,
    Unauthorized,
    UnsupportedAction,
    ProtectedWorkload,
    StaleRevision,
    ActiveMutation,
    IdempotencyConflict,
    AuthorityUnavailable,
    IncompatibleProtocol,
    WorkloadNotFound,
    OperationNotFound,
    InvalidCapacity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadControlFailure {
    pub(crate) code: WorkloadControlErrorCode,
    /// Sanitized, provider-neutral text limited to 1,024 Unicode characters.
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadControlError {
    pub(crate) protocol: String,
    pub(crate) code: WorkloadControlErrorCode,
    /// Sanitized, provider-neutral text limited to 1,024 Unicode characters.
    pub(crate) message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) current_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadOperationResult {
    pub(crate) state: WorkloadOperationalState,
    pub(crate) observed_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OperationRecord {
    pub(crate) protocol: String,
    pub(crate) operation_id: String,
    pub(crate) request: WorkloadMutationRequest,
    pub(crate) authority: WorkloadControlAuthority,
    pub(crate) phase: WorkloadOperationPhase,
    pub(crate) requested_at_unix_ms: u64,
    pub(crate) decided_at_unix_ms: u64,
    pub(crate) updated_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finished_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<WorkloadOperationResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failure: Option<WorkloadControlFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadObservation {
    pub(crate) protocol: String,
    pub(crate) workload: WorkloadReference,
    pub(crate) state: WorkloadOperationalState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) observed_revision: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) capabilities: BTreeSet<WorkloadControlCapability>,
    pub(crate) protection: WorkloadProtection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) active_operation: Option<String>,
    pub(crate) observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkloadObservationRequest {
    pub(crate) protocol: String,
    pub(crate) workload: WorkloadReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "document",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum WorkloadControlMessage {
    ObservationRequest(WorkloadObservationRequest),
    Observation(WorkloadObservation),
    MutationRequest(WorkloadMutationRequest),
    OperationRecord(OperationRecord),
    Error(WorkloadControlError),
}

pub(crate) fn workload_control_schema() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(WorkloadControlMessage))
        .expect("Workload Control schema must serialize");
    schema["$id"] = Value::String(
        "https://contracts.lenso.local/workload-control/lenso.workload-control.v1.schema.json"
            .to_owned(),
    );
    schema["title"] = Value::String("Lenso Workload Control Messages".to_owned());
    for definition in [
        "WorkloadObservationRequest",
        "WorkloadObservation",
        "WorkloadMutationRequest",
        "OperationRecord",
        "WorkloadControlError",
    ] {
        schema["$defs"][definition]["properties"]["protocol"] =
            json!({ "type": "string", "const": WORKLOAD_CONTROL_PROTOCOL });
    }
    for field in ["systemId", "serviceId", "workloadId"] {
        patch_control_scalar(&mut schema, "WorkloadReference", field);
    }
    patch_control_scalar(&mut schema, "WorkloadMutationRequest", "observedRevision");
    patch_control_scalar(&mut schema, "WorkloadMutationRequest", "idempotencyKey");
    patch_control_scalar(&mut schema, "WorkloadControlActor", "subject");
    patch_control_scalar(&mut schema, "WorkloadObservation", "observedRevision");
    patch_control_scalar(&mut schema, "WorkloadObservation", "activeOperation");
    patch_control_scalar(&mut schema, "WorkloadOperationResult", "observedRevision");
    patch_control_scalar(&mut schema, "OperationRecord", "operationId");
    patch_control_scalar(&mut schema, "WorkloadControlAuthority", "adapterId");
    patch_safe_message(&mut schema, "WorkloadControlFailure", "message");
    patch_safe_message(&mut schema, "WorkloadControlError", "message");
    for field in ["operationId", "currentRevision", "activeOperation"] {
        patch_control_scalar(&mut schema, "WorkloadControlError", field);
    }
    schema
}

fn patch_control_scalar(schema: &mut Value, definition: &str, field: &str) {
    schema["$defs"][definition]["properties"][field]["minLength"] = json!(1);
    schema["$defs"][definition]["properties"][field]["maxLength"] = json!(255);
    schema["$defs"][definition]["properties"][field]["pattern"] = json!(r".*\S.*");
}

fn patch_safe_message(schema: &mut Value, definition: &str, field: &str) {
    schema["$defs"][definition]["properties"][field]["minLength"] = json!(1);
    schema["$defs"][definition]["properties"][field]["maxLength"] = json!(1_024);
    schema["$defs"][definition]["properties"][field]["pattern"] = json!(r".*\S.*");
}

pub(crate) fn workload_control_schema_digest() -> String {
    let bytes = serde_json_canonicalizer::to_vec(&workload_control_schema())
        .expect("Workload Control schema must canonicalize");
    let mut rendered = String::with_capacity("sha256:".len() + 64);
    rendered.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut rendered, "{byte:02x}").expect("writing to a String cannot fail");
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrored_schema_matches_the_frozen_framework_contract() {
        let schema = workload_control_schema();

        assert_eq!(
            schema["$id"],
            "https://contracts.lenso.local/workload-control/lenso.workload-control.v1.schema.json"
        );
        assert_eq!(
            workload_control_schema_digest(),
            WORKLOAD_CONTROL_SCHEMA_DIGEST
        );
    }
}
