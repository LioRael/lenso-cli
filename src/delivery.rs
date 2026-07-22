use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

#[cfg(test)]
const SERVICE_RELEASE_PROTOCOL: &str = "lenso.service-release.v1";
const POLICY_EVIDENCE_PROTOCOL: &str = "lenso.policy-evidence.v1";
const POLICY_EVALUATION_INPUT_PROTOCOL: &str = "lenso.policy-evaluation-input.v1";
const POLICY_PACK_PROTOCOL: &str = "lenso.policy-pack.v1";
const ELIGIBILITY_PROTOCOL: &str = "lenso.production-eligibility.v1";
#[cfg(test)]
#[allow(dead_code)]
const RELEASE_TRUST_EVIDENCE_PROTOCOL: &str = "lenso.release-trust-evidence.v1";
#[cfg(test)]
const DEPLOYMENT_PLAN_PROTOCOL: &str = "lenso.deployment-plan.v1";
#[cfg(test)]
const GATEWAY_PLAN_PROTOCOL: &str = "lenso.gateway-plan.v1";
const OPERATOR_EXPORT_PROTOCOL: &str = "lenso.operator-export.v1";
const OPERATOR_OBSERVATION_PROTOCOL: &str = "lenso.operator-observation.v1";
const PROMOTION_APPROVAL_PROTOCOL: &str = "lenso.promotion-approval.v1";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeliveryEvidenceReference {
    reference: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseModule {
    module_id: String,
    module_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReleaseWorkloadRole {
    Api,
    Worker,
    Migration,
    Extension,
}

impl ReleaseWorkloadRole {
    #[cfg(test)]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Migration => "migration",
            Self::Extension => "extension",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseProvenance {
    reference: String,
    digest: String,
    source: String,
    builder: String,
    #[serde(default)]
    input_digests: Vec<String>,
    #[serde(default)]
    subject_digests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadArtifact {
    workload_id: String,
    role: ReleaseWorkloadRole,
    artifact_reference: String,
    artifact_digest: String,
    media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_tag: Option<String>,
    sbom: DeliveryEvidenceReference,
    provenance: ReleaseProvenance,
    signature_subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseContractVersion {
    contract_id: String,
    version: String,
    kind: String,
    artifact: DeliveryEvidenceReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseMigration {
    migration_id: String,
    phase: String,
    artifact: DeliveryEvidenceReference,
    reversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseRolloutGate {
    gate_id: String,
    evidence_kind: String,
    required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseRollbackConstraints {
    previous_release_required: bool,
    automatic_allowed: bool,
    blocked_by_irreversible_migration: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseRetention {
    evidence_days: u32,
    artifact_days: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseSignature {
    signer: String,
    subject_digest: String,
    signature: String,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceReleaseInput {
    service_id: String,
    service_version: String,
    modules: Vec<ReleaseModule>,
    workloads: Vec<WorkloadArtifact>,
    contract_versions: Vec<ReleaseContractVersion>,
    config_contract: DeliveryEvidenceReference,
    reliability_contract: DeliveryEvidenceReference,
    migrations: Vec<ReleaseMigration>,
    workflow_compatibility: Vec<DeliveryEvidenceReference>,
    verification_evidence: Vec<DeliveryEvidenceReference>,
    rollout_gates: Vec<ReleaseRolloutGate>,
    rollback: ReleaseRollbackConstraints,
    retention: ReleaseRetention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceRelease {
    protocol: String,
    release_id: String,
    release_digest: String,
    service_id: String,
    service_version: String,
    modules: Vec<ReleaseModule>,
    workloads: Vec<WorkloadArtifact>,
    contract_versions: Vec<ReleaseContractVersion>,
    config_contract: DeliveryEvidenceReference,
    reliability_contract: DeliveryEvidenceReference,
    migrations: Vec<ReleaseMigration>,
    workflow_compatibility: Vec<DeliveryEvidenceReference>,
    verification_evidence: Vec<DeliveryEvidenceReference>,
    rollout_gates: Vec<ReleaseRolloutGate>,
    rollback: ReleaseRollbackConstraints,
    retention: ReleaseRetention,
    #[serde(default)]
    signatures: Vec<ReleaseSignature>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceReleaseContent<'a> {
    protocol: &'a str,
    service_id: &'a str,
    service_version: &'a str,
    modules: &'a [ReleaseModule],
    workloads: &'a [WorkloadArtifact],
    contract_versions: &'a [ReleaseContractVersion],
    config_contract: &'a DeliveryEvidenceReference,
    reliability_contract: &'a DeliveryEvidenceReference,
    migrations: &'a [ReleaseMigration],
    workflow_compatibility: &'a [DeliveryEvidenceReference],
    verification_evidence: &'a [DeliveryEvidenceReference],
    rollout_gates: &'a [ReleaseRolloutGate],
    rollback: ReleaseRollbackConstraints,
    retention: ReleaseRetention,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeliveryIssue {
    code: String,
    message: String,
    remediation: String,
    next_actions: Vec<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseDiffEntry {
    subject: String,
    before: Option<String>,
    after: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseDiff {
    protocol: &'static str,
    from_release_id: String,
    to_release_id: String,
    entries: Vec<ReleaseDiffEntry>,
    effects: Effects,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Effects {
    contacts_environment: bool,
    mutates_environment: bool,
    writes_ledger: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeploymentAdapterKind {
    Local,
    ExternallyManaged,
    Kubernetes,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentWorkloadSettings {
    workload_id: String,
    replicas: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    health_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disruption_min_available: Option<u32>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentWorkloadPlan {
    workload_id: String,
    role: ReleaseWorkloadRole,
    artifact_reference: String,
    artifact_digest: String,
    media_type: String,
    settings: DeploymentWorkloadSettings,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeliveryEffectsArtifact {
    mutates_environment: bool,
    mutates_configuration: bool,
    mutates_gateway: bool,
    mutates_deployment: bool,
    appends_ledger: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentPlanArtifact {
    protocol: String,
    plan_id: String,
    plan_digest: String,
    adapter: DeploymentAdapterKind,
    environment: String,
    expected_environment_revision: u64,
    release_id: String,
    release_digest: String,
    service_id: String,
    config_revision_id: String,
    secret_reference_ids: Vec<String>,
    endpoints: BTreeMap<String, String>,
    placement: BTreeMap<String, String>,
    workloads: Vec<DeploymentWorkloadPlan>,
    adapter_inputs: BTreeMap<String, String>,
    gateway_plan_digest: String,
    #[serde(default)]
    policy_evidence_references: Vec<String>,
    rollback_capable: bool,
    next_actions: Vec<String>,
    #[serde(default)]
    effects: DeliveryEffectsArtifact,
}

#[cfg(test)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeploymentPlanDigestInput<'a> {
    protocol: &'a str,
    adapter: DeploymentAdapterKind,
    environment: &'a str,
    expected_environment_revision: u64,
    release_id: &'a str,
    release_digest: &'a str,
    service_id: &'a str,
    config_revision_id: &'a str,
    secret_reference_ids: &'a [String],
    endpoints: &'a BTreeMap<String, String>,
    placement: &'a BTreeMap<String, String>,
    workloads: &'a [DeploymentWorkloadPlan],
    adapter_inputs: &'a BTreeMap<String, String>,
    gateway_plan_digest: &'a str,
    policy_evidence_references: &'a [String],
    rollback_capable: bool,
    next_actions: &'a [String],
    effects: &'a DeliveryEffectsArtifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PolicyDecision {
    Passed,
    Advisory,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PolicyEnvironmentProfile {
    Production,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PolicyRuleSeverity {
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyRule {
    rule_id: String,
    evidence_key: String,
    severity: PolicyRuleSeverity,
    advisory_in_development: bool,
    remediation: String,
    next_action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyRuleResult {
    rule_id: String,
    severity: PolicyRuleSeverity,
    decision: PolicyDecision,
    evidence_references: Vec<String>,
    remediation: String,
    next_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyDeliveryIssue {
    code: String,
    message: String,
    evidence_references: Vec<String>,
    remediation: String,
    next_actions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyEffects {
    mutates_environment: bool,
    mutates_configuration: bool,
    mutates_gateway: bool,
    mutates_deployment: bool,
    appends_ledger: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyEvidenceArtifact {
    protocol: String,
    evidence_id: String,
    evidence_digest: String,
    pack_id: String,
    pack_digest: String,
    evaluated_subject: String,
    input_digests: BTreeMap<String, String>,
    decision: PolicyDecision,
    rule_results: Vec<PolicyRuleResult>,
    issues: Vec<PolicyDeliveryIssue>,
    effects: PolicyEffects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyEvaluationInput {
    protocol: String,
    release: ServiceRelease,
    trust: ReleaseTrustEvidenceArtifact,
    config_contract: ConfigContractArtifact,
    config: ConfigRevisionArtifact,
    eligibility_input: Value,
    eligibility: EligibilityEvidenceArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReleaseSignerStatus {
    Trusted,
    Untrusted,
    Revoked,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadTrustEvidenceArtifact {
    workload_id: String,
    artifact_digest: String,
    sbom_reference: String,
    provenance_reference: String,
    provenance_subject_matches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignatureTrustEvidenceArtifact {
    signer: String,
    subject_digest: String,
    status: ReleaseSignerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseTrustEvidenceArtifact {
    protocol: String,
    release_id: String,
    release_digest: String,
    decision: PolicyDecision,
    evidence_digest: String,
    workloads: Vec<WorkloadTrustEvidenceArtifact>,
    signatures: Vec<SignatureTrustEvidenceArtifact>,
    issues: Vec<Value>,
    effects: PolicyEffects,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EligibilityEvidenceArtifact {
    protocol: String,
    evidence_id: String,
    evidence_digest: String,
    release_id: String,
    release_digest: String,
    provider_id: String,
    input_digest: String,
    system_graph_digest: String,
    decision: PolicyDecision,
    facts: BTreeMap<String, Option<bool>>,
    contract_retirement: Vec<Value>,
    issues: Vec<Value>,
    effects: PolicyEffects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigFieldScope {
    Service,
    Workload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigValueType {
    String,
    Integer,
    Number,
    Boolean,
    Object,
    Array,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigFieldSensitivity {
    Public,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigFieldActivation {
    Hot,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFieldArtifact {
    path: String,
    value_type: ConfigValueType,
    required: bool,
    sensitivity: ConfigFieldSensitivity,
    scope: ConfigFieldScope,
    activation: ConfigFieldActivation,
    mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigContractArtifact {
    protocol: String,
    reference: String,
    digest: String,
    fields: Vec<ConfigFieldArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SecretReferenceStatus {
    Resolved,
    Unresolved,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretReference {
    reference_id: String,
    provider: String,
    purpose: String,
    scope: String,
    status: SecretReferenceStatus,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustedSecretObservation {
    provider: String,
    status: SecretReferenceStatus,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustedAttestation {
    subject_digest: String,
    proof: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigImpact {
    path: String,
    scope: ConfigFieldScope,
    activation: ConfigFieldActivation,
    mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigRevisionArtifact {
    protocol: String,
    revision_id: String,
    revision_digest: String,
    service_id: String,
    contract_reference: String,
    contract_digest: String,
    values: BTreeMap<String, Value>,
    secret_references: Vec<SecretReference>,
    impacts: Vec<ConfigImpact>,
}

#[derive(Serialize)]
struct PolicyEvidenceContent<'a> {
    protocol: &'a str,
    pack_id: &'a str,
    pack_digest: &'a str,
    evaluated_subject: &'a str,
    input_digests: &'a BTreeMap<String, String>,
    decision: PolicyDecision,
    rule_results: &'a [PolicyRuleResult],
    issues: &'a [PolicyDeliveryIssue],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentVerificationDigestInput<'a> {
    protocol: &'a str,
    environment: &'a str,
    environment_revision: u64,
    release_id: &'a str,
    release_digest: &'a str,
    workload_digests: &'a Value,
    workload_health: &'a Value,
    config_revision_id: &'a str,
    trust_evidence_digest: &'a str,
    policy_evidence_id: &'a str,
    policy_evidence_digest: &'a str,
    deployment_plan_id: &'a str,
    deployment_plan_digest: &'a str,
    deployment_receipt_id: &'a str,
    deployment_observation_id: &'a str,
    operator_observation_id: &'a str,
    operator_observation_digest: &'a str,
    operator_observation_authority_id: &'a str,
    operator_observation_authority_proof: &'a str,
    operator_observation_claims: &'a Value,
    gateway_plan_id: &'a str,
    gateway_plan_digest: &'a str,
    gateway_observation_id: &'a str,
    gateway_resource_uid: &'a str,
    gateway_resource_version: &'a str,
    gateway_authority_context: &'a str,
    gateway_configuration_identity: &'a str,
    gateway_observation_revision: u64,
    gateway_observation_observed_after: &'a str,
    gateway_observation_fresh: bool,
    gateway_observation_provider_id: &'a str,
    gateway_observation_provider_proof: &'a str,
    topology_digest: &'a str,
    evidence_references: &'a Value,
    freshness_horizon_revision: u64,
    decision: &'a Value,
    issues: &'a Value,
}

pub(crate) fn assemble_release(input: &Path, output: Option<&Path>) -> Result<()> {
    let input: lenso_service::ServiceReleaseInput = read_json(input)?;
    let release = lenso_service::assemble_service_release(input).map_err(|issues| {
        anyhow::anyhow!("{}", serde_json::to_string(&issues).unwrap_or_default())
    })?;
    write_json(&release, output)
}

pub(crate) fn check_release(artifact: &Path, output: Option<&Path>) -> Result<()> {
    let release: lenso_service::ServiceRelease = read_json(artifact)?;
    if !lenso_service::service_release_integrity_is_valid(&release) {
        bail!("release_tampered: Service Release identity or immutable content is invalid")
    }
    write_json(&release, output)
}

pub(crate) fn diff_releases(from: &Path, to: &Path, output: Option<&Path>) -> Result<()> {
    let from: lenso_service::ServiceRelease = read_json(from)?;
    let to: lenso_service::ServiceRelease = read_json(to)?;
    if !lenso_service::service_release_integrity_is_valid(&from)
        || !lenso_service::service_release_integrity_is_valid(&to)
    {
        bail!("release_tampered: Service Release identity or immutable content is invalid")
    }
    write_json(&lenso_service::diff_service_releases(&from, &to), output)
}

pub(crate) fn check_policy_evidence(artifact: &Path, output: Option<&Path>) -> Result<()> {
    let trusted_signatures = trusted_release_signatures()?;
    let trusted_eligibility = trusted_eligibility_attestations()?;
    let trusted_secrets = trusted_secret_observations()?;
    check_policy_evidence_with_trust(
        artifact,
        output,
        &trusted_signatures,
        &trusted_eligibility,
        &trusted_secrets,
    )
}

fn check_policy_evidence_with_trust(
    artifact: &Path,
    output: Option<&Path>,
    trusted_signatures: &BTreeMap<String, String>,
    trusted_eligibility: &BTreeMap<String, String>,
    trusted_secrets: &BTreeMap<String, TrustedSecretObservation>,
) -> Result<()> {
    let value: Value = read_json(artifact)?;
    if value["protocol"] == POLICY_EVALUATION_INPUT_PROTOCOL {
        ensure_secret_free(&value)?;
        let input: PolicyEvaluationInput = serde_json::from_value(value)
            .context("policy_input_invalid: canonical Policy evaluation input is malformed")?;
        let evidence = evaluate_policy_input(
            &input,
            trusted_signatures,
            trusted_eligibility,
            trusted_secrets,
        )?;
        let blocked = evidence.decision == PolicyDecision::Blocked;
        write_json(&evidence, output)?;
        if blocked {
            bail!(
                "policy_rule_blocked: production Policy Evidence is blocked; inspect ruleResults and nextActions"
            )
        }
        return Ok(());
    }
    check_protocol(&value, POLICY_EVIDENCE_PROTOCOL)?;
    ensure_secret_free(&value)?;
    require_fields(
        &value,
        &[
            "evidenceId",
            "evidenceDigest",
            "packId",
            "evaluatedSubject",
            "decision",
        ],
    )?;
    let evidence: PolicyEvidenceArtifact = serde_json::from_value(value)
        .context("policy_input_invalid: Policy Evidence shape is malformed")?;
    if !policy_evidence_integrity_is_valid(&evidence) {
        bail!("policy_input_invalid: Policy Evidence identity or digest is invalid")
    }
    bail!(
        "policy_source_required: precomputed Policy Evidence cannot prove its facts; provide lenso.policy-evaluation-input.v1 so the CLI can evaluate the canonical sources"
    )
}

fn evaluate_policy_input(
    input: &PolicyEvaluationInput,
    trusted_signatures: &BTreeMap<String, String>,
    trusted_eligibility: &BTreeMap<String, String>,
    trusted_secrets: &BTreeMap<String, TrustedSecretObservation>,
) -> Result<PolicyEvidenceArtifact> {
    if input.protocol != POLICY_EVALUATION_INPUT_PROTOCOL {
        bail!("policy_input_invalid: unsupported Policy evaluation input protocol")
    }
    let mut trust_keys = trusted_signatures.clone();
    trust_keys.extend(trusted_eligibility.clone());
    let trust_provider = lenso_service::DeterministicTrustProvider::new(trust_keys);
    let secret_provider_name = input
        .config
        .secret_references
        .first()
        .map(|reference| reference.provider.clone())
        .or_else(|| {
            trusted_secrets
                .values()
                .next()
                .map(|observation| observation.provider.clone())
        })
        .unwrap_or_else(|| "cli-trusted-secret-provider".to_owned());
    let secret_observations = trusted_secrets
        .iter()
        .map(|(reference_id, observation)| {
            serde_json::from_value(
                serde_json::to_value(observation).expect("observation serializes"),
            )
            .map(|observation| (reference_id.clone(), observation))
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("secret_provider_unavailable: trusted Secret observations are malformed")?;
    let secret_provider =
        lenso_service::DeterministicSecretProvider::new(secret_provider_name, secret_observations);
    let inputs = lenso_service::DeliveryPolicyInputs {
        release: shared_contract(serde_json::to_value(&input.release)?)?,
        trust: shared_contract(serde_json::to_value(&input.trust)?)?,
        config_contract: shared_contract(serde_json::to_value(&input.config_contract)?)?,
        config: shared_contract(serde_json::to_value(&input.config)?)?,
        eligibility: shared_contract(serde_json::to_value(&input.eligibility)?)?,
        eligibility_input: shared_contract(input.eligibility_input.clone())?,
    };
    let evidence = lenso_service::evaluate_delivery_policy(
        &lenso_service::production_policy_pack(),
        &inputs,
        &trust_provider,
        &secret_provider,
        lenso_service::PolicyEvaluationSurface::Cli,
    );
    serde_json::from_value(serde_json::to_value(evidence)?)
        .context("policy_input_invalid: shared Policy Evidence is not CLI-serializable")
}

fn shared_contract<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .context("policy_input_invalid: input diverges from the shared delivery contract")
}

fn trusted_release_signatures() -> Result<BTreeMap<String, String>> {
    let value = std::env::var("LENSO_TRUSTED_RELEASE_SIGNATURES")
        .context("trust_provider_unavailable: LENSO_TRUSTED_RELEASE_SIGNATURES is required")?;
    serde_json::from_str(&value).context(
        "trust_provider_unavailable: LENSO_TRUSTED_RELEASE_SIGNATURES must be a JSON signer-to-key map",
    )
}

fn trusted_eligibility_attestations() -> Result<BTreeMap<String, String>> {
    let value = std::env::var("LENSO_TRUSTED_ELIGIBILITY_ATTESTATIONS").context(
        "eligibility_provider_unavailable: LENSO_TRUSTED_ELIGIBILITY_ATTESTATIONS is required",
    )?;
    serde_json::from_str(&value).context(
        "eligibility_provider_unavailable: LENSO_TRUSTED_ELIGIBILITY_ATTESTATIONS must be a JSON provider-to-key map",
    )
}

fn trusted_secret_observations() -> Result<BTreeMap<String, TrustedSecretObservation>> {
    let value = std::env::var("LENSO_TRUSTED_SECRET_OBSERVATIONS")
        .context("secret_provider_unavailable: LENSO_TRUSTED_SECRET_OBSERVATIONS is required")?;
    serde_json::from_str(&value).context(
        "secret_provider_unavailable: LENSO_TRUSTED_SECRET_OBSERVATIONS must be a JSON reference-to-observation map",
    )
}

fn trusted_edge_attestations() -> Result<BTreeMap<String, String>> {
    let value = std::env::var("LENSO_TRUSTED_EDGE_ATTESTATIONS")
        .context("edge_provider_unavailable: LENSO_TRUSTED_EDGE_ATTESTATIONS is required")?;
    serde_json::from_str(&value).context(
        "edge_provider_unavailable: LENSO_TRUSTED_EDGE_ATTESTATIONS must be a JSON provider-to-key map",
    )
}

fn trusted_operator_observation_authorities() -> Result<BTreeMap<String, String>> {
    let value = std::env::var("LENSO_TRUSTED_OPERATOR_OBSERVATION_AUTHORITIES").context(
        "operator_observation_authority_unavailable: LENSO_TRUSTED_OPERATOR_OBSERVATION_AUTHORITIES is required",
    )?;
    serde_json::from_str(&value).context(
        "operator_observation_authority_unavailable: LENSO_TRUSTED_OPERATOR_OBSERVATION_AUTHORITIES must be a JSON authority-to-key map",
    )
}

fn trusted_gateway_observation_authorities() -> Result<BTreeMap<String, String>> {
    let value = std::env::var("LENSO_TRUSTED_GATEWAY_OBSERVATION_AUTHORITIES").context(
        "gateway_observation_authority_unavailable: LENSO_TRUSTED_GATEWAY_OBSERVATION_AUTHORITIES is required",
    )?;
    serde_json::from_str(&value).context(
        "gateway_observation_authority_unavailable: LENSO_TRUSTED_GATEWAY_OBSERVATION_AUTHORITIES must be a JSON authority-to-key map",
    )
}

#[cfg(test)]
#[allow(dead_code)]
fn release_trust_evidence_integrity_is_valid(
    evidence: &ReleaseTrustEvidenceArtifact,
    release: &ServiceRelease,
    trusted_signatures: &BTreeMap<String, String>,
) -> bool {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Content<'a> {
        protocol: &'a str,
        release_id: &'a str,
        release_digest: &'a str,
        workloads: &'a [WorkloadTrustEvidenceArtifact],
        signatures: &'a [SignatureTrustEvidenceArtifact],
        issues: &'a [Value],
    }
    let expected_workloads = release
        .workloads
        .iter()
        .map(|workload| WorkloadTrustEvidenceArtifact {
            workload_id: workload.workload_id.clone(),
            artifact_digest: workload.artifact_digest.clone(),
            sbom_reference: workload.sbom.reference.clone(),
            provenance_reference: workload.provenance.reference.clone(),
            provenance_subject_matches: workload
                .provenance
                .subject_digests
                .contains(&workload.artifact_digest),
        })
        .collect::<Vec<_>>();
    let signature_subjects_match = evidence.signatures.len() == release.signatures.len()
        && evidence
            .signatures
            .iter()
            .zip(&release.signatures)
            .all(|(observed, declared)| {
                observed.signer == declared.signer
                    && observed.subject_digest == declared.subject_digest
                    && observed.subject_digest == release.release_digest
                    && trusted_signatures.get(&declared.signer) == Some(&declared.signature)
                    && observed.status == ReleaseSignerStatus::Trusted
            });
    let expected_decision = if evidence.issues.is_empty()
        && evidence
            .workloads
            .iter()
            .all(|item| item.provenance_subject_matches)
        && !evidence.signatures.is_empty()
        && evidence
            .signatures
            .iter()
            .all(|item| item.status == ReleaseSignerStatus::Trusted)
    {
        PolicyDecision::Passed
    } else {
        PolicyDecision::Blocked
    };
    let content = Content {
        protocol: evidence.protocol.as_str(),
        release_id: evidence.release_id.as_str(),
        release_digest: evidence.release_digest.as_str(),
        workloads: &evidence.workloads,
        signatures: &evidence.signatures,
        issues: &evidence.issues,
    };
    release_issues(release).is_empty()
        && evidence.protocol == RELEASE_TRUST_EVIDENCE_PROTOCOL
        && evidence.release_id == release.release_id
        && evidence.release_digest == release.release_digest
        && evidence.workloads == expected_workloads
        && signature_subjects_match
        && evidence.decision == expected_decision
        && evidence.effects == PolicyEffects::default()
        && digest_json(&content) == evidence.evidence_digest
}

#[cfg(test)]
#[allow(dead_code)]
fn eligibility_evidence_integrity_is_valid(
    evidence: &EligibilityEvidenceArtifact,
    input: &Value,
    release: &ServiceRelease,
    trusted_eligibility: &BTreeMap<String, TrustedAttestation>,
) -> bool {
    #[derive(Serialize)]
    struct Content<'a> {
        protocol: &'a str,
        release_id: &'a str,
        release_digest: &'a str,
        provider_id: &'a str,
        input_digest: &'a str,
        system_graph_digest: &'a str,
        facts: &'a BTreeMap<String, Option<bool>>,
        contract_retirement: &'a [Value],
        issues: &'a [Value],
    }
    let expected_decision = if evidence.issues.is_empty()
        && evidence.facts.get("production.eligible") == Some(&Some(true))
    {
        PolicyDecision::Passed
    } else {
        PolicyDecision::Blocked
    };
    let content = Content {
        protocol: evidence.protocol.as_str(),
        release_id: evidence.release_id.as_str(),
        release_digest: evidence.release_digest.as_str(),
        provider_id: evidence.provider_id.as_str(),
        input_digest: evidence.input_digest.as_str(),
        system_graph_digest: evidence.system_graph_digest.as_str(),
        facts: &evidence.facts,
        contract_retirement: &evidence.contract_retirement,
        issues: &evidence.issues,
    };
    evidence.protocol == ELIGIBILITY_PROTOCOL
        && !evidence.system_graph_digest.trim().is_empty()
        && input["releaseId"].as_str() == Some(release.release_id.as_str())
        && input["releaseDigest"].as_str() == Some(release.release_digest.as_str())
        && evidence.release_id == release.release_id
        && evidence.release_digest == release.release_digest
        && input["providerId"].as_str() == Some(evidence.provider_id.as_str())
        && eligibility_input_digest(input).is_some_and(|subject_digest| {
            subject_digest == evidence.input_digest
                && input["providerProof"].as_str().is_some_and(|proof| {
                    trusted_eligibility
                        .get(evidence.provider_id.as_str())
                        .is_some_and(|attestation| {
                            attestation.subject_digest == subject_digest
                                && attestation.proof == proof
                        })
                })
        })
        && input["systemGraphDigest"].as_str() == Some(evidence.system_graph_digest.as_str())
        && eligibility_facts_from_input(input, release).is_some_and(|facts| facts == evidence.facts)
        && evidence.evidence_id == format!("production-eligibility:{}", evidence.evidence_digest)
        && digest_json(&content) == evidence.evidence_digest
        && evidence.decision == expected_decision
        && evidence.effects == PolicyEffects::default()
}

#[cfg(test)]
#[allow(dead_code)]
fn eligibility_facts_from_input(
    input: &Value,
    release: &ServiceRelease,
) -> Option<BTreeMap<String, Option<bool>>> {
    let contracts = input["contracts"].as_array()?;
    let release_contracts = release
        .contract_versions
        .iter()
        .map(|contract| contract.contract_id.as_str())
        .collect::<BTreeSet<_>>();
    let input_contracts = contracts
        .iter()
        .filter_map(|contract| contract["contractId"].as_str())
        .collect::<BTreeSet<_>>();
    if input_contracts.len() != contracts.len()
        || release_contracts.len() != release.contract_versions.len()
        || input_contracts != release_contracts
        || contracts.iter().any(|candidate| {
            !release.contract_versions.iter().any(|declared| {
                candidate["contractId"].as_str() == Some(declared.contract_id.as_str())
                    && candidate["candidateMajor"].as_u64()
                        == declared
                            .version
                            .trim_start_matches('v')
                            .split('.')
                            .next()
                            .and_then(|value| value.parse::<u64>().ok())
            })
        })
    {
        return None;
    }
    let contracts_safe = contracts.iter().all(|contract| {
        contract["compatible"].as_bool().is_some_and(|value| {
            value
                || (contract["candidateMajor"].as_u64().unwrap_or_default()
                    > contract["currentMajor"].as_u64().unwrap_or_default()
                    && contract["consumerMigrationEvidence"].as_bool() == Some(true))
        })
    });
    let retirements_ready = contracts.iter().all(|contract| {
        contract["retiring"].as_bool() != Some(true)
            || (contract["activeConsumers"]
                .as_array()
                .is_some_and(Vec::is_empty)
                && contract["deprecationWindowComplete"].as_bool() == Some(true))
    });
    let migrations = input["migrations"].as_array()?;
    let release_migrations = release
        .migrations
        .iter()
        .map(|migration| migration.migration_id.as_str())
        .collect::<BTreeSet<_>>();
    let input_migrations = migrations
        .iter()
        .filter_map(|migration| migration["migrationId"].as_str())
        .collect::<BTreeSet<_>>();
    if input_migrations.len() != migrations.len()
        || release_migrations.len() != release.migrations.len()
        || input_migrations != release_migrations
        || migrations.iter().any(|candidate| {
            !release.migrations.iter().any(|declared| {
                candidate["migrationId"].as_str() == Some(declared.migration_id.as_str())
                    && candidate["phase"].as_str() == Some(declared.phase.as_str())
                    && (declared.reversible || candidate["phase"] != "irreversible")
            })
        })
    {
        return None;
    }
    let irreversible = migrations
        .iter()
        .any(|migration| migration["phase"] == "irreversible");
    let mut lineages = BTreeMap::<&str, Vec<&Value>>::new();
    for migration in migrations {
        lineages
            .entry(migration["lineageId"].as_str().unwrap_or_default())
            .or_default()
            .push(migration);
    }
    let migrations_safe = lineages.iter().all(|(lineage, entries)| {
        let sequences = entries
            .iter()
            .filter_map(|entry| entry["sequence"].as_u64())
            .collect::<BTreeSet<_>>();
        !lineage.is_empty()
            && sequences.len() == entries.len()
            && !sequences.contains(&0)
            && entries.iter().all(|entry| {
                let sequence = entry["sequence"].as_u64().unwrap_or_default();
                let verified = entry["verified"].as_bool() == Some(true);
                let ordered_contract = entry["phase"] != "contract"
                    || (entries.iter().any(|candidate| {
                        candidate["phase"] == "expand"
                            && candidate["verified"].as_bool() == Some(true)
                            && candidate["sequence"].as_u64().unwrap_or_default() < sequence
                    }) && entries.iter().any(|candidate| {
                        candidate["phase"] == "verify"
                            && candidate["verified"].as_bool() == Some(true)
                            && candidate["sequence"].as_u64().unwrap_or_default() < sequence
                    }));
                !entry["migrationId"].as_str().unwrap_or_default().is_empty()
                    && verified
                    && ordered_contract
            })
    });
    let all_true =
        |value: &Value, keys: &[&str]| keys.iter().all(|key| value[*key].as_bool() == Some(true));
    let workflows_safe = all_true(
        &input["workflows"],
        &["newStartsCompatible", "inFlightCompatible", "downgradeSafe"],
    );
    let rollback_safe = !irreversible
        && all_true(
            &input["rollback"],
            &[
                "priorReleaseCompatible",
                "schemaCompatible",
                "workflowCompatible",
                "configCompatible",
                "secretReferencesCompatible",
                "edgeCompatible",
                "adapterCapable",
            ],
        );
    let mut facts = BTreeMap::from([
        ("contracts.compatible".to_owned(), Some(contracts_safe)),
        ("migrations.safe".to_owned(), Some(migrations_safe)),
        ("workflows.compatible".to_owned(), Some(workflows_safe)),
        ("rollback.safe".to_owned(), Some(rollback_safe)),
    ]);
    for (fact, field) in [
        ("providers.compatible", "providerCompatibilityVerified"),
        ("identity.production", "workloadIdentityProduction"),
        ("tenancy.mode.production", "tenancyModeProduction"),
        ("tenancy.enforced", "tenantContextEnforced"),
        ("call_policies.declared", "callPoliciesDeclared"),
        ("dependencies.ready", "dependenciesReady"),
        ("resilience.declared", "resilienceDeclared"),
        ("reliability.complete", "reliabilityContractComplete"),
        ("edge.valid", "edgeContractValid"),
        (
            "environment.verification_fresh",
            "environmentVerificationFresh",
        ),
    ] {
        facts.insert(fact.to_owned(), input[field].as_bool());
    }
    facts.insert(
        "production.eligible".to_owned(),
        Some(retirements_ready && facts.values().all(|value| *value == Some(true))),
    );
    Some(facts)
}

#[cfg(test)]
#[allow(dead_code)]
fn eligibility_input_digest(input: &Value) -> Option<String> {
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ContractInput {
        contract_id: String,
        current_major: u32,
        candidate_major: u32,
        compatible: Option<bool>,
        active_consumers: Vec<String>,
        consumer_migration_evidence: bool,
        retiring: bool,
        deprecation_window_complete: bool,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MigrationInput {
        migration_id: String,
        lineage_id: String,
        sequence: u32,
        phase: Value,
        verified: bool,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct WorkflowInput {
        new_starts_compatible: Option<bool>,
        in_flight_compatible: Option<bool>,
        downgrade_safe: Option<bool>,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RollbackInput {
        prior_release_compatible: Option<bool>,
        schema_compatible: Option<bool>,
        workflow_compatible: Option<bool>,
        config_compatible: Option<bool>,
        secret_references_compatible: Option<bool>,
        edge_compatible: Option<bool>,
        adapter_capable: Option<bool>,
        previous_release_id: String,
        previous_release_digest: String,
        previous_deployment_plan_id: String,
        previous_deployment_plan_digest: String,
        previous_config_revision_id: String,
        previous_config_revision_digest: String,
        previous_secret_reference_ids: Vec<String>,
        previous_gateway_plan_id: String,
        previous_gateway_plan_digest: String,
        previous_gateway_configuration_identity: String,
        previous_adapter: String,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Content<'a> {
        release_id: &'a str,
        release_digest: &'a str,
        provider_id: &'a str,
        system_graph_digest: &'a str,
        contracts: &'a [ContractInput],
        migrations: &'a [MigrationInput],
        workflows: &'a WorkflowInput,
        rollback: &'a RollbackInput,
        provider_compatibility_verified: Option<bool>,
        workload_identity_production: Option<bool>,
        tenancy_mode_production: Option<bool>,
        tenant_context_enforced: Option<bool>,
        call_policies_declared: Option<bool>,
        dependencies_ready: Option<bool>,
        resilience_declared: Option<bool>,
        reliability_contract_complete: Option<bool>,
        edge_contract_valid: Option<bool>,
        environment_verification_fresh: Option<bool>,
    }
    let contracts =
        serde_json::from_value::<Vec<ContractInput>>(input["contracts"].clone()).ok()?;
    let migrations =
        serde_json::from_value::<Vec<MigrationInput>>(input["migrations"].clone()).ok()?;
    let workflows = serde_json::from_value::<WorkflowInput>(input["workflows"].clone()).ok()?;
    let rollback = serde_json::from_value::<RollbackInput>(input["rollback"].clone()).ok()?;
    Some(digest_json(&Content {
        release_id: input["releaseId"].as_str()?,
        release_digest: input["releaseDigest"].as_str()?,
        provider_id: input["providerId"].as_str()?,
        system_graph_digest: input["systemGraphDigest"].as_str()?,
        contracts: &contracts,
        migrations: &migrations,
        workflows: &workflows,
        rollback: &rollback,
        provider_compatibility_verified: input["providerCompatibilityVerified"].as_bool(),
        workload_identity_production: input["workloadIdentityProduction"].as_bool(),
        tenancy_mode_production: input["tenancyModeProduction"].as_bool(),
        tenant_context_enforced: input["tenantContextEnforced"].as_bool(),
        call_policies_declared: input["callPoliciesDeclared"].as_bool(),
        dependencies_ready: input["dependenciesReady"].as_bool(),
        resilience_declared: input["resilienceDeclared"].as_bool(),
        reliability_contract_complete: input["reliabilityContractComplete"].as_bool(),
        edge_contract_valid: input["edgeContractValid"].as_bool(),
        environment_verification_fresh: input["environmentVerificationFresh"].as_bool(),
    }))
}

fn canonical_production_policy_rules() -> Vec<PolicyRule> {
    [
        "release.integrity",
        "supply_chain.trusted",
        "config.valid",
        "contracts.compatible",
        "migrations.safe",
        "workflows.compatible",
        "rollback.safe",
        "providers.compatible",
        "identity.production",
        "tenancy.mode.production",
        "tenancy.enforced",
        "call_policies.declared",
        "dependencies.ready",
        "resilience.declared",
        "reliability.complete",
        "edge.valid",
        "environment.verification_fresh",
        "production.eligible",
    ]
    .into_iter()
    .map(|key| PolicyRule {
        rule_id: format!("lenso.production.{}", key.replace('_', "-")),
        evidence_key: key.to_owned(),
        severity: PolicyRuleSeverity::Required,
        advisory_in_development: matches!(
            key,
            "rollback.safe"
                | "providers.compatible"
                | "identity.production"
                | "tenancy.mode.production"
                | "environment.verification_fresh"
                | "production.eligible"
        ),
        remediation: format!("Provide passing canonical evidence for `{key}`."),
        next_action: "Refresh canonical evidence and evaluate the same Policy Pack again."
            .to_owned(),
    })
    .collect()
}

fn canonical_production_policy_identity(rules: &[PolicyRule]) -> (String, String) {
    let version = "v1";
    let digest = digest_json(&(
        POLICY_PACK_PROTOCOL,
        version,
        PolicyEnvironmentProfile::Production,
        rules,
    ));
    (format!("policy-pack:{digest}"), digest)
}

fn policy_evidence_integrity_is_valid(evidence: &PolicyEvidenceArtifact) -> bool {
    let rules = canonical_production_policy_rules();
    let (pack_id, pack_digest) = canonical_production_policy_identity(&rules);
    let results_are_canonical =
        evidence
            .rule_results
            .iter()
            .zip(rules.iter())
            .all(|(result, rule)| {
                result.rule_id == rule.rule_id
                    && result.severity == rule.severity
                    && matches!(
                        result.decision,
                        PolicyDecision::Passed | PolicyDecision::Blocked
                    )
                    && result.evidence_references == [rule.evidence_key.clone()]
                    && result.remediation == rule.remediation
                    && result.next_actions == [rule.next_action.clone()]
            });
    let expected_issues = rules
        .iter()
        .zip(evidence.rule_results.iter())
        .filter(|(_, result)| result.decision == PolicyDecision::Blocked)
        .map(|(rule, _)| PolicyDeliveryIssue {
            code: "policy_rule_blocked".to_owned(),
            message: format!(
                "Policy rule `{}` is blocked because `{}` is false or unknown.",
                rule.rule_id, rule.evidence_key
            ),
            evidence_references: vec![rule.evidence_key.clone()],
            remediation: rule.remediation.clone(),
            next_actions: vec![rule.next_action.clone()],
        })
        .collect::<Vec<_>>();
    let expected_decision = if expected_issues.is_empty() {
        PolicyDecision::Passed
    } else {
        PolicyDecision::Blocked
    };
    let content = PolicyEvidenceContent {
        protocol: evidence.protocol.as_str(),
        pack_id: evidence.pack_id.as_str(),
        pack_digest: evidence.pack_digest.as_str(),
        evaluated_subject: evidence.evaluated_subject.as_str(),
        input_digests: &evidence.input_digests,
        decision: evidence.decision,
        rule_results: &evidence.rule_results,
        issues: &evidence.issues,
    };
    evidence.protocol == POLICY_EVIDENCE_PROTOCOL
        && evidence.pack_id == pack_id
        && evidence.pack_digest == pack_digest
        && evidence.rule_results.len() == rules.len()
        && results_are_canonical
        && evidence.decision == expected_decision
        && evidence.issues == expected_issues
        && evidence.evidence_id == format!("policy-evidence:{}", evidence.evidence_digest)
        && digest_json(&content) == evidence.evidence_digest
}

#[cfg(test)]
fn config_revision_integrity_is_valid(config: &ConfigRevisionArtifact) -> bool {
    config.protocol == "lenso.config-revision.v1"
        && config.revision_id == format!("config-revision:{}", config.revision_digest)
        && digest_json(&(
            config.protocol.as_str(),
            config.service_id.as_str(),
            config.contract_reference.as_str(),
            config.contract_digest.as_str(),
            &config.values,
            &config.secret_references,
            &config.impacts,
        )) == config.revision_digest
}

#[cfg(test)]
fn config_revision_matches_contract(
    config: &ConfigRevisionArtifact,
    contract: &ConfigContractArtifact,
    trusted_secrets: &BTreeMap<String, TrustedSecretObservation>,
) -> bool {
    let mut canonical_fields = contract.fields.clone();
    canonical_fields.sort();
    let unique_paths = contract
        .fields
        .iter()
        .map(|field| field.path.as_str())
        .collect::<BTreeSet<_>>();
    let contract_valid = contract.protocol == "lenso.config-contract.v1"
        && !contract.reference.trim().is_empty()
        && !contract.fields.is_empty()
        && contract.fields == canonical_fields
        && unique_paths.len() == contract.fields.len()
        && !unique_paths.contains("")
        && digest_json(&(
            contract.protocol.as_str(),
            contract.reference.as_str(),
            contract.fields.as_slice(),
        )) == contract.digest;
    if !contract_valid
        || !config_revision_integrity_is_valid(config)
        || config.contract_reference != contract.reference
        || config.contract_digest != contract.digest
    {
        return false;
    }
    let fields = contract
        .fields
        .iter()
        .map(|field| (field.path.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    if config.values.iter().any(|(path, value)| {
        !fields.get(path.as_str()).is_some_and(|field| {
            field.sensitivity == ConfigFieldSensitivity::Public
                && config_value_matches(field.value_type, value)
        })
    }) {
        return false;
    }
    let mut canonical_references = config.secret_references.clone();
    canonical_references.sort_by(|left, right| {
        (&left.purpose, &left.reference_id).cmp(&(&right.purpose, &right.reference_id))
    });
    let reference_ids = config
        .secret_references
        .iter()
        .map(|reference| reference.reference_id.as_str())
        .collect::<BTreeSet<_>>();
    let reference_purposes = config
        .secret_references
        .iter()
        .map(|reference| reference.purpose.as_str())
        .collect::<BTreeSet<_>>();
    if config.secret_references != canonical_references
        || reference_ids.len() != config.secret_references.len()
        || reference_purposes.len() != config.secret_references.len()
        || config.secret_references.iter().any(|reference| {
            reference.reference_id.trim().is_empty()
                || reference.provider.trim().is_empty()
                || reference.purpose.trim().is_empty()
                || reference.scope.trim().is_empty()
                || reference.status != SecretReferenceStatus::Resolved
                || !secret_metadata_is_safe(&reference.metadata)
                || trusted_secrets
                    .get(reference.reference_id.as_str())
                    .map(|observation| {
                        observation.provider != reference.provider
                            || observation.status != reference.status
                            || observation.metadata != reference.metadata
                    })
                    .unwrap_or(true)
                || !fields
                    .get(reference.purpose.as_str())
                    .is_some_and(|field| field.sensitivity == ConfigFieldSensitivity::Sensitive)
        })
    {
        return false;
    }
    if contract.fields.iter().any(|field| {
        field.required
            && match field.sensitivity {
                ConfigFieldSensitivity::Public => !config.values.contains_key(&field.path),
                ConfigFieldSensitivity::Sensitive => !config
                    .secret_references
                    .iter()
                    .any(|reference| reference.purpose == field.path),
            }
    }) {
        return false;
    }
    let expected_impacts = contract
        .fields
        .iter()
        .filter(|field| {
            config.values.contains_key(&field.path)
                || config
                    .secret_references
                    .iter()
                    .any(|reference| reference.purpose == field.path)
        })
        .map(|field| ConfigImpact {
            path: field.path.clone(),
            scope: field.scope,
            activation: field.activation,
            mutable: field.mutable,
        })
        .collect::<Vec<_>>();
    config.impacts == expected_impacts
}

#[cfg(test)]
fn config_value_matches(expected: ConfigValueType, value: &Value) -> bool {
    match expected {
        ConfigValueType::String => value.is_string(),
        ConfigValueType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        ConfigValueType::Number => value.is_number(),
        ConfigValueType::Boolean => value.is_boolean(),
        ConfigValueType::Object => value.is_object(),
        ConfigValueType::Array => value.is_array(),
    }
}

#[cfg(test)]
fn secret_metadata_is_safe(metadata: &BTreeMap<String, String>) -> bool {
    metadata.iter().all(|(key, value)| match key.as_str() {
        "leaseExpiresAt" | "lastResolvedAt" => {
            value.len() >= 20
                && value.len() <= 40
                && value.as_bytes().get(4) == Some(&b'-')
                && value.as_bytes().get(7) == Some(&b'-')
                && value.as_bytes().get(10) == Some(&b'T')
                && value.ends_with('Z')
                && value.bytes().all(|byte| {
                    byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'T' | b'.' | b'Z')
                })
        }
        "rotationRevision" | "providerRevision" => {
            !value.is_empty()
                && value.len() <= 20
                && value.bytes().all(|byte| byte.is_ascii_digit())
        }
        "rotationStatus" => matches!(
            value.as_str(),
            "current" | "due" | "rotating" | "stale" | "revoked"
        ),
        _ => false,
    })
}

pub(crate) fn can_i_deploy(artifact: &Path, output: Option<&Path>) -> Result<()> {
    let value: Value = read_json(artifact)?;
    if value["protocol"] == POLICY_EVALUATION_INPUT_PROTOCOL {
        let input: PolicyEvaluationInput = serde_json::from_value(value)
            .context("production_ineligible: canonical eligibility input is malformed")?;
        let trust_provider =
            lenso_service::DeterministicTrustProvider::new(trusted_eligibility_attestations()?);
        let release: lenso_service::ServiceRelease =
            shared_contract(serde_json::to_value(&input.release)?)?;
        let eligibility_input: lenso_service::ProductionEligibilityInput =
            shared_contract(input.eligibility_input.clone())?;
        let eligibility = lenso_service::evaluate_production_eligibility(
            &eligibility_input,
            &release,
            &trust_provider,
        );
        if serde_json::to_value(&eligibility)? != serde_json::to_value(&input.eligibility)? {
            bail!("production_ineligible: eligibility evidence does not match its canonical input")
        }
        let blocked = eligibility.decision != lenso_service::DeliveryDecision::Passed;
        write_json(&eligibility, output)?;
        if blocked {
            bail!(
                "production_ineligible: compatibility or rollback evidence blocked delivery; inspect issues and nextActions"
            )
        }
        return Ok(());
    }
    check_protocol(&value, ELIGIBILITY_PROTOCOL)?;
    ensure_secret_free(&value)?;
    require_fields(
        &value,
        &[
            "evidenceId",
            "evidenceDigest",
            "systemGraphDigest",
            "decision",
        ],
    )?;
    let evidence: EligibilityEvidenceArtifact = serde_json::from_value(value.clone())
        .context("production_ineligible: eligibility evidence is malformed")?;
    if !policy_evidence_integrity_is_valid_shape(&evidence) {
        bail!("production_ineligible: eligibility evidence identity or digest is invalid")
    }
    bail!(
        "eligibility_source_required: precomputed eligibility evidence cannot prove its facts; provide lenso.policy-evaluation-input.v1"
    )
}

fn policy_evidence_integrity_is_valid_shape(evidence: &EligibilityEvidenceArtifact) -> bool {
    evidence.protocol == ELIGIBILITY_PROTOCOL
        && evidence.evidence_id == format!("production-eligibility:{}", evidence.evidence_digest)
}

pub(crate) fn check_deployment_plan(artifact: &Path, output: Option<&Path>) -> Result<()> {
    let plan: lenso_service::DeploymentPlan = read_json(artifact)?;
    if !lenso_service::deployment_plan_integrity_is_valid(&plan) {
        bail!("deployment_input_invalid: Deployment plan identity or protected inputs are invalid")
    }
    write_json(&plan, output)
}

pub(crate) fn export_operator_resource(
    deployment_plan: &Path,
    previous: Option<&Path>,
    output: Option<&Path>,
) -> Result<()> {
    let plan: Value = read_json(deployment_plan)?;
    let previous_export = previous.map(read_json::<Value>).transpose()?;
    validate_deployment_plan(&plan)?;
    let core_plan: lenso_service::DeploymentPlan = serde_json::from_value(plan.clone())
        .context("deployment_input_invalid: Deployment plan is not the shared adapter contract")?;
    if !lenso_service::deployment_plan_integrity_is_valid(&core_plan) {
        bail!("deployment_input_invalid: shared Deployment plan validation failed")
    }
    if plan["adapter"] != "kubernetes" {
        bail!(
            "deployment_input_invalid: Operator export requires the shared Kubernetes adapter plan"
        )
    }
    let bound_rollback_release_id = plan["adapterInputs"]["rollbackReleaseId"]
        .as_str()
        .map_or(Value::Null, |value| Value::String(value.to_owned()));
    if let Some(previous) = &previous_export {
        check_protocol(previous, OPERATOR_EXPORT_PROTOCOL)?;
        let previous_resource = previous
            .get("resource")
            .context("deployment_input_invalid: previous Operator export lacks resource")?;
        if previous["resourceDigest"].as_str() != Some(digest_json(previous_resource).as_str()) {
            bail!("release_tampered: previous Operator export resource digest is invalid")
        }
        if previous.pointer("/resource/spec/releaseId") != Some(&bound_rollback_release_id) {
            bail!("release_tampered: rollback release differs from Deployment adapter inputs")
        }
    }
    let resource = operator_resource_from_plan(&plan)?;
    ensure_secret_free(&resource)?;
    let resource_digest = digest_json(&resource);
    let previous_digest = previous_export
        .as_ref()
        .and_then(|value| value.get("resourceDigest"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let export = json!({
        "protocol": OPERATOR_EXPORT_PROTOCOL,
        "resourceDigest": resource_digest,
        "deploymentPlanId": plan["planId"].clone(),
        "deploymentPlanDigest": plan["planDigest"].clone(),
        "resource": resource,
        "diff": {
            "changed": previous_digest.as_deref() != Some(resource_digest.as_str()),
            "previousResourceDigest": previous_digest,
            "desiredResourceDigest": resource_digest
        },
        "effects": Effects::default()
    });
    write_json(&export, output)
}

fn operator_resource_from_plan(plan: &Value) -> Result<Value> {
    let config_identity = required_text(plan, "configRevisionId")?
        .strip_prefix("config-revision:sha256:")
        .unwrap_or(required_text(plan, "configRevisionId")?);
    let config_map_name = format!(
        "{}-config-{}",
        kubernetes_name(required_text(plan, "serviceId")?),
        &kubernetes_name(config_identity)[..kubernetes_name(config_identity).len().min(12)],
    );
    let mut workloads = plan["workloads"]
        .as_array()
        .context("deployment_input_invalid: workloads must be an array")?
        .iter()
        .map(|workload| {
            let role = required_text(workload, "role")?;
            let replicas = workload["settings"]["replicas"]
                .as_u64()
                .context("deployment_input_invalid: Workload replicas must be an integer")?;
            let artifact_reference = required_text(workload, "artifactReference")?;
            let artifact_digest = required_text(workload, "artifactDigest")?;
            Ok(json!({
                "workloadId": required_text(workload, "workloadId")?,
                "role": role,
                "image": format!("{artifact_reference}@{artifact_digest}"),
                "replicas": replicas,
                "port": workload["settings"]["port"].clone(),
                "command": workload["settings"]["command"].clone(),
                "configMapName": config_map_name.clone(),
                "secretReferenceIds": plan["secretReferenceIds"].clone(),
                "placement": {"nodeSelector": plan["placement"].clone()},
                "scaling": {
                    "minReplicas": replicas,
                    "maxReplicas": replicas,
                    "targetCpuUtilization": 70
                },
                "disruptionMinAvailable": workload["settings"]["disruptionMinAvailable"].clone(),
                "networkPolicyEnabled": true,
                "readinessPath": workload["settings"]["healthPath"].clone(),
                "livenessPath": workload["settings"]["healthPath"].clone()
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    workloads.sort_by_key(|workload| {
        workload["workloadId"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    });
    let default_name = format!(
        "{}-{}",
        kubernetes_name(required_text(&plan, "serviceId")?),
        kubernetes_name(required_text(&plan, "environment")?)
    );
    let name = plan["adapterInputs"]["resourceName"]
        .as_str()
        .map(kubernetes_name)
        .unwrap_or(default_name);
    let secret_references = plan["secretReferenceIds"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|id| {
            let reference_id = id
                .as_str()
                .context("deployment_input_invalid: Secret Reference IDs must be strings")?;
            Ok(json!({
                "referenceId": reference_id,
                "provider": "external",
                "targetName": kubernetes_name(reference_id)
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let rollback_release_id = plan["adapterInputs"]["rollbackReleaseId"]
        .as_str()
        .map_or(Value::Null, |value| Value::String(value.to_owned()));
    let resource = json!({
        "apiVersion": "lenso.dev/v1alpha1",
        "kind": "LensoAutonomousService",
        "metadata": {"name": name},
        "spec": {
            "serviceId": plan["serviceId"].clone(),
            "environment": plan["environment"].clone(),
            "releaseId": plan["releaseId"].clone(),
            "releaseDigest": plan["releaseDigest"].clone(),
            "configRevisionId": plan["configRevisionId"].clone(),
            "expectedEnvironmentRevision": plan["expectedEnvironmentRevision"].clone(),
            "secretReferences": secret_references,
            "policyEvidenceReferences": plan["policyEvidenceReferences"].clone(),
            "evidenceReferences": [plan["planId"].clone()],
            "workloads": workloads,
            "rolloutStrategy": "migration_first",
            "rollbackReleaseId": rollback_release_id
        }
    });
    Ok(resource)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn authorize_promotion_apply(
    promotion_plan: &Path,
    approval: &Path,
    protected_evidence: &Path,
    environment_verification: &Path,
    source_observation: &Path,
    source_gateway_observation: &Path,
    target_observation: &Path,
    operator_export: &Path,
    output: Option<&Path>,
) -> Result<()> {
    let plan: Value = read_json(promotion_plan)?;
    let approval: Value = read_json(approval)?;
    let protected: Value = read_json(protected_evidence)?;
    let verification: Value = read_json(environment_verification)?;
    let observation: Value = read_json(source_observation)?;
    let gateway_observation: Value = read_json(source_gateway_observation)?;
    let target_observation: Value = read_json(target_observation)?;
    let export: Value = read_json(operator_export)?;
    for artifact in [
        &plan,
        &approval,
        &protected,
        &verification,
        &observation,
        &gateway_observation,
        &target_observation,
        &export,
    ] {
        ensure_secret_free(artifact)?;
    }
    let trusted_edge = trusted_edge_attestations()?;
    let trusted_operator_authorities = trusted_operator_observation_authorities()?;
    let trusted_gateway_authorities = trusted_gateway_observation_authorities()?;
    validate_promotion_plan(&plan, &trusted_edge)?;
    validate_environment_verification(
        &verification,
        &trusted_operator_authorities,
        &trusted_gateway_authorities,
    )?;
    check_protocol(&approval, PROMOTION_APPROVAL_PROTOCOL)?;
    check_protocol(&observation, OPERATOR_OBSERVATION_PROTOCOL)?;
    check_protocol(&target_observation, OPERATOR_OBSERVATION_PROTOCOL)?;
    check_protocol(&export, OPERATOR_EXPORT_PROTOCOL)?;
    validate_operator_observation(&observation, &trusted_operator_authorities)?;
    validate_operator_observation(&target_observation, &trusted_operator_authorities)?;
    let current_gateway_observation =
        validate_gateway_observation(&gateway_observation, &trusted_gateway_authorities)?;
    validate_operator_export(&export, &plan["targetDeployment"])?;

    let authority_key = std::env::var("LENSO_PROMOTION_AUTHORITY_KEY")
        .context("approval_invalid: LENSO_PROMOTION_AUTHORITY_KEY is required")?;
    let actor = required_text(&approval, "actor")?;
    let authority = required_text(&approval, "authority")?;
    let plan_digest = required_text(&plan, "planDigest")?;
    let target_revision = plan["targetEnvironmentRevision"]
        .as_u64()
        .context("promotion_input_invalid: targetEnvironmentRevision must be an integer")?;
    let expected_authority_proof = digest_json(&(
        "lenso.promotion-authority-proof.v1",
        authority,
        actor,
        plan_digest,
        target_revision,
        authority_key.as_str(),
    ));
    let authority_proof = required_text(&approval, "authorityProof")?;
    let expected_approval_id = format!(
        "promotion-approval:{}",
        digest_json(&(
            PROMOTION_APPROVAL_PROTOCOL,
            plan_digest,
            actor,
            authority,
            Some(authority_proof),
            target_revision,
            true,
        ))
    );
    if approval["approved"] != true
        || approval["planDigest"] != plan["planDigest"]
        || approval["issuedForTargetRevision"] != target_revision
        || authority_proof != expected_authority_proof
        || required_text(&approval, "approvalId")? != expected_approval_id
        || !actor.starts_with("user:")
    {
        bail!("approval_invalid: Promotion approval is not authorized for the exact plan")
    }

    let expected_protected = json!({
        "sourceVerificationId": plan["sourceVerificationId"].clone(),
        "sourceVerificationDigest": plan["sourceVerificationDigest"].clone(),
        "policyEvidenceId": plan["policyEvidenceId"].clone(),
        "policyEvidenceDigest": plan["policyEvidenceDigest"].clone(),
        "sourceEnvironmentRevision": plan["sourceEnvironmentRevision"].clone(),
        "sourceTopologyDigest": plan["sourceTopologyDigest"].clone(),
        "targetTopologyDigest": plan["targetTopologyDigest"].clone(),
        "configRevisionId": plan["configRevisionId"].clone(),
        "secretReferenceIds": plan["secretReferenceIds"].clone(),
        "evidenceReferences": plan["evidenceReferences"].clone(),
    });
    if protected != expected_protected {
        bail!("stale_input: protected Promotion evidence changed after planning")
    }
    if verification["verificationId"] != plan["sourceVerificationId"]
        || verification["verificationDigest"] != plan["sourceVerificationDigest"]
        || verification["decision"] != "passed"
        || verification["releaseId"] != plan["releaseId"]
        || verification["releaseDigest"] != plan["releaseDigest"]
        || verification["configRevisionId"] != plan["configRevisionId"]
        || verification["environmentRevision"] != plan["sourceEnvironmentRevision"]
    {
        bail!("stale_input: source Environment Verification does not match Promotion")
    }
    let observation_id = required_text(&observation, "observationId")?;
    let observation_digest = required_text(&observation, "observationDigest")?;
    let observation_authority_id = required_text(&observation, "authorityId")?;
    let observation_authority_proof = required_text(&observation, "authorityProof")?;
    let current_claims: lenso_service::OperatorObservationClaims =
        serde_json::from_value(observation["claims"].clone())
            .context("observation_stale: current source Operator claims are malformed")?;
    let verified_claims: lenso_service::OperatorObservationClaims =
        serde_json::from_value(verification["operatorObservationClaims"].clone())
            .context("promotion_input_invalid: verified Operator claims are malformed")?;
    let currentness_context = format!(
        "promotion-currentness:{plan_digest}:{}",
        required_text(&approval, "approvalId")?
    );
    if !promotion_source_currentness_is_valid(
        &current_claims,
        &verified_claims,
        &currentness_context,
    ) || verification["operatorObservationAuthorityId"] != observation_authority_id
    {
        bail!(
            "observation_stale: source Operator observation is not a challenge-bound current read of the approved verification state"
        )
    }
    let verified_environment: lenso_service::EnvironmentVerification =
        serde_json::from_value(verification.clone())
            .context("promotion_input_invalid: Environment Verification is malformed")?;
    if !promotion_gateway_currentness_is_valid(
        &current_gateway_observation,
        &verified_environment,
        observation_id,
        &currentness_context,
    ) {
        bail!(
            "observation_stale: source Gateway observation is not a challenge-bound current read of the approved Edge state"
        )
    }
    let previous_release_id = required_text(
        &plan["targetDeployment"]["adapterInputs"],
        "rollbackReleaseId",
    )?;
    let target_uid = required_text(&target_observation["claims"], "resourceUid")?;
    let target_resource_version = required_text(&target_observation["claims"], "resourceVersion")?;
    let target_authority_id = required_text(&target_observation, "authorityId")?;
    let target_authority_proof = required_text(&target_observation, "authorityProof")?;
    if target_observation["claims"]["decision"] != "passed"
        || target_observation["claims"]["fresh"] != true
        || target_observation["claims"]["drifted"] != false
        || target_observation["claims"]["environment"] != plan["targetEnvironment"]
        || target_observation["claims"]["expectedEnvironmentRevision"] != target_revision
        || target_observation["claims"]["desiredReleaseId"] != previous_release_id
        || target_observation["claims"]["observedReleaseId"] != previous_release_id
        || target_observation["claims"]["desiredReleaseId"]
            != target_observation["claims"]["observedReleaseId"]
        || target_uid.trim().is_empty()
        || target_resource_version.trim().is_empty()
    {
        bail!(
            "stale_input: target Operator observation does not match the exact pre-Promotion resource"
        )
    }
    let mut resource = export["resource"].clone();
    let metadata = resource
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .context("release_tampered: Operator export metadata is missing")?;
    metadata.insert("uid".to_owned(), json!(target_uid));
    metadata.insert("resourceVersion".to_owned(), json!(target_resource_version));
    let mut authorization = json!({
        "protocol": "lenso.promotion-apply-authorization.v1",
        "planId": plan["planId"].clone(),
        "approvalId": approval["approvalId"].clone(),
        "sourceVerificationId": verification["verificationId"].clone(),
        "sourceObservationId": observation_id,
        "sourceObservationDigest": observation_digest,
        "sourceObservationAuthorityId": observation_authority_id,
        "sourceObservationAuthorityProof": observation_authority_proof,
        "sourceGatewayObservationId": current_gateway_observation.observation_id,
        "sourceGatewayResourceUid": current_gateway_observation.resource_uid,
        "sourceGatewayResourceVersion": current_gateway_observation.resource_version,
        "targetObservationId": target_observation["observationId"].clone(),
        "targetObservationDigest": target_observation["observationDigest"].clone(),
        "targetObservationAuthorityId": target_authority_id,
        "targetObservationAuthorityProof": target_authority_proof,
        "targetResourceUid": target_uid,
        "targetResourceVersion": target_resource_version,
        "resource": resource,
        "effects": Effects::default(),
    });
    let authorization_digest = digest_json(&authorization);
    let authorization_object = authorization
        .as_object_mut()
        .expect("Promotion authorization content must be an object");
    authorization_object.insert(
        "authorizationId".to_owned(),
        json!(format!(
            "promotion-apply-authorization:{authorization_digest}"
        )),
    );
    authorization_object.insert(
        "authorizationDigest".to_owned(),
        json!(authorization_digest),
    );
    write_json(&authorization, output)
}

fn validate_operator_observation(
    observation: &Value,
    trusted_authorities: &BTreeMap<String, String>,
) -> Result<()> {
    check_protocol(observation, OPERATOR_OBSERVATION_PROTOCOL)?;
    let claims: lenso_service::OperatorObservationClaims =
        serde_json::from_value(observation["claims"].clone())
            .context("operator_observation_invalid: canonical claims are required")?;
    let digest = lenso_service::operator_observation_claims_digest(&claims);
    let authority_id = required_text(observation, "authorityId")?;
    let provider =
        lenso_service::Ed25519OperatorObservationAuthorityProvider::from_base64_public_keys(
            trusted_authorities.clone(),
        )
        .map_err(anyhow::Error::msg)
        .context("operator_observation_invalid: trusted authority public keys are invalid")?;
    let attestation = lenso_service::OperatorObservationAttestation {
        observation_id: required_text(observation, "observationId")?.to_owned(),
        observation_digest: required_text(observation, "observationDigest")?.to_owned(),
        authority_id: authority_id.to_owned(),
        authority_proof: required_text(observation, "authorityProof")?.to_owned(),
        claims: claims.clone(),
    };
    if required_text(observation, "observationDigest")? != digest
        || required_text(observation, "observationId")? != format!("operator-observation:{digest}")
        || !lenso_service::operator_observation_attestation_is_valid(&attestation, &provider)
        || observation["serviceId"] != claims.service_id
        || observation["environment"] != claims.environment
        || observation["resourceUid"] != claims.resource_uid
        || observation["resourceVersion"] != claims.resource_version
        || observation["expectedEnvironmentRevision"] != claims.expected_environment_revision
        || observation["environmentRevision"] != claims.environment_revision
        || observation["authorityContext"] != claims.authority_context
        || observation["desiredReleaseId"] != claims.desired_release_id
        || observation["desiredReleaseDigest"] != claims.desired_release_digest
        || observation["observedReleaseId"] != claims.observed_release_id
        || observation["observedReleaseDigest"] != claims.observed_release_digest
        || observation["configRevisionId"] != claims.config_revision_id
        || observation["state"] != claims.state
        || observation["rolloutPhase"] != claims.rollout_phase
        || observation["rollbackState"] != claims.rollback_state
        || observation["fresh"] != claims.fresh
        || observation["drifted"] != claims.drifted
        || observation["decision"] != serde_json::to_value(claims.decision)?
    {
        bail!(
            "operator_observation_invalid: observation content, identity, or authority proof was modified"
        )
    }
    Ok(())
}

fn promotion_source_currentness_is_valid(
    current: &lenso_service::OperatorObservationClaims,
    verified: &lenso_service::OperatorObservationClaims,
    expected_context: &str,
) -> bool {
    current.decision == lenso_service::DeliveryDecision::Passed
        && current.fresh
        && !current.drifted
        && current.authority_context == expected_context
        && current.service_id == verified.service_id
        && current.environment == verified.environment
        && current.deployment_plan_id == verified.deployment_plan_id
        && current.deployment_plan_digest == verified.deployment_plan_digest
        && current.expected_environment_revision == verified.expected_environment_revision
        && current.environment_revision == verified.environment_revision
        && current.resource_uid == verified.resource_uid
        && opaque_resource_versions_are_present(
            &current.resource_version,
            &verified.resource_version,
        )
        && current.desired_release_id == verified.desired_release_id
        && current.desired_release_digest == verified.desired_release_digest
        && current.observed_release_id == verified.observed_release_id
        && current.observed_release_digest == verified.observed_release_digest
        && current.desired_workload_digests == verified.desired_workload_digests
        && current.observed_workload_digests == verified.observed_workload_digests
        && current.workload_health == verified.workload_health
        && current.config_revision_id == verified.config_revision_id
        && current.state == "ready"
        && current.rollout_phase == "ready"
}

fn opaque_resource_versions_are_present(current: &str, verified: &str) -> bool {
    !current.trim().is_empty() && !verified.trim().is_empty()
}

fn validate_gateway_observation(
    observation: &Value,
    trusted_authorities: &BTreeMap<String, String>,
) -> Result<lenso_service::GatewayObservation> {
    let observation: lenso_service::GatewayObservation =
        serde_json::from_value(observation.clone())
            .context("gateway_observation_invalid: shared Gateway observation is malformed")?;
    let key = trusted_authorities
        .get(&observation.provider_id)
        .context("gateway_observation_invalid: Gateway observation authority is not trusted")?;
    let provider = lenso_service::Ed25519GatewayObservationProvider::from_base64_public_key(
        observation.provider_id.clone(),
        key,
    )
    .map_err(anyhow::Error::msg)
    .context("gateway_observation_invalid: trusted authority public key is invalid")?;
    if !lenso_service::gateway_observation_integrity_is_valid(&observation, &provider) {
        bail!("gateway_observation_invalid: Gateway observation authority proof is invalid")
    }
    Ok(observation)
}

fn promotion_gateway_currentness_is_valid(
    current: &lenso_service::GatewayObservation,
    verified: &lenso_service::EnvironmentVerification,
    current_operator_observation_id: &str,
    expected_context: &str,
) -> bool {
    current.fresh
        && current.authority_context == expected_context
        && current.plan_id == verified.gateway_plan_id
        && current.plan_digest == verified.gateway_plan_digest
        && current.environment == verified.environment
        && current.release_id == verified.release_id
        && current.release_digest == verified.release_digest
        && current.configuration_identity == verified.gateway_configuration_identity
        && current.revision == verified.gateway_observation_revision
        && current.observed_after == current_operator_observation_id
        && current.resource_uid == verified.gateway_resource_uid
        && opaque_resource_versions_are_present(
            &current.resource_version,
            &verified.gateway_resource_version,
        )
        && current.provider_id == verified.gateway_observation_provider_id
}

fn validate_operator_export(export: &Value, deployment_plan: &Value) -> Result<()> {
    check_protocol(export, OPERATOR_EXPORT_PROTOCOL)?;
    validate_deployment_plan(deployment_plan)?;
    let resource = export
        .get("resource")
        .context("release_tampered: Operator export resource is missing")?;
    let expected_resource = operator_resource_from_plan(deployment_plan)?;
    let resource_digest = digest_json(resource);
    if required_text(export, "deploymentPlanId")? != required_text(deployment_plan, "planId")?
        || required_text(export, "deploymentPlanDigest")?
            != required_text(deployment_plan, "planDigest")?
        || required_text(export, "resourceDigest")? != resource_digest
        || resource != &expected_resource
    {
        bail!(
            "release_tampered: target Operator export image, replicas, configuration, Secret References, or identity differ from the approved Deployment plan"
        )
    }
    Ok(())
}

fn validate_promotion_plan(plan: &Value, trusted_edge: &BTreeMap<String, String>) -> Result<()> {
    let plan: lenso_service::PromotionPlan = serde_json::from_value(plan.clone())
        .context("promotion_input_invalid: Promotion plan is not the shared contract")?;
    if !lenso_service::promotion_plan_integrity_is_valid(&plan) {
        bail!("promotion_input_invalid: shared Promotion plan validation failed")
    }
    let provider = lenso_service::DeterministicTrustProvider::new(trusted_edge.clone());
    if !lenso_service::gateway_plan_authority_is_valid(&plan.target_gateway, &provider) {
        bail!("promotion_input_invalid: target Gateway authority is invalid")
    }
    Ok(())
}

#[cfg(test)]
fn validate_gateway_plan(plan: &Value, trusted_edge: &BTreeMap<String, String>) -> Result<()> {
    let plan: lenso_service::GatewayConfigurationPlan = serde_json::from_value(plan.clone())
        .context("promotion_input_invalid: target Gateway is not the shared contract")?;
    let provider = lenso_service::DeterministicTrustProvider::new(trusted_edge.clone());
    if !lenso_service::gateway_plan_authority_is_valid(&plan, &provider) {
        bail!("promotion_input_invalid: shared Gateway authority validation failed")
    }
    Ok(())
}

fn validate_environment_verification(
    verification: &Value,
    trusted_operator_authorities: &BTreeMap<String, String>,
    trusted_gateway_authorities: &BTreeMap<String, String>,
) -> Result<()> {
    check_protocol(verification, "lenso.environment-verification.v1")?;
    let digest = digest_json(&EnvironmentVerificationDigestInput {
        protocol: "lenso.environment-verification.v1",
        environment: required_text(verification, "environment")?,
        environment_revision: verification["environmentRevision"]
            .as_u64()
            .context("promotion_input_invalid: environment revision must be an integer")?,
        release_id: required_text(verification, "releaseId")?,
        release_digest: required_text(verification, "releaseDigest")?,
        workload_digests: &verification["workloadDigests"],
        workload_health: &verification["workloadHealth"],
        config_revision_id: required_text(verification, "configRevisionId")?,
        trust_evidence_digest: required_text(verification, "trustEvidenceDigest")?,
        policy_evidence_id: required_text(verification, "policyEvidenceId")?,
        policy_evidence_digest: required_text(verification, "policyEvidenceDigest")?,
        deployment_plan_id: required_text(verification, "deploymentPlanId")?,
        deployment_plan_digest: required_text(verification, "deploymentPlanDigest")?,
        deployment_receipt_id: required_text(verification, "deploymentReceiptId")?,
        deployment_observation_id: required_text(verification, "deploymentObservationId")?,
        operator_observation_id: required_text(verification, "operatorObservationId")?,
        operator_observation_digest: required_text(verification, "operatorObservationDigest")?,
        operator_observation_authority_id: required_text(
            verification,
            "operatorObservationAuthorityId",
        )?,
        operator_observation_authority_proof: required_text(
            verification,
            "operatorObservationAuthorityProof",
        )?,
        operator_observation_claims: &verification["operatorObservationClaims"],
        gateway_plan_id: required_text(verification, "gatewayPlanId")?,
        gateway_plan_digest: required_text(verification, "gatewayPlanDigest")?,
        gateway_observation_id: required_text(verification, "gatewayObservationId")?,
        gateway_resource_uid: required_text(verification, "gatewayResourceUid")?,
        gateway_resource_version: required_text(verification, "gatewayResourceVersion")?,
        gateway_authority_context: required_text(verification, "gatewayAuthorityContext")?,
        gateway_configuration_identity: required_text(
            verification,
            "gatewayConfigurationIdentity",
        )?,
        gateway_observation_revision: verification["gatewayObservationRevision"]
            .as_u64()
            .context("promotion_input_invalid: gateway observation revision must be an integer")?,
        gateway_observation_observed_after: required_text(
            verification,
            "gatewayObservationObservedAfter",
        )?,
        gateway_observation_fresh: verification["gatewayObservationFresh"]
            .as_bool()
            .context("promotion_input_invalid: gateway observation freshness must be a boolean")?,
        gateway_observation_provider_id: required_text(
            verification,
            "gatewayObservationProviderId",
        )?,
        gateway_observation_provider_proof: required_text(
            verification,
            "gatewayObservationProviderProof",
        )?,
        topology_digest: required_text(verification, "topologyDigest")?,
        evidence_references: &verification["evidenceReferences"],
        freshness_horizon_revision: verification["freshnessHorizonRevision"]
            .as_u64()
            .context("promotion_input_invalid: freshness horizon must be an integer")?,
        decision: &verification["decision"],
        issues: &verification["issues"],
    });
    if required_text(verification, "verificationDigest")? != digest
        || required_text(verification, "verificationId")?
            != format!("environment-verification:{digest}")
    {
        bail!("promotion_input_invalid: Environment Verification identity is invalid")
    }
    let operator_digest = required_text(verification, "operatorObservationDigest")?;
    let operator_id = required_text(verification, "operatorObservationId")?;
    let operator_authority = required_text(verification, "operatorObservationAuthorityId")?;
    let operator_claims: lenso_service::OperatorObservationClaims =
        serde_json::from_value(verification["operatorObservationClaims"].clone())
            .context("promotion_input_invalid: Operator observation claims are malformed")?;
    let operator_provider =
        lenso_service::Ed25519OperatorObservationAuthorityProvider::from_base64_public_keys(
            trusted_operator_authorities.clone(),
        )
        .map_err(anyhow::Error::msg)
        .context("promotion_input_invalid: Operator authority public keys are invalid")?;
    let operator_attestation = lenso_service::OperatorObservationAttestation {
        observation_id: operator_id.to_owned(),
        observation_digest: operator_digest.to_owned(),
        authority_id: operator_authority.to_owned(),
        authority_proof: required_text(verification, "operatorObservationAuthorityProof")?
            .to_owned(),
        claims: operator_claims.clone(),
    };
    if operator_digest != lenso_service::operator_observation_claims_digest(&operator_claims)
        || operator_id != format!("operator-observation:{operator_digest}")
        || !lenso_service::operator_observation_attestation_is_valid(
            &operator_attestation,
            &operator_provider,
        )
        || operator_claims.environment != required_text(verification, "environment")?
        || operator_claims.environment_revision
            != verification["environmentRevision"]
                .as_u64()
                .context("promotion_input_invalid: environment revision must be an integer")?
        || operator_claims.authority_context != required_text(verification, "deploymentPlanId")?
        || operator_claims.deployment_plan_id != required_text(verification, "deploymentPlanId")?
        || operator_claims.deployment_plan_digest
            != required_text(verification, "deploymentPlanDigest")?
        || operator_claims.desired_release_id != required_text(verification, "releaseId")?
        || operator_claims.desired_release_digest != required_text(verification, "releaseDigest")?
        || operator_claims.observed_release_id != required_text(verification, "releaseId")?
        || operator_claims.observed_release_digest != required_text(verification, "releaseDigest")?
        || serde_json::to_value(&operator_claims.desired_workload_digests)?
            != verification["workloadDigests"]
        || serde_json::to_value(&operator_claims.observed_workload_digests)?
            != verification["workloadDigests"]
        || serde_json::to_value(&operator_claims.workload_health)? != verification["workloadHealth"]
        || operator_claims.config_revision_id != required_text(verification, "configRevisionId")?
        || operator_claims.state != "ready"
        || operator_claims.rollout_phase != "ready"
        || operator_claims.decision != lenso_service::DeliveryDecision::Passed
        || !operator_claims.fresh
        || operator_claims.drifted
    {
        bail!("promotion_input_invalid: Operator observation authority proof is invalid")
    }
    let gateway_provider = required_text(verification, "gatewayObservationProviderId")?;
    let gateway_key = trusted_gateway_authorities
        .get(gateway_provider)
        .context("promotion_input_invalid: Gateway observation authority is not trusted")?;
    let gateway_observation_digest = digest_json(&(
        "lenso.gateway-observation.v1",
        required_text(verification, "gatewayPlanId")?,
        required_text(verification, "gatewayPlanDigest")?,
        required_text(verification, "environment")?,
        required_text(verification, "releaseId")?,
        required_text(verification, "releaseDigest")?,
        required_text(verification, "gatewayResourceUid")?,
        required_text(verification, "gatewayResourceVersion")?,
        required_text(verification, "gatewayAuthorityContext")?,
        required_text(verification, "gatewayConfigurationIdentity")?,
        verification["gatewayObservationRevision"]
            .as_u64()
            .context("promotion_input_invalid: gateway observation revision must be an integer")?,
        required_text(verification, "gatewayObservationObservedAfter")?,
        verification["gatewayObservationFresh"]
            .as_bool()
            .context("promotion_input_invalid: gateway observation freshness must be a boolean")?,
        gateway_provider,
    ));
    let gateway_observation_id = required_text(verification, "gatewayObservationId")?;
    let gateway_authority =
        lenso_service::Ed25519GatewayObservationProvider::from_base64_public_key(
            gateway_provider,
            gateway_key,
        )
        .map_err(anyhow::Error::msg)
        .context("promotion_input_invalid: Gateway authority public key is invalid")?;
    if gateway_observation_id != format!("gateway-observation:{gateway_observation_digest}")
        || !lenso_service::GatewayObservationProvider::verify(
            &gateway_authority,
            gateway_observation_id,
            required_text(verification, "gatewayObservationProviderProof")?,
        )
        || required_text(verification, "gatewayAuthorityContext")?
            != required_text(verification, "gatewayPlanId")?
        || verification["gatewayObservationFresh"] != true
        || required_text(verification, "gatewayObservationObservedAfter")? != operator_id
    {
        bail!("promotion_input_invalid: Gateway observation authority proof is invalid")
    }
    Ok(())
}

#[cfg(test)]
fn assemble(
    mut input: ServiceReleaseInput,
) -> std::result::Result<ServiceRelease, Vec<DeliveryIssue>> {
    normalize_input(&mut input);
    let issues = input_issues(&input);
    if !issues.is_empty() {
        return Err(issues);
    }
    let digest = digest_json(&content_from_input(&input));
    Ok(ServiceRelease {
        protocol: SERVICE_RELEASE_PROTOCOL.to_owned(),
        release_id: format!("service-release:{digest}"),
        release_digest: digest,
        service_id: input.service_id,
        service_version: input.service_version,
        modules: input.modules,
        workloads: input.workloads,
        contract_versions: input.contract_versions,
        config_contract: input.config_contract,
        reliability_contract: input.reliability_contract,
        migrations: input.migrations,
        workflow_compatibility: input.workflow_compatibility,
        verification_evidence: input.verification_evidence,
        rollout_gates: input.rollout_gates,
        rollback: input.rollback,
        retention: input.retention,
        signatures: Vec::new(),
    })
}

#[cfg(test)]
fn normalize_input(input: &mut ServiceReleaseInput) {
    input.modules.sort();
    input.workloads.sort_by(|left, right| {
        (left.role.as_str(), left.workload_id.as_str())
            .cmp(&(right.role.as_str(), right.workload_id.as_str()))
    });
    input.contract_versions.sort_by(|left, right| {
        (&left.contract_id, &left.version, &left.kind).cmp(&(
            &right.contract_id,
            &right.version,
            &right.kind,
        ))
    });
    input.migrations.sort_by(|left, right| {
        (&left.migration_id, &left.phase).cmp(&(&right.migration_id, &right.phase))
    });
    input.workflow_compatibility.sort();
    input.verification_evidence.sort();
    input.rollout_gates.sort();
    for workload in &mut input.workloads {
        workload.provenance.input_digests.sort();
        workload.provenance.subject_digests.sort();
    }
}

#[cfg(test)]
fn input_issues(input: &ServiceReleaseInput) -> Vec<DeliveryIssue> {
    let mut issues = Vec::new();
    if input.service_id.trim().is_empty()
        || input.service_version.trim().is_empty()
        || input.modules.is_empty()
        || input.workloads.is_empty()
        || input.contract_versions.is_empty()
    {
        issues.push(issue(
            "release_input_invalid",
            "Service identity, version, Modules, Workloads, and Contract Versions are required.",
            "Supply complete environment-independent release inputs.",
            "Correct the input and assemble again.",
        ));
    }
    let mut workload_ids = BTreeSet::new();
    for workload in &input.workloads {
        if !workload_ids.insert(workload.workload_id.as_str())
            || workload.workload_id.trim().is_empty()
            || workload.artifact_reference.trim().is_empty()
            || workload.media_type.trim().is_empty()
            || workload.signature_subject.trim().is_empty()
        {
            issues.push(issue(
                "release_input_invalid",
                "Every Workload requires one unique identity, artifact reference, media type, and signature subject.",
                "Declare every Workload artifact exactly once.",
                "Correct the Workload declaration and assemble again.",
            ));
        }
        if !valid_digest(&workload.artifact_digest) {
            issues.push(issue(
                "mutable_artifact_reference",
                "Every Workload must carry an immutable sha256 artifact digest.",
                "Resolve existing CI artifacts to immutable digests without rebuilding.",
                "Pin the artifact digest and assemble again.",
            ));
        }
        if workload.sbom.reference.trim().is_empty() || !valid_digest(&workload.sbom.digest) {
            issues.push(issue(
                "missing_sbom",
                "Every Workload requires an addressable SBOM with an immutable digest.",
                "Attach the CI-produced SBOM evidence.",
                "Provide SBOM evidence and assemble again.",
            ));
        }
        if workload.provenance.reference.trim().is_empty()
            || workload.provenance.source.trim().is_empty()
            || workload.provenance.builder.trim().is_empty()
            || !valid_digest(&workload.provenance.digest)
            || workload.provenance.input_digests.is_empty()
            || workload
                .provenance
                .input_digests
                .iter()
                .any(|digest| !valid_digest(digest))
        {
            issues.push(issue(
                "missing_provenance",
                "Every Workload requires source, builder, input, and subject provenance.",
                "Attach the CI-produced provenance evidence.",
                "Provide provenance evidence and assemble again.",
            ));
        }
        if !workload
            .provenance
            .subject_digests
            .contains(&workload.artifact_digest)
        {
            issues.push(issue(
                "provenance_subject_mismatch",
                "Provenance subjects do not include the exact Workload artifact digest.",
                "Regenerate provenance for the existing immutable artifact.",
                "Attach matching provenance and assemble again.",
            ));
        }
    }
    for evidence in input
        .contract_versions
        .iter()
        .map(|contract| &contract.artifact)
        .chain(std::iter::once(&input.config_contract))
        .chain(std::iter::once(&input.reliability_contract))
        .chain(input.migrations.iter().map(|migration| &migration.artifact))
        .chain(input.workflow_compatibility.iter())
        .chain(input.verification_evidence.iter())
    {
        if evidence.reference.trim().is_empty() || !valid_digest(&evidence.digest) {
            issues.push(issue(
                "release_input_invalid",
                "Release evidence requires a stable reference and immutable sha256 digest.",
                "Regenerate and pin every Contract, configuration, reliability, migration, workflow, and verification artifact.",
                "Correct the evidence and assemble again.",
            ));
        }
    }
    issues
}

#[cfg(test)]
#[allow(dead_code)]
fn release_issues(release: &ServiceRelease) -> Vec<DeliveryIssue> {
    let input = ServiceReleaseInput {
        service_id: release.service_id.clone(),
        service_version: release.service_version.clone(),
        modules: release.modules.clone(),
        workloads: release.workloads.clone(),
        contract_versions: release.contract_versions.clone(),
        config_contract: release.config_contract.clone(),
        reliability_contract: release.reliability_contract.clone(),
        migrations: release.migrations.clone(),
        workflow_compatibility: release.workflow_compatibility.clone(),
        verification_evidence: release.verification_evidence.clone(),
        rollout_gates: release.rollout_gates.clone(),
        rollback: release.rollback,
        retention: release.retention,
    };
    let mut issues = input_issues(&input);
    let mut normalized = input.clone();
    normalize_input(&mut normalized);
    if input != normalized {
        issues.push(issue(
            "release_input_invalid",
            "Service Release collections are not in canonical order.",
            "Use the deterministic release assembler instead of constructing Release JSON manually.",
            "Reassemble the Service Release before any protected operation.",
        ));
    }
    let digest = digest_json(&content_from_release(release));
    if release.protocol != SERVICE_RELEASE_PROTOCOL
        || release.release_id != format!("service-release:{digest}")
        || release.release_digest != digest
    {
        issues.push(issue(
            "release_tampered",
            "Service Release content does not match its canonical identity.",
            "Discard changed content and assemble a new immutable release.",
            "Reassemble before any protected operation.",
        ));
    }
    issues
}

#[cfg(test)]
fn release_diff(from: &ServiceRelease, to: &ServiceRelease) -> ReleaseDiff {
    let mut entries = Vec::new();
    let from_content = serde_json::to_value(content_from_release(from))
        .expect("Service Release content must serialize");
    let to_content = serde_json::to_value(content_from_release(to))
        .expect("Service Release content must serialize");
    for (field, subject) in [
        ("serviceId", "service.identity"),
        ("serviceVersion", "service.version"),
        ("modules", "modules"),
        ("workloads", "workloads"),
        ("contractVersions", "contracts"),
        ("configContract", "config.contract"),
        ("reliabilityContract", "reliability.contract"),
        ("migrations", "migrations"),
        ("workflowCompatibility", "workflow.compatibility"),
        ("verificationEvidence", "verification.evidence"),
        ("rolloutGates", "rollout.gates"),
        ("rollback", "rollback.constraints"),
        ("retention", "retention"),
    ] {
        let before = from_content.get(field);
        let after = to_content.get(field);
        if before != after {
            entries.push(ReleaseDiffEntry {
                subject: subject.to_owned(),
                before: before.map(stable_json),
                after: after.map(stable_json),
            });
        }
    }
    ReleaseDiff {
        protocol: "lenso.service-release-diff.v1",
        from_release_id: from.release_id.clone(),
        to_release_id: to.release_id.clone(),
        entries,
        effects: Effects::default(),
    }
}

#[cfg(test)]
fn stable_json(value: &Value) -> String {
    serde_json::to_string(value).expect("Service Release diff value must serialize")
}

fn validate_deployment_plan(value: &Value) -> Result<()> {
    ensure_secret_free(value)?;
    let plan: lenso_service::DeploymentPlan = serde_json::from_value(value.clone())
        .context("deployment_input_invalid: Deployment plan is not the shared adapter contract")?;
    if !lenso_service::deployment_plan_integrity_is_valid(&plan) {
        bail!("deployment_input_invalid: shared Deployment plan validation failed")
    }
    Ok(())
}

#[cfg(test)]
fn content_from_input(input: &ServiceReleaseInput) -> ServiceReleaseContent<'_> {
    ServiceReleaseContent {
        protocol: SERVICE_RELEASE_PROTOCOL,
        service_id: &input.service_id,
        service_version: &input.service_version,
        modules: &input.modules,
        workloads: &input.workloads,
        contract_versions: &input.contract_versions,
        config_contract: &input.config_contract,
        reliability_contract: &input.reliability_contract,
        migrations: &input.migrations,
        workflow_compatibility: &input.workflow_compatibility,
        verification_evidence: &input.verification_evidence,
        rollout_gates: &input.rollout_gates,
        rollback: input.rollback,
        retention: input.retention,
    }
}

#[cfg(test)]
fn content_from_release(release: &ServiceRelease) -> ServiceReleaseContent<'_> {
    ServiceReleaseContent {
        protocol: &release.protocol,
        service_id: &release.service_id,
        service_version: &release.service_version,
        modules: &release.modules,
        workloads: &release.workloads,
        contract_versions: &release.contract_versions,
        config_contract: &release.config_contract,
        reliability_contract: &release.reliability_contract,
        migrations: &release.migrations,
        workflow_compatibility: &release.workflow_compatibility,
        verification_evidence: &release.verification_evidence,
        rollout_gates: &release.rollout_gates,
        rollback: release.rollback,
        retention: release.retention,
    }
}

fn ensure_secret_free(value: &Value) -> Result<()> {
    fn visit(value: &Value) -> Option<String> {
        match value {
            Value::Object(object) => object.iter().find_map(|(key, value)| {
                let key = key.to_ascii_lowercase();
                if [
                    "secretvalue",
                    "password",
                    "credential",
                    "privatekey",
                    "signingkey",
                ]
                .iter()
                .any(|forbidden| key.contains(forbidden))
                {
                    Some(key)
                } else {
                    visit(value)
                }
            }),
            Value::Array(values) => values.iter().find_map(visit),
            _ => None,
        }
    }
    if let Some(key) = visit(value) {
        bail!(
            "plaintext_secret_detected: delivery artifact contains forbidden value-shaped field `{key}`"
        )
    }
    Ok(())
}

fn require_fields(value: &Value, fields: &[&str]) -> Result<()> {
    for field in fields {
        if value.get(field).is_none_or(Value::is_null) {
            bail!("delivery_input_invalid: required field `{field}` is missing")
        }
    }
    Ok(())
}

fn check_protocol(value: &Value, expected: &str) -> Result<()> {
    if value["protocol"] != expected {
        bail!("delivery_input_invalid: expected protocol `{expected}`")
    }
    Ok(())
}

fn required_text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("delivery_input_invalid: `{field}` must be a string"))
}

fn kubernetes_name(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[cfg(test)]
fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
fn issue(code: &str, message: &str, remediation: &str, next_action: &str) -> DeliveryIssue {
    DeliveryIssue {
        code: code.to_owned(),
        message: message.to_owned(),
        remediation: remediation.to_owned(),
        next_actions: vec![next_action.to_owned()],
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn issues_error(issues: Vec<DeliveryIssue>) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "protocol": "lenso.delivery-rejection.v1",
            "issues": issues,
            "effects": Effects::default()
        }))
        .expect("delivery rejection must serialize")
    )
}

fn digest_json(value: &impl Serialize) -> String {
    digest_bytes(serde_json::to_vec(value).expect("canonical delivery values must serialize"))
}

fn digest_bytes(value: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(value.as_ref());
    let mut rendered = String::with_capacity(7 + digest.len() * 2);
    rendered.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut rendered, "{byte:02x}").expect("writing to String cannot fail");
    }
    rendered
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read delivery artifact `{}`", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse delivery artifact `{}`", path.display()))
}

fn write_json(value: &impl Serialize, output: Option<&Path>) -> Result<()> {
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(value).context("failed to render stable delivery JSON")?
    );
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        fs::write(path, rendered)
            .with_context(|| format!("failed to write `{}`", path.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_assembly_is_canonical_and_environment_independent() {
        let mut input = fixture_input();
        let first = assemble(input.clone()).expect("fixture should assemble");
        input.modules.reverse();
        input.workloads.reverse();
        let reordered = assemble(input).expect("reordered fixture should assemble");
        assert_eq!(first.release_id, reordered.release_id);
        let rendered = serde_json::to_string(&first).unwrap();
        for forbidden in ["environment", "namespace", "replicas", "secretValue"] {
            assert!(!rendered.contains(forbidden));
        }

        let mut changed_input = fixture_input();
        changed_input.config_contract = DeliveryEvidenceReference {
            reference: "config:v2".to_owned(),
            digest: digest_bytes("config:v2"),
        };
        let changed = assemble(changed_input).expect("changed fixture should assemble");
        let diff = release_diff(&first, &changed);
        assert_eq!(diff.entries.len(), 1);
        assert_eq!(diff.entries[0].subject, "config.contract");
    }

    #[test]
    fn operator_export_validation_requires_shared_digest_pinned_plan() {
        let release_digest = digest_json(&"release");
        let workload = DeploymentWorkloadPlan {
            workload_id: "support-api".to_owned(),
            role: ReleaseWorkloadRole::Api,
            artifact_reference: "ghcr.io/liorael/support".to_owned(),
            artifact_digest: digest_json(&"api"),
            media_type: "application/vnd.oci.image.manifest.v1+json".to_owned(),
            settings: DeploymentWorkloadSettings {
                workload_id: "support-api".to_owned(),
                replicas: 2,
                port: Some(8080),
                command: Vec::new(),
                health_path: Some("/health/ready".to_owned()),
                disruption_min_available: Some(1),
            },
        };
        let release_id = format!("service-release:{release_digest}");
        let policy_evidence_references = vec!["policy-evidence:test".to_owned()];
        let next_actions = Vec::<String>::new();
        let effects = DeliveryEffectsArtifact::default();
        let plan_digest = digest_json(&DeploymentPlanDigestInput {
            protocol: DEPLOYMENT_PLAN_PROTOCOL,
            adapter: DeploymentAdapterKind::Kubernetes,
            environment: "production",
            expected_environment_revision: 5,
            release_id: &release_id,
            release_digest: &release_digest,
            service_id: "service:support",
            config_revision_id: "config:5",
            secret_reference_ids: &[],
            endpoints: &BTreeMap::new(),
            placement: &BTreeMap::new(),
            workloads: std::slice::from_ref(&workload),
            adapter_inputs: &BTreeMap::new(),
            gateway_plan_digest: "gateway:5",
            policy_evidence_references: &policy_evidence_references,
            rollback_capable: true,
            next_actions: &next_actions,
            effects: &effects,
        });
        let plan = serde_json::to_value(DeploymentPlanArtifact {
            protocol: DEPLOYMENT_PLAN_PROTOCOL.to_owned(),
            plan_id: format!("deployment-plan:{plan_digest}"),
            plan_digest,
            adapter: DeploymentAdapterKind::Kubernetes,
            environment: "production".to_owned(),
            expected_environment_revision: 5,
            release_id,
            release_digest,
            service_id: "service:support".to_owned(),
            config_revision_id: "config:5".to_owned(),
            secret_reference_ids: Vec::new(),
            endpoints: BTreeMap::new(),
            placement: BTreeMap::new(),
            workloads: vec![workload],
            adapter_inputs: BTreeMap::new(),
            gateway_plan_digest: "gateway:5".to_owned(),
            policy_evidence_references,
            rollback_capable: true,
            next_actions,
            effects,
        })
        .unwrap();
        validate_deployment_plan(&plan).expect("shared plan should validate");
        let resource = operator_resource_from_plan(&plan).unwrap();
        let resource_digest = digest_json(&resource);
        let export = json!({
            "protocol": OPERATOR_EXPORT_PROTOCOL,
            "resourceDigest": resource_digest,
            "deploymentPlanId": plan["planId"].clone(),
            "deploymentPlanDigest": plan["planDigest"].clone(),
            "resource": resource,
        });
        validate_operator_export(&export, &plan).expect("exact export should validate");
        let mut substituted = export.clone();
        substituted["resource"]["spec"]["workloads"][0]["image"] =
            json!("ghcr.io/attacker/substitute@sha256:changed");
        substituted["resourceDigest"] = json!(digest_json(&substituted["resource"]));
        assert!(validate_operator_export(&substituted, &plan).is_err());
        let mut substituted_rollback = export;
        substituted_rollback["resource"]["spec"]["rollbackReleaseId"] =
            json!("service-release:sha256:forged");
        substituted_rollback["resourceDigest"] =
            json!(digest_json(&substituted_rollback["resource"]));
        assert!(validate_operator_export(&substituted_rollback, &plan).is_err());
        let mut forged_capability = plan.clone();
        forged_capability["rollbackCapable"] = json!(false);
        assert!(validate_deployment_plan(&forged_capability).is_err());
        let mut forged_effects = plan.clone();
        forged_effects["effects"]["mutatesDeployment"] = json!(true);
        assert!(validate_deployment_plan(&forged_effects).is_err());
        let mut unsafe_plan = plan;
        unsafe_plan["serviceId"] = json!("service:other");
        assert!(validate_deployment_plan(&unsafe_plan).is_err());
        let mut unsafe_plan = unsafe_plan;
        unsafe_plan["workloads"][0]["artifactDigest"] = json!("latest");
        assert!(validate_deployment_plan(&unsafe_plan).is_err());
    }

    #[test]
    fn policy_requires_source_inputs_and_evaluates_them_canonically() {
        let input = passing_policy_input();
        let trust_root = BTreeMap::from([(
            input.release.signatures[0].signer.clone(),
            "test-key".to_owned(),
        )]);
        let eligibility_root =
            BTreeMap::from([(input.eligibility.provider_id.clone(), "test-key".to_owned())]);
        let secret_root = BTreeMap::new();
        let evidence = evaluate_policy_input(&input, &trust_root, &eligibility_root, &secret_root)
            .expect("canonical inputs should evaluate");
        assert_eq!(evidence.decision, PolicyDecision::Passed);
        assert!(policy_evidence_integrity_is_valid(&evidence));
        let mut forged = input.clone();
        forged.trust.release_id = "service-release:forged".to_owned();
        let forged = evaluate_policy_input(&forged, &trust_root, &eligibility_root, &secret_root)
            .expect("forged facts must evaluate safely");
        assert_eq!(forged.decision, PolicyDecision::Blocked);
        let mut forged_signature = input.clone();
        forged_signature.release.signatures[0].signature = digest_json(&"forged-signature");
        forged_signature.trust.signatures[0].status = ReleaseSignerStatus::Trusted;
        let forged_signature = evaluate_policy_input(
            &forged_signature,
            &trust_root,
            &eligibility_root,
            &secret_root,
        )
        .expect("forged signature status must evaluate safely");
        assert_eq!(forged_signature.decision, PolicyDecision::Blocked);
        let mut forged_eligibility = input.clone();
        forged_eligibility.eligibility_input["workloadIdentityProduction"] = Value::Null;
        forged_eligibility
            .eligibility
            .facts
            .insert("identity.production".to_owned(), Some(true));
        let forged_eligibility = evaluate_policy_input(
            &forged_eligibility,
            &trust_root,
            &eligibility_root,
            &secret_root,
        )
        .expect("forged eligibility facts must evaluate safely");
        assert_eq!(forged_eligibility.decision, PolicyDecision::Blocked);

        let root = std::env::temp_dir().join(format!(
            "lenso-cli-m5-policy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let evidence_path = root.join("evidence.json");
        let input_path = root.join("input.json");
        let output_path = root.join("output.json");
        std::fs::write(&evidence_path, serde_json::to_vec(&evidence).unwrap()).unwrap();
        std::fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();

        let error = check_policy_evidence_with_trust(
            &evidence_path,
            None,
            &trust_root,
            &eligibility_root,
            &secret_root,
        )
        .expect_err("precomputed evidence must not be trusted without its sources");
        assert!(error.to_string().contains("policy_source_required"));
        check_policy_evidence_with_trust(
            &input_path,
            Some(&output_path),
            &trust_root,
            &eligibility_root,
            &secret_root,
        )
        .expect("source inputs should produce canonical evidence");
        let evaluated: PolicyEvidenceArtifact = read_json(&output_path).unwrap();
        assert_eq!(evaluated, evidence);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn operator_resource_names_sanitize_opaque_identifiers() {
        assert_eq!(
            kubernetes_name("secret:support:database:v5"),
            "secret-support-database-v5"
        );
        assert_eq!(kubernetes_name("service:support"), "service-support");
    }

    #[test]
    fn config_revision_requires_the_trusted_secret_provider_observation() {
        let fields = vec![ConfigFieldArtifact {
            path: "DB_PASSWORD".to_owned(),
            value_type: ConfigValueType::String,
            required: true,
            sensitivity: ConfigFieldSensitivity::Sensitive,
            scope: ConfigFieldScope::Service,
            activation: ConfigFieldActivation::Restart,
            mutable: true,
        }];
        let contract_digest = digest_json(&(
            "lenso.config-contract.v1",
            "config:secret:v1",
            fields.as_slice(),
        ));
        let contract = ConfigContractArtifact {
            protocol: "lenso.config-contract.v1".to_owned(),
            reference: "config:secret:v1".to_owned(),
            digest: contract_digest.clone(),
            fields,
        };
        let secret_references = vec![SecretReference {
            reference_id: "secret:support:database:v5".to_owned(),
            provider: "vault:test".to_owned(),
            purpose: "DB_PASSWORD".to_owned(),
            scope: "service:support".to_owned(),
            status: SecretReferenceStatus::Resolved,
            metadata: BTreeMap::from([("rotationRevision".to_owned(), "7".to_owned())]),
        }];
        let impacts = vec![ConfigImpact {
            path: "DB_PASSWORD".to_owned(),
            scope: ConfigFieldScope::Service,
            activation: ConfigFieldActivation::Restart,
            mutable: true,
        }];
        let values = BTreeMap::new();
        let revision_digest = digest_json(&(
            "lenso.config-revision.v1",
            "service:support",
            contract.reference.as_str(),
            contract.digest.as_str(),
            &values,
            &secret_references,
            &impacts,
        ));
        let config = ConfigRevisionArtifact {
            protocol: "lenso.config-revision.v1".to_owned(),
            revision_id: format!("config-revision:{revision_digest}"),
            revision_digest,
            service_id: "service:support".to_owned(),
            contract_reference: contract.reference.clone(),
            contract_digest,
            values,
            secret_references,
            impacts,
        };
        let trusted = BTreeMap::from([(
            "secret:support:database:v5".to_owned(),
            TrustedSecretObservation {
                provider: "vault:test".to_owned(),
                status: SecretReferenceStatus::Resolved,
                metadata: BTreeMap::from([("rotationRevision".to_owned(), "7".to_owned())]),
            },
        )]);
        assert!(config_revision_matches_contract(
            &config, &contract, &trusted
        ));
        assert!(!config_revision_matches_contract(
            &config,
            &contract,
            &BTreeMap::new(),
        ));
        let mut stale = trusted;
        stale
            .get_mut("secret:support:database:v5")
            .expect("observation exists")
            .metadata
            .insert("rotationRevision".to_owned(), "6".to_owned());
        assert!(!config_revision_matches_contract(
            &config, &contract, &stale
        ));
    }

    #[test]
    fn gateway_plan_binds_release_catalog_attestation_and_all_public_fields() {
        let effects = DeliveryEffectsArtifact::default();
        let binding = lenso_service::GatewayEnvironmentBinding {
            environment: "production".to_owned(),
            gateway_adapter: "nginx:test".to_owned(),
            public_origin: "https://support.example.test".to_owned(),
            expected_gateway_revision: 9,
        };
        let mut plan = json!({
            "protocol": GATEWAY_PLAN_PROTOCOL,
            "planId": "",
            "planDigest": "",
            "edgeContractId": "edge-contract:test",
            "edgeContractDigest": digest_json(&"edge-contract"),
            "edgeReleaseId": "service-release:test",
            "edgeReleaseDigest": digest_json(&"release"),
            "operationCatalogDigest": digest_json(&"catalog"),
            "edgeProviderId": "edge:test",
            "edgeProviderProof": "proof:test",
            "environment": "production",
            "gatewayAdapter": "nginx:test",
            "publicOrigin": "https://support.example.test",
            "expectedGatewayRevision": 9,
            "configurationIdentity": digest_json(&"configuration"),
            "routes": [],
            "diff": [],
            "drifted": false,
            "issues": [],
            "nextActions": ["apply"],
            "effects": effects,
        });
        let authority_subject = digest_json(&(
            "lenso.edge-authority-subject.v1",
            "service-release:test",
            plan["edgeReleaseDigest"].as_str().unwrap(),
            plan["operationCatalogDigest"].as_str().unwrap(),
            &plan["routes"],
        ));
        plan["edgeProviderProof"] =
            json!(digest_bytes(format!("edge-test-key\0{authority_subject}")));
        let digest = digest_json(&(
            GATEWAY_PLAN_PROTOCOL,
            "edge-contract:test",
            plan["edgeContractDigest"].as_str().unwrap(),
            "service-release:test",
            plan["edgeReleaseDigest"].as_str().unwrap(),
            plan["operationCatalogDigest"].as_str().unwrap(),
            "edge:test",
            plan["edgeProviderProof"].as_str().unwrap(),
            binding,
            plan["configurationIdentity"].as_str().unwrap(),
            Vec::<lenso_service::ResolvedEdgeRoute>::new(),
            Vec::<lenso_service::GatewayPlanDiffEntry>::new(),
            false,
            Vec::<lenso_service::DeliveryIssue>::new(),
            vec!["apply".to_owned()],
            lenso_service::DeliveryEffects::default(),
        ));
        plan["planDigest"] = json!(digest);
        plan["planId"] = json!(format!(
            "gateway-plan:{}",
            plan["planDigest"].as_str().unwrap()
        ));
        let trust = BTreeMap::from([("edge:test".to_owned(), "edge-test-key".to_owned())]);
        validate_gateway_plan(&plan, &trust).expect("trusted gateway should validate");
        plan["issues"] = json!([{"code": "forged"}]);
        assert!(validate_gateway_plan(&plan, &trust).is_err());
    }

    fn passing_policy_input() -> PolicyEvaluationInput {
        let fields = vec![ConfigFieldArtifact {
            path: "MAX_CONCURRENCY".to_owned(),
            value_type: ConfigValueType::Integer,
            required: true,
            sensitivity: ConfigFieldSensitivity::Public,
            scope: ConfigFieldScope::Service,
            activation: ConfigFieldActivation::Hot,
            mutable: true,
        }];
        let contract_digest =
            digest_json(&("lenso.config-contract.v1", "config:v1", fields.as_slice()));
        let config_contract = ConfigContractArtifact {
            protocol: "lenso.config-contract.v1".to_owned(),
            reference: "config:v1".to_owned(),
            digest: contract_digest,
            fields,
        };
        let mut release_input = fixture_input();
        release_input.config_contract = DeliveryEvidenceReference {
            reference: config_contract.reference.clone(),
            digest: config_contract.digest.clone(),
        };
        let mut release = assemble(release_input).expect("release fixture should assemble");
        release.signatures.push(ReleaseSignature {
            signer: "ci:trusted".to_owned(),
            subject_digest: release.release_digest.clone(),
            signature: digest_bytes(format!("test-key\0{}", release.release_digest)),
        });
        let values = BTreeMap::from([("MAX_CONCURRENCY".to_owned(), json!(32))]);
        let secret_references = Vec::new();
        let impacts = vec![ConfigImpact {
            path: "MAX_CONCURRENCY".to_owned(),
            scope: ConfigFieldScope::Service,
            activation: ConfigFieldActivation::Hot,
            mutable: true,
        }];
        let revision_digest = digest_json(&(
            "lenso.config-revision.v1",
            release.service_id.as_str(),
            release.config_contract.reference.as_str(),
            release.config_contract.digest.as_str(),
            &values,
            &secret_references,
            &impacts,
        ));
        let config = ConfigRevisionArtifact {
            protocol: "lenso.config-revision.v1".to_owned(),
            revision_id: format!("config-revision:{revision_digest}"),
            revision_digest,
            service_id: release.service_id.clone(),
            contract_reference: release.config_contract.reference.clone(),
            contract_digest: release.config_contract.digest.clone(),
            values,
            secret_references,
            impacts,
        };
        let system_graph_digest = digest_json(&"support-system");
        let mut eligibility_input = json!({
            "releaseId": release.release_id,
            "releaseDigest": release.release_digest,
            "providerId": "eligibility:test",
            "providerProof": "",
            "systemGraphDigest": system_graph_digest.clone(),
            "contracts": [{
                "contractId": "support-http",
                "currentMajor": 1,
                "candidateMajor": 1,
                "compatible": true,
                "activeConsumers": [],
                "consumerMigrationEvidence": true,
                "retiring": false,
                "deprecationWindowComplete": false
            }],
            "migrations": [{
                "migrationId": "support-0001",
                "lineageId": "support-0001",
                "sequence": 1,
                "phase": "expand",
                "verified": true
            }],
            "workflows": {
                "newStartsCompatible": true,
                "inFlightCompatible": true,
                "downgradeSafe": true
            },
            "rollback": {
                "priorReleaseCompatible": true,
                "schemaCompatible": true,
                "workflowCompatible": true,
                "configCompatible": true,
                "secretReferencesCompatible": true,
                "edgeCompatible": true,
                "adapterCapable": true,
                "previousReleaseId": "service-release:previous",
                "previousReleaseDigest": "sha256:previous-release",
                "previousDeploymentPlanId": "deployment-plan:previous",
                "previousDeploymentPlanDigest": "sha256:previous-deployment",
                "previousConfigRevisionId": "config-revision:previous",
                "previousConfigRevisionDigest": "sha256:previous-config",
                "previousSecretReferenceIds": ["secret:previous"],
                "previousGatewayPlanId": "gateway-plan:previous",
                "previousGatewayPlanDigest": "sha256:previous-gateway",
                "previousGatewayConfigurationIdentity": "gateway-config:previous",
                "previousAdapter": "kubernetes"
            },
            "providerCompatibilityVerified": true,
            "workloadIdentityProduction": true,
            "tenancyModeProduction": true,
            "tenantContextEnforced": true,
            "callPoliciesDeclared": true,
            "dependenciesReady": true,
            "resilienceDeclared": true,
            "reliabilityContractComplete": true,
            "edgeContractValid": true,
            "environmentVerificationFresh": true
        });
        let provider = lenso_service::DeterministicTrustProvider::new([
            ("ci:trusted", "test-key"),
            ("eligibility:test", "test-key"),
        ]);
        let core_release: lenso_service::ServiceRelease =
            serde_json::from_value(serde_json::to_value(&release).unwrap()).unwrap();
        let core_eligibility_input: lenso_service::ProductionEligibilityInput =
            serde_json::from_value(eligibility_input.clone()).unwrap();
        let core_eligibility_input = lenso_service::attest_production_eligibility_input(
            &core_release,
            &provider,
            "eligibility:test",
            core_eligibility_input,
        )
        .unwrap();
        eligibility_input = serde_json::to_value(&core_eligibility_input).unwrap();
        let trust: ReleaseTrustEvidenceArtifact = serde_json::from_value(
            serde_json::to_value(lenso_service::verify_service_release_trust(
                &core_release,
                &provider,
            ))
            .unwrap(),
        )
        .unwrap();
        let eligibility: EligibilityEvidenceArtifact = serde_json::from_value(
            serde_json::to_value(lenso_service::evaluate_production_eligibility(
                &core_eligibility_input,
                &core_release,
                &provider,
            ))
            .unwrap(),
        )
        .unwrap();
        PolicyEvaluationInput {
            protocol: POLICY_EVALUATION_INPUT_PROTOCOL.to_owned(),
            trust,
            eligibility_input,
            eligibility,
            release,
            config_contract,
            config,
        }
    }

    fn fixture_input() -> ServiceReleaseInput {
        let digest = |value: &str| digest_bytes(value.as_bytes());
        let evidence = |reference: &str| DeliveryEvidenceReference {
            reference: reference.to_owned(),
            digest: digest(reference),
        };
        let workload = |workload_id: &str, role: ReleaseWorkloadRole| WorkloadArtifact {
            workload_id: workload_id.to_owned(),
            role,
            artifact_reference: "ghcr.io/liorael/support".to_owned(),
            artifact_digest: digest(workload_id),
            media_type: "application/vnd.oci.image.manifest.v1+json".to_owned(),
            display_tag: Some("5.0.0".to_owned()),
            sbom: evidence(&format!("sbom:{workload_id}")),
            provenance: ReleaseProvenance {
                reference: format!("provenance:{workload_id}"),
                digest: digest(&format!("provenance:{workload_id}")),
                source: "https://github.com/LioRael/lenso-examples".to_owned(),
                builder: "https://github.com/LioRael/lenso-examples/actions".to_owned(),
                input_digests: vec![digest("source")],
                subject_digests: vec![digest(workload_id)],
            },
            signature_subject: format!("workload:{workload_id}"),
        };
        ServiceReleaseInput {
            service_id: "service:support".to_owned(),
            service_version: "5.0.0".to_owned(),
            modules: vec![
                ReleaseModule {
                    module_id: "support-ticket".to_owned(),
                    module_version: "4.0.0".to_owned(),
                },
                ReleaseModule {
                    module_id: "support-sla".to_owned(),
                    module_version: "2.0.0".to_owned(),
                },
            ],
            workloads: vec![
                workload("support-worker", ReleaseWorkloadRole::Worker),
                workload("support-api", ReleaseWorkloadRole::Api),
                workload("support-migration", ReleaseWorkloadRole::Migration),
            ],
            contract_versions: vec![ReleaseContractVersion {
                contract_id: "support-http".to_owned(),
                version: "v1".to_owned(),
                kind: "request_response".to_owned(),
                artifact: evidence("contract:http:v1"),
            }],
            config_contract: evidence("config:v1"),
            reliability_contract: evidence("reliability:v1"),
            migrations: vec![ReleaseMigration {
                migration_id: "support-0001".to_owned(),
                phase: "expand".to_owned(),
                artifact: evidence("migration:0001"),
                reversible: true,
            }],
            workflow_compatibility: vec![evidence("workflow:v1")],
            verification_evidence: vec![evidence("verification:m4")],
            rollout_gates: vec![ReleaseRolloutGate {
                gate_id: "reliability".to_owned(),
                evidence_kind: "service_reliability".to_owned(),
                required: true,
            }],
            rollback: ReleaseRollbackConstraints {
                previous_release_required: true,
                automatic_allowed: true,
                blocked_by_irreversible_migration: true,
            },
            retention: ReleaseRetention {
                evidence_days: 90,
                artifact_days: 365,
            },
        }
    }
}
