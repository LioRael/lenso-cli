use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use lenso_contracts::{
    ConsoleUiArtifact, LinkedModuleDelivery, MODULE_RELEASE_PROTOCOL,
    ModuleCompatibilityDeclaration, ModuleDelivery, ModuleManifest, ModuleRelease, digest_json,
};
use lenso_module_management::ConsoleCompositionArtifact;
use lenso_service::system_plane::{
    CORE_PROTOCOL, CoreDocument, Ed25519EnrollmentSigner, Ed25519EnrollmentTrustStore,
    EnrollmentCapabilityGrant, EnrollmentOffer, EnrollmentPolicyGrant, EnrollmentReceipt,
    EnrollmentSignature, EnrollmentSignatureAlgorithm, enrollment_offer_digest,
    enrollment_receipt_digest, sign_enrollment_offer, sign_enrollment_receipt,
    verify_enrollment_offer, verify_enrollment_receipt,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{console_connection, console_dev, console_operator, host, module, service};

const CONSOLE_SERVICE_ID: &str = "lenso-console";
const CONSOLE_SERVICE_PRINCIPAL: &str = "service:lenso-console";
const LOCAL_SYSTEM_PLANE_PROTOCOL: &str = "lenso.local-system-plane.v1";
const LOCAL_CONSOLE_STATE_PROTOCOL: &str = "lenso.local-console-state.v1";
const AUTH_TEMPLATE_PROTOCOL: &str = "lenso.console-artifact-template.v1";
const CONSOLE_KEY_ID: &str = "console-local-1";

#[derive(Debug, Clone)]
pub(crate) struct DevConsoleOptions {
    pub(crate) console_root: PathBuf,
    pub(crate) module_services_file: Option<PathBuf>,
    pub(crate) no_workspace: bool,
    pub(crate) operator_identifier: String,
    pub(crate) operator_password_file: Option<PathBuf>,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) separate_worker: bool,
    pub(crate) skip_db: bool,
    pub(crate) skip_migrate: bool,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedEnrollment {
    base_url: String,
    bearer_token: String,
    core: CoreDocument,
    offer: EnrollmentOffer,
    receipt: EnrollmentReceipt,
    receipt_digest: String,
    service_key_id: String,
    service_public_key: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalConsoleState {
    protocol: String,
    system_id: String,
    console_secret_key: String,
    enrollments: BTreeMap<String, ManagedEnrollment>,
}

#[derive(Debug)]
struct BuiltAuthRelease {
    digest: String,
    artifact: ConsoleCompositionArtifact,
    surface_api: SurfaceApiTemplate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthReleaseTemplate {
    protocol: String,
    module_id: String,
    version: String,
    manifest: Value,
    artifact_path: String,
    artifact: AuthArtifactTemplate,
    delivery: AuthDeliveryTemplate,
    compatibility: AuthCompatibilityTemplate,
    surface_api: SurfaceApiTemplate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthArtifactTemplate {
    digest: String,
    format: String,
    protocol_major: u32,
    entry: String,
    entries: Value,
    style_assets: Value,
    manifest: Value,
    requested_permissions: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthDeliveryTemplate {
    kind: String,
    package: String,
    crate_version: String,
    default_features: bool,
    features: Vec<String>,
    binding: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthCompatibilityTemplate {
    lenso_requirement: Option<String>,
    host_api_requirement: Option<String>,
    console_ui_requirement: Option<String>,
    rust_requirement: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceApiTemplate {
    contract_digest: String,
    operation_ids: Vec<String>,
    contract_artifact: SurfaceContractArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceContractArtifact {
    format: String,
    document: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemTopology {
    protocol: String,
    system_id: String,
    services: Vec<SystemTopologyService>,
    modules: Vec<SystemTopologyModule>,
    adapters: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemTopologyService {
    service_id: String,
    service_principal: String,
    revision: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    workloads: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemTopologyModule {
    module_id: String,
    delivery: String,
    service_id: Option<String>,
    module_release_digest: String,
    console_ui_artifact_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    surface_api_grant: Option<SurfaceApiGrant>,
    runtime_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SurfaceApiGrant {
    artifact_digest: String,
    module_release_digest: String,
    contract_digest: String,
    operation_ids: Vec<String>,
    contract_artifact: SurfaceContractArtifact,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagementBinding {
    system_id: String,
    topology_digest: String,
    service_ids: Vec<String>,
    adapter_ids: Vec<String>,
    permissions: Vec<String>,
    policy: ManagementPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagementPolicy {
    policy_id: String,
    revision: u64,
    digest: String,
}

pub(crate) async fn dev_up(options: DevConsoleOptions) -> Result<()> {
    let mut started = Vec::new();
    let result = dev_up_inner(options, &mut started).await;
    let cleanup = module::stop_started_module_services(&mut started).await;
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => {
            eprintln!("Local Service cleanup also failed: {cleanup:#}");
            Err(error)
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(()), cleanup) => cleanup,
    }
}

async fn dev_up_inner(
    options: DevConsoleOptions,
    started: &mut Vec<module::StartedModuleService>,
) -> Result<()> {
    let repo_root = absolute_existing(options.repo_root.as_deref().unwrap_or(Path::new(".")))?;
    let console_root = absolute_existing(&options.console_root)?;
    let host_env = host::ensure_local_environment(&repo_root)?;
    let system_id = local_system_id(&repo_root)?;
    let host_service_id = host::dotenv_value(&repo_root, "SERVICE_NAME")
        .context("Host SERVICE_NAME is missing from .env")?;
    let host_base_url = host::serve_base_url(&repo_root);
    let (mut local_state, console_signer) = local_console_state(&repo_root, &system_id)?;
    let host_core = CoreDocument {
        protocol: CORE_PROTOCOL.to_owned(),
        service_id: host_service_id.clone(),
        service_principal: format!("service:{host_service_id}"),
        service_revision: "1".to_owned(),
        capabilities: Vec::new(),
    };
    let host_bearer_token = local_state
        .enrollments
        .get(&host_service_id)
        .map(|enrollment| enrollment.bearer_token.clone())
        .map_or_else(random_token, Ok)?;
    let host_enrollment = stable_enrollment(
        &mut local_state,
        &system_id,
        host_base_url.clone(),
        host_bearer_token,
        host_core,
        &console_signer,
        "host-local-1",
    )?;
    write_local_console_state(&repo_root, &local_state)?;
    write_host_system_plane_config(&repo_root, &host_env, &host_enrollment)?;

    let workspace_enrollments = if options.no_workspace {
        Vec::new()
    } else {
        service::prepare_local_workspace_enrollments(&repo_root, options.workspace_file.as_deref())?
    };
    if !options.no_workspace {
        service::start_service_workspace_services_tracked(
            &repo_root,
            options.workspace_file.as_deref(),
            started,
        )
        .await?;
    }
    module::start_declared_module_services_tracked(
        Some(&repo_root),
        options.module_services_file.as_deref(),
        started,
    )
    .await?;
    let discovered = service::discover_local_workspace_services(&workspace_enrollments).await?;
    let mut managed = vec![host_enrollment];
    let mut providers = Vec::new();
    for discovered in discovered {
        let core: CoreDocument = serde_json::from_value(discovered.core)
            .context("decode discovered Service System Plane Core")?;
        let service_key_id = local_service_key_id(&core.service_id);
        providers.push(discovered.provider);
        managed.push(stable_enrollment(
            &mut local_state,
            &system_id,
            discovered.base_url,
            discovered.bearer_token,
            core,
            &console_signer,
            &service_key_id,
        )?);
    }
    write_local_console_state(&repo_root, &local_state)?;

    let mut host_process = host::start(
        Some(&repo_root),
        options.skip_db,
        options.skip_migrate,
        options.separate_worker,
    )
    .await?;
    wait_for_http(
        &format!("{host_base_url}/readyz"),
        "Host readiness",
        Duration::from_secs(600),
    )
    .await?;
    host_process.check()?;
    let auth_template = fetch_auth_release_template(&host_base_url).await?;
    let auth_release = build_auth_release(&repo_root, &host_base_url, auth_template)?;
    let trust = enrollment_trust(&system_id, &console_signer, &managed);
    let environment = BTreeMap::from([(
        "LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST".to_owned(),
        serde_json::to_string(&trust).context("encode local Console enrollment trust")?,
    )]);
    let console_runtime = console_dev::prepare_console_dev(Some(&console_root), &environment)?;
    let mut console_process = console_dev::spawn_console_dev(&console_runtime)?;
    wait_for_http(
        &format!("{}health/ready", console_runtime.url),
        "Console readiness",
        Duration::from_secs(120),
    )
    .await?;
    console_process.check()?;

    let operator =
        console_operator::ensure_local_operator(console_operator::LocalOperatorOptions {
            console_root: console_root.clone(),
            console_url: console_runtime.url.clone(),
            env_file: console_runtime.env_file.clone(),
            identifier: options.operator_identifier,
            password_file: options.operator_password_file,
        })
        .await?;
    console_process.stop();
    console_process = console_dev::spawn_console_dev(&console_runtime)?;
    wait_for_http(
        &format!("{}health/ready", console_runtime.url),
        "restarted Console readiness",
        Duration::from_secs(120),
    )
    .await?;

    let bundle = build_connect_bundle(
        &repo_root,
        &system_id,
        &host_service_id,
        &managed,
        &auth_release,
        &providers,
    )?;
    let state_dir = repo_root.join(".lenso");
    fs::create_dir_all(&state_dir).context("create local Lenso state directory")?;
    let token_file = state_dir.join("console-operator-token");
    write_private(&token_file, operator.token.as_bytes())?;
    let bundle_file = state_dir.join("console-connect.json");
    write_private(
        &bundle_file,
        serde_json::to_vec_pretty(&bundle)
            .context("encode generated Console connection bundle")?
            .as_slice(),
    )?;
    console_connection::connect(console_connection::ConnectOptions {
        bundle: bundle_file,
        console_url: console_runtime.url.clone(),
        json: false,
        token_env: "LENSO_CONSOLE_TOKEN".to_owned(),
        token_file: Some(token_file),
    })
    .await?;

    eprintln!();
    eprintln!("Lenso local System is ready");
    eprintln!("  Host:     {host_base_url}");
    eprintln!("  Console:  {}", console_runtime.url.trim_end_matches('/'));
    eprintln!("  System:   {system_id}");
    eprintln!("  Operator: {}", operator.user_id);
    eprintln!();
    eprintln!("Press Ctrl-C to stop Host, Console, and Services started by this command.");
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for Ctrl-C")?;
                return Ok(());
            }
            () = tokio::time::sleep(Duration::from_millis(500)) => {
                host_process.check()?;
                console_process.check()?;
                module::check_started_module_services(started)?;
            }
        }
    }
}

fn stable_enrollment(
    state: &mut LocalConsoleState,
    system_id: &str,
    base_url: String,
    bearer_token: String,
    core: CoreDocument,
    console_signer: &Ed25519EnrollmentSigner,
    service_key_id: &str,
) -> Result<ManagedEnrollment> {
    if let Some(existing) = state.enrollments.get(&core.service_id) {
        validate_stored_enrollment(
            existing,
            system_id,
            &base_url,
            &bearer_token,
            &core,
            console_signer,
        )?;
        return Ok(existing.clone());
    }
    let enrollment = signed_enrollment(
        system_id,
        base_url,
        bearer_token,
        core,
        console_signer,
        service_key_id,
    )?;
    state
        .enrollments
        .insert(enrollment.core.service_id.clone(), enrollment.clone());
    Ok(enrollment)
}

fn signed_enrollment(
    system_id: &str,
    base_url: String,
    bearer_token: String,
    core: CoreDocument,
    console_signer: &Ed25519EnrollmentSigner,
    service_key_id: &str,
) -> Result<ManagedEnrollment> {
    let service_signer = random_signer(service_key_id)?;
    let issued_at_unix_ms = now_unix_ms()?;
    let expires_at_unix_ms = issued_at_unix_ms + 365 * 24 * 60 * 60 * 1_000;
    let capabilities = core
        .capabilities
        .iter()
        .map(|capability| EnrollmentCapabilityGrant {
            contract_id: capability.contract_id.clone(),
            schema_digest: capability.schema_digest.clone(),
            feature_ids: capability.feature_ids.clone(),
        })
        .collect::<Vec<_>>();
    let policy = local_enrollment_policy(system_id)?;
    let nonce = random_token()?;
    let offer = sign_enrollment_offer(
        EnrollmentOffer {
            protocol: String::new(),
            system_id: system_id.to_owned(),
            console_service_principal: CONSOLE_SERVICE_PRINCIPAL.to_owned(),
            nonce: nonce.clone(),
            issued_at_unix_ms,
            expires_at_unix_ms,
            requested_capabilities: capabilities.clone(),
            requested_policy: policy.clone(),
            signature: placeholder_signature("console-local-1"),
        },
        console_signer,
    )
    .map_err(|issues| anyhow!("sign local Enrollment Offer: {issues:?}"))?;
    let offer_digest = enrollment_offer_digest(&offer);
    let receipt = sign_enrollment_receipt(
        EnrollmentReceipt {
            protocol: String::new(),
            offer_digest,
            system_id: system_id.to_owned(),
            managed_service_id: core.service_id.clone(),
            managed_service_principal: core.service_principal.clone(),
            managed_service_revision: core.service_revision.clone(),
            console_service_principal: CONSOLE_SERVICE_PRINCIPAL.to_owned(),
            nonce,
            issued_at_unix_ms,
            expires_at_unix_ms,
            grant_revision: 1,
            authorization_epoch: 1,
            granted_capabilities: capabilities,
            granted_policy: policy,
            signature: placeholder_signature(service_key_id),
        },
        &service_signer,
    )
    .map_err(|issues| anyhow!("sign local Enrollment Receipt: {issues:?}"))?;
    let receipt_digest = enrollment_receipt_digest(&receipt);
    Ok(ManagedEnrollment {
        base_url,
        bearer_token,
        core,
        offer,
        receipt,
        receipt_digest,
        service_key_id: service_key_id.to_owned(),
        service_public_key: URL_SAFE_NO_PAD.encode(service_signer.verifying_key_bytes()),
    })
}

fn validate_stored_enrollment(
    enrollment: &ManagedEnrollment,
    system_id: &str,
    base_url: &str,
    bearer_token: &str,
    core: &CoreDocument,
    console_signer: &Ed25519EnrollmentSigner,
) -> Result<()> {
    let unchanged = enrollment.base_url == base_url
        && enrollment.bearer_token == bearer_token
        && serde_json::to_value(&enrollment.core)? == serde_json::to_value(core)?
        && enrollment.receipt.system_id == system_id
        && enrollment.receipt_digest == enrollment_receipt_digest(&enrollment.receipt);
    if !unchanged {
        bail!(
            "stored local enrollment for {} no longer matches its Service identity or endpoint",
            core.service_id
        );
    }
    let console_trust =
        Ed25519EnrollmentTrustStore::new([(CONSOLE_KEY_ID, console_signer.verifying_key_bytes())])
            .map_err(|error| anyhow!("restore local Console enrollment trust: {error}"))?;
    let service_key = decode_key(&enrollment.service_public_key, "managed Service public key")?;
    let service_trust =
        Ed25519EnrollmentTrustStore::new([(enrollment.service_key_id.as_str(), service_key)])
            .map_err(|error| anyhow!("restore local Service enrollment trust: {error}"))?;
    let now = now_unix_ms()?;
    verify_enrollment_offer(&enrollment.offer, &console_trust, now)
        .map_err(|issues| anyhow!("stored local Enrollment Offer is invalid: {issues:?}"))?;
    verify_enrollment_receipt(&enrollment.receipt, &enrollment.offer, &service_trust, now)
        .map_err(|issues| anyhow!("stored local Enrollment Receipt is invalid: {issues:?}"))?;
    Ok(())
}

fn local_console_state(
    repo_root: &Path,
    system_id: &str,
) -> Result<(LocalConsoleState, Ed25519EnrollmentSigner)> {
    let path = repo_root.join(".lenso/local-console-state.json");
    if path.exists() {
        let state: LocalConsoleState = serde_json::from_slice(&read_private(&path)?)
            .with_context(|| format!("decode local Console state {}", path.display()))?;
        if state.protocol != LOCAL_CONSOLE_STATE_PROTOCOL || state.system_id != system_id {
            bail!("local Console state does not belong to System `{system_id}`");
        }
        let secret = decode_key(&state.console_secret_key, "Console signing key")?;
        let signer = Ed25519EnrollmentSigner::new(CONSOLE_KEY_ID, secret)
            .map_err(|error| anyhow!("restore local Console signer: {error}"))?;
        return Ok((state, signer));
    }

    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret)
        .map_err(|error| anyhow!("generate Console signing key: {error}"))?;
    let signer = Ed25519EnrollmentSigner::new(CONSOLE_KEY_ID, secret)
        .map_err(|error| anyhow!("create local Console signer: {error}"))?;
    Ok((
        LocalConsoleState {
            protocol: LOCAL_CONSOLE_STATE_PROTOCOL.to_owned(),
            system_id: system_id.to_owned(),
            console_secret_key: URL_SAFE_NO_PAD.encode(secret),
            enrollments: BTreeMap::new(),
        },
        signer,
    ))
}

fn write_local_console_state(repo_root: &Path, state: &LocalConsoleState) -> Result<()> {
    write_private(
        &repo_root.join(".lenso/local-console-state.json"),
        &serde_json::to_vec_pretty(state).context("encode local Console state")?,
    )
}

fn read_private(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect private local state {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("private local state must be a regular file and not a symbolic link");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("private local state must not be accessible by group or others");
        }
    }
    fs::read(path).with_context(|| format!("read private local state {}", path.display()))
}

fn decode_key(value: &str, label: &str) -> Result<[u8; 32]> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .with_context(|| format!("decode {label}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("{label} must contain exactly 32 bytes"))
}

fn local_service_key_id(service_id: &str) -> String {
    format!(
        "service-local-{}",
        sha256_bytes(service_id.as_bytes())
            .trim_start_matches("sha256:")
            .chars()
            .take(16)
            .collect::<String>()
    )
}

fn placeholder_signature(key_id: &str) -> EnrollmentSignature {
    EnrollmentSignature {
        algorithm: EnrollmentSignatureAlgorithm::Ed25519,
        key_id: key_id.to_owned(),
        subject_digest: format!("sha256:{}", "0".repeat(64)),
        value: "pending".to_owned(),
    }
}

fn random_signer(key_id: &str) -> Result<Ed25519EnrollmentSigner> {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).map_err(|error| anyhow!("generate enrollment key: {error}"))?;
    Ed25519EnrollmentSigner::new(key_id, secret)
        .map_err(|error| anyhow!("create enrollment signer: {error}"))
}

fn random_token() -> Result<String> {
    let mut value = [0_u8; 32];
    getrandom::fill(&mut value).map_err(|error| anyhow!("generate local token: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

fn local_enrollment_policy(system_id: &str) -> Result<EnrollmentPolicyGrant> {
    let policy_id = format!("{system_id}-local-console");
    let policy_revision = "1".to_owned();
    let policy_digest = digest_json(&json!({
        "policyId": policy_id,
        "policyRevision": policy_revision,
        "permissions": [
            "console.module.business.read",
            "console.module.business.write"
        ]
    }))?;
    Ok(EnrollmentPolicyGrant {
        policy_id,
        policy_revision,
        policy_digest,
    })
}

fn write_host_system_plane_config(
    repo_root: &Path,
    env_file: &Path,
    enrollment: &ManagedEnrollment,
) -> Result<()> {
    let state_dir = repo_root.join(".lenso");
    fs::create_dir_all(&state_dir).context("create local Lenso state directory")?;
    let path = state_dir.join("local-system-plane.json");
    let receipt = &enrollment.receipt;
    let config = json!({
        "protocol": LOCAL_SYSTEM_PLANE_PROTOCOL,
        "bearerToken": enrollment.bearer_token,
        "enrollment": {
            "systemId": receipt.system_id,
            "managedServiceId": receipt.managed_service_id,
            "managedServicePrincipal": receipt.managed_service_principal,
            "managedServiceRevision": receipt.managed_service_revision,
            "consoleServicePrincipal": receipt.console_service_principal,
            "offerDigest": receipt.offer_digest,
            "receiptDigest": enrollment.receipt_digest,
            "grantRevision": receipt.grant_revision,
            "authorizationEpoch": receipt.authorization_epoch,
            "expiresAtUnixMs": receipt.expires_at_unix_ms,
            "capabilities": receipt.granted_capabilities,
            "policy": receipt.granted_policy,
        }
    });
    write_private(&path, &serde_json::to_vec_pretty(&config)?)?;
    let source = fs::read_to_string(env_file)
        .with_context(|| format!("read Host environment {}", env_file.display()))?;
    let updated = upsert_env(
        &source,
        "LENSO_LOCAL_SYSTEM_PLANE_CONFIG",
        ".lenso/local-system-plane.json",
    );
    write_private(env_file, updated.as_bytes())
}

fn enrollment_trust(
    system_id: &str,
    console_signer: &Ed25519EnrollmentSigner,
    enrollments: &[ManagedEnrollment],
) -> Value {
    json!({
        "consoleAuthorityKeys": [{
            "keyId": CONSOLE_KEY_ID,
            "publicKeyBase64url": URL_SAFE_NO_PAD.encode(console_signer.verifying_key_bytes()),
            "consoleServicePrincipal": CONSOLE_SERVICE_PRINCIPAL,
        }],
        "managedServiceKeys": enrollments.iter().map(|enrollment| json!({
            "keyId": enrollment.service_key_id,
            "publicKeyBase64url": enrollment.service_public_key,
            "systemId": system_id,
            "managedServiceId": enrollment.core.service_id,
            "managedServicePrincipal": enrollment.core.service_principal,
            "baseUrl": enrollment.base_url,
            "systemPlaneBearerToken": enrollment.bearer_token,
        })).collect::<Vec<_>>()
    })
}

async fn fetch_auth_release_template(host_base_url: &str) -> Result<AuthReleaseTemplate> {
    let url = format!("{host_base_url}/v1/auth/console/release");
    let template = reqwest::get(&url)
        .await
        .with_context(|| format!("fetch Auth Console release template from {url}"))?
        .error_for_status()
        .context("Host Auth Module does not expose its Console artifact release")?
        .json::<AuthReleaseTemplate>()
        .await
        .context("decode Auth Console release template")?;
    if template.protocol != AUTH_TEMPLATE_PROTOCOL {
        bail!("Auth Console release template protocol is unsupported");
    }
    Ok(template)
}

fn build_auth_release(
    repo_root: &Path,
    host_base_url: &str,
    template: AuthReleaseTemplate,
) -> Result<BuiltAuthRelease> {
    if template.module_id != "lenso/auth"
        || template.delivery.kind != "linked"
        || template.version != template.delivery.crate_version
    {
        bail!("Auth Console release template identity is inconsistent");
    }
    let (locked_version, archive_checksum) =
        cargo_lock_package(&repo_root.join("Cargo.lock"), &template.delivery.package)?;
    if locked_version != template.delivery.crate_version {
        bail!(
            "Host locks {}@{}, but its Auth Console artifact declares {}",
            template.delivery.package,
            locked_version,
            template.delivery.crate_version
        );
    }
    if sha256_bytes(template.surface_api.contract_artifact.document.as_bytes())
        != template.surface_api.contract_digest
    {
        bail!("Auth Surface API contract digest does not match its exact document");
    }
    let manifest: ModuleManifest =
        serde_json::from_value(template.manifest).context("decode Auth Module Manifest")?;
    let artifact_locator = format!(
        "{}{}",
        host_base_url.trim_end_matches('/'),
        template.artifact_path
    );
    let console_ui_artifact: ConsoleUiArtifact = serde_json::from_value(json!({
        "artifact": {
            "locator": artifact_locator,
            "digest": template.artifact.digest,
        },
        "format": template.artifact.format,
        "protocolMajor": template.artifact.protocol_major,
        "entry": template.artifact.entry,
        "entries": template.artifact.entries,
        "styleAssets": template.artifact.style_assets,
        "manifest": template.artifact.manifest,
        "requestedPermissions": template.artifact.requested_permissions,
        "provenance": [],
    }))
    .context("decode Auth Console UI artifact")?;
    let manifest_digest = digest_json(&manifest).context("digest Auth Module Manifest")?;
    let release = ModuleRelease {
        protocol: MODULE_RELEASE_PROTOCOL.to_owned(),
        module_id: template.module_id,
        version: template.version,
        manifest,
        manifest_digest,
        delivery: ModuleDelivery::Linked(LinkedModuleDelivery {
            package: template.delivery.package,
            crate_version: template.delivery.crate_version,
            archive_checksum,
            default_features: template.delivery.default_features,
            features: template.delivery.features,
            binding: template.delivery.binding,
            attestations: Vec::new(),
            migrations: Vec::new(),
        }),
        console_ui_artifact: Some(console_ui_artifact.clone()),
        compatibility: ModuleCompatibilityDeclaration {
            lenso_requirement: template.compatibility.lenso_requirement,
            host_api_requirement: template.compatibility.host_api_requirement,
            console_ui_requirement: template.compatibility.console_ui_requirement,
            rust_requirement: template.compatibility.rust_requirement,
            targets: Vec::new(),
            transports: Vec::new(),
            protocol_digests: Vec::new(),
        },
        provenance: Vec::new(),
    };
    let issues = release.validate();
    if !issues.is_empty() {
        bail!("Auth Module Release is invalid: {issues:?}");
    }
    let release_digest = digest_json(&release)?;
    let artifact = ConsoleCompositionArtifact {
        module_id: release.module_id.clone(),
        module_release_digest: release_digest.clone(),
        locator: console_ui_artifact.artifact.locator.clone(),
        digest: console_ui_artifact.artifact.digest.clone(),
        format: console_ui_artifact.format,
        protocol_major: console_ui_artifact.protocol_major,
        entry: console_ui_artifact.entry,
        entries: console_ui_artifact.entries,
        style_assets: console_ui_artifact.style_assets,
        manifest: console_ui_artifact.manifest,
        requested_permissions: console_ui_artifact.requested_permissions,
    };
    Ok(BuiltAuthRelease {
        digest: release_digest,
        artifact,
        surface_api: template.surface_api,
    })
}

fn build_connect_bundle(
    repo_root: &Path,
    system_id: &str,
    host_service_id: &str,
    managed: &[ManagedEnrollment],
    auth_release: &BuiltAuthRelease,
    providers: &[Value],
) -> Result<Value> {
    let mut services = managed
        .iter()
        .map(|enrollment| SystemTopologyService {
            service_id: enrollment.core.service_id.clone(),
            service_principal: enrollment.core.service_principal.clone(),
            revision: enrollment.core.service_revision.parse().unwrap_or(1),
            workloads: Vec::new(),
        })
        .collect::<Vec<_>>();
    services.sort_by(|left, right| left.service_id.cmp(&right.service_id));
    let mut operations = auth_release.surface_api.operation_ids.clone();
    operations.sort();
    operations.dedup();
    if operations.is_empty() {
        bail!("Auth Surface API operation grant is empty");
    }
    let mut modules = vec![SystemTopologyModule {
        module_id: "lenso/auth".to_owned(),
        delivery: "linked".to_owned(),
        service_id: Some(host_service_id.to_owned()),
        module_release_digest: auth_release.digest.clone(),
        console_ui_artifact_digest: Some(auth_release.artifact.digest.clone()),
        surface_api_grant: Some(SurfaceApiGrant {
            artifact_digest: auth_release.artifact.digest.clone(),
            module_release_digest: auth_release.digest.clone(),
            contract_digest: auth_release.surface_api.contract_digest.clone(),
            operation_ids: operations,
            contract_artifact: auth_release.surface_api.contract_artifact.clone(),
        }),
        runtime_status: Some("active".to_owned()),
    }];
    let story_release_digest = digest_json(&json!({
        "delivery": "linked",
        "moduleId": "lenso/platform-story",
        "owner": CONSOLE_SERVICE_ID,
        "protocol": "lenso.local-linked-module.v1"
    }))?;
    modules.push(SystemTopologyModule {
        module_id: "lenso/platform-story".to_owned(),
        delivery: "linked".to_owned(),
        service_id: None,
        module_release_digest: story_release_digest,
        console_ui_artifact_digest: None,
        surface_api_grant: None,
        runtime_status: Some("active".to_owned()),
    });
    for provider in providers {
        let service_id = provider
            .get("serviceId")
            .and_then(Value::as_str)
            .context("Provider descriptor serviceId is required")?;
        let exports = provider
            .get("exports")
            .and_then(Value::as_array)
            .context("Provider descriptor exports are required")?;
        for export in exports {
            modules.push(SystemTopologyModule {
                module_id: export
                    .get("moduleId")
                    .and_then(Value::as_str)
                    .context("Provider export moduleId is required")?
                    .to_owned(),
                delivery: "service".to_owned(),
                service_id: Some(service_id.to_owned()),
                module_release_digest: export
                    .get("moduleReleaseDigest")
                    .and_then(Value::as_str)
                    .context("Provider export moduleReleaseDigest is required")?
                    .to_owned(),
                console_ui_artifact_digest: None,
                surface_api_grant: None,
                runtime_status: Some("active".to_owned()),
            });
        }
    }
    let topology = SystemTopology {
        protocol: "lenso.system.v2".to_owned(),
        system_id: system_id.to_owned(),
        services,
        modules: {
            modules.sort_by(|left, right| left.module_id.cmp(&right.module_id));
            modules
        },
        adapters: Vec::new(),
    };
    let topology_digest = topology_digest(&topology)?;
    let service_ids = topology
        .services
        .iter()
        .map(|service| service.service_id.clone())
        .collect::<Vec<_>>();
    let binding = ManagementBinding {
        system_id: system_id.to_owned(),
        topology_digest: topology_digest.clone(),
        service_ids,
        adapter_ids: Vec::new(),
        permissions: vec![
            "console.module.business.read".to_owned(),
            "console.module.business.write".to_owned(),
        ],
        policy: management_policy_from_enrollments(managed)?,
    };
    let lock_digest =
        sha256_bytes(&fs::read(repo_root.join("Cargo.lock")).context("read Host Cargo.lock")?);
    Ok(json!({
        "protocol": "lenso.console-connect.v1",
        "enrollmentReceipts": managed.iter().map(|enrollment| json!({
            "offer": enrollment.offer,
            "receipt": enrollment.receipt,
            "baseUrl": enrollment.base_url,
        })).collect::<Vec<_>>(),
        "artifactComposition": {
            "kind": "console_composition",
            "effect_id": "local-console-composition:lenso-console",
            "console_service_id": CONSOLE_SERVICE_ID,
            "candidate_lock_digest": lock_digest,
            "artifacts": [auth_release.artifact.clone()],
            "theme_bundles": [],
        },
        "systemConnection": {
            "systemId": system_id,
            "topologyDigest": topology_digest,
            "topology": topology,
            "managementBinding": binding,
        }
    }))
}

fn management_policy_from_enrollments(managed: &[ManagedEnrollment]) -> Result<ManagementPolicy> {
    let signed_policy = managed
        .first()
        .map(|enrollment| &enrollment.receipt.granted_policy)
        .context("local Console connection requires at least one signed enrollment")?;
    if managed.iter().any(|enrollment| {
        let policy = &enrollment.receipt.granted_policy;
        policy.policy_id != signed_policy.policy_id
            || policy.policy_revision != signed_policy.policy_revision
            || policy.policy_digest != signed_policy.policy_digest
    }) {
        bail!("local Service enrollments do not share one Management Policy");
    }
    let revision = signed_policy
        .policy_revision
        .parse::<u64>()
        .context("signed local Management Policy revision is not a positive integer")?;
    if revision == 0 || revision.to_string() != signed_policy.policy_revision {
        bail!("signed local Management Policy revision is not normalized");
    }
    Ok(ManagementPolicy {
        policy_id: signed_policy.policy_id.clone(),
        revision,
        digest: signed_policy.policy_digest.clone(),
    })
}

fn topology_digest(topology: &SystemTopology) -> Result<String> {
    let mut canonical = topology.clone();
    canonical
        .services
        .sort_by(|left, right| left.service_id.cmp(&right.service_id));
    canonical
        .modules
        .sort_by(|left, right| left.module_id.cmp(&right.module_id));
    Ok(sha256_bytes(&serde_json::to_vec(&canonical)?))
}

fn cargo_lock_package(path: &Path, package: &str) -> Result<(String, String)> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("read Host lockfile {}", path.display()))?;
    let matches = source
        .split("[[package]]")
        .skip(1)
        .filter(|block| lock_string_field(block, "name").as_deref() == Some(package))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!("Host Cargo.lock must contain exactly one `{package}` package");
    }
    let block = matches[0];
    let version =
        lock_string_field(block, "version").context("locked package version is missing")?;
    let checksum = lock_string_field(block, "checksum")
        .map(|checksum| format!("sha256:{checksum}"))
        .unwrap_or_else(|| sha256_bytes(block.as_bytes()));
    Ok((version, checksum))
}

fn lock_string_field(block: &str, field: &str) -> Option<String> {
    let prefix = format!("{field} = \"");
    block.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .and_then(|value| value.strip_suffix('"'))
            .map(ToOwned::to_owned)
    })
}

async fn wait_for_http(url: &str, label: &str, timeout: Duration) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("build local readiness client")?;
    let deadline = Instant::now() + timeout;
    loop {
        let last = match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => format!("HTTP {}", response.status()),
            Err(error) => error.to_string(),
        };
        if Instant::now() >= deadline {
            bail!("{label} timed out at {url}: {last}");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn local_system_id(repo_root: &Path) -> Result<String> {
    let value = repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .context("Host directory name is not valid UTF-8")?
        .to_ascii_lowercase()
        .replace('_', "-");
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("Host directory name must be a lowercase path identity");
    }
    Ok(value)
}

fn absolute_existing(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("resolve {}", path.display()))
}

fn now_unix_ms() -> Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?;
    u64::try_from(duration.as_millis()).context("system time does not fit u64 milliseconds")
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("sha256:{hex}")
}

fn upsert_env(source: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut found = false;
    let mut lines = source
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                format!("{prefix}{value}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>();
    if !found {
        lines.push(format!("{prefix}{value}"));
    }
    format!("{}\n", lines.join("\n"))
}

fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("protect {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_stable_local_system_identity_from_host_directory() {
        let root = Path::new("/tmp/lenso-taste");
        assert_eq!(local_system_id(root).unwrap(), "lenso-taste");
        assert!(local_system_id(Path::new("/tmp/Lenso Taste")).is_err());
    }

    #[test]
    fn upserts_environment_without_duplicate_keys() {
        assert_eq!(
            upsert_env(
                "A=1\nLENSO_LOCAL_SYSTEM_PLANE_CONFIG=old\n",
                "LENSO_LOCAL_SYSTEM_PLANE_CONFIG",
                ".lenso/local-system-plane.json"
            ),
            "A=1\nLENSO_LOCAL_SYSTEM_PLANE_CONFIG=.lenso/local-system-plane.json\n"
        );
        assert_eq!(upsert_env("A=1\n", "B", "2"), "A=1\nB=2\n");
    }

    #[test]
    fn reads_exact_registry_checksum_from_host_lockfile() {
        let root = std::env::temp_dir().join(format!(
            "lenso-console-local-lock-{}",
            now_unix_ms().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let lock = root.join("Cargo.lock");
        fs::write(
            &lock,
            "[[package]]\nname = \"lenso-auth-module\"\nversion = \"0.2.2\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"abc123\"\n",
        )
        .unwrap();

        assert_eq!(
            cargo_lock_package(&lock, "lenso-auth-module").unwrap(),
            ("0.2.2".to_owned(), "sha256:abc123".to_owned())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_and_reuses_exact_local_enrollment_evidence() {
        let root = std::env::temp_dir().join(format!(
            "lenso-console-local-state-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        fs::create_dir_all(&root).unwrap();
        let core = CoreDocument {
            protocol: CORE_PROTOCOL.to_owned(),
            service_id: "lenso-taste".to_owned(),
            service_principal: "service:lenso-taste".to_owned(),
            service_revision: "1".to_owned(),
            capabilities: Vec::new(),
        };
        let (mut state, signer) = local_console_state(&root, "lenso-taste").unwrap();
        let first = stable_enrollment(
            &mut state,
            "lenso-taste",
            "http://127.0.0.1:3000".to_owned(),
            "local-console-token-0123456789abcdef".to_owned(),
            core.clone(),
            &signer,
            "host-local-1",
        )
        .unwrap();
        write_local_console_state(&root, &state).unwrap();

        let (mut restored, restored_signer) = local_console_state(&root, "lenso-taste").unwrap();
        let second = stable_enrollment(
            &mut restored,
            "lenso-taste",
            "http://127.0.0.1:3000".to_owned(),
            "local-console-token-0123456789abcdef".to_owned(),
            core,
            &restored_signer,
            "host-local-1",
        )
        .unwrap();

        assert_eq!(first.receipt_digest, second.receipt_digest);
        assert_eq!(first.offer, second.offer);
        assert_eq!(first.receipt, second.receipt);
        let policy = management_policy_from_enrollments(&[first, second]).unwrap();
        assert_eq!(policy.policy_id, "lenso-taste-local-console");
        assert_eq!(policy.revision, 1);
        fs::remove_dir_all(root).unwrap();
    }
}
