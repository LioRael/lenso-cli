//! Public error projections exclude internal causes and arbitrary legacy error strings.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, ExecutionSummary};

/// Stable machine categories, independent of human wording and underlying libraries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// Input or an answer does not satisfy its contract.
    InvalidInput,
    /// Two installed aliases claim the same path.
    CommandConflict,
    /// A required host capability was not granted.
    PermissionDenied,
    /// A file plan is malformed or unsafe.
    InvalidPlan,
    /// A reviewed destination or read dependency changed.
    StalePlan,
    /// An operation stopped after some effects may have occurred.
    PartialFailure,
    /// An external program could not be started.
    ProcessSpawnFailed,
    /// A supervised child exited unsuccessfully.
    ProcessFailed,
    /// A supervised process exceeded its declared lifetime.
    ProcessTimeout,
    /// An explicitly requested readiness or health check did not pass.
    VerificationFailed,
    /// The user explicitly stopped execution.
    Cancelled,
    /// The host's Wasm call deadline expired.
    WasmTimeout,
    /// The module consumed its instruction budget.
    WasmFuelExhausted,
    /// A Wasm module failed without a more specific host diagnosis.
    WasmFailed,
    /// An extension returned invalid protocol data.
    InvalidResponse,
    /// A dependency is unavailable under the selected Host or resolution policy.
    DependencyUnavailable,
    /// Locked dependency bytes do not match their advertised digest.
    IntegrityMismatch,
    /// A host filesystem or stream operation failed.
    IoError,
    /// An operation failed without a more specific public diagnosis.
    OperationFailed,
}

impl DiagnosticCode {
    fn message(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid input or configuration",
            Self::CommandConflict => "command conflict between installed providers",
            Self::PermissionDenied => "permission denied: required capability is not granted",
            Self::InvalidPlan => "invalid or unsafe file plan",
            Self::StalePlan => "project changed after review; create and review a new plan",
            Self::PartialFailure => {
                "operation partially completed; inspect recorded effects before retrying"
            }
            Self::ProcessSpawnFailed => "external process could not be started",
            Self::ProcessFailed => "external process exited unsuccessfully",
            Self::ProcessTimeout => "external process exceeded its time limit",
            Self::VerificationFailed => "declared verification did not pass",
            Self::Cancelled => "cancelled",
            Self::WasmTimeout => "Wasm call exceeded its time limit",
            Self::WasmFuelExhausted => "Wasm call exhausted its instruction budget",
            Self::WasmFailed => "Wasm call failed",
            Self::InvalidResponse => "extension returned an invalid response",
            Self::DependencyUnavailable => {
                "dependency unavailable under the current resolution policy"
            }
            Self::IntegrityMismatch => "dependency integrity verification failed",
            Self::IoError => "host input or output failed",
            Self::OperationFailed => "operation failed",
        }
    }
}

/// A recovery instruction describes a safe next action, not authorization to execute it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recovery {
    /// Stable action kind, such as `review_plan` or `configure_capability`.
    pub kind: String,
    /// Public instruction without interpolated input, environment values, or causes.
    pub message: String,
}

/// JSON-compatible failure envelope; only explicitly public details belong here.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    r#type: &'static str,
    status: &'static str,
    code: DiagnosticCode,
    message: &'static str,
    details: Value,
    retryable: Option<bool>,
    recovery: Option<Recovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution: Option<Box<ExecutionSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<String>,
    #[serde(skip)]
    pub(crate) exit_code: i32,
}

impl Diagnostic {
    /// Describe a missing capability by name without copying its denied value or command arguments.
    #[must_use]
    pub fn missing_capability(capability: &str) -> Self {
        Self::new(DiagnosticCode::PermissionDenied)
            .with_details(json!({"capability":capability}))
            .with_retryable(false)
            .with_recovery(
                "configure_capability",
                "Review and explicitly grant the required capability before retrying.",
            )
    }
    /// Create an envelope with fixed wording and unknown automatic retry safety.
    #[must_use]
    pub fn new(code: DiagnosticCode) -> Self {
        Self {
            r#type: "diagnostic",
            status: "failed",
            code,
            message: code.message(),
            details: json!({}),
            retryable: None,
            recovery: None,
            execution: None,
            report: None,
            exit_code: if code == DiagnosticCode::Cancelled {
                130
            } else {
                1
            },
        }
    }

    /// Add public field names, capability names, identifiers, or effect receipts.
    ///
    /// The trusted author must exclude input values, secrets, and internal exception text.
    #[must_use]
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    /// Declare whether repeating the operation without repair is safe and useful.
    #[must_use]
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = Some(retryable);
        self
    }

    /// Attach a public recovery instruction; this grants no execution capability.
    #[must_use]
    pub fn with_recovery(mut self, kind: &str, message: &str) -> Self {
        self.recovery = Some(Recovery {
            kind: kind.into(),
            message: message.into(),
        });
        self
    }

    /// Return the stable category without inspecting human error text.
    #[must_use]
    pub fn code(&self) -> DiagnosticCode {
        self.code
    }

    /// Inspect explicitly public details without exposing an internal cause.
    #[must_use]
    pub fn details(&self) -> &Value {
        &self.details
    }

    /// Read the public recovery instruction, when the adapter can suggest one.
    #[must_use]
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    pub(crate) fn with_execution(mut self, execution: ExecutionSummary) -> Self {
        self.execution = Some(Box::new(execution));
        self
    }

    pub(crate) fn with_report(mut self, report: String) -> Self {
        self.report = Some(report);
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for Diagnostic {}

impl From<Diagnostic> for Error {
    fn from(diagnostic: Diagnostic) -> Self {
        Self::Diagnostic(Box::new(diagnostic))
    }
}

impl Error {
    /// Project errors into public data; legacy strings and internal causes are not copied.
    #[must_use]
    pub fn diagnostic(&self) -> Diagnostic {
        let mut diagnostic = match self {
            Self::Diagnostic(diagnostic) => (**diagnostic).clone(),
            Self::Execution { source, summary } => {
                source.diagnostic().with_execution((**summary).clone())
            }
            Self::Invalid(_) => Diagnostic::new(DiagnosticCode::InvalidInput),
            Self::Operation(_) => Diagnostic::new(DiagnosticCode::OperationFailed),
            Self::Permission(_) => Diagnostic::new(DiagnosticCode::PermissionDenied)
                .with_retryable(false)
                .with_recovery(
                    "configure_capability",
                    "Review and explicitly grant the required capability before retrying.",
                ),
            Self::Cancelled => Diagnostic::new(DiagnosticCode::Cancelled).with_retryable(false),
            Self::ProcessExit(code) => Diagnostic::new(DiagnosticCode::ProcessFailed)
                .with_details(json!({"exit_code":code})),
            Self::Io(error) => Diagnostic::new(DiagnosticCode::IoError)
                .with_details(json!({"kind":format!("{:?}",error.kind())})),
            Self::Json(error) => Diagnostic::new(DiagnosticCode::InvalidInput)
                .with_details(json!({"line":error.line(),"column":error.column()})),
        };
        diagnostic.exit_code = self.exit_code();
        diagnostic
    }

    /// Preserve standalone child exit codes and cancellation through execution wrappers.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Execution { source, .. } => source.exit_code(),
            Self::Cancelled => 130,
            Self::ProcessExit(code) => (*code).clamp(1, 255),
            Self::Diagnostic(diagnostic) => diagnostic.exit_code,
            _ => 1,
        }
    }
}
