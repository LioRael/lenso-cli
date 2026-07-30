use std::collections::{BTreeMap, BTreeSet};
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
const RECOVERY_SET_SCHEMA: &str = "lenso.console-recovery-set.v1";
const RESTORE_PLAN_SCHEMA: &str = "lenso.console-restore-plan.v1";
const RECOVERY_STATE_SCHEMA: &str = "lenso.console-recovery-state.v1";
const RECONCILIATION_INPUT_SCHEMA: &str = "lenso.console-reconciliation-input.v1";
const RECONCILIATION_PLAN_SCHEMA: &str = "lenso.console-reconciliation-plan.v1";
const RECONCILIATION_EVIDENCE_SCHEMA: &str = "lenso.console-reconciliation-evidence.v1";
const DOCTOR_SCHEMA: &str = "lenso.console-doctor.v1";
const TRUSTED_RELEASE_REPOSITORY: &str = "LioRael/lenso-runtime-console";
const TRUSTED_SIGNER_WORKFLOW: &str = "LioRael/lenso-runtime-console/.github/workflows/publish.yml";
const TRUSTED_IMAGE_REPOSITORY: &str = "ghcr.io/liorael/lenso-console";
const STATE_FILE: &str = "installation-state.json";
const ATTEMPT_FILE: &str = "installation-attempt.json";
const LOCK_FILE: &str = "installation.lock";
const MANIFEST_FILE: &str = "release-manifest.json";
const COMPOSE_FILE: &str = "compose.yaml";
const RECOVERY_MANIFEST_FILE: &str = "recovery-set.json";
const RECOVERY_STORE_FILE: &str = "store.dump.age";
const RECOVERY_STATE_FILE: &str = "recovery-state.json";
const RECONCILIATION_EVIDENCE_FILE: &str = "reconciliation-evidence.json";
const RECOVERY_PG_DUMP_ARGS: &[&str] = &[
    "--format=custom",
    "--no-owner",
    "--no-privileges",
    "--exclude-table-data=auth.sessions",
];

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

#[derive(Debug, Clone)]
pub struct BackupOptions {
    pub root: PathBuf,
    pub env_file: PathBuf,
    pub output: PathBuf,
    pub recipient: String,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct RestoreOptions {
    pub root: PathBuf,
    pub recovery_set: PathBuf,
    pub current_env_file: PathBuf,
    pub recovery_env_file: PathBuf,
    pub output: Option<PathBuf>,
    pub apply: bool,
    pub approve_plan_digest: Option<String>,
    pub identity_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ReconcileOptions {
    pub root: PathBuf,
    pub env_file: PathBuf,
    pub evidence: PathBuf,
    pub output: Option<PathBuf>,
    pub apply: bool,
    pub approve_plan_digest: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoverySetManifest {
    schema: String,
    recovery_set_id: String,
    recovery_set_digest: String,
    created_at_unix_ms: u64,
    release_id: String,
    release_digest: String,
    image_reference: String,
    schema_digest: String,
    composition_digest: String,
    contract_digest: String,
    configuration_digest: String,
    store: RecoveryStore,
    secret_references: Vec<String>,
    restore_preconditions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryStore {
    format: String,
    encrypted: bool,
    encryption: String,
    payload: String,
    payload_digest: String,
    recipient: String,
    excluded_data: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestorePlan {
    schema: &'static str,
    action: &'static str,
    plan_digest: String,
    installation_root: String,
    recovery_set_path: String,
    recovery_set_id: String,
    recovery_set_digest: String,
    release_id: String,
    release_digest: String,
    payload_digest: String,
    target_store_identity_digest: String,
    steps: Vec<String>,
    approval_boundaries: Vec<String>,
    target_requirements: Vec<String>,
    next_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RecoveryStatus {
    Applying,
    Failed,
    AwaitingReconciliation,
    ReadyForActivation,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryState {
    schema: String,
    recovery_set_id: String,
    recovery_set_digest: String,
    plan_digest: String,
    release_id: String,
    target_store_identity_digest: String,
    status: RecoveryStatus,
    phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reconciliation_evidence_digest: Option<String>,
    started_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconciliationInput {
    schema: String,
    recovery_set_id: String,
    observed_at_unix_ms: u64,
    reviewed_by: String,
    authority_evidence_ref: String,
    identity_evidence_ref: String,
    outbox_evidence_ref: String,
    single_authoritative_deployment: bool,
    identity_and_enrollment_continuity_verified: bool,
    outbox_reconciled: bool,
    managed_services: Vec<ReconciledManagedService>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconciledManagedService {
    service_id: String,
    service_principal: String,
    enrollment_receipt_digest: String,
    authorization_epoch: u64,
    core_document_digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoreRecoveryObservation {
    session_count: u64,
    stale_session_count: u64,
    outbox_status_counts: BTreeMap<String, u64>,
    outbox_snapshot_digest: String,
    managed_services: Vec<ObservedManagedService>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObservedManagedService {
    service_id: String,
    service_principal: String,
    enrollment_receipt_digest: String,
    enrollment_grant_revision: u64,
    authorization_epoch: u64,
    enrollment_state: String,
    version: u64,
    core_document_digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawStoreRecoveryObservation {
    session_count: u64,
    stale_session_count: u64,
    outbox_status_counts: BTreeMap<String, u64>,
    managed_services: Vec<RawObservedManagedService>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawObservedManagedService {
    service_id: String,
    service_principal: String,
    enrollment_receipt_digest: String,
    enrollment_grant_revision: u64,
    authorization_epoch: u64,
    enrollment_state: String,
    version: u64,
    core_document: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReconciliationPlan {
    schema: &'static str,
    action: &'static str,
    plan_digest: String,
    recovery_set_id: String,
    recovery_set_digest: String,
    restore_plan_digest: String,
    release_id: String,
    target_store_identity_digest: String,
    store_observation_digest: String,
    reconciliation_input_digest: String,
    observed_at_unix_ms: u64,
    steps: Vec<String>,
    approval_boundaries: Vec<String>,
    next_actions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconciliationEvidence {
    schema: String,
    evidence_id: String,
    evidence_digest: String,
    recovery_set_id: String,
    recovery_set_digest: String,
    restore_plan_digest: String,
    reconciliation_plan_digest: String,
    release_id: String,
    target_store_identity_digest: String,
    store_observation_digest: String,
    reconciliation_input_digest: String,
    observed_at_unix_ms: u64,
    reviewed_by: String,
    authority_evidence_ref: String,
    identity_evidence_ref: String,
    outbox_evidence_ref: String,
    single_authoritative_deployment: bool,
    identity_and_enrollment_continuity_verified: bool,
    outbox_reconciled: bool,
    reconciled_managed_services: Vec<ReconciledManagedService>,
    store: StoreRecoveryObservation,
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

pub fn backup(options: BackupOptions) -> Result<()> {
    let root = absolute_path(&options.root)?;
    validate_installation_root(&root)?;
    let lock = InstallationLock::acquire(&root)?;
    let state = read_state_optional(&root)?.context(
        "Lenso Console is not installed; create a Recovery Set only from an installed Service",
    )?;
    validate_installed_evidence(&root, &state).context(
        "installed Console evidence is drifted; run `lenso console doctor` before backup",
    )?;
    verify_attestation(&root.join(MANIFEST_FILE))
        .context("verify installed Console Release Manifest attestation")?;
    let env_file = absolute_existing_file(&options.env_file, "Console environment file")?;
    let database_url = console_database_url(&env_file)?;
    let output = absolute_path(&options.output)?;
    let manifest = backup_with(
        &PostgresAgeBackupAdapter,
        &state,
        &database_url,
        options.recipient.trim(),
        &output,
    )?;
    lock.release()?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
    } else {
        eprintln!(
            "Created encrypted Lenso Console Recovery Set {} at {}.",
            manifest.recovery_set_id,
            output.display()
        );
    }
    Ok(())
}

pub fn restore(options: RestoreOptions) -> Result<()> {
    let root = absolute_path(&options.root)?;
    validate_installation_root(&root)?;
    let mut lock = options
        .apply
        .then(|| InstallationLock::acquire(&root))
        .transpose()?;
    let state = read_state_optional(&root)?
        .context("Lenso Console is not installed; restore requires the exact installed release")?;
    validate_installed_evidence(&root, &state).context(
        "installed Console evidence is drifted; run `lenso console doctor` before restore",
    )?;
    verify_attestation(&root.join(MANIFEST_FILE))
        .context("verify installed Console Release Manifest attestation")?;
    let recovery_set = absolute_existing_directory(&options.recovery_set, "Console Recovery Set")?;
    let manifest = read_recovery_set(&recovery_set)?;
    validate_recovery_set_for_state(&manifest, &state)?;
    let current_env_file = absolute_existing_file(
        &options.current_env_file,
        "current Console environment file",
    )?;
    let recovery_env_file = absolute_existing_file(
        &options.recovery_env_file,
        "recovery Console environment file",
    )?;
    validate_restore_environments(&current_env_file, &recovery_env_file)?;
    let target_database_url = console_database_url(&recovery_env_file)?;
    let target_store_identity_digest = database_identity_digest(&target_database_url)?;
    let mut protected_files = vec![current_env_file.clone(), recovery_env_file.clone()];
    if let Some(identity_file) = options.identity_file.as_deref() {
        protected_files.push(absolute_path(identity_file)?);
    }
    validate_plan_output_path(
        options.output.as_deref(),
        &protected_files,
        &[root.clone(), recovery_set.clone()],
    )?;

    let mut plan = build_restore_plan(
        &root,
        &recovery_set,
        &manifest,
        &target_store_identity_digest,
    );
    plan.plan_digest = restore_plan_digest(&plan)?;
    print_or_write_restore_plan(&plan, options.output.as_deref())?;
    if !options.apply {
        eprintln!(
            "Plan only. Re-run with --apply --approve-plan-digest {} --identity-file <private-age-key>.",
            plan.plan_digest
        );
        return Ok(());
    }
    if options.approve_plan_digest.as_deref() != Some(plan.plan_digest.as_str()) {
        bail!("--approve-plan-digest must exactly match the current restore plan digest");
    }
    let identity_file = private_identity_file(
        options
            .identity_file
            .as_deref()
            .context("--identity-file is required with --apply")?,
    )?;
    apply_restore_with(
        &PostgresComposeRestoreAdapter,
        &root,
        &current_env_file,
        &recovery_env_file,
        &target_database_url,
        &target_store_identity_digest,
        &identity_file,
        &recovery_set.join(RECOVERY_STORE_FILE),
        &manifest,
        &plan,
    )?;
    if let Some(installation_lock) = lock.take() {
        installation_lock.release()?;
    }
    eprintln!(
        "Restored {} into an isolated Store. Console remains fenced in recovery mode pending reconciliation.",
        manifest.recovery_set_id
    );
    Ok(())
}

pub fn reconcile(options: ReconcileOptions) -> Result<()> {
    reconcile_with(&PostgresReconciliationAdapter, options)
}

trait ReconciliationAdapter {
    fn observe_store(
        &self,
        database_url: &str,
        recovery_started_at_unix_ms: u64,
    ) -> Result<StoreRecoveryObservation>;
}

#[derive(Debug, Clone, Copy)]
struct PostgresReconciliationAdapter;

impl ReconciliationAdapter for PostgresReconciliationAdapter {
    fn observe_store(
        &self,
        database_url: &str,
        recovery_started_at_unix_ms: u64,
    ) -> Result<StoreRecoveryObservation> {
        const OBSERVATION_SQL: &str = r#"
select json_build_object(
    'sessionCount', (select count(*) from auth.sessions),
    'staleSessionCount', (
        select count(*) from auth.sessions
        where created_at < to_timestamp(__RECOVERY_STARTED_AT_UNIX_MS__ / 1000.0)
    ),
    'outboxStatusCounts', (
        select coalesce(json_object_agg(status, total), '{}'::json)
        from (
            select status, count(*) as total
            from platform.outbox
            group by status
            order by status
        ) counts
    ),
    'managedServices', (
        select coalesce(json_agg(json_build_object(
            'serviceId', service_id,
            'servicePrincipal', service_principal,
            'enrollmentReceiptDigest', enrollment_receipt_digest,
            'enrollmentGrantRevision', enrollment_grant_revision,
            'authorizationEpoch', authorization_epoch,
            'enrollmentState', enrollment_state,
            'version', version,
            'coreDocument', core_document
        ) order by service_id), '[]'::json)
        from console.managed_services
    )
)::text
"#;
        let observation_sql = OBSERVATION_SQL.replace(
            "__RECOVERY_STARTED_AT_UNIX_MS__",
            &recovery_started_at_unix_ms.to_string(),
        );
        let output = Command::new("psql")
            .args([
                "--no-psqlrc",
                "--tuples-only",
                "--no-align",
                "--set",
                "ON_ERROR_STOP=1",
                "--command",
                &observation_sql,
            ])
            .env("PGDATABASE", database_url)
            .output()
            .context("observe restored Console Store for reconciliation")?;
        if !output.status.success() {
            bail!("psql failed while observing the restored Console Store");
        }
        let raw: RawStoreRecoveryObservation =
            serde_json::from_slice(output.stdout.trim_ascii())
                .context("decode restored Console Store observation")?;
        normalize_store_observation(raw, observe_outbox_snapshot_digest(database_url)?)
    }
}

fn observe_outbox_snapshot_digest(database_url: &str) -> Result<String> {
    const OUTBOX_SNAPSHOT_SQL: &str = r#"
copy (
    select row_to_json(outbox_row)::text
    from (
        select id, event_name, event_version, source_module, aggregate_type, aggregate_id,
            correlation_id, causation_id, occurred_at, payload, headers, status, attempts,
            max_attempts, available_at, locked_at, locked_by, published_at, last_error, created_at
        from platform.outbox
        order by id
    ) outbox_row
) to stdout
"#;
    let mut child = Command::new("psql")
        .args([
            "--no-psqlrc",
            "--set",
            "ON_ERROR_STOP=1",
            "--command",
            OUTBOX_SNAPSHOT_SQL,
        ])
        .env("PGDATABASE", database_url)
        .env("PGTZ", "UTC")
        .stdout(Stdio::piped())
        .spawn()
        .context("start restored Console Outbox observation")?;
    let mut stdout = child
        .stdout
        .take()
        .context("capture restored Console Outbox observation")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = stdout
            .read(&mut buffer)
            .context("read restored Console Outbox observation")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let status = child
        .wait()
        .context("wait for restored Console Outbox observation")?;
    if !status.success() {
        bail!("psql failed while observing the restored Console Outbox");
    }
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(encoded)
}

fn reconcile_with(adapter: &impl ReconciliationAdapter, options: ReconcileOptions) -> Result<()> {
    let root = absolute_path(&options.root)?;
    validate_installation_root(&root)?;
    let mut lock = options
        .apply
        .then(|| InstallationLock::acquire(&root))
        .transpose()?;
    let installed = read_state_optional(&root)?
        .context("Lenso Console is not installed; recovery reconciliation is unavailable")?;
    validate_installed_evidence(&root, &installed).context(
        "installed Console evidence is drifted; run `lenso console doctor` before reconciliation",
    )?;
    verify_attestation(&root.join(MANIFEST_FILE))
        .context("verify installed Console Release Manifest attestation")?;
    let recovery = read_recovery_state_optional(&root)?
        .context("no Console restore is awaiting reconciliation")?;
    validate_recovery_for_reconciliation(&recovery, &installed)?;
    let env_file = absolute_existing_file(&options.env_file, "recovery Console environment file")?;
    if console_env_value(&env_file, "CONSOLE_RECOVERY_MODE")? != "restore" {
        bail!("reconciliation environment must set CONSOLE_RECOVERY_MODE=restore");
    }
    let database_url = console_database_url(&env_file)?;
    if database_identity_digest(&database_url)? != recovery.target_store_identity_digest {
        bail!("reconciliation environment does not identify the restored target Store");
    }
    let input_path = absolute_existing_file(&options.evidence, "reconciliation input")?;
    let input: ReconciliationInput = serde_json::from_slice(
        &fs::read(&input_path)
            .with_context(|| format!("read reconciliation input {}", input_path.display()))?,
    )
    .context("decode Console reconciliation input")?;
    validate_plan_output_path(
        options.output.as_deref(),
        &[env_file.clone(), input_path],
        std::slice::from_ref(&root),
    )?;
    validate_reconciliation_input(&input, &recovery)?;
    let store = adapter.observe_store(&database_url, recovery.started_at_unix_ms)?;
    validate_store_observation(&store)?;
    validate_reconciliation_services(&input, &store)?;

    let store_observation_digest = sha256(&serde_json::to_vec(&store)?);
    let reconciliation_input_digest = sha256(&serde_json::to_vec(&input)?);
    let mut plan = build_reconciliation_plan(
        &recovery,
        &store_observation_digest,
        &reconciliation_input_digest,
        input.observed_at_unix_ms,
    );
    plan.plan_digest = reconciliation_plan_digest(&plan)?;
    print_or_write_reconciliation_plan(&plan, options.output.as_deref())?;
    if !options.apply {
        eprintln!(
            "Plan only. Review the external evidence and re-run with --apply --approve-plan-digest {}.",
            plan.plan_digest
        );
        return Ok(());
    }
    if options.approve_plan_digest.as_deref() != Some(plan.plan_digest.as_str()) {
        bail!("--approve-plan-digest must exactly match the current reconciliation plan digest");
    }

    let evidence = build_reconciliation_evidence(
        &recovery,
        &plan,
        &input,
        store,
        store_observation_digest,
        reconciliation_input_digest,
    )?;
    publish_reconciliation_evidence(&root, &evidence)?;
    write_reconciled_recovery_state(&root, &recovery, &evidence)?;
    if let Some(installation_lock) = lock.take() {
        installation_lock.release()?;
    }
    eprintln!(
        "Recorded reconciliation evidence {}. Console remains fenced pending a separate activation approval.",
        evidence.evidence_id
    );
    Ok(())
}

fn normalize_store_observation(
    raw: RawStoreRecoveryObservation,
    outbox_snapshot_digest: String,
) -> Result<StoreRecoveryObservation> {
    let mut managed_services = raw
        .managed_services
        .into_iter()
        .map(|service| {
            Ok(ObservedManagedService {
                service_id: service.service_id,
                service_principal: service.service_principal,
                enrollment_receipt_digest: service.enrollment_receipt_digest,
                enrollment_grant_revision: service.enrollment_grant_revision,
                authorization_epoch: service.authorization_epoch,
                enrollment_state: service.enrollment_state,
                version: service.version,
                core_document_digest: service
                    .core_document
                    .map(|document| serde_json::to_vec(&document).map(|bytes| sha256(&bytes)))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    managed_services.sort_by(|left, right| left.service_id.cmp(&right.service_id));
    Ok(StoreRecoveryObservation {
        session_count: raw.session_count,
        stale_session_count: raw.stale_session_count,
        outbox_status_counts: raw.outbox_status_counts,
        outbox_snapshot_digest,
        managed_services,
    })
}

fn validate_recovery_for_reconciliation(
    recovery: &RecoveryState,
    installed: &InstallationState,
) -> Result<()> {
    if recovery.release_id != installed.release_id {
        bail!("recovery state does not match the installed Console release");
    }
    if recovery.status != RecoveryStatus::AwaitingReconciliation {
        bail!("Console restore is not in a reconcilable state");
    }
    Ok(())
}

fn validate_reconciliation_input(
    input: &ReconciliationInput,
    recovery: &RecoveryState,
) -> Result<()> {
    let now = unix_time_ms()?;
    if input.schema != RECONCILIATION_INPUT_SCHEMA
        || input.recovery_set_id != recovery.recovery_set_id
        || input.observed_at_unix_ms < recovery.updated_at_unix_ms
        || input.observed_at_unix_ms > now.saturating_add(300_000)
        || !valid_evidence_reference(&input.reviewed_by)
        || !valid_evidence_reference(&input.authority_evidence_ref)
        || !valid_evidence_reference(&input.identity_evidence_ref)
        || !valid_evidence_reference(&input.outbox_evidence_ref)
        || !input.single_authoritative_deployment
        || !input.identity_and_enrollment_continuity_verified
        || !input.outbox_reconciled
    {
        bail!("Console reconciliation input is invalid or does not pass every recovery boundary");
    }
    let mut previous = None;
    for service in &input.managed_services {
        if service.service_id.trim().is_empty()
            || service.service_principal.trim().is_empty()
            || !is_sha256(&service.enrollment_receipt_digest)
            || service
                .core_document_digest
                .as_deref()
                .is_some_and(|digest| !is_sha256(digest))
            || previous.is_some_and(|value| value >= service.service_id.as_str())
        {
            bail!("Console reconciliation managed-Service evidence is invalid or unsorted");
        }
        previous = Some(service.service_id.as_str());
    }
    Ok(())
}

fn valid_evidence_reference(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_whitespace())
        && !value.contains("secret=")
        && !value.contains("BEGIN_")
}

fn validate_store_observation(store: &StoreRecoveryObservation) -> Result<()> {
    if store.stale_session_count != 0 || store.stale_session_count > store.session_count {
        bail!(
            "restored Store contains pre-recovery browser sessions excluded by the Recovery Set contract"
        );
    }
    if !is_sha256(&store.outbox_snapshot_digest) {
        bail!("restored Store Outbox snapshot digest is invalid");
    }
    let supported_outbox_states = ["failed", "pending", "processing", "published"];
    if store
        .outbox_status_counts
        .iter()
        .any(|(status, count)| !supported_outbox_states.contains(&status.as_str()) || *count == 0)
    {
        bail!("restored Store contains an unsupported Outbox observation");
    }
    let mut previous = None;
    for service in &store.managed_services {
        if service.service_id.trim().is_empty()
            || service.service_principal.trim().is_empty()
            || !is_sha256(&service.enrollment_receipt_digest)
            || service.enrollment_grant_revision == 0
            || !matches!(service.enrollment_state.as_str(), "active" | "revoked")
            || service.version == 0
            || service
                .core_document_digest
                .as_deref()
                .is_some_and(|digest| !is_sha256(digest))
            || previous.is_some_and(|value| value >= service.service_id.as_str())
        {
            bail!("restored Store managed-Service identity evidence is invalid");
        }
        previous = Some(service.service_id.as_str());
    }
    Ok(())
}

fn validate_reconciliation_services(
    input: &ReconciliationInput,
    store: &StoreRecoveryObservation,
) -> Result<()> {
    if input.managed_services != reconciled_services_from_store(store) {
        bail!("reviewed managed-Service evidence does not match the restored Store identity set");
    }
    Ok(())
}

fn reconciled_services_from_store(
    store: &StoreRecoveryObservation,
) -> Vec<ReconciledManagedService> {
    store
        .managed_services
        .iter()
        .map(|service| ReconciledManagedService {
            service_id: service.service_id.clone(),
            service_principal: service.service_principal.clone(),
            enrollment_receipt_digest: service.enrollment_receipt_digest.clone(),
            authorization_epoch: service.authorization_epoch,
            core_document_digest: service.core_document_digest.clone(),
        })
        .collect()
}

fn build_reconciliation_plan(
    recovery: &RecoveryState,
    store_observation_digest: &str,
    reconciliation_input_digest: &str,
    observed_at_unix_ms: u64,
) -> ReconciliationPlan {
    ReconciliationPlan {
        schema: RECONCILIATION_PLAN_SCHEMA,
        action: "reconcile",
        plan_digest: String::new(),
        recovery_set_id: recovery.recovery_set_id.clone(),
        recovery_set_digest: recovery.recovery_set_digest.clone(),
        restore_plan_digest: recovery.plan_digest.clone(),
        release_id: recovery.release_id.clone(),
        target_store_identity_digest: recovery.target_store_identity_digest.clone(),
        store_observation_digest: store_observation_digest.to_owned(),
        reconciliation_input_digest: reconciliation_input_digest.to_owned(),
        observed_at_unix_ms,
        steps: vec![
            "verify_recovery_state_and_release".to_owned(),
            "observe_restored_store_without_mutation".to_owned(),
            "bind_managed_service_identity_continuity".to_owned(),
            "bind_single_deployment_and_outbox_reconciliation".to_owned(),
            "record_ready_for_activation".to_owned(),
        ],
        approval_boundaries: vec!["recovery_reconciliation".to_owned()],
        next_actions: vec![
            "Keep the Console in restore mode after reconciliation.".to_owned(),
            "Review a separate activation plan before enabling management mutations or background work."
                .to_owned(),
        ],
    }
}

fn reconciliation_plan_digest(plan: &ReconciliationPlan) -> Result<String> {
    let mut value = serde_json::to_value(plan).context("encode Console reconciliation plan")?;
    value["planDigest"] = Value::String(String::new());
    Ok(sha256(&serde_json::to_vec(&value)?))
}

fn print_or_write_reconciliation_plan(
    plan: &ReconciliationPlan,
    output: Option<&Path>,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(plan).context("encode Console reconciliation plan")?;
    if let Some(path) = output {
        atomic_write(path, &bytes)?;
        eprintln!("Wrote Console reconciliation plan to {}.", path.display());
    } else {
        println!(
            "{}",
            String::from_utf8(bytes).context("render Console reconciliation plan")?
        );
    }
    Ok(())
}

fn validate_plan_output_path(
    output: Option<&Path>,
    protected_files: &[PathBuf],
    protected_directories: &[PathBuf],
) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let output = normalized_absolute_path(output)?;
    if protected_files
        .iter()
        .map(|path| normalized_absolute_path(path))
        .collect::<Result<Vec<_>>>()?
        .contains(&output)
        || protected_directories
            .iter()
            .map(|path| normalized_absolute_path(path))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .any(|directory| output.starts_with(directory))
    {
        bail!("plan output must not overwrite Console recovery inputs or owned state");
    }
    Ok(())
}

fn normalized_absolute_path(path: &Path) -> Result<PathBuf> {
    let path = absolute_path(path)?;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    bail!("path escapes its filesystem root");
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn build_reconciliation_evidence(
    recovery: &RecoveryState,
    plan: &ReconciliationPlan,
    input: &ReconciliationInput,
    store: StoreRecoveryObservation,
    store_observation_digest: String,
    reconciliation_input_digest: String,
) -> Result<ReconciliationEvidence> {
    let mut evidence = ReconciliationEvidence {
        schema: RECONCILIATION_EVIDENCE_SCHEMA.to_owned(),
        evidence_id: String::new(),
        evidence_digest: String::new(),
        recovery_set_id: recovery.recovery_set_id.clone(),
        recovery_set_digest: recovery.recovery_set_digest.clone(),
        restore_plan_digest: recovery.plan_digest.clone(),
        reconciliation_plan_digest: plan.plan_digest.clone(),
        release_id: recovery.release_id.clone(),
        target_store_identity_digest: recovery.target_store_identity_digest.clone(),
        store_observation_digest,
        reconciliation_input_digest,
        observed_at_unix_ms: input.observed_at_unix_ms,
        reviewed_by: input.reviewed_by.clone(),
        authority_evidence_ref: input.authority_evidence_ref.clone(),
        identity_evidence_ref: input.identity_evidence_ref.clone(),
        outbox_evidence_ref: input.outbox_evidence_ref.clone(),
        single_authoritative_deployment: input.single_authoritative_deployment,
        identity_and_enrollment_continuity_verified: input
            .identity_and_enrollment_continuity_verified,
        outbox_reconciled: input.outbox_reconciled,
        reconciled_managed_services: input.managed_services.clone(),
        store,
    };
    evidence.evidence_digest = reconciliation_evidence_digest(&evidence)?;
    evidence.evidence_id = format!("recon_{}", &evidence.evidence_digest[7..23]);
    Ok(evidence)
}

fn reconciliation_evidence_digest(evidence: &ReconciliationEvidence) -> Result<String> {
    let mut value = serde_json::to_value(evidence).context("encode reconciliation evidence")?;
    value["evidenceId"] = Value::String(String::new());
    value["evidenceDigest"] = Value::String(String::new());
    Ok(sha256(&serde_json::to_vec(&value)?))
}

fn publish_reconciliation_evidence(root: &Path, evidence: &ReconciliationEvidence) -> Result<()> {
    let path = root.join(RECONCILIATION_EVIDENCE_FILE);
    let bytes = serde_json::to_vec_pretty(evidence).context("encode reconciliation evidence")?;
    if let Some(existing) = read_optional_regular_file(&path, "Console reconciliation evidence")? {
        if existing != bytes {
            bail!("different Console reconciliation evidence already exists");
        }
        return Ok(());
    }
    atomic_write(&path, &bytes)
}

fn write_reconciled_recovery_state(
    root: &Path,
    recovery: &RecoveryState,
    evidence: &ReconciliationEvidence,
) -> Result<()> {
    let state = RecoveryState {
        schema: RECOVERY_STATE_SCHEMA.to_owned(),
        recovery_set_id: recovery.recovery_set_id.clone(),
        recovery_set_digest: recovery.recovery_set_digest.clone(),
        plan_digest: recovery.plan_digest.clone(),
        release_id: recovery.release_id.clone(),
        target_store_identity_digest: recovery.target_store_identity_digest.clone(),
        status: RecoveryStatus::ReadyForActivation,
        phase: "activation_approval".to_owned(),
        reconciliation_evidence_digest: Some(evidence.evidence_digest.clone()),
        started_at_unix_ms: recovery.started_at_unix_ms,
        updated_at_unix_ms: unix_time_ms()?,
    };
    atomic_write(
        &root.join(RECOVERY_STATE_FILE),
        &serde_json::to_vec_pretty(&state).context("encode Console recovery state")?,
    )
}

trait RestoreAdapter {
    fn verify_payload(&self, identity_file: &Path, payload: &Path) -> Result<()>;
    fn target_is_clean(&self, database_url: &str) -> Result<bool>;
    fn fence(&self, root: &Path, env_file: &Path, compose_file: &Path) -> Result<()>;
    fn restore_store(&self, database_url: &str, identity_file: &Path, payload: &Path)
    -> Result<()>;
    fn start_recovery(&self, root: &Path, env_file: &Path, compose_file: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct PostgresComposeRestoreAdapter;

impl RestoreAdapter for PostgresComposeRestoreAdapter {
    fn verify_payload(&self, identity_file: &Path, payload: &Path) -> Result<()> {
        let mut decrypt = Command::new("age")
            .args(["--decrypt", "--identity"])
            .arg(identity_file)
            .arg(payload)
            .stdout(Stdio::piped())
            .spawn()
            .context("start age preflight for Console Recovery Set")?;
        let plaintext = decrypt
            .stdout
            .take()
            .context("capture Recovery Set preflight stream")?;
        let mut inspect = match Command::new("pg_restore")
            .arg("--list")
            .stdin(Stdio::from(plaintext))
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = decrypt.kill();
                let _ = decrypt.wait();
                return Err(error).context("start pg_restore Recovery Set preflight");
            }
        };
        let decrypt_status = decrypt
            .wait()
            .context("wait for Recovery Set preflight decryption")?;
        let inspect_status = inspect
            .wait()
            .context("wait for Recovery Set archive preflight")?;
        if !decrypt_status.success() || !inspect_status.success() {
            bail!("Recovery Set identity or PostgreSQL archive preflight failed");
        }
        Ok(())
    }

    fn target_is_clean(&self, database_url: &str) -> Result<bool> {
        const CLEAN_STORE_SQL: &str = "select count(*) from pg_catalog.pg_class c join pg_catalog.pg_namespace n on n.oid = c.relnamespace where c.relkind in ('r','p','v','m','S','f') and n.nspname not in ('pg_catalog','information_schema')";
        let output = Command::new("psql")
            .args([
                "--no-psqlrc",
                "--tuples-only",
                "--no-align",
                "--command",
                CLEAN_STORE_SQL,
            ])
            .env("PGDATABASE", database_url)
            .output()
            .context("inspect isolated Console restore Store")?;
        if !output.status.success() {
            bail!("psql failed while checking the isolated Console restore Store");
        }
        let count = String::from_utf8(output.stdout)
            .context("decode clean Store observation")?
            .trim()
            .parse::<u64>()
            .context("parse clean Store observation")?;
        Ok(count == 0)
    }

    fn fence(&self, root: &Path, env_file: &Path, compose_file: &Path) -> Result<()> {
        DockerComposeAdapter.run(
            root,
            env_file,
            compose_file,
            &["stop", "--timeout", "30", "console"],
        )
    }

    fn restore_store(
        &self,
        database_url: &str,
        identity_file: &Path,
        payload: &Path,
    ) -> Result<()> {
        let mut decrypt = Command::new("age")
            .args(["--decrypt", "--identity"])
            .arg(identity_file)
            .arg(payload)
            .stdout(Stdio::piped())
            .spawn()
            .context("start age decryption for Console restore")?;
        let plaintext = decrypt
            .stdout
            .take()
            .context("capture decrypted Console Store stream")?;
        let mut restore = match Command::new("pg_restore")
            .args([
                "--dbname=",
                "--exit-on-error",
                "--single-transaction",
                "--no-owner",
                "--no-privileges",
            ])
            .env("PGDATABASE", database_url)
            .stdin(Stdio::from(plaintext))
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = decrypt.kill();
                let _ = decrypt.wait();
                return Err(error).context("start pg_restore for Console Recovery Set");
            }
        };
        let decrypt_status = decrypt.wait().context("wait for Recovery Set decryption")?;
        let restore_status = restore.wait().context("wait for Console Store restore")?;
        if !decrypt_status.success() {
            bail!("age failed while decrypting the Console Recovery Set");
        }
        if !restore_status.success() {
            bail!("pg_restore failed while restoring the isolated Console Store");
        }
        Ok(())
    }

    fn start_recovery(&self, root: &Path, env_file: &Path, compose_file: &Path) -> Result<()> {
        DockerComposeAdapter.run(
            root,
            env_file,
            compose_file,
            &[
                "up",
                "--detach",
                "--wait",
                "--wait-timeout",
                "120",
                "console",
            ],
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_restore_with(
    adapter: &impl RestoreAdapter,
    root: &Path,
    current_env_file: &Path,
    recovery_env_file: &Path,
    target_database_url: &str,
    target_store_identity_digest: &str,
    identity_file: &Path,
    payload: &Path,
    manifest: &RecoverySetManifest,
    plan: &RestorePlan,
) -> Result<()> {
    let started_at_unix_ms = unix_time_ms()?;
    let mut phase = "verify_recovery_payload";
    write_recovery_state(
        root,
        manifest,
        plan,
        RecoveryStatus::Applying,
        phase,
        target_store_identity_digest,
        started_at_unix_ms,
    )?;
    let result = (|| -> Result<()> {
        adapter.verify_payload(identity_file, payload)?;
        phase = "clean_store_check";
        write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::Applying,
            phase,
            target_store_identity_digest,
            started_at_unix_ms,
        )?;
        if !adapter.target_is_clean(target_database_url)? {
            bail!("recovery target Store is not clean; restore refused without mutation");
        }
        phase = "fence_previous_deployment";
        write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::Applying,
            phase,
            target_store_identity_digest,
            started_at_unix_ms,
        )?;
        adapter.fence(root, current_env_file, &root.join(COMPOSE_FILE))?;

        phase = "restore_store";
        write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::Applying,
            phase,
            target_store_identity_digest,
            started_at_unix_ms,
        )?;
        adapter.restore_store(target_database_url, identity_file, payload)?;

        phase = "start_recovery_mode";
        write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::Applying,
            phase,
            target_store_identity_digest,
            started_at_unix_ms,
        )?;
        adapter.start_recovery(root, recovery_env_file, &root.join(COMPOSE_FILE))?;
        write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::AwaitingReconciliation,
            "reconciliation",
            target_store_identity_digest,
            started_at_unix_ms,
        )
    })();
    if result.is_err() {
        let _ = write_recovery_state(
            root,
            manifest,
            plan,
            RecoveryStatus::Failed,
            phase,
            target_store_identity_digest,
            started_at_unix_ms,
        );
    }
    result
}

trait StoreBackupAdapter {
    fn export_encrypted(&self, database_url: &str, recipient: &str, output: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct PostgresAgeBackupAdapter;

impl StoreBackupAdapter for PostgresAgeBackupAdapter {
    fn export_encrypted(&self, database_url: &str, recipient: &str, output: &Path) -> Result<()> {
        let mut dump = Command::new("pg_dump")
            .args(RECOVERY_PG_DUMP_ARGS)
            .env("PGDATABASE", database_url)
            .stdout(Stdio::piped())
            .spawn()
            .context("start pg_dump for Console Recovery Set")?;
        let dump_stdout = dump
            .stdout
            .take()
            .context("capture pg_dump output for Console Recovery Set")?;
        let mut encrypt = match Command::new("age")
            .args(["--encrypt", "--recipient", recipient, "--output"])
            .arg(output)
            .stdin(Stdio::from(dump_stdout))
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = dump.kill();
                let _ = dump.wait();
                return Err(error).context("start age encryption for Console Recovery Set");
            }
        };
        let dump_status = dump.wait().context("wait for Console Store export")?;
        let encrypt_status = encrypt
            .wait()
            .context("wait for Console Store encryption")?;
        if !dump_status.success() {
            bail!("pg_dump failed while creating the Console Recovery Set");
        }
        if !encrypt_status.success() {
            bail!("age failed while encrypting the Console Recovery Set");
        }
        Ok(())
    }
}

fn backup_with(
    adapter: &impl StoreBackupAdapter,
    state: &InstallationState,
    database_url: &str,
    recipient: &str,
    output: &Path,
) -> Result<RecoverySetManifest> {
    if recipient.is_empty() || recipient.chars().any(char::is_whitespace) {
        bail!("--recipient must be one non-empty age recipient");
    }
    if output.exists() {
        bail!(
            "Console Recovery Set output already exists: {}",
            output.display()
        );
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .context("Console Recovery Set output has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create Recovery Set parent {}", parent.display()))?;
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .context("Console Recovery Set output has no valid directory name")?;
    let staging = parent.join(format!(".{name}.tmp-{}", Uuid::now_v7()));
    fs::create_dir(&staging).with_context(|| {
        format!(
            "create Recovery Set staging directory {}",
            staging.display()
        )
    })?;
    restrict_directory_permissions(&staging)?;
    let result = (|| -> Result<RecoverySetManifest> {
        let payload = staging.join(RECOVERY_STORE_FILE);
        adapter.export_encrypted(database_url, recipient, &payload)?;
        let (payload_digest, payload_size) = sha256_file(&payload)?;
        if payload_size == 0 {
            bail!("encrypted Console Store payload is empty");
        }
        let mut manifest = RecoverySetManifest {
            schema: RECOVERY_SET_SCHEMA.to_owned(),
            recovery_set_id: format!("rcv_{}", Uuid::now_v7()),
            recovery_set_digest: String::new(),
            created_at_unix_ms: unix_time_ms()?,
            release_id: state.release_id.clone(),
            release_digest: state.release_digest.clone(),
            image_reference: state.image_reference.clone(),
            schema_digest: state.schema_digest.clone(),
            composition_digest: state.composition_digest.clone(),
            contract_digest: state.contract_digest.clone(),
            configuration_digest: state.configuration_digest.clone(),
            store: RecoveryStore {
                format: "postgresql-custom".to_owned(),
                encrypted: true,
                encryption: "age-v1".to_owned(),
                payload: RECOVERY_STORE_FILE.to_owned(),
                payload_digest,
                recipient: recipient.to_owned(),
                excluded_data: vec!["auth.sessions".to_owned()],
            },
            secret_references: vec!["CONSOLE_DATABASE_URL".to_owned()],
            restore_preconditions: vec![
                "clean_store".to_owned(),
                "exact_release_and_composition".to_owned(),
                "external_secret_resolution".to_owned(),
                "outbound_mutations_disabled".to_owned(),
                "single_authoritative_deployment".to_owned(),
                "identity_and_enrollment_continuity_validation".to_owned(),
            ],
        };
        manifest.recovery_set_digest = recovery_set_digest(&manifest)?;
        atomic_write(
            &staging.join(RECOVERY_MANIFEST_FILE),
            &serde_json::to_vec_pretty(&manifest).context("encode Recovery Set manifest")?,
        )?;
        fs::rename(&staging, output)
            .with_context(|| format!("commit Console Recovery Set {}", output.display()))?;
        Ok(manifest)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn recovery_set_digest(manifest: &RecoverySetManifest) -> Result<String> {
    let mut value = serde_json::to_value(manifest).context("encode Recovery Set manifest")?;
    value["recoverySetDigest"] = Value::String(String::new());
    Ok(sha256(&serde_json::to_vec(&value)?))
}

fn read_recovery_set(root: &Path) -> Result<RecoverySetManifest> {
    let manifest_path = absolute_existing_file(
        &root.join(RECOVERY_MANIFEST_FILE),
        "Console Recovery Set manifest",
    )?;
    let payload_path = absolute_existing_file(
        &root.join(RECOVERY_STORE_FILE),
        "encrypted Console Store payload",
    )?;
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("read Recovery Set manifest {}", manifest_path.display()))?;
    let manifest: RecoverySetManifest =
        serde_json::from_slice(&bytes).context("decode Recovery Set manifest")?;
    validate_recovery_set_manifest(&manifest)?;
    if manifest.recovery_set_digest != recovery_set_digest(&manifest)? {
        bail!("Console Recovery Set manifest digest does not match its content");
    }
    let (payload_digest, payload_size) = sha256_file(&payload_path)?;
    if payload_size == 0 || manifest.store.payload_digest != payload_digest {
        bail!("Console Recovery Set payload is empty or does not match its digest");
    }
    Ok(manifest)
}

fn validate_recovery_set_manifest(manifest: &RecoverySetManifest) -> Result<()> {
    let recovery_id = manifest
        .recovery_set_id
        .strip_prefix("rcv_")
        .and_then(|value| Uuid::parse_str(value).ok());
    let expected_preconditions = [
        "clean_store",
        "exact_release_and_composition",
        "external_secret_resolution",
        "outbound_mutations_disabled",
        "single_authoritative_deployment",
        "identity_and_enrollment_continuity_validation",
    ];
    if manifest.schema != RECOVERY_SET_SCHEMA
        || recovery_id.is_none()
        || !is_sha256(&manifest.recovery_set_digest)
        || manifest.created_at_unix_ms == 0
        || release_version(&manifest.release_id).is_none()
        || !is_sha256(&manifest.release_digest)
        || !trusted_image_reference(&manifest.image_reference)
        || !is_sha256(&manifest.schema_digest)
        || !is_sha256(&manifest.composition_digest)
        || !is_sha256(&manifest.contract_digest)
        || !is_sha256(&manifest.configuration_digest)
        || manifest.store.format != "postgresql-custom"
        || !manifest.store.encrypted
        || manifest.store.encryption != "age-v1"
        || manifest.store.payload != RECOVERY_STORE_FILE
        || !is_sha256(&manifest.store.payload_digest)
        || manifest.store.recipient.trim().is_empty()
        || manifest.store.recipient.chars().any(char::is_whitespace)
        || manifest.store.excluded_data != ["auth.sessions"]
        || manifest.secret_references != ["CONSOLE_DATABASE_URL"]
        || manifest.restore_preconditions != expected_preconditions
    {
        bail!("Console Recovery Set manifest is invalid or unsupported");
    }
    Ok(())
}

fn validate_recovery_set_for_state(
    manifest: &RecoverySetManifest,
    state: &InstallationState,
) -> Result<()> {
    if manifest.release_id != state.release_id
        || manifest.release_digest != state.release_digest
        || manifest.image_reference != state.image_reference
        || manifest.schema_digest != state.schema_digest
        || manifest.composition_digest != state.composition_digest
        || manifest.contract_digest != state.contract_digest
        || manifest.configuration_digest != state.configuration_digest
    {
        bail!("Console Recovery Set does not match the exact installed release and composition");
    }
    Ok(())
}

fn build_restore_plan(
    root: &Path,
    recovery_set: &Path,
    manifest: &RecoverySetManifest,
    target_store_identity_digest: &str,
) -> RestorePlan {
    RestorePlan {
        schema: RESTORE_PLAN_SCHEMA,
        action: "restore",
        plan_digest: String::new(),
        installation_root: root.display().to_string(),
        recovery_set_path: recovery_set.display().to_string(),
        recovery_set_id: manifest.recovery_set_id.clone(),
        recovery_set_digest: manifest.recovery_set_digest.clone(),
        release_id: manifest.release_id.clone(),
        release_digest: manifest.release_digest.clone(),
        payload_digest: manifest.store.payload_digest.clone(),
        target_store_identity_digest: target_store_identity_digest.to_owned(),
        steps: vec![
            "verify_decryption_identity_and_archive".to_owned(),
            "verify_clean_isolated_store".to_owned(),
            "fence_previous_deployment".to_owned(),
            "decrypt_and_restore_store".to_owned(),
            "start_api_in_recovery_mode".to_owned(),
            "record_awaiting_reconciliation".to_owned(),
        ],
        approval_boundaries: vec!["disaster_recovery_restore".to_owned()],
        target_requirements: manifest.restore_preconditions.clone(),
        next_actions: vec![
            "Reconcile managed-Service operations and identity continuity.".to_owned(),
            "Prove exactly one authoritative Console deployment.".to_owned(),
            "Keep CONSOLE_RECOVERY_MODE=restore until a separately reviewed activation.".to_owned(),
        ],
    }
}

fn restore_plan_digest(plan: &RestorePlan) -> Result<String> {
    let mut value = serde_json::to_value(plan).context("encode Console restore plan")?;
    value["planDigest"] = Value::String(String::new());
    Ok(sha256(&serde_json::to_vec(&value)?))
}

fn print_or_write_restore_plan(plan: &RestorePlan, output: Option<&Path>) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(plan).context("encode Console restore plan")?;
    if let Some(path) = output {
        atomic_write(path, &bytes)?;
        eprintln!("Wrote Console restore plan to {}.", path.display());
    } else {
        println!(
            "{}",
            String::from_utf8(bytes).context("render Console restore plan")?
        );
    }
    Ok(())
}

fn validate_restore_environments(current: &Path, recovery: &Path) -> Result<()> {
    if console_env_value(current, "CONSOLE_RECOVERY_MODE")? != "normal" {
        bail!("current environment must set CONSOLE_RECOVERY_MODE=normal");
    }
    if console_env_value(recovery, "CONSOLE_RECOVERY_MODE")? != "restore" {
        bail!("recovery environment must set CONSOLE_RECOVERY_MODE=restore");
    }
    let current_url = console_database_url(current)?;
    let recovery_url = console_database_url(recovery)?;
    if database_identity(&current_url)? == database_identity(&recovery_url)? {
        bail!("recovery environment must target a distinct isolated Console Store");
    }
    Ok(())
}

fn database_identity(value: &str) -> Result<(String, u16, String)> {
    let url = Url::parse(value).context("parse Console database identity")?;
    Ok((
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default().unwrap_or(5432),
        url.path().to_owned(),
    ))
}

fn database_identity_digest(value: &str) -> Result<String> {
    Ok(sha256(&serde_json::to_vec(&database_identity(value)?)?))
}

fn write_recovery_state(
    root: &Path,
    manifest: &RecoverySetManifest,
    plan: &RestorePlan,
    status: RecoveryStatus,
    phase: &str,
    target_store_identity_digest: &str,
    started_at_unix_ms: u64,
) -> Result<()> {
    let state = RecoveryState {
        schema: RECOVERY_STATE_SCHEMA.to_owned(),
        recovery_set_id: manifest.recovery_set_id.clone(),
        recovery_set_digest: manifest.recovery_set_digest.clone(),
        plan_digest: plan.plan_digest.clone(),
        release_id: manifest.release_id.clone(),
        target_store_identity_digest: target_store_identity_digest.to_owned(),
        status,
        phase: phase.to_owned(),
        reconciliation_evidence_digest: None,
        started_at_unix_ms,
        updated_at_unix_ms: unix_time_ms()?,
    };
    atomic_write(
        &root.join(RECOVERY_STATE_FILE),
        &serde_json::to_vec_pretty(&state).context("encode Console recovery state")?,
    )
}

fn private_identity_file(path: &Path) -> Result<PathBuf> {
    let path = absolute_existing_file(path, "age identity file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if fs::metadata(&path)?.permissions().mode() & 0o077 != 0 {
            bail!("age identity file must be readable only by its owner");
        }
    }
    Ok(path)
}

fn absolute_existing_directory(path: &Path, label: &str) -> Result<PathBuf> {
    let path = absolute_path(path)?;
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{label} must be a directory and not a symbolic link");
    }
    Ok(path)
}

fn console_database_url(env_file: &Path) -> Result<String> {
    let value = console_env_value(env_file, "CONSOLE_DATABASE_URL")?;
    let url = Url::parse(&value).context("parse CONSOLE_DATABASE_URL")?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str().is_none()
        || url.path().trim_matches('/').is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("CONSOLE_DATABASE_URL must be an unambiguous PostgreSQL URL with host and database");
    }
    Ok(value)
}

fn console_env_value(env_file: &Path, key: &str) -> Result<String> {
    let source = fs::read_to_string(env_file)
        .with_context(|| format!("read Console environment file {}", env_file.display()))?;
    let mut values = source.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (env_key, value) = line.split_once('=')?;
        (env_key.trim() == key).then(|| value.trim())
    });
    let value = values
        .next()
        .with_context(|| format!("{key} is missing from the Console environment file"))?;
    if values.next().is_some() {
        bail!("{key} is duplicated in the Console environment file");
    }
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value);
    Ok(value.to_owned())
}

fn restrict_directory_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("restrict permissions on {}", path.display()))?;
    }
    Ok(())
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
        "name: lenso-console\n\nservices:\n  migrate:\n    image: {image}\n    command: [\"/usr/local/bin/lenso-console-migrate\"]\n    environment: &console-environment\n      APP_ENV: production\n      CORS_ALLOWED_ORIGINS: ${{CONSOLE_PUBLIC_ORIGIN:?set CONSOLE_PUBLIC_ORIGIN}}\n      DATABASE_URL: ${{CONSOLE_DATABASE_URL:?set CONSOLE_DATABASE_URL}}\n      CONSOLE_RECOVERY_MODE: ${{CONSOLE_RECOVERY_MODE:-normal}}\n      LENSO_COMPOSITION_PROFILE: core\n      SERVICE_NAME: lenso-console\n    read_only: true\n    security_opt:\n      - no-new-privileges:true\n    cap_drop:\n      - ALL\n    tmpfs:\n      - /tmp\n  console:\n    image: {image}\n    environment: *console-environment\n    ports:\n      - \"${{CONSOLE_HTTP_PORT:-3030}}:3030\"\n    read_only: true\n    restart: unless-stopped\n    security_opt:\n      - no-new-privileges:true\n    cap_drop:\n      - ALL\n    tmpfs:\n      - /tmp\n"
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
    check_recovery_state(&root, &mut checks);
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

fn check_recovery_state(root: &Path, checks: &mut Vec<DoctorCheck>) {
    match read_recovery_state_optional(root) {
        Ok(None) => checks.push(pass("recovery", "no Console recovery is active")),
        Ok(Some(state)) => {
            let detail = if state.status == RecoveryStatus::ReadyForActivation {
                match read_reconciliation_evidence_optional(root) {
                    Ok(Some(evidence))
                        if state.reconciliation_evidence_digest.as_deref()
                            == Some(evidence.evidence_digest.as_str()) =>
                    {
                        if evidence.recovery_set_id == state.recovery_set_id
                            && evidence.restore_plan_digest == state.plan_digest
                            && evidence.target_store_identity_digest
                                == state.target_store_identity_digest
                        {
                            format!(
                                "Console recovery for {} is ready for a separate activation approval; keep recovery mode fenced",
                                state.recovery_set_id
                            )
                        } else {
                            "Console reconciliation evidence does not match recovery state; keep recovery mode fenced"
                                .to_owned()
                        }
                    }
                    _ => "Console reconciliation evidence is missing, invalid, or drifted; keep recovery mode fenced"
                        .to_owned(),
                }
            } else {
                format!(
                    "Console recovery for {} is {:?} at phase {}; keep recovery mode fenced",
                    state.recovery_set_id, state.status, state.phase
                )
                .to_lowercase()
            };
            checks.push(fail("recovery", &detail));
        }
        Err(_) => checks.push(fail("recovery", "Console recovery evidence is invalid")),
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

fn read_recovery_state_optional(root: &Path) -> Result<Option<RecoveryState>> {
    let path = root.join(RECOVERY_STATE_FILE);
    let Some(bytes) = read_optional_regular_file(&path, "Console recovery state")? else {
        return Ok(None);
    };
    let state: RecoveryState =
        serde_json::from_slice(&bytes).context("decode Console recovery state")?;
    let status_phase_valid = match state.status {
        RecoveryStatus::Applying | RecoveryStatus::Failed => matches!(
            state.phase.as_str(),
            "verify_recovery_payload"
                | "clean_store_check"
                | "fence_previous_deployment"
                | "restore_store"
                | "start_recovery_mode"
        ),
        RecoveryStatus::AwaitingReconciliation => state.phase == "reconciliation",
        RecoveryStatus::ReadyForActivation => state.phase == "activation_approval",
    };
    if state.schema != RECOVERY_STATE_SCHEMA
        || state
            .recovery_set_id
            .strip_prefix("rcv_")
            .and_then(|value| Uuid::parse_str(value).ok())
            .is_none()
        || !is_sha256(&state.recovery_set_digest)
        || !is_sha256(&state.plan_digest)
        || release_version(&state.release_id).is_none()
        || !is_sha256(&state.target_store_identity_digest)
        || !status_phase_valid
        || !matches!(
            state.phase.as_str(),
            "clean_store_check"
                | "verify_recovery_payload"
                | "fence_previous_deployment"
                | "restore_store"
                | "start_recovery_mode"
                | "reconciliation"
                | "activation_approval"
        )
        || (state.status == RecoveryStatus::ReadyForActivation
            && (state.phase != "activation_approval"
                || state.reconciliation_evidence_digest.is_none()))
        || (state.status != RecoveryStatus::ReadyForActivation
            && state.reconciliation_evidence_digest.is_some())
        || state
            .reconciliation_evidence_digest
            .as_deref()
            .is_some_and(|digest| !is_sha256(digest))
        || state.started_at_unix_ms == 0
        || state.updated_at_unix_ms < state.started_at_unix_ms
    {
        bail!("Console recovery state is invalid");
    }
    Ok(Some(state))
}

fn read_reconciliation_evidence_optional(root: &Path) -> Result<Option<ReconciliationEvidence>> {
    let path = root.join(RECONCILIATION_EVIDENCE_FILE);
    let Some(bytes) = read_optional_regular_file(&path, "Console reconciliation evidence")? else {
        return Ok(None);
    };
    let evidence: ReconciliationEvidence =
        serde_json::from_slice(&bytes).context("decode Console reconciliation evidence")?;
    if evidence.schema != RECONCILIATION_EVIDENCE_SCHEMA
        || !evidence.evidence_id.starts_with("recon_")
        || !is_sha256(&evidence.evidence_digest)
        || evidence.evidence_digest != reconciliation_evidence_digest(&evidence)?
        || evidence.evidence_id != format!("recon_{}", &evidence.evidence_digest[7..23])
        || evidence
            .recovery_set_id
            .strip_prefix("rcv_")
            .and_then(|value| Uuid::parse_str(value).ok())
            .is_none()
        || !is_sha256(&evidence.recovery_set_digest)
        || !is_sha256(&evidence.restore_plan_digest)
        || !is_sha256(&evidence.reconciliation_plan_digest)
        || release_version(&evidence.release_id).is_none()
        || !is_sha256(&evidence.target_store_identity_digest)
        || !is_sha256(&evidence.store_observation_digest)
        || !is_sha256(&evidence.reconciliation_input_digest)
        || evidence.observed_at_unix_ms == 0
        || !valid_evidence_reference(&evidence.reviewed_by)
        || !valid_evidence_reference(&evidence.authority_evidence_ref)
        || !valid_evidence_reference(&evidence.identity_evidence_ref)
        || !valid_evidence_reference(&evidence.outbox_evidence_ref)
        || !evidence.single_authoritative_deployment
        || !evidence.identity_and_enrollment_continuity_verified
        || !evidence.outbox_reconciled
        || evidence.reconciled_managed_services != reconciled_services_from_store(&evidence.store)
        || evidence.store_observation_digest != sha256(&serde_json::to_vec(&evidence.store)?)
        || validate_store_observation(&evidence.store).is_err()
    {
        bail!("Console reconciliation evidence is invalid");
    }
    Ok(Some(evidence))
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

fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)
        .with_context(|| format!("open file for digest calculation {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read file for digest calculation {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size
            .checked_add(u64::try_from(read).context("digest chunk length exceeds u64")?)
            .context("file length exceeds u64")?;
    }
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in hasher.finalize() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok((encoded, size))
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

    #[derive(Debug)]
    struct RecordingStoreBackupAdapter {
        payload: &'static [u8],
        fail: bool,
    }

    #[derive(Debug)]
    struct RecordingRestoreAdapter {
        calls: RefCell<Vec<&'static str>>,
        clean: bool,
        fail_on: Option<&'static str>,
    }

    impl RestoreAdapter for RecordingRestoreAdapter {
        fn verify_payload(&self, _identity_file: &Path, payload: &Path) -> Result<()> {
            assert!(payload.is_file());
            self.calls.borrow_mut().push("preflight");
            if self.fail_on == Some("preflight") {
                bail!("simulated preflight failure");
            }
            Ok(())
        }

        fn target_is_clean(&self, database_url: &str) -> Result<bool> {
            assert_eq!(database_url, "postgres://console:secret@restore/console");
            self.calls.borrow_mut().push("clean");
            Ok(self.clean)
        }

        fn fence(&self, _root: &Path, _env_file: &Path, _compose_file: &Path) -> Result<()> {
            self.calls.borrow_mut().push("fence");
            if self.fail_on == Some("fence") {
                bail!("simulated fence failure");
            }
            Ok(())
        }

        fn restore_store(
            &self,
            database_url: &str,
            _identity_file: &Path,
            payload: &Path,
        ) -> Result<()> {
            assert_eq!(database_url, "postgres://console:secret@restore/console");
            assert!(payload.is_file());
            self.calls.borrow_mut().push("restore");
            if self.fail_on == Some("restore") {
                bail!("simulated restore failure");
            }
            Ok(())
        }

        fn start_recovery(
            &self,
            _root: &Path,
            _env_file: &Path,
            _compose_file: &Path,
        ) -> Result<()> {
            self.calls.borrow_mut().push("start");
            if self.fail_on == Some("start") {
                bail!("simulated recovery start failure");
            }
            Ok(())
        }
    }

    impl StoreBackupAdapter for RecordingStoreBackupAdapter {
        fn export_encrypted(
            &self,
            database_url: &str,
            recipient: &str,
            output: &Path,
        ) -> Result<()> {
            assert_eq!(database_url, "postgres://console:secret@db/console");
            assert_eq!(recipient, "age1recipient");
            if self.fail {
                bail!("simulated Store export failure");
            }
            fs::write(output, self.payload).context("write fake encrypted payload")
        }
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

    fn recovery_state() -> RecoveryState {
        RecoveryState {
            schema: RECOVERY_STATE_SCHEMA.to_owned(),
            recovery_set_id: "rcv_018f3f6a-7b8c-7def-8123-456789abcdef".to_owned(),
            recovery_set_digest: digest('6'),
            plan_digest: digest('7'),
            release_id: "lenso-console@0.1.0".to_owned(),
            target_store_identity_digest: digest('a'),
            status: RecoveryStatus::AwaitingReconciliation,
            phase: "reconciliation".to_owned(),
            reconciliation_evidence_digest: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: 2,
        }
    }

    fn store_recovery_observation() -> StoreRecoveryObservation {
        StoreRecoveryObservation {
            session_count: 0,
            stale_session_count: 0,
            outbox_status_counts: BTreeMap::from([
                ("pending".to_owned(), 2),
                ("published".to_owned(), 5),
            ]),
            outbox_snapshot_digest: digest('b'),
            managed_services: vec![ObservedManagedService {
                service_id: "support".to_owned(),
                service_principal: "service:support".to_owned(),
                enrollment_receipt_digest: digest('8'),
                enrollment_grant_revision: 3,
                authorization_epoch: 4,
                enrollment_state: "active".to_owned(),
                version: 6,
                core_document_digest: Some(digest('9')),
            }],
        }
    }

    fn reconciliation_input() -> ReconciliationInput {
        ReconciliationInput {
            schema: RECONCILIATION_INPUT_SCHEMA.to_owned(),
            recovery_set_id: recovery_state().recovery_set_id,
            observed_at_unix_ms: 3,
            reviewed_by: "operator:alice".to_owned(),
            authority_evidence_ref: "change:console-dr-42".to_owned(),
            identity_evidence_ref: "audit:identity-continuity-42".to_owned(),
            outbox_evidence_ref: "audit:outbox-reconciliation-42".to_owned(),
            single_authoritative_deployment: true,
            identity_and_enrollment_continuity_verified: true,
            outbox_reconciled: true,
            managed_services: vec![ReconciledManagedService {
                service_id: "support".to_owned(),
                service_principal: "service:support".to_owned(),
                enrollment_receipt_digest: digest('8'),
                authorization_epoch: 4,
                core_document_digest: Some(digest('9')),
            }],
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
    fn backup_atomically_binds_encrypted_store_to_installed_evidence() {
        assert!(RECOVERY_PG_DUMP_ARGS.contains(&"--exclude-table-data=auth.sessions"));
        let root = test_root("recovery-set");
        let output = root.join("backup");
        fs::create_dir_all(&root).unwrap();
        let installed = state();
        let manifest = backup_with(
            &RecordingStoreBackupAdapter {
                payload: b"age-encrypted-store",
                fail: false,
            },
            &installed,
            "postgres://console:secret@db/console",
            "age1recipient",
            &output,
        )
        .unwrap();

        assert_eq!(manifest.schema, RECOVERY_SET_SCHEMA);
        assert_eq!(manifest.release_digest, installed.release_digest);
        assert_eq!(
            manifest.store.payload_digest,
            sha256(b"age-encrypted-store")
        );
        assert!(manifest.store.encrypted);
        assert_eq!(manifest.store.excluded_data, ["auth.sessions"]);
        assert_eq!(
            manifest.recovery_set_digest,
            recovery_set_digest(&manifest).unwrap()
        );
        assert_eq!(
            fs::read(output.join(RECOVERY_STORE_FILE)).unwrap(),
            b"age-encrypted-store"
        );
        let recorded: RecoverySetManifest =
            serde_json::from_slice(&fs::read(output.join(RECOVERY_MANIFEST_FILE)).unwrap())
                .unwrap();
        assert_eq!(recorded.recovery_set_digest, manifest.recovery_set_digest);
        assert!(
            recorded
                .restore_preconditions
                .contains(&"clean_store".to_owned())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_refuses_overwrite_and_cleans_failed_staging() {
        let root = test_root("recovery-set-failure");
        let output = root.join("backup");
        fs::create_dir_all(&output).unwrap();
        let adapter = RecordingStoreBackupAdapter {
            payload: b"unused",
            fail: false,
        };
        assert!(
            backup_with(
                &adapter,
                &state(),
                "postgres://console:secret@db/console",
                "age1recipient",
                &output,
            )
            .is_err()
        );
        fs::remove_dir(&output).unwrap();

        let failing = RecordingStoreBackupAdapter {
            payload: b"unused",
            fail: true,
        };
        assert!(
            backup_with(
                &failing,
                &state(),
                "postgres://console:secret@db/console",
                "age1recipient",
                &output,
            )
            .is_err()
        );
        assert!(!output.exists());
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_plan_binds_verified_recovery_set_and_is_deterministic() {
        let root = test_root("restore-plan");
        let recovery_root = root.join("recovery");
        fs::create_dir_all(&root).unwrap();
        let installed = state();
        let manifest = backup_with(
            &RecordingStoreBackupAdapter {
                payload: b"age-encrypted-store",
                fail: false,
            },
            &installed,
            "postgres://console:secret@db/console",
            "age1recipient",
            &recovery_root,
        )
        .unwrap();
        let verified = read_recovery_set(&recovery_root).unwrap();
        assert_eq!(verified.recovery_set_digest, manifest.recovery_set_digest);
        validate_recovery_set_for_state(&verified, &installed).unwrap();
        let mut plan = build_restore_plan(&root, &recovery_root, &verified, &digest('a'));
        plan.plan_digest = restore_plan_digest(&plan).unwrap();
        assert!(is_sha256(&plan.plan_digest));
        assert_eq!(plan.steps[0], "verify_decryption_identity_and_archive");
        assert_eq!(plan.target_store_identity_digest, digest('a'));
        assert!(
            plan.approval_boundaries
                .contains(&"disaster_recovery_restore".to_owned())
        );

        fs::write(recovery_root.join(RECOVERY_STORE_FILE), b"tampered").unwrap();
        assert!(read_recovery_set(&recovery_root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_requires_distinct_store_and_explicit_recovery_modes() {
        let root = test_root("restore-environments");
        fs::create_dir_all(&root).unwrap();
        let current = root.join("current.env");
        let recovery = root.join("recovery.env");
        fs::write(
            &current,
            "CONSOLE_RECOVERY_MODE=normal\nCONSOLE_DATABASE_URL=postgres://console:old@primary/console\n",
        )
        .unwrap();
        fs::write(
            &recovery,
            "CONSOLE_RECOVERY_MODE=restore\nCONSOLE_DATABASE_URL=postgres://console:new@restore/console\n",
        )
        .unwrap();
        validate_restore_environments(&current, &recovery).unwrap();
        assert_eq!(
            database_identity_digest("postgres://console:old@restore/console").unwrap(),
            database_identity_digest("postgres://console:new@restore/console").unwrap()
        );

        fs::write(
            &recovery,
            "CONSOLE_RECOVERY_MODE=restore\nCONSOLE_DATABASE_URL=postgres://other:new@primary/console\n",
        )
        .unwrap();
        assert!(validate_restore_environments(&current, &recovery).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restore_preflights_before_fencing_and_remains_awaiting_reconciliation() {
        let root = test_root("restore-apply");
        let recovery_root = root.join("recovery");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(COMPOSE_FILE), "compose").unwrap();
        let installed = state();
        let manifest = backup_with(
            &RecordingStoreBackupAdapter {
                payload: b"age-encrypted-store",
                fail: false,
            },
            &installed,
            "postgres://console:secret@db/console",
            "age1recipient",
            &recovery_root,
        )
        .unwrap();
        let mut plan = build_restore_plan(&root, &recovery_root, &manifest, &digest('a'));
        plan.plan_digest = restore_plan_digest(&plan).unwrap();
        let adapter = RecordingRestoreAdapter {
            calls: RefCell::new(Vec::new()),
            clean: true,
            fail_on: None,
        };
        apply_restore_with(
            &adapter,
            &root,
            Path::new("current.env"),
            Path::new("recovery.env"),
            "postgres://console:secret@restore/console",
            &digest('a'),
            Path::new("identity.txt"),
            &recovery_root.join(RECOVERY_STORE_FILE),
            &manifest,
            &plan,
        )
        .unwrap();
        assert_eq!(
            *adapter.calls.borrow(),
            ["preflight", "clean", "fence", "restore", "start"]
        );
        let recovery = read_recovery_state_optional(&root).unwrap().unwrap();
        assert_eq!(recovery.status, RecoveryStatus::AwaitingReconciliation);
        assert_eq!(recovery.phase, "reconciliation");

        let invalid_archive = RecordingRestoreAdapter {
            calls: RefCell::new(Vec::new()),
            clean: true,
            fail_on: Some("preflight"),
        };
        assert!(
            apply_restore_with(
                &invalid_archive,
                &root,
                Path::new("current.env"),
                Path::new("recovery.env"),
                "postgres://console:secret@restore/console",
                &digest('a'),
                Path::new("identity.txt"),
                &recovery_root.join(RECOVERY_STORE_FILE),
                &manifest,
                &plan,
            )
            .is_err()
        );
        assert_eq!(*invalid_archive.calls.borrow(), ["preflight"]);

        let blocked = RecordingRestoreAdapter {
            calls: RefCell::new(Vec::new()),
            clean: false,
            fail_on: None,
        };
        assert!(
            apply_restore_with(
                &blocked,
                &root,
                Path::new("current.env"),
                Path::new("recovery.env"),
                "postgres://console:secret@restore/console",
                &digest('a'),
                Path::new("identity.txt"),
                &recovery_root.join(RECOVERY_STORE_FILE),
                &manifest,
                &plan,
            )
            .is_err()
        );
        assert_eq!(*blocked.calls.borrow(), ["preflight", "clean"]);
        assert_eq!(
            read_recovery_state_optional(&root).unwrap().unwrap().status,
            RecoveryStatus::Failed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconciliation_binds_store_and_external_evidence_before_activation() {
        let root = test_root("reconciliation");
        fs::create_dir_all(&root).unwrap();
        let recovery = recovery_state();
        let input = reconciliation_input();
        let store = store_recovery_observation();
        validate_reconciliation_input(&input, &recovery).unwrap();
        validate_store_observation(&store).unwrap();
        validate_reconciliation_services(&input, &store).unwrap();

        let store_digest = sha256(&serde_json::to_vec(&store).unwrap());
        let input_digest = sha256(&serde_json::to_vec(&input).unwrap());
        let mut plan = build_reconciliation_plan(&recovery, &store_digest, &input_digest, 3);
        plan.plan_digest = reconciliation_plan_digest(&plan).unwrap();
        assert!(is_sha256(&plan.plan_digest));
        assert_eq!(plan.approval_boundaries, ["recovery_reconciliation"]);

        let evidence = build_reconciliation_evidence(
            &recovery,
            &plan,
            &input,
            store,
            store_digest,
            input_digest,
        )
        .unwrap();
        publish_reconciliation_evidence(&root, &evidence).unwrap();
        write_reconciled_recovery_state(&root, &recovery, &evidence).unwrap();

        let recorded = read_reconciliation_evidence_optional(&root)
            .unwrap()
            .unwrap();
        assert_eq!(recorded.evidence_digest, evidence.evidence_digest);
        let state = read_recovery_state_optional(&root).unwrap().unwrap();
        assert_eq!(state.status, RecoveryStatus::ReadyForActivation);
        assert_eq!(state.phase, "activation_approval");
        assert_eq!(
            state.reconciliation_evidence_digest.as_deref(),
            Some(evidence.evidence_digest.as_str())
        );
        let mut checks = Vec::new();
        check_recovery_state(&root, &mut checks);
        assert!(matches!(checks[0].status, CheckStatus::Fail));
        assert!(checks[0].detail.contains("separate activation approval"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconciliation_rejects_sessions_and_managed_service_identity_drift() {
        let recovery = recovery_state();
        let mut input = reconciliation_input();
        let mut store = store_recovery_observation();
        store.session_count = 1;
        store.stale_session_count = 1;
        assert!(validate_store_observation(&store).is_err());

        store.session_count = 0;
        store.stale_session_count = 0;
        input.managed_services[0].authorization_epoch += 1;
        assert!(validate_reconciliation_services(&input, &store).is_err());

        input = reconciliation_input();
        input.single_authoritative_deployment = false;
        assert!(validate_reconciliation_input(&input, &recovery).is_err());
    }

    #[test]
    fn recovery_plan_output_cannot_overwrite_inputs_or_owned_state() {
        let root = test_root("protected-plan-output");
        let recovery_set = root.join("recovery-set");
        let env_file = root.with_extension("env");
        assert!(
            validate_plan_output_path(
                Some(&root.join("recovery-state.json")),
                std::slice::from_ref(&env_file),
                &[root.clone(), recovery_set.clone()],
            )
            .is_err()
        );
        assert!(
            validate_plan_output_path(
                Some(&env_file),
                std::slice::from_ref(&env_file),
                &[root.clone(), recovery_set],
            )
            .is_err()
        );
        assert!(
            validate_plan_output_path(
                Some(&root.with_extension("plan.json")),
                &[env_file],
                &[root],
            )
            .is_ok()
        );
    }

    #[test]
    fn console_database_url_is_read_without_logging_or_ambiguity() {
        let root = test_root("backup-env");
        fs::create_dir_all(&root).unwrap();
        let env = root.join("console.env");
        fs::write(
            &env,
            "# operator secrets\nexport CONSOLE_DATABASE_URL='postgresql://console:secret@db/console'\n",
        )
        .unwrap();
        assert_eq!(
            console_database_url(&env).unwrap(),
            "postgresql://console:secret@db/console"
        );
        fs::write(
            &env,
            "CONSOLE_DATABASE_URL=postgres://console:a@db/console\nCONSOLE_DATABASE_URL=postgres://console:b@db/console\n",
        )
        .unwrap();
        assert!(console_database_url(&env).is_err());
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
        assert!(document.contains("CONSOLE_RECOVERY_MODE: ${CONSOLE_RECOVERY_MODE:-normal}"));
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
