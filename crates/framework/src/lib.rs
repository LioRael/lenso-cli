//! Build branded CLIs from native handlers and explicitly installed workflows.

mod cli;
mod description;
mod diagnostic;
mod engine;
mod events;
mod protocol;
mod summary;
mod terminal;

pub use cli::{Argument, ArgumentKind, Cli, CommandSpec};
pub use description::{CommandCatalog, CommandContract, CommandDescription};
pub use diagnostic::{Diagnostic, DiagnosticCode, Recovery};
pub use engine::{Engine, Handler, HandlerFuture, Session};
pub use events::{EventSink, EventStream, ExecutionEvent};
pub use protocol::{
    Context, Error, Hooks, Outcome, Question, QuestionKind, Report, Status, Step, Workflow,
};
pub use serde_json;
pub use summary::{CallSummary, ExecutionSummary, StepSummary, VerificationSummary};
