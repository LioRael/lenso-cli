//! Optional file planning and guarded application, independent of the execution engine.

/// Native provider version, independent of the executable release version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod plan;

pub use plan::{FileChange, Plan, PlanError};

use lenso_cli::{
    Context, Diagnostic, DiagnosticCode, Engine, Error, Outcome, Question, QuestionKind,
    StepSummary,
};

/// Register optional `project/plan`, `project/diff`, and `project/apply` handlers.
///
/// `project/plan` consumes one dependency output containing `files`. `project/apply`
/// consumes one dependency output containing `plan` and requests digest-bound consent.
///
/// # Errors
/// Returns a registration conflict if these handler IDs already exist.
pub fn register(engine: &mut Engine) -> Result<(), Error> {
    for (handler, writable) in [("project/plan", true), ("project/diff", false)] {
        engine.handler(handler, move |ctx: Context| async move {
        let sources: Vec<_> = ctx.steps.values().filter_map(|v| v.get("files")).collect();
        if sources.len() != 1 { return Err(Error::Invalid("project/plan requires exactly one files dependency".into())); }
        let files: Vec<FileChange> = serde_json::from_value(sources[0].clone())?;
        let deletions: Vec<std::path::PathBuf> = ctx.steps.values().filter_map(|output| output.get("deletions")).map(|value| serde_json::from_value::<Vec<std::path::PathBuf>>(value.clone())).collect::<Result<Vec<_>,_>>()?.into_iter().flatten().collect();
        if writable { check_files_grant(&ctx, deletions.iter().map(std::path::PathBuf::as_path))?; }
        else { check_reads_grant(&ctx, deletions.iter().map(std::path::PathBuf::as_path))?; }
        if writable { check_files_grant(&ctx, files.iter().map(|file| file.path.as_path()))?; }
        else { check_reads_grant(&ctx, files.iter().map(|file| file.path.as_path()))?; }
        let mut evidence = lenso_runtime::Evidence::default();
        let mut commands = Vec::new();
        for output in ctx.steps.values() {
            if let Some(value) = output.get("commands") { commands.extend(serde_json::from_value::<Vec<lenso_runtime::ProcessSpec>>(value.clone())?); }
            if let Some(value) = output.get("_lenso") {
                let observed: lenso_runtime::Evidence = serde_json::from_value(value.clone())?;
                for (path, digest) in observed.reads {
                    if let Some(old) = evidence.reads.insert(path.clone(), digest.clone()) && old != digest { return Err(Error::Operation(format!("project input changed: {}", path.display()))); }
                }
                for (url, digest) in observed.sources {
                    if let Some(previous) = evidence.sources.insert(url.clone(), digest.clone()) && previous != digest { return Err(Error::Operation(format!("HTTP source changed between steps: {url}"))); }
                }
                for (key,pin) in observed.resources {
                    if let Some(old)=evidence.resources.get_mut(&key) {
                        if old.commit!=pin.commit || old.api!=pin.api || pin.files.iter().any(|(path,sha)|old.files.get(path).is_some_and(|previous|previous!=sha)) {return Err(Error::Operation("repository pin changed between steps".into()));}
                        old.files.extend(pin.files);
                    }else{evidence.resources.insert(key,pin);}
                }
                evidence.modules.extend(observed.modules);
            }
        }
        check_reads_grant(&ctx, evidence.reads.keys().map(std::path::PathBuf::as_path))?;
        let finalizations: Vec<FileChange> = ctx.steps.values().filter_map(|output|output.get("finalizations")).map(|value|serde_json::from_value::<Vec<FileChange>>(value.clone())).collect::<Result<Vec<_>,_>>()?.into_iter().flatten().collect();
        let plan = Plan::build_operations(&ctx.project, files, deletions).and_then(|plan|plan.with_finalizations(finalizations)).and_then(|p| p.with_evidence(evidence)).map_err(plan_error)?.with_commands(commands);
        Ok(Outcome::complete(serde_json::json!({"id": plan.id().map_err(plan_error)?, "diff":plan.preview(), "plan":plan})))
    })?;
        engine.summarizer(handler, |output| {
            Some(StepSummary {
                affected: Some(vec![]),
                identifiers: std::collections::BTreeMap::from([(
                    "plan_id".into(),
                    output.get("id")?.as_str()?.into(),
                )]),
                operations: Some(0),
            })
        })?;
    }
    engine.handler("project/apply", |ctx: Context| async move {
        let sources: Vec<_> = ctx.steps.values().filter_map(|v| v.get("plan")).collect();
        if sources.len() != 1 {
            return Err(Error::Invalid(
                "project/apply requires exactly one plan dependency".into(),
            ));
        }
        let plan: Plan = serde_json::from_value(sources[0].clone())?;
        check_files_grant(&ctx, plan.paths())?;
        check_reads_grant(&ctx, plan.reads())?;
        let granted: Vec<lenso_runtime::ProcessSpec> = ctx.capabilities.get("host.processes").cloned().map(serde_json::from_value).transpose()?.unwrap_or_default();
        for command in plan.commands() { if !granted.contains(command) { return Err(Diagnostic::missing_capability("host.processes").into()); } }
        if plan.project() != ctx.project {
            return Err(Error::Permission(
                "plan belongs to a different project".into(),
            ));
        }
        let id = plan.id().map_err(plan_error)?;
        let question_id = format!("apply:{id}");
        if plan.paths().next().is_some() { match ctx.answers.get(&question_id) {
            None => return Ok(Outcome::NeedsInput {
                question: Question {
                    id: question_id,
                    message: format!("{}\nApprove this plan's file changes?", plan.preview()),
                    kind: QuestionKind::Confirm,
                },
            }),
            Some(serde_json::Value::Bool(true)) => {},
            Some(serde_json::Value::Bool(false)) => return Err(Error::Cancelled),
            Some(_) => return Err(Error::Invalid("file approval must be a boolean".into())),
        } }
        if !plan.commands().is_empty() {
            let question_id = format!("commands:{id}");
            match ctx.answers.get(&question_id) {
                None => return Ok(Outcome::NeedsInput { question:Question { id:question_id, message:format!("{}\nAlso authorize these external commands? No file changes have been applied yet.", plan.preview()), kind:QuestionKind::Confirm } }),
                Some(serde_json::Value::Bool(true)) => {},
                Some(serde_json::Value::Bool(false)) => return Err(Error::Cancelled),
                Some(_) => return Err(Error::Invalid("command approval must be a boolean".into())),
            }
        }
        let completed = plan.apply(&id).map_err(plan_error)?;
        let mut executed = Vec::new();
        for command in plan.commands() {
            if let Err(error) = command.run(ctx.clone()).await {
                return Err(Diagnostic::new(DiagnosticCode::PartialFailure)
                    .with_details(serde_json::json!({"plan_id":id,"completed_files":completed,
                        "completed_commands":(0..executed.len()).collect::<Vec<_>>(),"failed_command_index":executed.len(),
                        "cause":error.diagnostic(),"partial_side_effects":true}))
                    .with_retryable(false).with_recovery("inspect_partial", "Inspect changed files and completed command indices against the reviewed plan before creating another plan.").into());
            }
            executed.push(command.clone());
        }
        let finalized = plan.finalize(&id).map_err(|error| Diagnostic::new(DiagnosticCode::PartialFailure)
            .with_details(serde_json::json!({"plan_id":id,"completed_files":completed,
                "completed_commands":(0..executed.len()).collect::<Vec<_>>(),"completion":plan_error(error).diagnostic(),"partial_side_effects":true}))
            .with_retryable(false).with_recovery("inspect_partial", "Inspect prepared files and completion records before planning recovery."))?;
        Ok(Outcome::complete(serde_json::json!({"applied":completed,"finalized":finalized,"commands":executed,"plan_id":id})))
    })?;
    engine.summarizer("project/apply", |output| {
        Some(StepSummary {
            affected: Some(serde_json::from_value(output.get("applied")?.clone()).ok()?),
            identifiers: std::collections::BTreeMap::from([(
                "plan_id".into(),
                output.get("plan_id")?.as_str()?.into(),
            )]),
            operations: Some(output.get("commands")?.as_array()?.len()),
        })
    })
}

fn plan_error(error: PlanError) -> Error {
    let diagnostic = match error {
        PlanError::Stale(path) => Diagnostic::new(DiagnosticCode::StalePlan)
            .with_details(serde_json::json!({"path":path,"plan_files_applied":false}))
            .with_recovery("review_plan", "Re-read dependencies and create a new plan; the old approval is stale."),
        PlanError::EvidenceChanged => Diagnostic::new(DiagnosticCode::StalePlan)
            .with_details(serde_json::json!({"dependency_kind":"repository","plan_files_applied":false}))
            .with_recovery("review_plan", "Refresh repository evidence and create a new plan; the old approval is stale."),
        PlanError::Partial { completed, .. } => Diagnostic::new(DiagnosticCode::PartialFailure)
            .with_details(serde_json::json!({"completed_files":completed,"partial_side_effects":true}))
            .with_recovery("inspect_partial", "Inspect the completed paths before planning recovery; no global rollback occurred."),
        PlanError::Invalid(_) => Diagnostic::new(DiagnosticCode::InvalidPlan)
            .with_recovery("review_plan", "Correct the plan's paths, limits, or approval digest and review it again."),
        PlanError::Io(error) => Diagnostic::new(DiagnosticCode::IoError)
            .with_details(serde_json::json!({"kind":format!("{:?}",error.kind()),"partial_side_effects":"unknown"})),
        PlanError::Json(_) => Diagnostic::new(DiagnosticCode::InvalidPlan),
    };
    diagnostic.with_retryable(false).into()
}

fn check_reads_grant<'a>(
    ctx: &Context,
    paths: impl Iterator<Item = &'a std::path::Path>,
) -> Result<(), Error> {
    let allowed: Vec<std::path::PathBuf> = ctx
        .capabilities
        .get("host.reads")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let roots: Vec<std::path::PathBuf> = ctx
        .capabilities
        .get("host.read_roots")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    for path in paths {
        if !lenso_runtime::path_allowed(path, &allowed, &roots) {
            return Err(Diagnostic::missing_capability("host.reads").into());
        }
    }
    Ok(())
}

fn check_files_grant<'a>(
    ctx: &Context,
    paths: impl Iterator<Item = &'a std::path::Path>,
) -> Result<(), Error> {
    let allowed: Vec<std::path::PathBuf> = ctx
        .capabilities
        .get("project.files")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let roots: Vec<std::path::PathBuf> = ctx
        .capabilities
        .get("project.write_roots")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    for path in paths {
        if !lenso_runtime::path_allowed(path, &allowed, &roots) {
            return Err(Diagnostic::missing_capability("project.files").into());
        }
    }
    Ok(())
}
