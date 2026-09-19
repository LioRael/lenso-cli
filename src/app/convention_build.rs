//! Execute only selected convention compilers. Discovery remains side-effect free.
use anyhow::{Context, bail};
use lenso_app_authoring::discovery::{
    Candidate,
    conventions::{ConventionPlan, generated_candidate},
};
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

pub(super) fn compile(plan: &ConventionPlan, output: &Path) -> anyhow::Result<Vec<Candidate>> {
    let mut candidates = Vec::new();
    for compilation in &plan.compilations {
        let project = output.join(&compilation.plugin_id);
        fs::create_dir(&project)?;
        let request = serde_json::json!({
            "schema":"lenso.convention-compile.v1", "entry":compilation.entry,
            "owner_project":compilation.owner_project, "plugin_id":compilation.plugin_id,
            "release_version":compilation.version, "convention":compilation.convention,
            "output": project,
        });
        let stdout = tempfile::tempfile()?;
        let stderr = tempfile::tempfile()?;
        let source_before = super::local_host::input_digest(&compilation.owner_project)?;
        let compiler_before = super::local_host::input_digest(&compilation.compiler_project)?;
        let input = serde_json::to_vec(&request)?;
        if input.len() > 16384 {
            bail!("compiler request exceeds 16 KiB");
        }
        let mut stdin = tempfile::tempfile()?;
        stdin.write_all(&input)?;
        std::io::Seek::rewind(&mut stdin)?;
        let mut command = Command::new(&compilation.compiler.program);
        command
            .args(&compilation.compiler.args)
            .current_dir(&compilation.compiler_project)
            .stdin(stdin)
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("start convention compiler {}", compilation.convention))?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline
                || stdout.metadata()?.len() > 1024 * 1024
                || stderr.metadata()?.len() > 1024 * 1024
            {
                #[cfg(unix)]
                {
                    use nix::{
                        sys::signal::{Signal, killpg},
                        unistd::Pid,
                    };
                    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                bail!("convention compiler exceeded its 60 second / 1 MiB output budget");
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        if stdout.metadata()?.len() > 1024 * 1024 || stderr.metadata()?.len() > 1024 * 1024 {
            bail!("compiler output exceeds 1 MiB");
        }
        use std::io::{Read, Seek};
        let mut stderr = stderr;
        stderr.rewind()?;
        let mut diagnostic = String::new();
        stderr.take(1024 * 1024).read_to_string(&mut diagnostic)?;
        if !status.success() {
            bail!(
                "convention compiler {} failed: {diagnostic}",
                compilation.convention
            );
        }
        let mut stdout = stdout;
        stdout.rewind()?;
        let mut response = Vec::new();
        stdout.take(1024 * 1024 + 1).read_to_end(&mut response)?;
        if response.len() > 1024 * 1024 {
            bail!("compiler response exceeds 1 MiB");
        }
        let response: serde_json::Value =
            serde_json::from_slice(&response).context("compiler must return one JSON response")?;
        if response != serde_json::json!({"schema":"lenso.convention-compiled.v1"}) {
            bail!("unsupported compiler response");
        }
        validate_tree(&project, &mut 0, &mut 0, 0)?;
        if source_before != super::local_host::input_digest(&compilation.owner_project)?
            || compiler_before != super::local_host::input_digest(&compilation.compiler_project)?
        {
            bail!("convention inputs changed during compilation; retry");
        }
        let candidate = generated_candidate(&project, compilation)?;
        if candidate.format == "bun" {
            let status = Command::new("bun")
                .args(["install", "--ignore-scripts"])
                .current_dir(&project)
                .status()?;
            if !status.success() {
                bail!("install selected convention output dependencies");
            }
        }
        if candidate.format == "cargo"
            && !Command::new("cargo")
                .arg("generate-lockfile")
                .current_dir(&project)
                .status()?
                .success()
        {
            bail!("resolve selected Rust convention output dependencies");
        }
        candidates.push(candidate);
    }
    Ok(candidates)
}

fn validate_tree(
    path: &Path,
    files: &mut usize,
    bytes: &mut u64,
    depth: usize,
) -> anyhow::Result<()> {
    if depth > 32 {
        bail!("compiler output exceeds 32 directory levels");
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.file_type()?;
        *files += 1;
        if meta.is_dir() {
            validate_tree(&entry.path(), files, bytes, depth + 1)?;
        } else if meta.is_file() {
            *bytes += entry.metadata()?.len();
        } else {
            bail!("compiler output cannot contain symlinks or special files");
        }
        if *files > 4096 || *bytes > 16 * 1024 * 1024 {
            bail!("compiler output exceeds 4096 entries / 16 MiB");
        }
    }
    Ok(())
}
