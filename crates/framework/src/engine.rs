use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::{
    Context, Error, EventSink, ExecutionEvent, Outcome, Question, Report, Status, Workflow,
};

/// Object-safe asynchronous result for registered handlers.
pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<Outcome, Error>> + Send>>;
/// A trusted native adapter or a sandboxed runtime invocation.
pub type Handler = Arc<dyn Fn(Context) -> HandlerFuture + Send + Sync>;

/// Registry of handlers and workflows; registration never executes a handler.
#[derive(Default, Clone)]
pub struct Engine {
    handlers: BTreeMap<String, Handler>,
    workflows: BTreeMap<String, Workflow>,
    grants: BTreeMap<String, BTreeMap<String, Value>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("handlers", &self.handlers.keys())
            .field("workflows", &self.workflows)
            .finish()
    }
}

impl Engine {
    /// Construct an empty engine.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a native async handler, rejecting duplicate identifiers.
    ///
    /// # Errors
    /// Returns an error for an empty or duplicate identifier.
    pub fn handler<F, Fut>(&mut self, id: impl Into<String>, handler: F) -> Result<(), Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Outcome, Error>> + Send + 'static,
    {
        let id = id.into();
        if id.is_empty() || self.handlers.contains_key(&id) {
            return Err(Error::Invalid(format!("duplicate or empty handler: {id}")));
        }
        self.handlers
            .insert(id, Arc::new(move |ctx| Box::pin(handler(ctx))));
        Ok(())
    }

    /// Register a workflow after checking handler references and the dependency graph.
    ///
    /// # Errors
    /// Rejects unknown handlers, duplicate IDs, cycles, and graphs exceeding 256 steps.
    pub fn workflow(&mut self, workflow: Workflow) -> Result<(), Error> {
        if workflow.id.is_empty() || self.workflows.contains_key(&workflow.id) {
            return Err(Error::Invalid(format!(
                "duplicate or empty workflow: {}",
                workflow.id
            )));
        }
        if workflow.steps.is_empty() || workflow.steps.len() > 256 {
            return Err(Error::Invalid("workflows require 1..=256 steps".into()));
        }
        let mut ids = BTreeSet::new();
        for step in &workflow.steps {
            if step.id.is_empty() || !ids.insert(step.id.clone()) {
                return Err(Error::Invalid(format!(
                    "duplicate or empty step: {}",
                    step.id
                )));
            }
            for handler in std::iter::once(&step.handler)
                .chain(&step.hooks.prepare)
                .chain(&step.hooks.before)
                .chain(&step.hooks.validate)
                .chain(&step.hooks.after)
            {
                if !self.handlers.contains_key(handler) {
                    return Err(Error::Invalid(format!("unknown handler: {handler}")));
                }
            }
        }
        let mut done = BTreeSet::new();
        while done.len() < workflow.steps.len() {
            let before = done.len();
            for step in &workflow.steps {
                if step.needs.iter().all(|id| done.contains(id)) {
                    done.insert(step.id.clone());
                }
            }
            if before == done.len() {
                return Err(Error::Invalid(format!(
                    "cycle or unknown dependency in {}",
                    workflow.id
                )));
            }
        }
        self.workflows.insert(workflow.id.clone(), workflow);
        Ok(())
    }

    /// Iterate installed workflow identifiers in stable order.
    pub fn workflows(&self) -> impl Iterator<Item = &str> {
        self.workflows.keys().map(String::as_str)
    }

    /// Whether an implementation is registered under this exact identifier.
    #[must_use]
    pub fn contains_handler(&self, id: &str) -> bool {
        self.handlers.contains_key(id)
    }

    /// Attach an application-owned capability value to a registered workflow.
    ///
    /// Extensions cannot edit these grants through handler outputs.
    ///
    /// # Errors
    /// Rejects unknown workflow IDs and empty capability names.
    pub fn grant(&mut self, workflow: &str, capability: &str, value: Value) -> Result<(), Error> {
        if !self.workflows.contains_key(workflow) || capability.is_empty() {
            return Err(Error::Invalid(
                "grant requires a registered workflow and capability name".into(),
            ));
        }
        self.grants
            .entry(workflow.to_owned())
            .or_default()
            .insert(capability.to_owned(), value);
        Ok(())
    }

    /// Create an isolated execution session for a workflow.
    ///
    /// # Errors
    /// Rejects unknown workflows, non-object input, and inaccessible project directories.
    pub fn session(
        &self,
        id: &str,
        project: &Path,
        input: Value,
        answers: BTreeMap<String, Value>,
    ) -> Result<Session, Error> {
        let workflow = self
            .workflows
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("unknown workflow: {id}")))?
            .clone();
        if !input.is_object() {
            return Err(Error::Invalid("command input must be an object".into()));
        }
        let project = project.canonicalize()?;
        if !project.is_dir() {
            return Err(Error::Invalid("project must be a directory".into()));
        }
        Ok(Session {
            workflow,
            context: Context {
                events: EventSink::default(),
                abi_version: 1,
                project,
                input,
                answers,
                steps: BTreeMap::new(),
                hooks: BTreeMap::new(),
                capabilities: self.grants.get(id).cloned().unwrap_or_default(),
            },
            questions: BTreeMap::new(),
            question_order: Vec::new(),
            outputs: BTreeMap::new(),
        })
    }

    async fn invoke(&self, id: &str, ctx: Context) -> Result<Outcome, Error> {
        self.handlers
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("unknown handler: {id}")))?(ctx)
        .await
    }
}

/// Resumable state; changing an observed answer invalidates its step and descendants.
#[derive(Debug)]
pub struct Session {
    workflow: Workflow,
    context: Context,
    questions: BTreeMap<String, (String, Question)>,
    question_order: Vec<String>,
    outputs: BTreeMap<String, Value>,
}

impl Session {
    /// Attach a native progress/log observer without changing the Wasm JSON context.
    pub fn observe(&mut self, events: EventSink) {
        self.context.events = events;
    }
    /// Supply or revise an observed answer without replaying unrelated completed steps.
    ///
    /// # Errors
    /// Rejects unknown questions and answers with the wrong type or value.
    pub fn answer(&mut self, id: &str, value: Value) -> Result<(), Error> {
        let (owner, question) = self
            .questions
            .get(id)
            .ok_or_else(|| Error::Invalid(format!("unknown question: {id}")))?;
        question.validate(&value)?;
        if self.context.answers.get(id) == Some(&value) {
            return Ok(());
        }
        let mut invalid = BTreeSet::from([owner.clone()]);
        loop {
            let before = invalid.len();
            for step in &self.workflow.steps {
                if step.needs.iter().any(|dep| invalid.contains(dep)) {
                    invalid.insert(step.id.clone());
                }
            }
            if before == invalid.len() {
                break;
            }
        }
        self.outputs.retain(|step, _| !invalid.contains(step));
        // Downstream consent is bound to its old inputs; it must be requested again.
        let position = self
            .question_order
            .iter()
            .position(|question| question == id);
        for (index, question_id) in self.question_order.iter().enumerate() {
            let (step, _) = &self.questions[question_id];
            if question_id != id
                && invalid.contains(step)
                && (step != owner || position.is_some_and(|position| index > position))
            {
                self.context.answers.remove(question_id);
            }
        }
        self.context.answers.insert(id.to_owned(), value);
        Ok(())
    }

    /// Advance until completion or the first unanswered question.
    ///
    /// Handlers and hooks that yield questions must be safe to repeat until their
    /// whole step completes. Completed steps are cached within this session.
    ///
    /// # Errors
    /// Propagates handler failures and rejects malformed or repeatedly answered questions.
    pub async fn advance(&mut self, engine: &Engine) -> Result<Report, Error> {
        while self.outputs.len() < self.workflow.steps.len() {
            let step = self
                .workflow
                .steps
                .iter()
                .find(|s| {
                    !self.outputs.contains_key(&s.id)
                        && s.needs.iter().all(|id| self.outputs.contains_key(id))
                })
                .ok_or_else(|| Error::Invalid("workflow cannot advance".into()))?;
            self.context.events.emit(ExecutionEvent::Started {
                id: step.id.clone(),
            });
            let mut ctx = self.context.clone();
            ctx.steps = step
                .needs
                .iter()
                .map(|id| (id.clone(), self.outputs[id].clone()))
                .collect();
            let mut output = Value::Null;
            let calls = step
                .hooks
                .prepare
                .iter()
                .map(|h| ("prepare", h))
                .chain(step.hooks.before.iter().map(|h| ("before", h)))
                .chain(std::iter::once(("result", &step.handler)))
                .chain(step.hooks.validate.iter().map(|h| ("validate", h)))
                .chain(step.hooks.after.iter().map(|h| ("after", h)));
            for (phase, handler) in calls {
                match engine.invoke(handler, ctx.clone()).await? {
                    Outcome::Complete { output: value } => {
                        if phase == "result" {
                            output = value.clone();
                            ctx.hooks.insert("result".into(), value);
                        } else {
                            ctx.hooks.insert(format!("{phase}:{handler}"), value);
                        }
                    }
                    Outcome::NeedsInput { question } => {
                        if question.id.is_empty() || question.message.is_empty() {
                            return Err(Error::Invalid(
                                "question ID and message must be nonempty".into(),
                            ));
                        }
                        if let Some((owner, _)) = self.questions.get(&question.id)
                            && owner != &step.id
                        {
                            return Err(Error::Invalid(format!(
                                "question ID reused across steps: {}",
                                question.id
                            )));
                        }
                        if let Some(answer) = self.context.answers.get(&question.id) {
                            question.validate(answer)?;
                            return Err(Error::Invalid(format!(
                                "handler requested already answered question {}",
                                question.id
                            )));
                        }
                        if !self.questions.contains_key(&question.id) {
                            self.question_order.push(question.id.clone());
                        }
                        self.questions
                            .insert(question.id.clone(), (step.id.clone(), question.clone()));
                        return Ok(self.report(Some(question)));
                    }
                }
            }
            self.context.events.emit(ExecutionEvent::Completed {
                id: step.id.clone(),
            });
            self.outputs.insert(step.id.clone(), output);
        }
        Ok(self.report(None))
    }

    fn report(&self, question: Option<Question>) -> Report {
        Report {
            r#type: "result".into(),
            workflow: self.workflow.id.clone(),
            status: if question.is_some() {
                Status::NeedsInput
            } else {
                Status::Complete
            },
            outputs: self.outputs.clone(),
            question,
        }
    }
}
