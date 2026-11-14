use std::sync::Arc;

use lenso_cli::{Context, Diagnostic, DiagnosticCode, Error, Outcome};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::config::{self, Health, Readiness, Workflow};

/// Keep stream evidence small even when a tool writes indefinitely.
const LOG_BYTES: usize = 4096;

#[derive(Debug, Serialize)]
struct Stream {
    text: String,
    bytes: u64,
    truncated: bool,
}

struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn capture(
    mut stream: impl AsyncRead + Unpin,
    secrets: Arc<Vec<Vec<u8>>>,
) -> Result<Stream, std::io::Error> {
    let lookahead = secrets.iter().map(Vec::len).max().unwrap_or(1);
    let mut raw = Vec::new();
    let mut bytes = 0;
    let mut chunk = [0; 4096];
    loop {
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        let keep = count.min((LOG_BYTES + lookahead).saturating_sub(raw.len()));
        raw.extend_from_slice(&chunk[..keep]);
    }
    let truncated = bytes > raw.len() as u64;
    // Lookahead prevents a captured prefix from exposing a secret cut by the byte budget.
    let end = if truncated {
        raw.len().saturating_sub(lookahead)
    } else {
        raw.len()
    };
    let mut cursor = 0;
    let mut public = Vec::new();
    while cursor < end {
        if let Some(secret) = secrets
            .iter()
            .find(|secret| raw[cursor..].starts_with(secret))
        {
            public.extend_from_slice(b"[redacted]");
            cursor += secret.len();
        } else {
            public.push(raw[cursor]);
            cursor += 1;
        }
    }
    let mut text = String::from_utf8_lossy(&public).into_owned();
    let truncated = truncated || text.len() > LOG_BYTES;
    let mut end = text.len().min(LOG_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    Ok(Stream {
        text,
        bytes,
        truncated,
    })
}

async fn readiness(ctx: &Context, workflow: &Workflow) -> Result<Health, Error> {
    let mut last = None;
    let polling = async {
        if matches!(workflow.readiness, Readiness::Exit { .. }) {
            return std::future::pending::<Result<Health, Error>>().await;
        }
        let client = config::health_client()?;
        loop {
            let evidence = config::probe(ctx, &client, &workflow.readiness).await?;
            if evidence.state == "ready" {
                return Ok(evidence);
            }
            last = Some(evidence);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    };
    match tokio::time::timeout(
        std::time::Duration::from_millis(workflow.readiness_timeout_ms),
        polling,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Diagnostic::new(DiagnosticCode::VerificationFailed)
            .with_details(json!({"reason":"readiness_deadline","last_probe":last}))
            .with_retryable(false)
            .into()),
    }
}

#[cfg(unix)]
struct Managed {
    child: tokio::process::Child,
    group: nix::unistd::Pid,
    cleaned: bool,
}

#[cfg(unix)]
impl Drop for Managed {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = nix::sys::signal::killpg(self.group, nix::sys::signal::Signal::SIGKILL);
        }
    }
}

#[cfg(unix)]
impl Managed {
    async fn cleanup(&mut self, already_exited: bool) -> Value {
        use nix::errno::Errno;
        use nix::sys::signal::{Signal, killpg};
        let signal = if already_exited {
            Signal::SIGKILL
        } else {
            Signal::SIGTERM
        };
        let first = killpg(self.group, signal);
        let mut delivered = first.is_ok() || first == Err(Errno::ESRCH);
        let mut error = first.err();
        if !already_exited && first != Err(Errno::ESRCH) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            // Darwin reports EPERM for a group containing only the unreaped leader.
            let _ = self.child.try_wait();
            let killed = killpg(self.group, Signal::SIGKILL);
            delivered = killed.is_ok() || killed == Err(Errno::ESRCH);
            error = killed.err();
        }
        let reaped = matches!(
            tokio::time::timeout(std::time::Duration::from_secs(1), self.child.wait()).await,
            Ok(Ok(_))
        );
        self.cleaned = delivered && reaped;
        json!({"group_signalled":delivered,"group_errno":if delivered {None} else {error.map(|error|error as i32)},"child_reaped":reaped,"scope":"owned_unix_process_group"})
    }
}

#[cfg(not(unix))]
pub(crate) async fn run(_ctx: Context) -> Result<Outcome, Error> {
    Err(config::invalid("platform", "unix_process_groups_required"))
}

#[cfg(unix)]
pub(crate) async fn run(ctx: Context) -> Result<Outcome, Error> {
    use nix::unistd::Pid;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::signal::unix::{SignalKind, signal};

    let config = config::load(&ctx)?;
    let Some(name) = config::selection(&ctx)? else {
        return Ok(config.question());
    };
    let workflow = config.workflow(name)?;
    let issues = config::preflight(&ctx, workflow).await;
    if !issues.is_empty() {
        let code = if issues
            .iter()
            .any(|issue| issue["code"] == "permission_denied")
        {
            DiagnosticCode::PermissionDenied
        } else {
            DiagnosticCode::InvalidInput
        };
        return Err(Diagnostic::new(code).with_details(json!({"reason":"preflight_failed","issues":issues}))
            .with_retryable(false).with_recovery("inspect_workflow", "Run ops inspect for this workflow and correct its prerequisites before starting it.").into());
    }
    let environment = workflow.environment(&ctx)?;
    let mut secrets: Vec<_> = environment
        .values()
        .map(|value| value.as_bytes().to_vec())
        .collect();
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    let secrets = Arc::new(secrets);
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut command = tokio::process::Command::new(workflow.executable(&ctx)?);
    command
        .args(&workflow.args)
        .current_dir(workflow.directory(&ctx)?)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    let child = command.spawn().map_err(|error| {
        Diagnostic::new(DiagnosticCode::ProcessSpawnFailed)
            .with_details(json!({"kind":format!("{:?}",error.kind())}))
            .with_retryable(false)
    })?;
    let pid = child
        .id()
        .ok_or_else(|| config::invalid("process", "missing_child_id"))?;
    let group = Pid::from_raw(
        i32::try_from(pid).map_err(|_| config::invalid("process", "invalid_child_id"))?,
    );
    let mut managed = Managed {
        child,
        group,
        cleaned: false,
    };
    let stdout = managed
        .child
        .stdout
        .take()
        .ok_or_else(|| config::invalid("process", "stdout_unavailable"))?;
    let stderr = managed
        .child
        .stderr
        .take()
        .ok_or_else(|| config::invalid("process", "stderr_unavailable"))?;
    let mut stdout = AbortOnDrop(tokio::spawn(capture(stdout, secrets.clone())));
    let mut stderr = AbortOnDrop(tokio::spawn(capture(stderr, secrets)));
    let checking = readiness(&ctx, workflow);
    tokio::pin!(checking);
    let lifetime = tokio::time::sleep(Duration::from_millis(workflow.timeout_ms));
    tokio::pin!(lifetime);
    let mut observed = None;
    enum Stop {
        Exited(std::io::Result<std::process::ExitStatus>),
        Cancelled(&'static str),
        Timeout,
        NotReady(Error),
    }
    let stop = loop {
        tokio::select! {
            biased;
            _=interrupt.recv()=>break Stop::Cancelled("SIGINT"),
            _=terminate.recv()=>break Stop::Cancelled("SIGTERM"),
            status=managed.child.wait()=>break Stop::Exited(status),
            ()=&mut lifetime=>break Stop::Timeout,
            health=&mut checking, if observed.is_none()=>match health {
                Ok(health)=>{
                    observed=Some(health);
                    ctx.events.preparing("Process ready; foreground supervision continues");
                },
                Err(error)=>break Stop::NotReady(error),
            }
        }
    };
    let mut cleanup = managed.cleanup(matches!(stop, Stop::Exited(_))).await;
    let logs = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(&mut stdout.0, &mut stderr.0)
    })
    .await;
    let (out, err) = match logs {
        Ok((Ok(Ok(out)), Ok(Ok(err)))) => (Some(out), Some(err)),
        _ => {
            stdout.0.abort();
            stderr.0.abort();
            (None, None)
        }
    };
    cleanup["logs_drained"] = json!(out.is_some() && err.is_some());
    let mut evidence = json!({"workflow":name,"pid":pid,"readiness":workflow.readiness,"ready_observed":observed,
        "cleanup":cleanup,"stdout":out,"stderr":err});
    let diagnostic = match stop {
        Stop::Cancelled(signal) => {
            evidence["state"] = json!("cancelled");
            evidence["signal"] = json!(signal);
            Diagnostic::new(DiagnosticCode::Cancelled)
        }
        Stop::Timeout => {
            evidence["state"] = json!("timed_out");
            evidence["timeout_ms"] = json!(workflow.timeout_ms);
            Diagnostic::new(DiagnosticCode::ProcessTimeout)
        }
        Stop::NotReady(error) => {
            evidence["state"] = json!("not_ready");
            evidence["cause"] = json!(error.diagnostic());
            Diagnostic::new(DiagnosticCode::VerificationFailed)
        }
        Stop::Exited(Err(error)) => {
            evidence["state"] = json!("failed");
            evidence["cause"] = json!(Error::Io(error).diagnostic());
            Diagnostic::new(DiagnosticCode::OperationFailed)
        }
        Stop::Exited(Ok(status)) => {
            evidence["state"] = json!(if status.success() {
                "not_ready"
            } else {
                "failed"
            });
            evidence["exit_code"] = json!(status.code());
            evidence["termination_signal"] = json!(status.signal());
            match &workflow.readiness {
                Readiness::Exit { code } if status.code() == Some(i32::from(*code)) => {
                    evidence["exit_verified"] = json!(true);
                    evidence["state"] = json!("complete");
                    return completed(evidence);
                }
                Readiness::Http { .. } if status.success() && observed.is_some() => {
                    evidence["state"] = json!("complete");
                    return completed(evidence);
                }
                _ if !status.success() => {
                    Error::ProcessExit(status.code().unwrap_or(1)).diagnostic()
                }
                _ => Diagnostic::new(DiagnosticCode::VerificationFailed),
            }
        }
    };
    Err(diagnostic.with_details(evidence).with_retryable(false)
        .with_recovery("inspect_run", "Inspect the readiness evidence, separate streams and cleanup result before starting another run.").into())
}

fn completed(mut evidence: Value) -> Result<Outcome, Error> {
    if evidence["cleanup"]["group_signalled"] != true
        || evidence["cleanup"]["child_reaped"] != true
        || evidence["cleanup"]["logs_drained"] != true
    {
        evidence["state"] = json!("cleanup_failed");
        return Err(Diagnostic::new(DiagnosticCode::OperationFailed)
            .with_details(evidence)
            .with_retryable(false)
            .with_recovery(
                "inspect_cleanup",
                "Inspect process cleanup before starting another run.",
            )
            .into());
    }
    Ok(Outcome::complete(evidence))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn graceful_exit_is_reaped_before_escalating_group_cleanup() {
        let child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let group = nix::unistd::Pid::from_raw(i32::try_from(child.id().unwrap()).unwrap());
        let mut managed = Managed {
            child,
            group,
            cleaned: false,
        };
        let evidence = managed.cleanup(false).await;
        assert_eq!(evidence["child_reaped"], true);
        assert_eq!(evidence["group_signalled"], true, "{evidence}");
    }

    #[tokio::test]
    async fn streams_are_separate_bounded_and_redact_split_secrets() {
        let secrets = Arc::new(vec![b"private-secret".to_vec()]);
        let data = [
            vec![b'x'; LOG_BYTES - 4],
            b"private-secret".to_vec(),
            vec![b'y'; LOG_BYTES],
        ]
        .concat();
        let stream = capture(data.as_slice(), secrets).await.unwrap();
        assert!(stream.truncated);
        assert!(stream.text.len() <= LOG_BYTES);
        assert!(!stream.text.contains("private"));
        let stream = capture(
            b"stdout private-secret end".as_slice(),
            Arc::new(vec![b"private-secret".to_vec()]),
        )
        .await
        .unwrap();
        assert_eq!(stream.text, "stdout [redacted] end");
    }
}
