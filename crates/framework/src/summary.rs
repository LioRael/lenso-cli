//! Execution receipts distinguish completed calls from application verification.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Optional public receipt supplied by a trusted handler's output summarizer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepSummary {
    /// Actually affected objects, or null when the handler cannot describe them.
    pub affected: Option<Vec<String>>,
    /// Public identifiers, such as a file-plan digest, without full payloads.
    pub identifiers: BTreeMap<String, String>,
    /// Completed external operations, or null when unknown.
    pub operations: Option<usize>,
}

/// One invocation, including a primary call that succeeded before a later hook failed.
#[derive(Debug, Clone, Serialize)]
pub struct CallSummary {
    /// Workflow-local owner.
    pub step: String,
    /// Registered implementation identifier.
    pub handler: String,
    /// `prepare`, `before`, `result`, `validate`, or `after`.
    pub phase: String,
    /// `complete`, `needs_input`, or `failed`; completion alone is not verification.
    pub status: String,
    /// Optional handler-owned projection; no arbitrary output is inferred as an effect.
    pub receipt: Option<StepSummary>,
}

/// Host-observed outcome for an explicitly declared validation hook.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationSummary {
    /// Workflow-local owner.
    pub step: String,
    /// Declared validation handler.
    pub handler: String,
    /// `passed`, `failed`, or `not_run` for the current workflow evaluation.
    pub status: String,
}

/// A receipt for this in-memory execution; it does not promise persistent resume or rollback.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionSummary {
    /// Correlation identifier, distinct from a plan digest or idempotency key.
    pub id: String,
    /// Namespaced workflow.
    pub workflow: String,
    /// `running`, `complete`, `needs_input`, `failed`, or `cancelled`.
    pub status: String,
    /// Steps whose primary handler and all hooks completed.
    pub completed_steps: Vec<String>,
    /// Actual calls, retained across answer revisions; no side effects are implied undone.
    pub calls: Vec<CallSummary>,
    /// Declared validation hooks; an empty list means no application check was declared.
    pub verification: Vec<VerificationSummary>,
    /// `passed`, `failed`, `not_run`, or `not_declared`; this is separate from execution status.
    pub verification_status: String,
    /// Hook success does not prove application health; this protocol does not infer that conclusion.
    pub application_verification: &'static str,
}
