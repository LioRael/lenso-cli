//! Build branded CLIs from native handlers and explicitly installed workflows.

mod cli;
mod engine;
mod events;
mod protocol;
mod terminal;

pub use cli::{Argument, ArgumentKind, Cli, CommandSpec};
pub use engine::{Engine, Handler, HandlerFuture, Session};
pub use events::{EventSink, EventStream, ExecutionEvent};
pub use protocol::{
    Context, Error, Hooks, Outcome, Question, QuestionKind, Report, Status, Step, Workflow,
};
pub use serde_json;
