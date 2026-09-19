//! CLI presentation only. Workflow loading, processing, and publication are library APIs.
use clap::{Args, Subcommand};
use lenso_engine::{Engine, bootstrap::BootstrapLock, session::Session};
use lenso_engine_runtime::RuntimeProcessor;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Debug, Subcommand)]
pub(crate) enum EngineCommand {
    Inspect(Inputs),
    Run(Inputs),
    Dev(Inputs),
    /// Resolve explicitly selected local processors and pin their existing artifacts.
    Lock {
        #[arg(long, default_value = "engine.json")]
        workflow: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}
#[derive(Debug, Args)]
pub(crate) struct Inputs {
    #[arg(long)]
    source: Vec<PathBuf>,
    #[arg(long)]
    workflow: Option<PathBuf>,
    #[arg(long = "plugin")]
    plugins: Vec<PathBuf>,
    #[arg(long)]
    markdown: bool,
    /// Publish immutable resources and atomically update current.json.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn load(inputs: &Inputs, inspect: bool) -> anyhow::Result<(Session, String)> {
    let mut engine = Engine::default();
    let mut sources = inputs.source.clone();
    let mut revision = String::new();
    if let Some(workflow) = &inputs.workflow {
        let lock = if inspect {
            BootstrapLock::resolve(workflow)?
        } else {
            BootstrapLock::load(&workflow.with_extension("lock.json"), workflow)?
        };
        revision = serde_json::to_string(&lock)?;
        sources.extend(lock.sources.clone());
        for processor in lock.processors()? {
            engine.register(RuntimeProcessor::new(processor))?;
        }
    }
    if inputs.markdown {
        engine.register(RuntimeProcessor::new(lenso_engine_markdown::Markdown))?;
    }
    for manifest in &inputs.plugins {
        engine.register(RuntimeProcessor::new(
            lenso_engine::external::ProcessPlugin::load(manifest)?,
        ))?;
    }
    if let Some(output) = &inputs.output {
        let output = if output.exists() {
            output.canonicalize()?
        } else {
            let parent = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            parent.canonicalize()?.join(
                output
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("output directory required"))?,
            )
        };
        for source in &sources {
            if output.starts_with(source.canonicalize()?) {
                anyhow::bail!("publication output must be outside input sources");
            }
        }
    }
    Ok((Session::new(engine, sources)?, revision))
}
fn execute(
    command: EngineCommand,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<serde_json::Value> {
    if let EngineCommand::Lock { workflow, out } = command {
        let lock = BootstrapLock::resolve(&workflow)?;
        let out = out.unwrap_or_else(|| workflow.with_extension("lock.json"));
        lock.save(&out)?;
        return Ok(serde_json::json!({"lock":out,"plugins":lock.plugins.len()}));
    }
    let inspect = matches!(command, EngineCommand::Inspect(_));
    let watch = matches!(command, EngineCommand::Dev(_));
    let (EngineCommand::Inspect(mut inputs)
    | EngineCommand::Run(mut inputs)
    | EngineCommand::Dev(mut inputs)) = command
    else {
        unreachable!()
    };
    if inputs.workflow.is_none() && inputs.source.is_empty() && Path::new("engine.json").is_file() {
        inputs.workflow = Some("engine.json".into());
    }
    let (mut session, mut revision) = load(&inputs, inspect)?;
    if inspect {
        return Ok(serde_json::to_value(
            session.engine().plan(session.snapshot()?)?,
        )?);
    }
    let mut last_error = None;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(serde_json::json!({"stopped":true}));
        }
        let update = (|| -> anyhow::Result<Option<serde_json::Value>> {
            if inputs.workflow.is_some() {
                let (replacement, next) = load(&inputs, false)?;
                if next != revision {
                    session = replacement;
                    revision = next;
                }
            }
            if let Some(generation) = session.refresh(cancelled)? {
                let publication = match inputs
                    .output
                    .as_ref()
                    .map(|out| lenso_engine::publication::publish(out, &generation))
                    .transpose()
                {
                    Ok(publication) => publication,
                    Err(error) => {
                        session.invalidate();
                        return Err(error);
                    }
                };
                return Ok(Some(
                    serde_json::json!({"outputs":generation.outputs,"cache_hits":generation.cache_hits,"publication":publication}),
                ));
            }
            Ok(None)
        })();
        match update {
            Ok(Some(value)) => {
                last_error = None;
                if !watch {
                    return Ok(value);
                }
                println!(
                    "{}",
                    serde_json::json!({"kind":"updated","generation":value})
                );
            }
            Ok(None) => {}
            Err(error) => {
                if !watch {
                    return Err(error);
                }
                let message = format!("{error:#}");
                if last_error.as_ref() != Some(&message) {
                    println!("{}", serde_json::json!({"kind":"failed","message":message}));
                    last_error = Some(message);
                }
            }
        }
        if !watch {
            anyhow::bail!("no generation produced");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub(crate) async fn run(command: EngineCommand) -> anyhow::Result<()> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancelled.clone();
    let mut task = tokio::task::spawn_blocking(move || execute(command, &worker_cancel));
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let result = tokio::select! {
        result=&mut task => result??,
        signal=tokio::signal::ctrl_c() => { signal?; cancelled.store(true,Ordering::SeqCst); task.await?? },
        ()=async {
            #[cfg(unix)] terminate.recv().await;
            #[cfg(not(unix))] std::future::pending::<()>().await;
        } => { cancelled.store(true,Ordering::SeqCst); task.await?? }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
