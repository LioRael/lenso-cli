use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const SUPPORT_PROTOCOL: &str = "lenso.ga-support-manifest.v1";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportManifest {
    protocol: String,
    manifest_id: String,
    manifest_digest: String,
    status: SupportStatus,
    components: Vec<SupportComponent>,
    manifest_formats: Vec<ManifestFormat>,
    state_versions: Vec<String>,
    adapter_versions: BTreeMap<String, String>,
    documentation: DocumentationIdentity,
    combinations: Vec<SupportCombination>,
    upgrade_edges: Vec<UpgradeEdge>,
    #[serde(default)]
    evidence_receipt_authorities: BTreeMap<String, String>,
    #[serde(default)]
    receipt_authority_public_keys: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportManifestInput {
    status: SupportStatus,
    components: Vec<SupportComponent>,
    manifest_formats: Vec<ManifestFormat>,
    state_versions: Vec<String>,
    adapter_versions: BTreeMap<String, String>,
    documentation: DocumentationIdentity,
    combinations: Vec<SupportCombination>,
    upgrade_edges: Vec<UpgradeEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum SupportStatus {
    Candidate,
    GeneralAvailability,
    Deprecated,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ComponentKind {
    Cli,
    Runtime,
    Contracts,
    Provider,
    Operator,
    RuntimeConsole,
    FirstPartyModule,
    Skill,
}

impl ComponentKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Runtime => "runtime",
            Self::Contracts => "contracts",
            Self::Provider => "provider",
            Self::Operator => "operator",
            Self::RuntimeConsole => "runtime_console",
            Self::FirstPartyModule => "first_party_module",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportComponent {
    kind: ComponentKind,
    component_id: String,
    version: String,
    digest: String,
}

impl SupportComponent {
    fn reference(&self) -> String {
        format!(
            "{}:{}@{}",
            self.kind.as_str(),
            self.component_id,
            self.version
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupportCombination {
    combination_id: String,
    component_references: Vec<String>,
    state_version: String,
    status: SupportStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ManifestKind {
    Provider,
    Service,
    System,
    Module,
    Backup,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFormat {
    kind: ManifestKind,
    version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentationIdentity {
    version: String,
    digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpgradeEdge {
    edge_id: String,
    source_format: String,
    target_format: String,
    #[serde(default)]
    mixed_version_references: Vec<String>,
    rollback_safe: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Issue {
    code: &'static str,
    message: String,
    remediation: String,
    next_actions: Vec<String>,
}

pub(crate) fn support_check(
    manifest_path: &Path,
    components: &[String],
    state_version: &str,
    json_output: bool,
) -> Result<()> {
    let manifest = read_manifest(manifest_path)?;
    let requested = components.iter().cloned().collect::<BTreeSet<_>>();
    let found = manifest.combinations.iter().find(|combination| {
        combination
            .component_references
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            == requested
            && combination.state_version == state_version
    });
    let (decision, combination_id, issues, next_actions) = match found {
        Some(combination) if combination.status == SupportStatus::GeneralAvailability => (
            "supported",
            Some(combination.combination_id.clone()),
            Vec::new(),
            vec!["Proceed with the exact manifest-bound component set.".to_owned()],
        ),
        Some(combination) => (
            "unsupported",
            Some(combination.combination_id.clone()),
            vec![issue(
                "ga_combination_unsupported",
                format!(
                    "The exact combination has '{}' status and is not supported for GA.",
                    format!("{:?}", combination.status).to_lowercase()
                ),
                "Select a General Availability combination from the GA Support Manifest.",
                "Inspect the support status and migration guidance.",
            )],
            vec!["Select a supported manifest combination.".to_owned()],
        ),
        None => (
            "unknown",
            None,
            vec![issue(
                "ga_combination_unknown",
                "The exact combination is absent from the GA Support Manifest.",
                "Do not infer compatibility from semantic-version proximity.",
                "Select an exact combination or collect reviewed compatibility evidence.",
            )],
            vec!["Choose an exact combination from the support manifest.".to_owned()],
        ),
    };
    let report = json!({
        "protocol":"lenso.ga-support-evaluation.v1",
        "manifestId":manifest.manifest_id,
        "manifestDigest":manifest.manifest_digest,
        "decision":decision,
        "combinationId":combination_id,
        "issues":issues,
        "nextActions":next_actions,
        "effects":{"mutatesSystem":false}
    });
    print_value(&report, json_output)?;
    if decision == "supported" {
        Ok(())
    } else {
        bail!("GA support evaluation returned {decision}")
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn manifest_migrate(
    manifest_path: &Path,
    source_path: &Path,
    target_format: &str,
    identity_pointers: &[String],
    target_path: Option<&Path>,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    let manifest = read_manifest(manifest_path)?;
    let source: Value = read_json(source_path)?;
    let source_format = source
        .get("protocol")
        .and_then(Value::as_str)
        .context("source manifest must declare protocol")?;
    let kind = if source_format.contains("system") {
        ManifestKind::System
    } else if source_format.contains("service") {
        ManifestKind::Service
    } else {
        bail!("unsupported manifest source protocol {source_format}")
    };
    let formats = manifest
        .manifest_formats
        .iter()
        .map(|format| (format.kind, format.version.as_str()))
        .collect::<BTreeSet<_>>();
    if !formats.contains(&(kind, source_format)) || !formats.contains(&(kind, target_format)) {
        bail!("manifest_format_unsupported: requested edge is absent from support manifest")
    }
    let mut migrated = source.clone();
    migrated
        .as_object_mut()
        .context("source manifest must be an object")?
        .insert(
            "protocol".to_owned(),
            Value::String(target_format.to_owned()),
        );
    for pointer in identity_pointers {
        if source.pointer(pointer) != migrated.pointer(pointer) {
            bail!("manifest_identity_changed: {pointer}")
        }
    }
    let source_digest = digest(&source);
    let migrated_digest = digest(&migrated);
    let plan_content = json!({
        "protocol":"lenso.manifest-migration-plan.v1",
        "manifestId":manifest.manifest_id,
        "manifestDigest":manifest.manifest_digest,
        "kind":kind,
        "sourceFormat":source_format,
        "targetFormat":target_format,
        "sourceDigest":source_digest,
        "migratedDigest":migrated_digest,
        "migrated":migrated,
        "identityPointers":identity_pointers,
        "effects":{"mutatesSource":false,"createsTarget":!dry_run}
    });
    let plan_digest = digest(&plan_content);
    let mut plan = plan_content;
    plan["planId"] = json!(format!("manifest-migration:{}", &plan_digest[7..23]));
    plan["planDigest"] = json!(plan_digest.clone());
    if !dry_run {
        let target = target_path.context("--target is required unless --dry-run is used")?;
        if target.exists() {
            let committed = read_json(target)?;
            if committed != plan["migrated"] {
                bail!(
                    "manifest_target_collision: {} contains different content",
                    target.display()
                )
            }
        } else {
            write_json(target, &plan["migrated"])?;
        }
        let receipt = json!({
            "protocol":"lenso.manifest-migration-receipt.v1",
            "receiptId":format!("manifest-migration-receipt:{}", &plan_digest[7..23]),
            "planDigest":plan_digest,
            "sourceDigest":source_digest,
            "migratedDigest":migrated_digest,
            "migrated":plan["migrated"].clone(),
            "target":target,
            "committed":true
        });
        return print_value(&receipt, json_output);
    }
    print_value(&plan, json_output)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpgradeInput {
    service_id: String,
    from_release_id: String,
    from_release_digest: String,
    to_release_id: String,
    to_release_digest: String,
    config_revision_id: String,
    config_revision_digest: String,
    source_state_version: String,
    target_state_version: String,
    #[serde(default)]
    workflow_artifact_digests: Vec<String>,
}

pub(crate) fn service_upgrade(
    manifest_path: &Path,
    input_path: &Path,
    json_output: bool,
) -> Result<()> {
    let manifest = read_manifest(manifest_path)?;
    let input: UpgradeInput = read_typed(input_path)?;
    let edge = manifest
        .upgrade_edges
        .iter()
        .find(|edge| {
            edge.source_format == input.source_state_version
                && edge.target_format == input.target_state_version
        })
        .context("service_upgrade_unsupported: state edge is absent from support manifest")?;
    let plan_content = json!({
        "protocol":"lenso.service-upgrade-plan.v1",
        "manifestId":manifest.manifest_id,
        "manifestDigest":manifest.manifest_digest,
        "edgeId":edge.edge_id,
        "input":input,
        "steps":[
            {"sequence":1,"workload":"migration","precondition":"exact source state and release digest remain current"},
            {"sequence":2,"workload":"api","precondition":"migration receipt is complete and target reader is compatible"},
            {"sequence":3,"workload":"worker","precondition":"migration receipt is complete and pinned workflows are structurally compatible"}
        ],
        "mixedVersionReferences":edge.mixed_version_references,
        "preservedIdentities":["service","workflow_instance","workflow_definition_artifact","inbox","outbox","timer","attempt","compensation","story_segment","config_revision","deployment_observation"],
        "rollback":{
            "automaticAllowed":edge.rollback_safe,
            "approvalBoundary":if edge.rollback_safe { Value::Null } else { json!("service_state_upgrade_intervention") }
        },
        "effects":{"mutatesState":false,"mutatesDeployment":false}
    });
    let plan_digest = digest(&plan_content);
    let mut plan = plan_content;
    plan["planId"] = json!(format!("service-upgrade:{}", &plan_digest[7..23]));
    plan["planDigest"] = json!(plan_digest);
    print_value(&plan, json_output)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetirementInput {
    system_graph_digest: String,
    environment_evidence_digest: String,
    evidence_fresh: bool,
    contract_id: String,
    retiring_version: String,
    replacement_version: String,
    deprecation_window_complete: bool,
    consumers: Vec<ConsumerEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsumerEvidence {
    consumer_id: String,
    active_version: Option<String>,
    replacement_verified: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetirementApproval {
    plan_digest: String,
    approver: String,
    reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractRetirementReceipt {
    protocol: String,
    receipt_id: String,
    receipt_digest: String,
    plan_digest: String,
    contract_id: String,
    retired_version: String,
    replacement_version: String,
    approver: String,
    approval_reason: String,
    retired: bool,
}

pub(crate) fn contract_retire(
    input_path: &Path,
    approval_path: Option<&Path>,
    output_path: Option<&Path>,
    json_output: bool,
) -> Result<()> {
    let input: RetirementInput = read_typed(input_path)?;
    let mut issues = Vec::new();
    if !input.evidence_fresh {
        issues.push(issue(
            "retirement_evidence_stale",
            "Consumer or Environment Verification evidence is stale.",
            "Refresh exact System evidence.",
            "Regenerate the Retirement plan.",
        ));
    }
    if !input.deprecation_window_complete {
        issues.push(issue(
            "retirement_deprecation_incomplete",
            "The deprecation window has not elapsed.",
            "Continue serving the old Contract Version.",
            "Retry after the declared window.",
        ));
    }
    let active = input
        .consumers
        .iter()
        .filter(|consumer| consumer.active_version.as_deref() == Some(&input.retiring_version))
        .map(|consumer| consumer.consumer_id.clone())
        .collect::<Vec<_>>();
    if !active.is_empty() {
        issues.push(issue(
            "retirement_active_consumer",
            "An active Consumer still uses the retiring version.",
            "Move every Consumer to the replacement version.",
            "Refresh replacement Compatibility Verification.",
        ));
    }
    if input
        .consumers
        .iter()
        .any(|consumer| consumer.active_version.is_none() || !consumer.replacement_verified)
    {
        issues.push(issue(
            "retirement_replacement_missing",
            "Replacement coverage is incomplete or unknown.",
            "Verify every Consumer against the replacement.",
            "Refresh Consumer inventory.",
        ));
    }
    let decision = if issues.is_empty() {
        "supported"
    } else {
        "unsupported"
    };
    let input_digest = digest(&input);
    let plan_content = json!({
        "protocol":"lenso.contract-retirement-plan.v1",
        "inputDigest":input_digest,
        "decision":decision,
        "contractId":input.contract_id,
        "retiringVersion":input.retiring_version,
        "replacementVersion":input.replacement_version,
        "affectedConsumers":active,
        "irreversibleEffects":["stop serving the retired Contract Version"],
        "issues":issues,
        "effects":{"retiresContract":false,"mutatesConsumers":false},
        "approvalBoundary":"contract_retirement"
    });
    let plan_digest = digest(&plan_content);
    let mut plan = plan_content;
    plan["planId"] = json!(format!("contract-retirement:{}", &plan_digest[7..23]));
    plan["planDigest"] = json!(plan_digest);

    let Some(approval_path) = approval_path else {
        print_value(&plan, json_output)?;
        return if decision == "supported" {
            Ok(())
        } else {
            bail!("Contract Retirement is unsupported")
        };
    };
    if decision != "supported" {
        print_value(&plan, json_output)?;
        bail!("Contract Retirement preconditions failed before mutation")
    }
    let approval: RetirementApproval = read_typed(approval_path)?;
    if approval.plan_digest != plan_digest
        || approval.approver.trim().is_empty()
        || approval.reason.trim().is_empty()
    {
        bail!("retirement_approval_invalid: approval must bind the exact plan digest")
    }
    let mut receipt = ContractRetirementReceipt {
        protocol: "lenso.contract-retirement-receipt.v2".into(),
        receipt_id: String::new(),
        receipt_digest: String::new(),
        plan_digest,
        contract_id: input.contract_id,
        retired_version: input.retiring_version,
        replacement_version: input.replacement_version,
        approver: approval.approver,
        approval_reason: approval.reason,
        retired: true,
    };
    receipt.receipt_digest = digest(&receipt);
    receipt.receipt_id = format!(
        "contract-retirement-receipt:{}",
        &receipt.receipt_digest[7..23]
    );
    let receipt = serde_json::to_value(receipt)?;
    if let Some(path) = output_path {
        write_json(path, &receipt)?;
    }
    print_value(&receipt, json_output)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailureInput {
    scenario_id: String,
    condition: String,
    expected: String,
    observations: Vec<FailureObservation>,
    #[serde(default)]
    effects: Vec<String>,
    cleanup_complete: bool,
    adapter_version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct FailureObservation {
    subject: String,
    outcome: String,
    evidence_digest: String,
}

pub(crate) fn failure_evaluate(input_path: &Path, json_output: bool) -> Result<()> {
    let input: FailureInput = read_typed(input_path)?;
    let mut issues = Vec::new();
    if input.observations.is_empty()
        || input
            .observations
            .iter()
            .any(|observation| observation.outcome != input.expected)
    {
        issues.push(issue(
            "failure_unexpected_outcome",
            "Observed behavior differs from the declared Failure Scenario outcome.",
            "Preserve authoritative evidence and fail the scenario.",
            "Inspect business effects and Service-owned state.",
        ));
    }
    if !input.cleanup_complete {
        issues.push(issue(
            "failure_cleanup_incomplete",
            "Disposable failure resources were not cleaned or isolated.",
            "Finish deterministic cleanup.",
            "Remove only scenario-owned state before accepting evidence.",
        ));
    }
    let decision = if issues.is_empty() {
        "supported"
    } else {
        "unsupported"
    };
    let evidence_content = json!({
        "protocol":"lenso.failure-scenario-evidence.v1",
        "scenarioId":input.scenario_id,
        "condition":input.condition,
        "expected":input.expected,
        "observations":input.observations,
        "effects":input.effects,
        "cleanupComplete":input.cleanup_complete,
        "adapterVersion":input.adapter_version,
        "decision":decision,
        "issues":issues
    });
    let evidence_digest = digest(&evidence_content);
    let mut evidence = evidence_content;
    evidence["evidenceId"] = json!(format!("failure-evidence:{}", &evidence_digest[7..23]));
    evidence["evidenceDigest"] = json!(evidence_digest);
    print_value(&evidence, json_output)?;
    if decision == "supported" {
        Ok(())
    } else {
        bail!("Failure Scenario evidence is unsupported")
    }
}

fn read_manifest(path: &Path) -> Result<SupportManifest> {
    let manifest: SupportManifest = read_typed(path)?;
    let mut input = SupportManifestInput {
        status: manifest.status,
        components: manifest.components.clone(),
        manifest_formats: manifest.manifest_formats.clone(),
        state_versions: manifest.state_versions.clone(),
        adapter_versions: manifest.adapter_versions.clone(),
        documentation: manifest.documentation.clone(),
        combinations: manifest.combinations.clone(),
        upgrade_edges: manifest.upgrade_edges.clone(),
    };
    input.components.sort();
    input.manifest_formats.sort();
    input.state_versions.sort();
    input.state_versions.dedup();
    input
        .combinations
        .sort_by(|left, right| left.combination_id.cmp(&right.combination_id));
    for combination in &mut input.combinations {
        combination.component_references.sort();
        combination.component_references.dedup();
    }
    input
        .upgrade_edges
        .sort_by(|left, right| left.edge_id.cmp(&right.edge_id));
    for edge in &mut input.upgrade_edges {
        edge.mixed_version_references.sort();
        edge.mixed_version_references.dedup();
    }
    let component_references = input
        .components
        .iter()
        .map(SupportComponent::reference)
        .collect::<BTreeSet<_>>();
    let calculated_digest = if manifest.evidence_receipt_authorities.is_empty()
        && manifest.receipt_authority_public_keys.is_empty()
    {
        digest(&input)
    } else {
        let mut canonical = manifest.clone();
        canonical.protocol.clear();
        canonical.manifest_id.clear();
        canonical.manifest_digest.clear();
        canonical.components = input.components.clone();
        canonical.manifest_formats = input.manifest_formats.clone();
        canonical.state_versions = input.state_versions.clone();
        canonical.combinations = input.combinations.clone();
        canonical.upgrade_edges = input.upgrade_edges.clone();
        digest(&canonical)
    };
    if manifest.protocol != SUPPORT_PROTOCOL
        || manifest.manifest_digest != calculated_digest
        || manifest.manifest_id != format!("ga-support:{}", &calculated_digest[7..23])
        || input.components.is_empty()
        || input.components.iter().any(|component| {
            component.component_id.trim().is_empty()
                || component.version.trim().is_empty()
                || !valid_digest(&component.digest)
        })
        || !valid_digest(&input.documentation.digest)
        || input.combinations.iter().any(|combination| {
            combination.component_references.is_empty()
                || combination
                    .component_references
                    .iter()
                    .any(|reference| !component_references.contains(reference))
                || !input.state_versions.contains(&combination.state_version)
        })
    {
        bail!("ga_manifest_invalid: invalid protocol, structure, identity, or digest")
    }
    Ok(manifest)
}

fn read_json(path: &Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn read_typed<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    serde_json::from_value(read_json(path)?)
        .with_context(|| format!("invalid GA input in {}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn print_value(value: &Value, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!(
            "{}",
            value["protocol"].as_str().unwrap_or("lenso.ga-result.v1")
        );
        if let Some(decision) = value["decision"].as_str() {
            println!("decision: {decision}");
        }
        for issue in value["issues"].as_array().into_iter().flatten() {
            println!(
                "issue: {} — {}",
                issue["code"].as_str().unwrap_or("ga_issue"),
                issue["message"].as_str().unwrap_or("unknown issue")
            );
            if let Some(remediation) = issue["remediation"].as_str() {
                println!("remediation: {remediation}");
            }
        }
        for action in value["nextActions"].as_array().into_iter().flatten() {
            if let Some(action) = action.as_str() {
                println!("next: {action}");
            }
        }
    }
    Ok(())
}

fn digest(value: &impl Serialize) -> String {
    lenso_service::extraction_input_digest(
        &serde_json::to_vec(value).expect("GA command values serialize"),
    )
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn issue(
    code: &'static str,
    message: impl Into<String>,
    remediation: impl Into<String>,
    next_action: impl Into<String>,
) -> Issue {
    Issue {
        code,
        message: message.into(),
        remediation: remediation.into(),
        next_actions: vec![next_action.into()],
    }
}
