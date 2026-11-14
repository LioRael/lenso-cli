use std::{process::Stdio, time::Duration};

use lenso_cli::{Diagnostic, DiagnosticCode, Error};
use tokio::{io::AsyncReadExt, process::Command, task::JoinHandle};
use tokio_util::sync::CancellationToken;

pub(crate) const MAX_JSON: usize = 4 * 1024 * 1024;

pub(crate) struct ProcessOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stopped: Option<DiagnosticCode>,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_exit_status_is_preserved_and_stderr_is_not_a_protocol_payload() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "printf stdout-marker; printf private-stderr >&2; exit 7",
        ]);
        let output = run(command, &CancellationToken::new(), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(output.code, 7);
        assert_eq!(output.stdout, b"stdout-marker");
        assert!(output.stopped.is_none());
    }

    #[tokio::test]
    async fn oversized_stdout_is_drained_and_rejected_with_no_payload_echo() {
        let mut command = Command::new("/usr/bin/head");
        command.args(["-c", &(MAX_JSON + 1).to_string(), "/dev/zero"]);
        let error = run(command, &CancellationToken::new(), Duration::from_secs(5))
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.diagnostic().details()["reason"],
            "json_limit_exceeded"
        );
    }

    #[tokio::test]
    async fn cancellation_escalates_for_an_interrupt_ignoring_target_and_reaps_it() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("pid");
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "trap '' INT; echo $$ > \"$1\"; exec /bin/sleep 30",
                "fixture",
            ])
            .arg(&marker);
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();
        let task =
            tokio::spawn(async move { run(command, &child_cancel, Duration::from_secs(10)).await });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !marker.is_file() {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let pid: i32 = std::fs::read_to_string(marker)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        cancel.cancel();
        let output = task.await.unwrap().unwrap();
        assert_eq!(output.stopped, Some(DiagnosticCode::Cancelled));
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
}

struct Reader(JoinHandle<std::io::Result<Vec<u8>>>);

impl Drop for Reader {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Group(u32);

impl Group {
    fn kill(&mut self) {
        let id = std::mem::replace(&mut self.0, 0);
        if id != 0 {
            #[cfg(unix)]
            if let Ok(pid) = i32::try_from(id) {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(crate) fn failure(code: DiagnosticCode, reason: &str) -> Error {
    Diagnostic::new(code)
        .with_details(serde_json::json!({"adapter":"lenso-agent","reason":reason}))
        .with_retryable(false)
        .into()
}

pub(crate) async fn run(
    mut command: Command,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<ProcessOutput, Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| failure(DiagnosticCode::ProcessSpawnFailed, "target_start_failed"))?;
    let mut group = Group(child.id().expect("spawned child has a PID"));
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    let mut reader = Reader(tokio::spawn(async move {
        let mut bytes = Vec::new();
        (&mut stdout)
            .take(MAX_JSON as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await?;
        Ok(bytes)
    }));
    let mut errors = Reader(tokio::spawn(async move {
        tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await?;
        Ok(Vec::new())
    }));
    let mut stopped = None;
    let status = tokio::select! {
        biased;
        _ = cancel.cancelled() => { stopped = Some(DiagnosticCode::Cancelled); None }
        _ = tokio::time::sleep(timeout) => { stopped = Some(DiagnosticCode::ProcessTimeout); None }
        result = child.wait() => Some(result?),
    };
    let status = if let Some(status) = status {
        status
    } else {
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(group.0) {
            // Signal the CLI, not its children, so its existing supervisors can clean up.
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGINT,
            );
        }
        match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                group.kill();
                child.kill().await?;
                child.wait().await?
            }
        }
    };
    // Successful CLI exit is not evidence of arbitrary descendant cleanup.
    group.0 = 0;
    let bytes = tokio::time::timeout(Duration::from_secs(1), &mut reader.0)
        .await
        .map_err(|_| failure(DiagnosticCode::OperationFailed, "stdout_drain_timeout"))?
        .map_err(|_| failure(DiagnosticCode::OperationFailed, "stdout_reader_failed"))??;
    tokio::time::timeout(Duration::from_secs(1), &mut errors.0)
        .await
        .map_err(|_| failure(DiagnosticCode::OperationFailed, "stderr_drain_timeout"))?
        .map_err(|_| failure(DiagnosticCode::OperationFailed, "stderr_reader_failed"))??;
    if bytes.len() > MAX_JSON {
        return Err(failure(
            DiagnosticCode::OperationFailed,
            "json_limit_exceeded",
        ));
    }
    Ok(ProcessOutput {
        code: status.code().unwrap_or(1),
        stdout: bytes,
        stopped,
    })
}
