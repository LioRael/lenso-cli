use std::process::Stdio;

use lenso_cli::{Context, Error, Outcome};
use serde::{Deserialize, Serialize};

/// An exact, shell-free process invocation; installations grant this complete value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessSpec {
    /// Executable resolved using the host PATH, or an explicit absolute path.
    pub program: String,
    /// Literal arguments, with no string interpolation or shell expansion.
    #[serde(default)]
    pub args: Vec<String>,
}

impl ProcessSpec {
    /// Supervise a granted process, forwarding logs to stderr and cancelling on Ctrl-C.
    ///
    /// The child receives no inherited environment except PATH. On Unix cancellation
    /// kills the process group; other platforms terminate the direct child only.
    ///
    /// # Errors
    /// Returns spawn, wait, cancellation, or nonzero-exit errors.
    pub async fn run(&self, ctx: Context) -> Result<Outcome, Error> {
        let events = ctx.events.clone();
        let mut command = tokio::process::Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(ctx.project)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        // Duplicate stderr so child stdout cannot corrupt the CLI's JSON protocol.
        command.stdout(Stdio::from(std::io::stderr()));
        if events.is_enabled() {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let logs = if events.is_enabled() {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| Error::Operation("child stdout unavailable".into()))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| Error::Operation("child stderr unavailable".into()))?;
            Some(tokio::spawn(async move {
                tokio::try_join!(forward(stdout, events.clone()), forward(stderr, events))?;
                Ok::<_, std::io::Error>(())
            }))
        } else {
            None
        };
        let status = tokio::select! {
            result = child.wait() => result?,
            signal = tokio::signal::ctrl_c() => {
                signal?;
                #[cfg(unix)]
                if let Some(id) = child.id() {
                    use nix::sys::signal::{Signal, killpg};
                    use nix::unistd::Pid;
                    let id = i32::try_from(id).map_err(|_| Error::Operation("invalid process group ID".into()))?;
                    killpg(Pid::from_raw(id), Signal::SIGKILL).map_err(|e| Error::Operation(format!("stopping process group: {e}")))?;
                }
                child.kill().await?;
                if let Some(logs) = logs { logs.await.map_err(|e| Error::Operation(e.to_string()))??; }
                return Err(Error::Cancelled);
            }
        };
        if let Some(logs) = logs {
            logs.await.map_err(|e| Error::Operation(e.to_string()))??;
        }
        if !status.success() {
            return Err(Error::ProcessExit(status.code().unwrap_or(1)));
        }
        Ok(Outcome::complete(serde_json::json!({"exit_code":0})))
    }
}

async fn forward(
    mut stream: impl tokio::io::AsyncRead + Unpin,
    events: lenso_cli::EventSink,
) -> Result<(), std::io::Error> {
    use tokio::io::AsyncReadExt;
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        events.log(buffer[..count].to_vec()).await;
    }
}
