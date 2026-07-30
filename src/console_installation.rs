use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const RELEASE_SCHEMA: &str = "lenso.console-service-release.v1";
const PLAN_SCHEMA: &str = "lenso.console-installation-plan.v1";
const STATE_SCHEMA: &str = "lenso.console-installation-state.v1";
const ATTEMPT_SCHEMA: &str = "lenso.console-installation-attempt.v1";
const LOCK_SCHEMA: &str = "lenso.console-installation-lock.v1";
const DOCTOR_SCHEMA: &str = "lenso.console-doctor.v1";
const TRUSTED_RELEASE_REPOSITORY: &str = "LioRael/lenso-runtime-console";
const TRUSTED_SIGNER_WORKFLOW: &str = "LioRael/lenso-runtime-console/.github/workflows/publish.yml";
const TRUSTED_IMAGE_REPOSITORY: &str = "ghcr.io/liorael/lenso-console";
const STATE_FILE: &str = "installation-state.json";
const ATTEMPT_FILE: &str = "installation-attempt.json";
const LOCK_FILE: &str = "installation.lock";
const MANIFEST_FILE: &str = "release-manifest.json";
const COMPOSE_FILE: &str = "compose.yaml";

#[derive(Debug, Clone)]
pub struct ChangeOptions {
    pub manifest: PathBuf,
    pub root: PathBuf,
    pub env_file: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub apply: bool,
    pub approve_plan_digest: Option<String>,
    pub approve_irreversible: bool,
}

#[derive(Debug, Clone)]
pub struct DoctorOptions {
    pub root: PathBuf,
    pub live_url: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConsoleReleaseManifest {
    schema: String,
    release_id: String,
    version: String,
    source_commit: String,
    image: ConsoleImage,
    composition_digest: String,
    schema_digest: String,
    contract_digest: String,
    configuration_digest: String,
    compatible_from_schema_digests: Vec<String>,
    irreversible_migrations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConsoleImage {
    reference: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallationPlan {
    schema: &'static str,
    action: ChangeAction,
    plan_digest: String,
    installation_root: String,
    release_id: String,
    release_digest: String,
    image_reference: String,
    from_release_id: Option<String>,
    from_schema_digest: Option<String>,
    to_schema_digest: String,
    irreversible_migrations: Vec<String>,
    approval_boundaries: Vec<String>,
    steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChangeAction {
    Install,
    Upgrade,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanStep {
    order: u8,
    workload: &'static str,
    effect: &'static str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationState {
    schema: String,
    release_id: String,
    release_digest: String,
    image_reference: String,
    schema_digest: String,
    composition_digest: String,
    contract_digest: String,
    configuration_digest: String,
    applied_plan_digest: String,
    applied_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AttemptStatus {
    Applying,
    Failed,
    Committed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationAttempt {
    schema: String,
    release_id: String,
    release_digest: String,
    plan_digest: String,
    status: AttemptStatus,
    phase: String,
    started_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum LockStatus {
    Active,
    Released,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationLockRecord {
    schema: String,
    owner_token: String,
    owner_pid: u32,
    status: LockStatus,
    acquired_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug)]
struct InstallationLock {
    file: File,
    path: PathBuf,
    record: InstallationLockRecord,
    released: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorReport {
    schema: &'static str,
    status: DoctorStatus,
    installation_root: String,
    release_id: Option<String>,
    release_digest: Option<String>,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum DoctorStatus {
    Ready,
    Attention,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DoctorCheck {
    id: &'static str,
    status: CheckStatus,
    detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Pass,
    Fail,
}

impl InstallationLock {
    fn acquire(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)
            .with_context(|| format!("create Console installation root {}", root.display()))?;
        let path = root.join(LOCK_FILE);
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && (metadata.file_type().is_symlink() || !metadata.is_file())
        {
            bail!("Console installation lock must be a regular file and not a symbolic link");
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open Console installation lock {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let owner = read_lock_record(&mut file)
                    .map(|record| format!("{} (pid {})", record.owner_token, record.owner_pid))
                    .unwrap_or_else(|_| "unknown owner".to_owned());
                bail!("another Console installation change is active: {owner}");
            }
            Err(TryLockError::Error(error)) => {
                return Err(error).context("acquire Console installation lock");
            }
        }
        let now = unix_time_ms()?;
        let record = InstallationLockRecord {
            schema: LOCK_SCHEMA.to_owned(),
            owner_token: Uuid::now_v7().to_string(),
            owner_pid: std::process::id(),
            status: LockStatus::Active,
            acquired_at_unix_ms: now,
            updated_at_unix_ms: now,
        };
        write_lock_record(&mut file, &record)?;
        Ok(Self {
            file,
            path,
            record,
            released: false,
        })
    }

    fn release(mut self) -> Result<()> {
        self.record.status = LockStatus::Released;
        self.record.updated_at_unix_ms = unix_time_ms()?;
        write_lock_record(&mut self.file, &self.record)?;
        self.file.unlock().with_context(|| {
            format!("release Console installation lock {}", self.path.display())
        })?;
        self.released = true;
        Ok(())
    }
}

impl Drop for InstallationLock {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.record.status = LockStatus::Released;
        if let Ok(now) = unix_time_ms() {
            self.record.updated_at_unix_ms = now;
        }
        let _ = write_lock_record(&mut self.file, &self.record);
        let _ = self.file.unlock();
    }
}

fn read_lock_record(file: &mut File) -> Result<InstallationLockRecord> {
    file.seek(SeekFrom::Start(0))
        .context("seek Console installation lock")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .context("read Console installation lock")?;
    let record: InstallationLockRecord =
        serde_json::from_slice(&bytes).context("decode Console installation lock")?;
    validate_lock_record(&record)?;
    Ok(record)
}

fn write_lock_record(file: &mut File, record: &InstallationLockRecord) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record).context("encode Console installation lock")?;
    file.set_len(0)
        .context("truncate Console installation lock")?;
    file.seek(SeekFrom::Start(0))
        .context("seek Console installation lock")?;
    file.write_all(&bytes)
        .context("write Console installation lock")?;
    file.sync_all().context("sync Console installation lock")
}

fn validate_lock_record(record: &InstallationLockRecord) -> Result<()> {
    if record.schema != LOCK_SCHEMA
        || Uuid::parse_str(&record.owner_token).is_err()
        || record.owner_pid == 0
        || record.acquired_at_unix_ms == 0
        || record.updated_at_unix_ms < record.acquired_at_unix_ms
    {
        bail!("Console installation lock is invalid");
    }
    Ok(())
}

pub fn install(options: ChangeOptions) -> Result<()> {
    change(ChangeAction::Install, options)
}

pub fn upgrade(options: ChangeOptions) -> Result<()> {
    change(ChangeAction::Upgrade, options)
}

fn change(action: ChangeAction, options: ChangeOptions) -> Result<()> {
    let root = absolute_path(&options.root)?;
    validate_installation_root(&root)?;
    let mut installation_lock = options
        .apply
        .then(|| InstallationLock::acquire(&root))
        .transpose()?;
    let manifest_path = absolute_existing_file(&options.manifest, "Console Release Manifest")?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read Console Release Manifest {}", manifest_path.display()))?;
    let manifest: ConsoleReleaseManifest =
        serde_json::from_slice(&manifest_bytes).context("decode Console Release Manifest")?;
    validate_manifest(&manifest)?;
    verify_attestation(&manifest_path)?;
    let release_digest = sha256(&manifest_bytes);
    let current = read_state_optional(&root)?;
    if matches!(action, ChangeAction::Install) {
        validate_action(action, current.as_ref(), &manifest)?;
    }
    if let Some(state) = current.as_ref() {
        validate_installed_evidence(&root, state).context(
            "installed Console evidence is drifted; run `lenso console doctor` before changing releases",
        )?;
        verify_attestation(&root.join(MANIFEST_FILE))
            .context("verify installed Console Release Manifest attestation")?;
    }
    if matches!(action, ChangeAction::Upgrade) {
        validate_action(action, current.as_ref(), &manifest)?;
    }
    let mut plan = build_plan(action, &root, &manifest, &release_digest, current.as_ref())?;
    plan.plan_digest = plan_digest(&plan)?;
    print_or_write_plan(&plan, options.output.as_deref())?;

    if !options.apply {
        eprintln!(
            "Plan only. Re-run with --apply --approve-plan-digest {}.",
            plan.plan_digest
        );
        return Ok(());
    }
    require_approval(&plan, &options)?;
    let env_file = absolute_existing_file(
        options
            .env_file
            .as_deref()
            .context("--env-file is required with --apply")?,
        "Console environment file",
    )?;
    apply_change_with(
        &DockerComposeAdapter,
        &root,
        &env_file,
        &manifest_bytes,
        &manifest,
        &plan,
    )?;
    if let Some(lock) = installation_lock.take() {
        lock.release()?;
    }
    eprintln!(
        "Applied Lenso Console {} {}.",
        match action {
            ChangeAction::Install => "installation",
            ChangeAction::Upgrade => "upgrade",
        },
        manifest.release_id
    );
    Ok(())
}

fn validate_manifest(manifest: &ConsoleReleaseManifest) -> Result<()> {
    if manifest.schema != RELEASE_SCHEMA {
        bail!("unsupported Console Release Manifest schema");
    }
    for (name, value) in [
        ("releaseId", manifest.release_id.as_str()),
        ("version", manifest.version.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("Console Release Manifest {name} is empty");
        }
    }
    if !is_canonical_version(&manifest.version)
        || manifest.release_id != format!("lenso-console@{}", manifest.version)
    {
        bail!("Console Release Manifest releaseId and version are inconsistent");
    }
    if !is_git_commit(&manifest.source_commit) {
        bail!("Console Release Manifest sourceCommit must be a full Git commit");
    }
    for (name, digest) in [
        ("image.digest", manifest.image.digest.as_str()),
        ("compositionDigest", manifest.composition_digest.as_str()),
        ("schemaDigest", manifest.schema_digest.as_str()),
        ("contractDigest", manifest.contract_digest.as_str()),
        (
            "configurationDigest",
            manifest.configuration_digest.as_str(),
        ),
    ] {
        if !is_sha256(digest) {
            bail!("Console Release Manifest {name} must be a canonical SHA-256 digest");
        }
    }
    if manifest.image.reference != format!("{TRUSTED_IMAGE_REPOSITORY}@{}", manifest.image.digest) {
        bail!("Console image reference must pin the reviewed image repository and digest");
    }
    if manifest
        .compatible_from_schema_digests
        .iter()
        .any(|digest| !is_sha256(digest))
    {
        bail!("compatibleFromSchemaDigests contains a non-canonical digest");
    }
    if !all_unique(&manifest.compatible_from_schema_digests) {
        bail!("compatibleFromSchemaDigests contains duplicate digests");
    }
    if manifest
        .irreversible_migrations
        .iter()
        .any(|migration| migration.trim().is_empty())
    {
        bail!("irreversibleMigrations contains an empty migration name");
    }
    if !all_unique(&manifest.irreversible_migrations) {
        bail!("irreversibleMigrations contains duplicate migration names");
    }
    Ok(())
}

fn verify_attestation(manifest: &Path) -> Result<()> {
    let status = Command::new("gh")
        .args(["attestation", "verify"])
        .arg(manifest)
        .args([
            "--repo",
            TRUSTED_RELEASE_REPOSITORY,
            "--signer-workflow",
            TRUSTED_SIGNER_WORKFLOW,
            "--deny-self-hosted-runners",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("run GitHub attestation verification for Console Release Manifest")?;
    if !status.success() {
        bail!(
            "Console Release Manifest attestation verification failed for trusted repository {TRUSTED_RELEASE_REPOSITORY}"
        );
    }
    Ok(())
}

fn validate_action(
    action: ChangeAction,
    current: Option<&InstallationState>,
    manifest: &ConsoleReleaseManifest,
) -> Result<()> {
    match (action, current) {
        (ChangeAction::Install, Some(_)) => {
            bail!("Lenso Console is already installed; use `lenso console upgrade`")
        }
        (ChangeAction::Upgrade, None) => {
            bail!("Lenso Console is not installed; use `lenso console install`")
        }
        (ChangeAction::Upgrade, Some(state)) => {
            let current_version = release_version(&state.release_id)
                .context("installed Console releaseId is invalid")?;
            let target_version = canonical_version(&manifest.version)
                .context("target Console version is invalid")?;
            if target_version <= current_version {
                bail!(
                    "Console upgrades require a newer version than the installed release {}",
                    state.release_id
                );
            }
            if !manifest
                .compatible_from_schema_digests
                .iter()
                .any(|digest| digest == &state.schema_digest)
            {
                bail!(
                    "Console Release does not declare compatibility from installed schema {}",
                    state.schema_digest
                );
            }
        }
        (ChangeAction::Install, None) => {}
    }
    Ok(())
}

fn build_plan(
    action: ChangeAction,
    root: &Path,
    manifest: &ConsoleReleaseManifest,
    release_digest: &str,
    current: Option<&InstallationState>,
) -> Result<InstallationPlan> {
    let mut approval_boundaries = vec!["attested_release_change".to_owned()];
    if !manifest.irreversible_migrations.is_empty() {
        approval_boundaries.push("irreversible_store_migration".to_owned());
    }
    if current.is_some_and(|state| state.composition_digest != manifest.composition_digest) {
        approval_boundaries.push("console_composition_change".to_owned());
    }
    Ok(InstallationPlan {
        schema: PLAN_SCHEMA,
        action,
        plan_digest: String::new(),
        installation_root: root.display().to_string(),
        release_id: manifest.release_id.clone(),
        release_digest: release_digest.to_owned(),
        image_reference: manifest.image.reference.clone(),
        from_release_id: current.map(|state| state.release_id.clone()),
        from_schema_digest: current.map(|state| state.schema_digest.clone()),
        to_schema_digest: manifest.schema_digest.clone(),
        irreversible_migrations: manifest.irreversible_migrations.clone(),
        approval_boundaries,
        steps: vec![
            PlanStep {
                order: 1,
                workload: "release",
                effect: "verify attested manifest and pull exact OCI image",
            },
            PlanStep {
                order: 2,
                workload: "migration",
                effect: "run the release migration workload once",
            },
            PlanStep {
                order: 3,
                workload: "console",
                effect: "replace the Console workload and wait for health",
            },
            PlanStep {
                order: 4,
                workload: "evidence",
                effect: "record the attempt and exact applied release state",
            },
        ],
    })
}

fn plan_digest(plan: &InstallationPlan) -> Result<String> {
    let mut value = serde_json::to_value(plan).context("encode Console installation plan")?;
    value["planDigest"] = Value::String(String::new());
    Ok(sha256(&serde_json::to_vec(&value)?))
}

fn require_approval(plan: &InstallationPlan, options: &ChangeOptions) -> Result<()> {
    if options.approve_plan_digest.as_deref() != Some(plan.plan_digest.as_str()) {
        bail!("--approve-plan-digest must exactly match the current plan digest");
    }
    if !plan.irreversible_migrations.is_empty() && !options.approve_irreversible {
        bail!("--approve-irreversible is required for this Console Release");
    }
    Ok(())
}

trait ComposeAdapter {
    fn run(&self, root: &Path, env_file: &Path, compose_file: &Path, args: &[&str]) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct DockerComposeAdapter;

impl ComposeAdapter for DockerComposeAdapter {
    fn run(&self, root: &Path, env_file: &Path, compose_file: &Path, args: &[&str]) -> Result<()> {
        let status = Command::new("docker")
            .args(["compose", "--project-name", "lenso-console", "--env-file"])
            .arg(env_file)
            .args(["--file"])
            .arg(compose_file)
            .args(args)
            .current_dir(root)
            .status()
            .context("run Docker Compose Console installation adapter")?;
        if !status.success() {
            bail!("Docker Compose Console installation step failed with {status}");
        }
        Ok(())
    }
}

fn apply_change_with(
    adapter: &impl ComposeAdapter,
    root: &Path,
    env_file: &Path,
    manifest_bytes: &[u8],
    manifest: &ConsoleReleaseManifest,
    plan: &InstallationPlan,
) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("create Console installation root {}", root.display()))?;
    let compose = compose_document(&manifest.image.reference);
    let candidate_id = Uuid::now_v7();
    let candidate_compose = root.join(format!(".{COMPOSE_FILE}.candidate-{candidate_id}"));
    let candidate_manifest = root.join(format!(".{MANIFEST_FILE}.candidate-{candidate_id}"));
    let started_at_unix_ms = unix_time_ms()?;
    let mut phase = "staging";
    write_attempt(
        root,
        plan,
        AttemptStatus::Applying,
        phase,
        started_at_unix_ms,
    )?;
    let result = (|| -> Result<()> {
        atomic_write(&candidate_compose, compose.as_bytes())?;
        atomic_write(&candidate_manifest, manifest_bytes)?;

        phase = "pull";
        write_attempt(
            root,
            plan,
            AttemptStatus::Applying,
            phase,
            started_at_unix_ms,
        )?;
        adapter.run(root, env_file, &candidate_compose, &["pull"])?;
        phase = "migration";
        write_attempt(
            root,
            plan,
            AttemptStatus::Applying,
            phase,
            started_at_unix_ms,
        )?;
        adapter.run(
            root,
            env_file,
            &candidate_compose,
            &["run", "--rm", "migrate"],
        )?;
        phase = "readiness";
        write_attempt(
            root,
            plan,
            AttemptStatus::Applying,
            phase,
            started_at_unix_ms,
        )?;
        adapter.run(
            root,
            env_file,
            &candidate_compose,
            &[
                "up",
                "--detach",
                "--wait",
                "--wait-timeout",
                "120",
                "console",
            ],
        )
        .context(
            "candidate Console workload did not become healthy; canonical installation state was not changed",
        )?;

        phase = "commit";
        write_attempt(
            root,
            plan,
            AttemptStatus::Applying,
            phase,
            started_at_unix_ms,
        )?;
        atomic_write(&root.join(COMPOSE_FILE), compose.as_bytes())?;
        atomic_write(&root.join(MANIFEST_FILE), manifest_bytes)?;
        let state = InstallationState {
            schema: STATE_SCHEMA.to_owned(),
            release_id: manifest.release_id.clone(),
            release_digest: plan.release_digest.clone(),
            image_reference: manifest.image.reference.clone(),
            schema_digest: manifest.schema_digest.clone(),
            composition_digest: manifest.composition_digest.clone(),
            contract_digest: manifest.contract_digest.clone(),
            configuration_digest: manifest.configuration_digest.clone(),
            applied_plan_digest: plan.plan_digest.clone(),
            applied_at_unix_ms: unix_time_ms()?,
        };
        atomic_write(
            &root.join(STATE_FILE),
            &serde_json::to_vec_pretty(&state).context("encode Console installation state")?,
        )?;
        write_attempt(
            root,
            plan,
            AttemptStatus::Committed,
            phase,
            started_at_unix_ms,
        )
    })();
    let _ = fs::remove_file(candidate_compose);
    let _ = fs::remove_file(candidate_manifest);
    if result.is_err() {
        let _ = write_attempt(root, plan, AttemptStatus::Failed, phase, started_at_unix_ms);
    }
    result
}

fn write_attempt(
    root: &Path,
    plan: &InstallationPlan,
    status: AttemptStatus,
    phase: &str,
    started_at_unix_ms: u64,
) -> Result<()> {
    let attempt = InstallationAttempt {
        schema: ATTEMPT_SCHEMA.to_owned(),
        release_id: plan.release_id.clone(),
        release_digest: plan.release_digest.clone(),
        plan_digest: plan.plan_digest.clone(),
        status,
        phase: phase.to_owned(),
        started_at_unix_ms,
        updated_at_unix_ms: unix_time_ms()?,
    };
    atomic_write(
        &root.join(ATTEMPT_FILE),
        &serde_json::to_vec_pretty(&attempt).context("encode Console installation attempt")?,
    )
}

fn compose_document(image: &str) -> String {
    format!(
        "name: lenso-console\n\nservices:\n  migrate:\n    image: {image}\n    command: [\"/usr/local/bin/lenso-console-migrate\"]\n    environment: &console-environment\n      APP_ENV: production\n      CORS_ALLOWED_ORIGINS: ${{CONSOLE_PUBLIC_ORIGIN:?set CONSOLE_PUBLIC_ORIGIN}}\n      DATABASE_URL: ${{CONSOLE_DATABASE_URL:?set CONSOLE_DATABASE_URL}}\n      LENSO_COMPOSITION_PROFILE: core\n      SERVICE_NAME: lenso-console\n    read_only: true\n    security_opt:\n      - no-new-privileges:true\n    cap_drop:\n      - ALL\n    tmpfs:\n      - /tmp\n  console:\n    image: {image}\n    environment: *console-environment\n    ports:\n      - \"${{CONSOLE_HTTP_PORT:-3030}}:3030\"\n    read_only: true\n    restart: unless-stopped\n    security_opt:\n      - no-new-privileges:true\n    cap_drop:\n      - ALL\n    tmpfs:\n      - /tmp\n"
    )
}

pub async fn doctor(options: DoctorOptions) -> Result<()> {
    let root = absolute_path(&options.root)?;
    validate_installation_root(&root)?;
    let state = read_state_optional(&root)?;
    let mut checks = Vec::new();
    let mut release_id = None;
    let mut release_digest = None;

    if let Some(state) = state.as_ref() {
        release_id = Some(state.release_id.clone());
        release_digest = Some(state.release_digest.clone());
        checks.push(pass("state", "installation state is present and valid"));
        check_installed_manifest(&root, &state, &mut checks);
        check_compose(&root, &state, &mut checks);
    } else {
        checks.push(fail("state", "installation state is missing"));
    }
    check_installation_attempt(&root, state.as_ref(), &mut checks);
    check_installation_lock(&root, &mut checks);
    if let Some(url) = options.live_url.as_deref() {
        checks.push(live_check(url).await);
    }
    let status = if checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Fail))
    {
        DoctorStatus::Attention
    } else {
        DoctorStatus::Ready
    };
    let attention = matches!(status, DoctorStatus::Attention);
    let report = DoctorReport {
        schema: DOCTOR_SCHEMA,
        status,
        installation_root: root.display().to_string(),
        release_id,
        release_digest,
        checks,
    };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Lenso Console doctor: {}",
            if attention { "attention" } else { "ready" }
        );
        for check in &report.checks {
            println!(
                "- {}: {} ({})",
                check.id,
                if matches!(check.status, CheckStatus::Pass) {
                    "pass"
                } else {
                    "fail"
                },
                check.detail
            );
        }
    }
    if attention {
        bail!("Lenso Console doctor found installation issues");
    }
    Ok(())
}

fn validate_installed_evidence(root: &Path, state: &InstallationState) -> Result<()> {
    validate_installed_manifest(root, state)?;
    validate_installed_compose(root, state)
}

fn validate_installed_manifest(root: &Path, state: &InstallationState) -> Result<()> {
    let path = absolute_existing_file(
        &root.join(MANIFEST_FILE),
        "installed Console Release Manifest",
    )?;
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let manifest: ConsoleReleaseManifest =
        serde_json::from_slice(&bytes).context("decode installed Console Release Manifest")?;
    validate_manifest(&manifest).context("validate installed Console Release Manifest")?;
    if sha256(&bytes) != state.release_digest
        || manifest.release_id != state.release_id
        || manifest.schema_digest != state.schema_digest
        || manifest.image.reference != state.image_reference
        || manifest.composition_digest != state.composition_digest
        || manifest.contract_digest != state.contract_digest
        || manifest.configuration_digest != state.configuration_digest
    {
        bail!("installed Console Release Manifest does not match applied state");
    }
    Ok(())
}

fn validate_installed_compose(root: &Path, state: &InstallationState) -> Result<()> {
    let path = absolute_existing_file(&root.join(COMPOSE_FILE), "installed Console deployment")?;
    let compose = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if compose != compose_document(&state.image_reference) {
        bail!("installed Console deployment does not match applied state");
    }
    Ok(())
}

fn check_installed_manifest(root: &Path, state: &InstallationState, checks: &mut Vec<DoctorCheck>) {
    match validate_installed_manifest(root, state) {
        Ok(()) => checks.push(pass(
            "release_manifest",
            "installed manifest matches applied state",
        )),
        Err(_) => checks.push(fail(
            "release_manifest",
            "installed manifest is missing, invalid, or drifted",
        )),
    }
}

fn check_compose(root: &Path, state: &InstallationState, checks: &mut Vec<DoctorCheck>) {
    match validate_installed_compose(root, state) {
        Ok(()) => checks.push(pass(
            "deployment",
            "Compose deployment exactly matches applied state",
        )),
        Err(_) => checks.push(fail(
            "deployment",
            "Compose deployment is missing, invalid, or drifted",
        )),
    }
}

fn check_installation_attempt(
    root: &Path,
    state: Option<&InstallationState>,
    checks: &mut Vec<DoctorCheck>,
) {
    match read_attempt_optional(root) {
        Ok(Some(attempt))
            if attempt.status == AttemptStatus::Committed
                && state.is_some_and(|state| {
                    attempt.release_id == state.release_id
                        && attempt.release_digest == state.release_digest
                        && attempt.plan_digest == state.applied_plan_digest
                }) =>
        {
            checks.push(pass(
                "last_change",
                "last installation change is committed and matches applied state",
            ));
        }
        Ok(Some(attempt)) => checks.push(fail(
            "last_change",
            &format!(
                "last installation change for {} is {:?} at phase {}",
                attempt.release_id, attempt.status, attempt.phase
            )
            .to_lowercase(),
        )),
        Ok(None) => checks.push(pass(
            "last_change",
            "no interrupted installation change is recorded",
        )),
        Err(_) => checks.push(fail(
            "last_change",
            "installation attempt evidence is invalid",
        )),
    }
}

fn check_installation_lock(root: &Path, checks: &mut Vec<DoctorCheck>) {
    let result = (|| -> Result<Option<(InstallationLockRecord, bool)>> {
        let path = root.join(LOCK_FILE);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("inspect Console installation lock {}", path.display())
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("Console installation lock is not a regular file");
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open Console installation lock {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => {
                let record = read_lock_record(&mut file)?;
                file.unlock().context("release Console doctor lock probe")?;
                Ok(Some((record, false)))
            }
            Err(TryLockError::WouldBlock) => Ok(Some((read_lock_record(&mut file)?, true))),
            Err(TryLockError::Error(error)) => {
                Err(error).context("probe Console installation lock")
            }
        }
    })();
    match result {
        Ok(None) => checks.push(pass(
            "change_lock",
            "no Console installation change lock exists",
        )),
        Ok(Some((record, false))) if record.status == LockStatus::Released => checks.push(pass(
            "change_lock",
            "Console installation change lock is released",
        )),
        Ok(Some((record, false))) => checks.push(fail(
            "change_lock",
            &format!(
                "stale Console installation lock from pid {} is recoverable on the next apply",
                record.owner_pid
            ),
        )),
        Ok(Some((record, true))) => checks.push(fail(
            "change_lock",
            &format!(
                "Console installation change is active in pid {}",
                record.owner_pid
            ),
        )),
        Err(_) => checks.push(fail(
            "change_lock",
            "Console installation change lock is invalid",
        )),
    }
}

async fn live_check(value: &str) -> DoctorCheck {
    let result = async {
        let url = secure_live_url(value)?;
        let endpoint = url.join("/health/ready")?;
        let response = Client::builder()
            .redirect(Policy::none())
            .build()?
            .get(endpoint)
            .send()
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            bail!("readiness returned HTTP {}", response.status())
        }
    }
    .await;
    match result {
        Ok(()) => pass("readiness", "Console readiness endpoint is healthy"),
        Err(error) => fail("readiness", &error.to_string()),
    }
}

fn secure_live_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("parse --live-url")?;
    let host = url.host_str().unwrap_or_default();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("--live-url must use HTTPS unless it targets loopback");
    }
    Ok(url)
}

fn read_state_optional(root: &Path) -> Result<Option<InstallationState>> {
    let path = root.join(STATE_FILE);
    let Some(bytes) = read_optional_regular_file(&path, "Console installation state")? else {
        return Ok(None);
    };
    let state: InstallationState =
        serde_json::from_slice(&bytes).context("decode Console installation state")?;
    if state.schema != STATE_SCHEMA
        || release_version(&state.release_id).is_none()
        || !is_sha256(&state.release_digest)
        || !is_sha256(&state.schema_digest)
        || !is_sha256(&state.composition_digest)
        || !is_sha256(&state.contract_digest)
        || !is_sha256(&state.configuration_digest)
        || !trusted_image_reference(&state.image_reference)
        || !is_sha256(&state.applied_plan_digest)
    {
        bail!("Console installation state is invalid");
    }
    Ok(Some(state))
}

fn read_attempt_optional(root: &Path) -> Result<Option<InstallationAttempt>> {
    let path = root.join(ATTEMPT_FILE);
    let Some(bytes) = read_optional_regular_file(&path, "Console installation attempt")? else {
        return Ok(None);
    };
    let attempt: InstallationAttempt =
        serde_json::from_slice(&bytes).context("decode Console installation attempt")?;
    if attempt.schema != ATTEMPT_SCHEMA
        || release_version(&attempt.release_id).is_none()
        || !is_sha256(&attempt.release_digest)
        || !is_sha256(&attempt.plan_digest)
        || !matches!(
            attempt.phase.as_str(),
            "staging" | "pull" | "migration" | "readiness" | "commit"
        )
        || attempt.started_at_unix_ms == 0
        || attempt.updated_at_unix_ms < attempt.started_at_unix_ms
    {
        bail!("Console installation attempt is invalid");
    }
    Ok(Some(attempt))
}

fn read_optional_regular_file(path: &Path, label: &str) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{label} must be a regular file and not a symbolic link");
    }
    fs::read(path)
        .with_context(|| format!("read {}", path.display()))
        .map(Some)
}

fn print_or_write_plan(plan: &InstallationPlan, output: Option<&Path>) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(plan).context("encode Console installation plan")?;
    if let Some(path) = output {
        atomic_write(path, &bytes)?;
        eprintln!("Wrote Console installation plan to {}.", path.display());
    } else {
        println!(
            "{}",
            String::from_utf8(bytes).context("render Console installation plan")?
        );
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("output path has no valid file name")?;
    let temp = path.with_file_name(format!(".{name}.tmp-{}", Uuid::now_v7()));
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("create {}", temp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {}", temp.display()))?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error).with_context(|| format!("replace {}", path.display()));
    }
    Ok(())
}

fn validate_installation_root(root: &Path) -> Result<()> {
    match fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("Console installation root must not be a symbolic link")
        }
        Ok(metadata) if !metadata.is_dir() => {
            bail!("Console installation root must be a directory")
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", root.display())),
    }
}

fn absolute_existing_file(path: &Path, label: &str) -> Result<PathBuf> {
    let path = absolute_path(path)?;
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{label} must be a regular file and not a symbolic link");
    }
    Ok(path)
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()
        .context("resolve current directory")?
        .join(path))
}

fn unix_time_ms() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_millis(),
    )
    .context("system clock exceeds supported range")
}

fn sha256(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn trusted_image_reference(value: &str) -> bool {
    value
        .strip_prefix(&format!("{TRUSTED_IMAGE_REPOSITORY}@"))
        .is_some_and(is_sha256)
}

fn is_canonical_version(value: &str) -> bool {
    canonical_version(value).is_some()
}

fn canonical_version(value: &str) -> Option<(u64, u64, u64)> {
    fn component(value: &str) -> Option<u64> {
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return None;
        }
        value.parse().ok()
    }

    let mut parts = value.split('.');
    let version = (
        component(parts.next()?)?,
        component(parts.next()?)?,
        component(parts.next()?)?,
    );
    parts.next().is_none().then_some(version)
}

fn release_version(release_id: &str) -> Option<(u64, u64, u64)> {
    canonical_version(release_id.strip_prefix("lenso-console@")?)
}

fn all_unique(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn is_git_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn pass(id: &'static str, detail: &str) -> DoctorCheck {
    DoctorCheck {
        id,
        status: CheckStatus::Pass,
        detail: detail.to_owned(),
    }
}

fn fail(id: &'static str, detail: &str) -> DoctorCheck {
    DoctorCheck {
        id,
        status: CheckStatus::Fail,
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingComposeAdapter {
        calls: RefCell<Vec<Vec<String>>>,
        fail_on: Option<usize>,
    }

    impl ComposeAdapter for RecordingComposeAdapter {
        fn run(
            &self,
            _root: &Path,
            _env_file: &Path,
            compose_file: &Path,
            args: &[&str],
        ) -> Result<()> {
            if !compose_file.is_file() {
                bail!("candidate Compose file is missing");
            }
            let mut calls = self.calls.borrow_mut();
            calls.push(args.iter().map(|value| (*value).to_owned()).collect());
            if self.fail_on == Some(calls.len()) {
                bail!("simulated Compose failure");
            }
            Ok(())
        }
    }

    fn digest(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn manifest() -> ConsoleReleaseManifest {
        let image_digest = digest('a');
        ConsoleReleaseManifest {
            schema: RELEASE_SCHEMA.to_owned(),
            release_id: "lenso-console@0.2.0".to_owned(),
            version: "0.2.0".to_owned(),
            source_commit: "b".repeat(40),
            image: ConsoleImage {
                reference: format!("ghcr.io/liorael/lenso-console@{image_digest}"),
                digest: image_digest,
            },
            composition_digest: digest('c'),
            schema_digest: digest('d'),
            contract_digest: digest('e'),
            configuration_digest: digest('f'),
            compatible_from_schema_digests: vec![digest('1')],
            irreversible_migrations: Vec::new(),
        }
    }

    fn state() -> InstallationState {
        InstallationState {
            schema: STATE_SCHEMA.to_owned(),
            release_id: "lenso-console@0.1.0".to_owned(),
            release_digest: digest('0'),
            image_reference: format!("ghcr.io/liorael/lenso-console@{}", digest('9')),
            schema_digest: digest('1'),
            composition_digest: digest('2'),
            contract_digest: digest('3'),
            configuration_digest: digest('4'),
            applied_plan_digest: digest('5'),
            applied_at_unix_ms: 1,
        }
    }

    fn state_for_manifest(
        release: &ConsoleReleaseManifest,
        manifest_bytes: &[u8],
    ) -> InstallationState {
        InstallationState {
            schema: STATE_SCHEMA.to_owned(),
            release_id: release.release_id.clone(),
            release_digest: sha256(manifest_bytes),
            image_reference: release.image.reference.clone(),
            schema_digest: release.schema_digest.clone(),
            composition_digest: release.composition_digest.clone(),
            contract_digest: release.contract_digest.clone(),
            configuration_digest: release.configuration_digest.clone(),
            applied_plan_digest: digest('5'),
            applied_at_unix_ms: 1,
        }
    }

    fn installation_plan(root: &Path, release: &ConsoleReleaseManifest) -> InstallationPlan {
        let manifest_bytes = serde_json::to_vec_pretty(release).unwrap();
        let mut plan = build_plan(
            ChangeAction::Install,
            root,
            release,
            &sha256(&manifest_bytes),
            None,
        )
        .unwrap();
        plan.plan_digest = plan_digest(&plan).unwrap();
        plan
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lenso-console-installation-{label}-{}",
            Uuid::now_v7()
        ))
    }

    fn candidate_files(root: &Path) -> Vec<PathBuf> {
        fs::read_dir(root)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".candidate-"))
            })
            .collect()
    }

    #[test]
    fn manifest_requires_exact_oci_digest_and_contract_digests() {
        assert!(validate_manifest(&manifest()).is_ok());
        let mut invalid = manifest();
        invalid.image.reference = "ghcr.io/liorael/lenso-console:latest".to_owned();
        assert!(validate_manifest(&invalid).is_err());
        invalid = manifest();
        invalid.contract_digest = "sha256:ABC".to_owned();
        assert!(validate_manifest(&invalid).is_err());
        invalid = manifest();
        invalid.image.reference = format!("ghcr.io/attacker/console@{}", invalid.image.digest);
        assert!(validate_manifest(&invalid).is_err());
        invalid = manifest();
        invalid.version = "01.2.0".to_owned();
        invalid.release_id = "lenso-console@01.2.0".to_owned();
        assert!(validate_manifest(&invalid).is_err());
        invalid = manifest();
        invalid.release_id = "lenso-console@0.2.1".to_owned();
        assert!(validate_manifest(&invalid).is_err());
    }

    #[test]
    fn release_authority_pins_repository_and_signer_workflow() {
        assert_eq!(TRUSTED_RELEASE_REPOSITORY, "LioRael/lenso-runtime-console");
        assert_eq!(
            TRUSTED_SIGNER_WORKFLOW,
            "LioRael/lenso-runtime-console/.github/workflows/publish.yml"
        );
        assert_eq!(TRUSTED_IMAGE_REPOSITORY, "ghcr.io/liorael/lenso-console");
    }

    #[test]
    fn installation_state_requires_canonical_release_and_image_authority() {
        let root = test_root("state-authority");
        fs::create_dir_all(&root).unwrap();
        let mut invalid = state();
        invalid.image_reference = format!("ghcr.io/attacker/console@{}", digest('9'));
        fs::write(
            root.join(STATE_FILE),
            serde_json::to_vec_pretty(&invalid).unwrap(),
        )
        .unwrap();
        assert!(read_state_optional(&root).is_err());

        invalid = state();
        invalid.release_id = "lenso-console@01.0.0".to_owned();
        fs::write(
            root.join(STATE_FILE),
            serde_json::to_vec_pretty(&invalid).unwrap(),
        )
        .unwrap();
        assert!(read_state_optional(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upgrade_requires_explicit_schema_compatibility() {
        let current = state();
        assert!(validate_action(ChangeAction::Upgrade, Some(&current), &manifest()).is_ok());
        let mut incompatible = manifest();
        incompatible.compatible_from_schema_digests.clear();
        assert!(validate_action(ChangeAction::Upgrade, Some(&current), &incompatible).is_err());
    }

    #[test]
    fn upgrade_requires_a_strictly_newer_release_version() {
        let mut current = state();
        current.release_id = "lenso-console@0.2.0".to_owned();
        assert!(validate_action(ChangeAction::Upgrade, Some(&current), &manifest()).is_err());

        current.release_id = "lenso-console@0.3.0".to_owned();
        assert!(validate_action(ChangeAction::Upgrade, Some(&current), &manifest()).is_err());

        current.release_id = "lenso-console@0.1.9".to_owned();
        assert!(validate_action(ChangeAction::Upgrade, Some(&current), &manifest()).is_ok());
    }

    #[test]
    fn installed_evidence_must_exactly_match_the_applied_state() {
        let root = test_root("installed-evidence");
        fs::create_dir_all(&root).unwrap();
        let release = manifest();
        let manifest_bytes = serde_json::to_vec_pretty(&release).unwrap();
        let installed = state_for_manifest(&release, &manifest_bytes);
        fs::write(root.join(MANIFEST_FILE), &manifest_bytes).unwrap();
        fs::write(
            root.join(COMPOSE_FILE),
            compose_document(&release.image.reference),
        )
        .unwrap();
        assert!(validate_installed_evidence(&root, &installed).is_ok());

        fs::write(
            root.join(COMPOSE_FILE),
            format!(
                "{}# local drift\n",
                compose_document(&release.image.reference)
            ),
        )
        .unwrap();
        assert!(validate_installed_evidence(&root, &installed).is_err());

        fs::write(
            root.join(COMPOSE_FILE),
            compose_document(&release.image.reference),
        )
        .unwrap();
        let mut drifted_state = installed;
        drifted_state.contract_digest = digest('9');
        assert!(validate_installed_evidence(&root, &drifted_state).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installation_lock_blocks_concurrent_changes_and_releases_cleanly() {
        let root = test_root("active-lock");
        let lock = InstallationLock::acquire(&root).unwrap();
        assert!(InstallationLock::acquire(&root).is_err());
        let mut checks = Vec::new();
        check_installation_lock(&root, &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Fail));
        assert!(checks[0].detail.contains("active"));

        lock.release().unwrap();
        checks.clear();
        check_installation_lock(&root, &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Pass));
        InstallationLock::acquire(&root).unwrap().release().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_installation_lock_is_reported_and_recovered_by_the_next_owner() {
        let root = test_root("stale-lock");
        let lock = InstallationLock::acquire(&root).unwrap();
        let mut stale = lock.record.clone();
        lock.release().unwrap();
        stale.status = LockStatus::Active;
        stale.updated_at_unix_ms = unix_time_ms().unwrap();
        fs::write(
            root.join(LOCK_FILE),
            serde_json::to_vec_pretty(&stale).unwrap(),
        )
        .unwrap();

        let mut checks = Vec::new();
        check_installation_lock(&root, &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Fail));
        assert!(checks[0].detail.contains("recoverable"));

        InstallationLock::acquire(&root).unwrap().release().unwrap();
        checks.clear();
        check_installation_lock(&root, &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Pass));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_digest_changes_with_target_or_release() {
        let current = state();
        let mut first = build_plan(
            ChangeAction::Upgrade,
            Path::new("/srv/lenso-console"),
            &manifest(),
            &digest('6'),
            Some(&current),
        )
        .unwrap();
        first.plan_digest = plan_digest(&first).unwrap();
        let mut changed = manifest();
        changed.release_id = "lenso-console@0.2.1".to_owned();
        let second = build_plan(
            ChangeAction::Upgrade,
            Path::new("/srv/lenso-console"),
            &changed,
            &digest('7'),
            Some(&current),
        )
        .unwrap();
        assert_ne!(first.plan_digest, plan_digest(&second).unwrap());
    }

    #[test]
    fn irreversible_release_adds_a_separate_approval_boundary() {
        let mut release = manifest();
        release.irreversible_migrations = vec!["20260730_drop_legacy_console".to_owned()];
        let mut plan = build_plan(
            ChangeAction::Upgrade,
            Path::new("/srv/lenso-console"),
            &release,
            &digest('6'),
            Some(&state()),
        )
        .unwrap();
        plan.plan_digest = plan_digest(&plan).unwrap();
        let options = ChangeOptions {
            manifest: PathBuf::new(),
            root: PathBuf::new(),
            env_file: None,
            output: None,
            apply: true,
            approve_plan_digest: Some(plan.plan_digest.clone()),
            approve_irreversible: false,
        };
        assert!(
            plan.approval_boundaries
                .contains(&"irreversible_store_migration".to_owned())
        );
        assert!(require_approval(&plan, &options).is_err());
    }

    #[test]
    fn compose_uses_only_the_exact_release_image() {
        let document = compose_document(&manifest().image.reference);
        assert_eq!(document.matches("@sha256:").count(), 2);
        assert!(!document.contains(":latest"));
        assert!(document.contains("lenso-console-migrate"));
        assert!(document.contains("read_only: true"));
    }

    #[test]
    fn apply_waits_for_health_before_committing_installation_state() {
        let root = test_root("healthy");
        fs::create_dir_all(&root).unwrap();
        let env_file = root.join("console.env");
        fs::write(&env_file, "CONSOLE_HTTP_PORT=3030\n").unwrap();
        let release = manifest();
        let manifest_bytes = serde_json::to_vec_pretty(&release).unwrap();
        let plan = installation_plan(&root, &release);
        let adapter = RecordingComposeAdapter::default();

        apply_change_with(&adapter, &root, &env_file, &manifest_bytes, &release, &plan).unwrap();

        assert_eq!(
            adapter.calls.into_inner(),
            vec![
                vec!["pull"],
                vec!["run", "--rm", "migrate"],
                vec![
                    "up",
                    "--detach",
                    "--wait",
                    "--wait-timeout",
                    "120",
                    "console",
                ],
            ]
        );
        assert_eq!(fs::read(root.join(MANIFEST_FILE)).unwrap(), manifest_bytes);
        assert!(root.join(COMPOSE_FILE).is_file());
        let installed = read_state_optional(&root).unwrap().unwrap();
        assert!(validate_installed_evidence(&root, &installed).is_ok());
        let attempt = read_attempt_optional(&root).unwrap().unwrap();
        assert_eq!(attempt.status, AttemptStatus::Committed);
        assert_eq!(attempt.phase, "commit");
        assert_eq!(attempt.release_id, installed.release_id);
        assert_eq!(attempt.plan_digest, installed.applied_plan_digest);
        let mut checks = Vec::new();
        check_installation_attempt(&root, Some(&installed), &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Pass));
        assert!(candidate_files(&root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_health_wait_preserves_the_previous_installation() {
        let root = test_root("unhealthy");
        fs::create_dir_all(&root).unwrap();
        let env_file = root.join("console.env");
        fs::write(&env_file, "CONSOLE_HTTP_PORT=3030\n").unwrap();
        let old_compose = b"previous compose\n";
        let old_manifest = b"previous manifest\n";
        let old_state = serde_json::to_vec_pretty(&state()).unwrap();
        fs::write(root.join(COMPOSE_FILE), old_compose).unwrap();
        fs::write(root.join(MANIFEST_FILE), old_manifest).unwrap();
        fs::write(root.join(STATE_FILE), &old_state).unwrap();
        let release = manifest();
        let manifest_bytes = serde_json::to_vec_pretty(&release).unwrap();
        let plan = installation_plan(&root, &release);
        let adapter = RecordingComposeAdapter {
            calls: RefCell::default(),
            fail_on: Some(3),
        };

        assert!(
            apply_change_with(&adapter, &root, &env_file, &manifest_bytes, &release, &plan,)
                .is_err()
        );

        assert_eq!(fs::read(root.join(COMPOSE_FILE)).unwrap(), old_compose);
        assert_eq!(fs::read(root.join(MANIFEST_FILE)).unwrap(), old_manifest);
        assert_eq!(fs::read(root.join(STATE_FILE)).unwrap(), old_state);
        let attempt = read_attempt_optional(&root).unwrap().unwrap();
        assert_eq!(attempt.status, AttemptStatus::Failed);
        assert_eq!(attempt.phase, "readiness");
        let mut checks = Vec::new();
        check_installation_attempt(&root, Some(&state()), &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Fail));
        assert!(candidate_files(&root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
