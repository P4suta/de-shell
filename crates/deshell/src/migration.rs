use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

const ZERO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub(crate) fn official_generator_digest() -> String {
    format!(
        "sha256:{}",
        crate::digest::sha256(
            format!(
                "deshell-official-generator-v1:{}",
                env!("CARGO_PKG_VERSION")
            )
            .as_bytes()
        )
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MigrationPlan {
    pub schema_version: u32,
    pub kind: PlanKind,
    pub plan_digest: String,
    pub inventory_digest: String,
    pub sources: Vec<PlanSource>,
    pub proposals: Vec<String>,
    pub required_scenarios: Vec<ScenarioRequirement>,
    pub required_cells: Vec<CellRequirement>,
    pub validation_commands: Vec<ExactCommand>,
    pub validation_limits: crate::config::ResourceLimits,
    pub network_replay_digest: Option<String>,
    pub coverage: Coverage,
    pub blockers: Vec<Blocker>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanKind {
    Migration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanSource {
    pub location: Location,
    pub kind: SourceKind,
    pub interpreter: String,
    pub content_digest: String,
    pub ir_digest: String,
    pub proposal_digest: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourceKind {
    ShellFile,
    EmbeddedShell,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Location {
    pub path: String,
    pub start_byte: u64,
    pub end_byte: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScenarioRequirement {
    pub name: String,
    pub digest: String,
    pub approval_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CellRequirement {
    pub id: String,
    pub platform_fingerprint: String,
    pub runtime_fingerprint: String,
    pub approval_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExactCommand {
    pub name: String,
    pub kind: crate::config::ValidationKind,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Coverage {
    pub total_bytes: u64,
    pub native_bytes: u64,
    pub delegated_bytes: u64,
    pub residual_bytes: u64,
    pub trivia_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Blocker {
    pub code: String,
    pub message: String,
    pub location: Option<Location>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MigrationRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub source: RequestSource,
    pub effect_ir_digest: String,
    pub effect_ir: serde_json::Value,
    pub interface: TypedInterface,
    pub call_sites: Vec<Location>,
    pub target: crate::config::MigrationTarget,
    pub module_root: String,
    pub policy: GeneratorPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RequestSource {
    pub location: Location,
    pub content_digest: String,
    pub interpreter: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypedInterface {
    pub arguments: Vec<String>,
    pub environment: Vec<String>,
    pub secrets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratorPolicy {
    pub context: crate::config::AgentContextPolicy,
    pub allow_network: bool,
    pub allow_source_send: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proposal {
    pub schema_version: u32,
    pub proposal_digest: String,
    pub request_digest: String,
    pub generator_digest: String,
    pub patches: Vec<GeneratorPatch>,
    pub build_argv: Vec<String>,
    pub run_argv: Vec<String>,
    pub validation: Vec<Vec<String>>,
    pub dependencies: Vec<Dependency>,
    pub source_map: Vec<GeneratedSpan>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MigrationEvidence {
    pub schema_version: u32,
    pub plan_digest: String,
    pub approval_digests: Vec<String>,
    pub cell: String,
    pub status: EvidenceStatus,
    pub repetitions: u32,
    pub checks: Vec<EvidenceCheck>,
    pub validation: Vec<ValidationEvidence>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceStatus {
    Verified,
    Different,
    Unavailable,
    Failed,
    Nondeterministic,
}

impl EvidenceStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Different => "different",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Nondeterministic => "nondeterministic",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceCheck {
    pub source: Location,
    pub scenario: String,
    pub key: EvidenceKey,
    pub status: EvidenceStatus,
    pub error: Option<String>,
    pub covered_nodes: Vec<String>,
    pub comparisons: Vec<TripleComparison>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceKey {
    pub source_digest: String,
    pub ir_digest: String,
    pub proposal_digest: String,
    pub generator_digest: String,
    pub toolchain_digest: String,
    pub scenario_digest: String,
    pub platform_fingerprint: String,
    pub runtime_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TripleComparison {
    pub original: Observation,
    pub ir: Observation,
    pub replacement: Observation,
    pub differences: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub exit_code: i32,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub stdout_base64: String,
    pub stderr_base64: String,
    pub files: Vec<FileChange>,
    pub network: Vec<crate::replay::NetworkExchange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileChange {
    pub path: String,
    pub kind: FileChangeKind,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub before_executable: Option<bool>,
    pub after_executable: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileChangeKind {
    Created,
    Modified,
    Removed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ValidationEvidence {
    pub name: String,
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub stdout_digest: String,
    pub stderr_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveManifest {
    pub schema_version: u32,
    pub plan_digest: String,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArchiveEntry {
    pub original: Location,
    pub plan_digest: String,
    pub kind: SourceKind,
    pub content_digest: String,
    pub archive_path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatchOperation {
    Create,
    Update,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratorPatch {
    pub operation: PatchOperation,
    pub path: String,
    pub expected_digest: Option<String>,
    pub content_base64: String,
    pub content_digest: String,
    pub permissions: u32,
}

impl GeneratorPatch {
    pub(crate) fn contents(&self) -> Result<Vec<u8>, String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.content_base64)
            .map_err(|error| format!("invalid proposal content base64: {error}"))?;
        if base64::engine::general_purpose::STANDARD.encode(&bytes) != self.content_base64 {
            return Err("proposal content_base64 is not canonical".into());
        }
        if crate::digest::sha256(&bytes) != self.content_digest {
            return Err(format!(
                "proposal content digest mismatch for {}",
                self.path
            ));
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dependency {
    pub ecosystem: DependencyEcosystem,
    pub name: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DependencyEcosystem {
    Cargo,
    Go,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedSpan {
    pub ir_node: String,
    pub generated: Location,
}

#[derive(Clone, Debug)]
pub(crate) struct PlanOutput {
    pub digest: String,
    pub artifact_path: String,
    pub diff: String,
    pub blockers: Vec<Blocker>,
    pub required_cells: Vec<String>,
    pub remaining_evidence: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MigrationIndex {
    pub schema_version: u32,
    pub active_plan_digest: String,
}

struct PlannedArtifacts {
    requests: Vec<MigrationRequest>,
    plans: BTreeMap<String, crate::ir::Plan>,
    proposals: Vec<Proposal>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratorHandshake {
    schema_version: u32,
    protocol: String,
    generator: GeneratorIdentity,
    max_frame_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratorIdentity {
    name: String,
    version: String,
    digest: String,
    capabilities: Vec<crate::config::MigrationTarget>,
}

pub(crate) fn create_plan(root: &Path) -> Result<PlanOutput, String> {
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    let inventory = crate::project::scan(root)?;
    let inventory_digest = canonical_digest(&inventory)?;
    let mut blockers = Vec::new();
    for error in &inventory.errors {
        blockers.push(Blocker {
            code: blocker_code(&error.message, "DESHELL_BLOCKER_SCAN_ERROR"),
            message: format!("{}: {}", error.stage, error.message),
            location: error.path.as_ref().map(|path| Location {
                path: path.clone(),
                start_byte: 0,
                end_byte: 0,
            }),
        });
    }
    for skipped in &inventory.skipped {
        blockers.push(Blocker {
            code: "DESHELL_BLOCKER_SCAN_INCOMPLETE".into(),
            message: format!("{} was skipped: {}", skipped.path, skipped.reason),
            location: Some(Location {
                path: skipped.path.clone(),
                start_byte: 0,
                end_byte: 0,
            }),
        });
    }

    let scenario_reviews = crate::approval::scenario_reviews(root)?;
    let approved_scenario_values = load_approved_scenario_values(root)?;
    let required_scenarios = approved_scenario_values
        .values()
        .map(|scenario| {
            Ok(ScenarioRequirement {
                name: scenario.name.clone(),
                digest: scenario.digest()?,
                approval_digest: scenario.approval_digest.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !inventory.findings.is_empty() {
        if scenario_reviews.is_empty() {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_UNAPPROVED_SCENARIO".into(),
                message: "no scenario is available for retirement evidence approval".into(),
                location: None,
            });
        } else {
            for review in scenario_reviews
                .iter()
                .filter(|review| review.status != crate::approval::ReviewStatus::Approved)
            {
                blockers.push(Blocker {
                    code: "DESHELL_BLOCKER_UNAPPROVED_SCENARIO".into(),
                    message: format!(
                        "scenario {} has no current approval for review digest {}",
                        review.name, review.digest
                    ),
                    location: None,
                });
            }
        }
    }
    let matrix_reviews = crate::approval::matrix_reviews(root)?;
    let required_cells = approved_cells(root, &config)?;
    if !inventory.findings.is_empty() {
        if matrix_reviews.is_empty() {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_PLATFORM_MATRIX_EMPTY".into(),
                message: "no platform/runtime cell is available for retirement evidence approval"
                    .into(),
                location: None,
            });
        } else {
            for review in matrix_reviews
                .iter()
                .filter(|review| review.status != crate::approval::ReviewStatus::Approved)
            {
                blockers.push(Blocker {
                    code: "DESHELL_BLOCKER_UNAPPROVED_MATRIX_CELL".into(),
                    message: format!(
                        "matrix cell {} has no current approval for review digest {}",
                        review.name, review.digest
                    ),
                    location: None,
                });
            }
        }
    }

    let retiring_paths = inventory
        .findings
        .iter()
        .filter(|finding| matches!(&finding.kind, crate::scanner::FindingKind::ShellFile))
        .map(|finding| finding.path.clone())
        .collect::<BTreeSet<_>>();
    let static_references = match crate::scanner::static_script_references(root, &retiring_paths) {
        Ok(references) => references,
        Err(message) => {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_REFERENCE_GRAPH".into(),
                message,
                location: None,
            });
            Vec::new()
        }
    };
    let direct_shell_wrappers = inventory
        .findings
        .iter()
        .filter_map(|finding| {
            thin_shell_file_target(finding, &retiring_paths)
                .map(|target| (finding.path.clone(), target))
        })
        .collect::<BTreeMap<_, _>>();
    let shell_wrappers = direct_shell_wrappers
        .keys()
        .filter_map(|path| {
            terminal_wrapper_target(path, &direct_shell_wrappers)
                .filter(|target| target != path)
                .map(|target| (path.clone(), target))
        })
        .collect::<BTreeMap<_, _>>();
    let mut wrapper_targets = inventory
        .findings
        .iter()
        .filter_map(|finding| {
            thin_project_interface_target(finding, &static_references).and_then(|target| {
                terminal_wrapper_target(&target, &direct_shell_wrappers).map(|target| {
                    (
                        Location {
                            path: finding.path.clone(),
                            start_byte: finding.span.start_byte,
                            end_byte: finding.span.end_byte,
                        },
                        target,
                    )
                })
            })
        })
        .collect::<BTreeMap<_, _>>();
    for finding in &inventory.findings {
        if let Some(target) = shell_wrappers.get(&finding.path) {
            wrapper_targets.insert(
                Location {
                    path: finding.path.clone(),
                    start_byte: finding.span.start_byte,
                    end_byte: finding.span.end_byte,
                },
                target.clone(),
            );
        }
    }

    let mut sources = Vec::new();
    let mut proposal_digests = Vec::new();
    let mut artifacts = PlannedArtifacts {
        requests: Vec::new(),
        plans: BTreeMap::new(),
        proposals: Vec::new(),
    };
    let mut coverage = Coverage::default();
    let mut targets = BTreeSet::new();
    let mut network_replay_digest = None;

    for finding in &inventory.findings {
        let location = Location {
            path: finding.path.clone(),
            start_byte: finding.span.start_byte,
            end_byte: finding.span.end_byte,
        };
        if finding.kind == crate::scanner::FindingKind::Candidate {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_DYNAMIC_CANDIDATE".into(),
                message: format!(
                    "dynamic shell candidate cannot be resolved statically: {}",
                    finding.locator.as_deref().unwrap_or("unknown location")
                ),
                location: Some(location),
            });
            continue;
        }
        let kind = match finding.kind {
            crate::scanner::FindingKind::ShellFile => SourceKind::ShellFile,
            crate::scanner::FindingKind::EmbeddedShell => SourceKind::EmbeddedShell,
            crate::scanner::FindingKind::Candidate => unreachable!(),
        };
        let wrapper_target = wrapper_targets.get(&location);
        if finding.interpreter.as_deref() == Some("package-shell") && wrapper_target.is_none() {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_UNIMPLEMENTED_HOST_INTERFACE".into(),
                message: format!(
                    "{} uses a package-script shell whose platform-specific interpreter and project-native replacement interface are not implemented",
                    finding.path
                ),
                location: Some(location),
            });
            continue;
        }
        let source_interpreter = resolved_finding_interpreter(finding)?;
        let plan = lower_finding(finding, config.policy.unknown_interpreter.clone())?;
        add_scenario_input_coverage_blockers(
            &plan,
            &approved_scenario_values,
            &location,
            &mut blockers,
        );
        let network_effects = network_effects(&plan);
        if !network_effects.is_empty() {
            match bind_network_replay(root, &plan) {
                Ok(digest) => network_replay_digest = Some(digest),
                Err(message) => {
                    for (effect, effect_location) in network_effects {
                        blockers.push(Blocker {
                            code: "DESHELL_BLOCKER_NETWORK_REPLAY_UNAVAILABLE".into(),
                            message: format!(
                                "{} requires network effect {effect}, but no enforced replay is available: {message}",
                                finding.path
                            ),
                            location: Some(effect_location.unwrap_or_else(|| location.clone())),
                        });
                    }
                }
            }
        }
        let ir_digest = canonical_digest(&plan)?;
        let source_coverage = classify_coverage(&plan, finding.source.len());
        coverage.total_bytes += source_coverage.total_bytes;
        coverage.native_bytes += source_coverage.native_bytes;
        coverage.delegated_bytes += source_coverage.delegated_bytes;
        coverage.residual_bytes += source_coverage.residual_bytes;
        coverage.trivia_bytes += source_coverage.trivia_bytes;
        let guarantees = guarantee_counts(&plan);
        if guarantees.1 != 0 {
            let reasons = delegated_reasons(&plan);
            let blocker_location = delegated_blocker_location(&reasons, &location, kind);
            blockers.push(Blocker {
                code: delegated_blocker_code(&reasons).into(),
                message: format!(
                    "{} contains {count} delegated node(s); whole-file delegation is not migratable: {reasons}",
                    finding.path,
                    count = guarantees.1,
                    reasons = reasons.join("; ")
                ),
                location: Some(blocker_location),
            });
        }
        if guarantees.2 != 0 {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_RESIDUAL_SOURCE".into(),
                message: format!(
                    "{} contains {count} residual node(s)",
                    finding.path,
                    count = guarantees.2
                ),
                location: Some(location.clone()),
            });
        }

        let mut proposal_digest = None;
        if guarantees.1 == 0 && guarantees.2 == 0 && wrapper_target.is_none() {
            let selection = generator_selection(&config, &location);
            let call_sites = static_references
                .iter()
                .filter(|reference| reference.target == finding.path)
                .map(|reference| Location {
                    path: reference.path.clone(),
                    start_byte: reference.span.start_byte,
                    end_byte: reference.span.end_byte,
                })
                .collect::<Vec<_>>();
            if kind == SourceKind::EmbeddedShell
                && selection.target != crate::config::MigrationTarget::Host
            {
                blockers.push(Blocker {
                    code: "DESHELL_BLOCKER_UNSUPPORTED_HOST_REWRITE".into(),
                    message: format!(
                        "embedded shell at {} requires the structured host generator",
                        finding.path
                    ),
                    location: Some(location.clone()),
                });
            } else {
                match build_request_and_proposal(
                    root,
                    &config,
                    finding,
                    &location,
                    &plan,
                    &ir_digest,
                    call_sites,
                    selection,
                    &mut targets,
                ) {
                    Ok((request, proposal)) => {
                        proposal_digest = Some(proposal.proposal_digest.clone());
                        proposal_digests.push(proposal.proposal_digest.clone());
                        artifacts.requests.push(request);
                        artifacts.proposals.push(proposal);
                    }
                    Err(message) => blockers.push(Blocker {
                        code: blocker_code(&message, "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED"),
                        message,
                        location: Some(location.clone()),
                    }),
                }
            }
        }
        artifacts.plans.entry(ir_digest.clone()).or_insert(plan);
        sources.push(PlanSource {
            location,
            kind,
            interpreter: source_interpreter,
            content_digest: finding.content_digest.clone(),
            ir_digest,
            proposal_digest,
        });
    }
    let source_proposals = sources
        .iter()
        .filter_map(|source| {
            source
                .proposal_digest
                .as_ref()
                .map(|digest| (source.location.path.clone(), digest.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    for source in &mut sources {
        let Some(target) = wrapper_targets.get(&source.location) else {
            continue;
        };
        match source_proposals.get(target) {
            Some(proposal) => source.proposal_digest = Some(proposal.clone()),
            None => blockers.push(Blocker {
                code: "DESHELL_BLOCKER_UNRESOLVED_CALL_SITE".into(),
                message: format!(
                    "{} wraps retiring script {target}, but that script has no replacement proposal",
                    source.location.path
                ),
                location: Some(source.location.clone()),
            }),
        }
    }
    let remaining_static_references = match remaining_static_references_after_proposals(
        root,
        &retiring_paths,
        &artifacts.proposals,
    ) {
        Ok(references) => references,
        Err(message) => {
            blockers.push(Blocker {
                code: "DESHELL_BLOCKER_REFERENCE_GRAPH".into(),
                message: format!("cannot validate the proposal-updated reference graph: {message}"),
                location: None,
            });
            static_references
        }
    };
    for reference in remaining_static_references {
        blockers.push(Blocker {
            code: "DESHELL_BLOCKER_UNRESOLVED_CALL_SITE".into(),
            message: format!(
                "{} still invokes retiring script {} through a process API",
                reference.path, reference.target
            ),
            location: Some(Location {
                path: reference.path,
                start_byte: reference.span.start_byte,
                end_byte: reference.span.end_byte,
            }),
        });
    }
    for source in &sources {
        if wrapper_targets.contains_key(&source.location) {
            continue;
        }
        if let Some(ir) = artifacts.plans.get(&source.ir_digest) {
            let task = ir.tasks.iter().find(|task| task.name == ir.entrypoint);
            if let Some(task) = task {
                let mut references = Vec::new();
                collect_ir_script_references(&task.body, &retiring_paths, &mut references);
                for (target, location) in references {
                    blockers.push(Blocker {
                        code: "DESHELL_BLOCKER_UNRESOLVED_CALL_SITE".into(),
                        message: format!(
                            "{} still invokes retiring script {target} from generated Effect IR",
                            source.location.path
                        ),
                        location: Some(location.unwrap_or_else(|| source.location.clone())),
                    });
                }
            }
        }
    }
    sources.sort_by(|left, right| left.location.cmp(&right.location));
    proposal_digests.sort();
    proposal_digests.dedup();
    blockers.sort_by(|left, right| {
        left.location
            .cmp(&right.location)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    blockers.dedup_by(|left, right| {
        left.code == right.code && left.location == right.location && left.message == right.message
    });
    let mut plan = MigrationPlan {
        schema_version: 1,
        kind: PlanKind::Migration,
        plan_digest: ZERO_DIGEST.into(),
        inventory_digest,
        sources,
        proposals: proposal_digests,
        required_scenarios,
        required_cells,
        validation_commands: config
            .validation_commands
            .iter()
            .map(|command| ExactCommand {
                name: command.name.clone(),
                kind: command.kind,
                argv: command.argv.clone(),
            })
            .collect(),
        validation_limits: config.limits,
        network_replay_digest,
        coverage,
        blockers,
    };
    plan.plan_digest = plan.computed_digest()?;
    plan.validate()?;
    let diff = proposal_diff(&artifacts.proposals)?;
    persist_plan(root, &plan, &artifacts, &diff)?;
    persist_active_index(root, &plan.plan_digest)?;
    Ok(PlanOutput {
        digest: plan.plan_digest.clone(),
        artifact_path: format!(".deshell/migrations/sha256/{}/plan.json", plan.plan_digest),
        diff,
        blockers: plan.blockers.clone(),
        required_cells: plan
            .required_cells
            .iter()
            .map(|cell| cell.id.clone())
            .collect(),
        remaining_evidence: plan.required_cells.len(),
    })
}

fn remaining_static_references_after_proposals(
    root: &Path,
    retiring_paths: &BTreeSet<String>,
    proposals: &[Proposal],
) -> Result<Vec<crate::scanner::ScriptReference>, String> {
    if proposals.is_empty() {
        return crate::scanner::static_script_references(root, retiring_paths);
    }
    let workspace = crate::workspace::private_snapshot(root)?;
    for proposal in proposals {
        apply_generator_patches(workspace.path(), proposal)?;
    }
    crate::scanner::static_script_references(workspace.path(), retiring_paths)
}

fn thin_project_interface_target(
    finding: &crate::scanner::Finding,
    references: &[crate::scanner::ScriptReference],
) -> Option<String> {
    if finding.kind != crate::scanner::FindingKind::EmbeddedShell
        || !(is_make_or_package_path(&finding.path) || is_github_workflow_path(&finding.path))
    {
        return None;
    }
    let reference = references.iter().find(|reference| {
        reference.path == finding.path
            && reference.span.start_byte == finding.span.start_byte
            && reference.span.end_byte == finding.span.end_byte
    })?;
    let command = std::str::from_utf8(&finding.source).ok()?;
    validate_thin_project_interface(&finding.path, command, &reference.target).ok()?;
    Some(reference.target.clone())
}

fn thin_shell_file_target(
    finding: &crate::scanner::Finding,
    retiring_paths: &BTreeSet<String>,
) -> Option<String> {
    if finding.kind != crate::scanner::FindingKind::ShellFile {
        return None;
    }
    let source = std::str::from_utf8(&finding.source).ok()?;
    let commands = source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim();
            (!(trimmed.is_empty()
                || (index == 0 && trimmed.starts_with("#!"))
                || trimmed.starts_with('#')))
            .then_some(trimmed)
        })
        .collect::<Vec<_>>();
    let [command] = commands.as_slice() else {
        return None;
    };
    let argv = static_shell_words(command).ok()?;
    let targets = argv
        .iter()
        .filter_map(|argument| {
            let normalized = argument.strip_prefix("./").unwrap_or(argument);
            retiring_paths.get(normalized).cloned()
        })
        .collect::<BTreeSet<_>>();
    if targets.len() != 1 {
        return None;
    }
    let target = targets.into_iter().next()?;
    validate_thin_project_interface(&finding.path, command, &target).ok()?;
    Some(target)
}

fn terminal_wrapper_target(start: &str, wrappers: &BTreeMap<String, String>) -> Option<String> {
    let mut current = start;
    let mut seen = BTreeSet::new();
    while let Some(next) = wrappers.get(current) {
        if !seen.insert(current) {
            return None;
        }
        current = next;
    }
    Some(current.into())
}

fn is_make_or_package_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    filename == "makefile"
        || filename == "gnumakefile"
        || lower.ends_with(".mk")
        || filename == "package.json"
}

fn is_github_workflow_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with(".github/workflows/") && (lower.ends_with(".yml") || lower.ends_with(".yaml"))
}

fn is_dockerfile_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    filename == "dockerfile"
        || filename.starts_with("dockerfile.")
        || lower.ends_with(".dockerfile")
}

#[derive(Clone, Copy)]
struct GeneratorSelection<'a> {
    generator: &'a str,
    target: crate::config::MigrationTarget,
    module_root: &'a str,
}

fn generator_selection<'a>(
    config: &'a crate::config::ProjectConfig,
    location: &Location,
) -> GeneratorSelection<'a> {
    config
        .location_overrides
        .iter()
        .find(|override_| {
            override_.path == location.path
                && override_.start_byte == location.start_byte
                && override_.end_byte == location.end_byte
        })
        .map_or(
            GeneratorSelection {
                generator: &config.migration.generator,
                target: config.migration.target,
                module_root: &config.migration.module_root,
            },
            |override_| GeneratorSelection {
                generator: &override_.generator,
                target: override_.target,
                module_root: &override_.module_root,
            },
        )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the arguments are a contract record; grouping them into a struct would hide which fields the caller must supply"
)]
fn build_request_and_proposal(
    root: &Path,
    config: &crate::config::ProjectConfig,
    finding: &crate::scanner::Finding,
    location: &Location,
    plan: &crate::ir::Plan,
    ir_digest: &str,
    call_sites: Vec<Location>,
    selection: GeneratorSelection<'_>,
    targets: &mut BTreeSet<String>,
) -> Result<(MigrationRequest, Proposal), String> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| "migration IR entrypoint task is missing".to_owned())?;
    let mut request = MigrationRequest {
        schema_version: 1,
        request_id: ZERO_DIGEST.into(),
        source: RequestSource {
            location: location.clone(),
            content_digest: finding.content_digest.clone(),
            interpreter: finding
                .interpreter
                .clone()
                .unwrap_or_else(|| "unknown".into()),
        },
        effect_ir_digest: ir_digest.into(),
        effect_ir: serde_json::to_value(plan).map_err(|error| error.to_string())?,
        interface: TypedInterface {
            arguments: task.inputs.iter().map(|input| input.name.clone()).collect(),
            environment: task.environment.clone(),
            secrets: task.secrets.clone(),
        },
        call_sites,
        target: selection.target,
        module_root: selection.module_root.into(),
        policy: GeneratorPolicy {
            context: config.migration.agent_context,
            allow_network: config.migration.allow_agent_network,
            allow_source_send: config.migration.allow_source_send,
        },
    };
    request.request_id = canonical_digest(&request)?;
    let stem = source_stem(&finding.path);
    if let Some(name) = selection.generator.strip_prefix("external:") {
        let registration = config
            .migration
            .external_generators
            .iter()
            .find(|generator| generator.name == name)
            .ok_or_else(|| {
                format!(
                    "DESHELL_BLOCKER_GENERATOR_POLICY: external generator {name} is not registered"
                )
            })?;
        let target = external_target_hint(
            selection.target,
            selection.module_root,
            &stem,
            &finding.path,
        );
        let expected_digest = current_target_digest(root, &target)?;
        let proposal = invoke_external_generator(InvokeExternalGeneratorArgs {
            root,
            config,
            registration,
            request: &request,
            target_path: &target,
            expected_digest,
            task,
        })
        .map_err(external_generator_blocker)?;
        for patch in &proposal.patches {
            if targets.contains(&patch.path) {
                return Err(format!(
                    "DESHELL_BLOCKER_DUPLICATE_TARGET: multiple sources generate {}",
                    patch.path
                ));
            }
        }
        targets.extend(proposal.patches.iter().map(|patch| patch.path.clone()));
        return Ok((request, proposal));
    }
    let mut host_build_argv = None;
    let mut host_run_argv = None;
    let mut generated_span = None;
    let mut host_additional_files = Vec::new();
    let target = match selection.target {
        crate::config::MigrationTarget::Rust => {
            format!("{}/{}.rs", selection.module_root, rust_binary_name(&stem))
        }
        crate::config::MigrationTarget::Go => {
            format!("{}/{stem}.go", selection.module_root)
        }
        crate::config::MigrationTarget::Host => finding.path.clone(),
        crate::config::MigrationTarget::Agent => {
            return Err("DESHELL_BLOCKER_GENERATOR_POLICY: agent target requires a digest-pinned external generator".into());
        }
    };
    if !targets.insert(target.clone()) {
        return Err(format!(
            "DESHELL_BLOCKER_DUPLICATE_TARGET: multiple sources generate {target}"
        ));
    }
    let generated = match selection.target {
        crate::config::MigrationTarget::Rust => generate_rust(plan)?,
        crate::config::MigrationTarget::Go => generate_go(plan)?,
        crate::config::MigrationTarget::Host => {
            let host = generate_structured_host(root, finding, plan)?;
            host_build_argv = Some(host.build_argv);
            host_run_argv = Some(host.run_argv);
            generated_span = Some(host.generated_span);
            host_additional_files = host.additional_files;
            host.bytes
        }
        crate::config::MigrationTarget::Agent => unreachable!(),
    };
    let canonical_root = canonical_root(root)?;
    let verification_output = verification_binary_path(&stem, std::env::consts::OS);
    let (build_argv, run_argv) = match selection.target {
        crate::config::MigrationTarget::Rust => (
            rust_build_argv(&target, &verification_output, std::env::consts::OS),
            vec![verification_output],
        ),
        crate::config::MigrationTarget::Go => (
            vec![
                "go".into(),
                "build".into(),
                "-p=1".into(),
                "-o".into(),
                verification_output.clone(),
                target.clone(),
            ],
            vec![verification_output],
        ),
        crate::config::MigrationTarget::Host => (
            host_build_argv.ok_or("host generator omitted exact build argv")?,
            host_run_argv.ok_or("host generator omitted exact run argv")?,
        ),
        crate::config::MigrationTarget::Agent => unreachable!(),
    };
    let generator_digest = official_generator_digest();
    let mut node_ids = Vec::new();
    collect_node_ids(&task.body, &mut node_ids);
    let mut patches = vec![generator_patch(
        &canonical_root,
        &target,
        generated.clone(),
        0o644,
    )?];
    let call_site_patches = official_call_site_patches(OfficialCallSitePatchesArgs {
        root,
        config,
        call_sites: &request.call_sites,
        retiring_source: &finding.path,
        selection,
        stem: &stem,
        generated_target: &target,
    })
    .unwrap_or_default();
    for patch in call_site_patches {
        if !targets.insert(patch.path.clone()) {
            return Err(format!(
                "DESHELL_BLOCKER_DUPLICATE_TARGET: multiple sources generate {}",
                patch.path
            ));
        }
        patches.push(patch);
    }
    for file in host_additional_files {
        if !targets.insert(file.path.clone()) {
            return Err(format!(
                "DESHELL_BLOCKER_DUPLICATE_TARGET: multiple sources generate {}",
                file.path
            ));
        }
        patches.push(generator_patch(
            &canonical_root,
            &file.path,
            file.bytes,
            file.permissions,
        )?);
    }
    let mut proposal = Proposal {
        schema_version: 1,
        proposal_digest: ZERO_DIGEST.into(),
        request_digest: request.request_id.clone(),
        generator_digest,
        patches,
        build_argv,
        run_argv,
        validation: config
            .validation_commands
            .iter()
            .map(|command| command.argv.clone())
            .collect(),
        dependencies: Vec::new(),
        source_map: node_ids
            .into_iter()
            .map(|id| GeneratedSpan {
                ir_node: id,
                generated: generated_span.clone().unwrap_or_else(|| Location {
                    path: target.clone(),
                    start_byte: 0,
                    end_byte: generated.len() as u64,
                }),
            })
            .collect(),
    };
    proposal.proposal_digest = canonical_digest(&proposal)?;
    validate_proposal(&proposal)?;
    Ok((request, proposal))
}

/// The inputs of [`official_call_site_patches`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`official_call_site_patches`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct OfficialCallSitePatchesArgs<'a> {
    root: &'a Path,
    config: &'a crate::config::ProjectConfig,
    call_sites: &'a [Location],
    retiring_source: &'a str,
    selection: GeneratorSelection<'a>,
    stem: &'a str,
    generated_target: &'a str,
}

fn official_call_site_patches(
    parts: OfficialCallSitePatchesArgs<'_>,
) -> Result<Vec<GeneratorPatch>, String> {
    // Destructured without `..`: see `OfficialCallSitePatchesArgs`.
    let OfficialCallSitePatchesArgs {
        root,
        config,
        call_sites,
        retiring_source,
        selection,
        stem,
        generated_target,
    } = parts;
    if call_sites.is_empty() {
        return Ok(Vec::new());
    }
    let replacement_argv = match selection.target {
        crate::config::MigrationTarget::Rust => {
            let binary = rust_binary_name(stem);
            if selection.module_root != config.integration.rust.module_root
                || selection.module_root != "src/bin"
                || generated_target != format!("src/bin/{binary}.rs")
            {
                return Err(
                    "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: Rust call-site migration requires the configured src/bin integration"
                        .into(),
                );
            }
            require_regular_project_file(root, "Cargo.toml", "Rust call-site migration")?;
            vec![
                "cargo".into(),
                "run".into(),
                "--quiet".into(),
                "--bin".into(),
                binary,
                "--".into(),
            ]
        }
        crate::config::MigrationTarget::Go => {
            if selection.module_root != config.integration.go.module_root {
                return Err(
                    "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: Go call-site migration requires the configured Go integration root"
                        .into(),
                );
            }
            require_regular_project_file(root, "go.mod", "Go call-site migration")?;
            vec!["go".into(), "run".into(), format!("./{generated_target}")]
        }
        crate::config::MigrationTarget::Host | crate::config::MigrationTarget::Agent => {
            return Err(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: selected generator has no project-native process entrypoint"
                    .into(),
            );
        }
    };

    let mut by_path = BTreeMap::<String, Vec<&Location>>::new();
    for call_site in call_sites {
        if !config.validation_commands.iter().any(|command| {
            command.argv.iter().any(|argument| {
                argument == &call_site.path
                    || argument.strip_prefix("./") == Some(call_site.path.as_str())
            })
        }) {
            return Err(format!(
                "DESHELL_BLOCKER_CALL_SITE_VALIDATION_MISSING: {} is not covered by an exact validation argv",
                call_site.path
            ));
        }
        by_path
            .entry(call_site.path.clone())
            .or_default()
            .push(call_site);
    }

    let mut patches = Vec::new();
    for (path, mut locations) in by_path {
        let absolute = crate::project::project_file_path(root, &path)?;
        let mut contents = std::fs::read(&absolute)
            .map_err(|error| format!("cannot read call site {path}: {error}"))?;
        locations.sort_by_key(|location| std::cmp::Reverse(location.start_byte));
        let mut additional_files = Vec::new();
        for location in locations {
            if is_github_workflow_path(&path) {
                let rewritten = rewrite_github_run_call_site(RewriteGithubRunCallSiteArgs {
                    path: &path,
                    contents: &contents,
                    location,
                    retiring_source,
                    replacement_argv: &replacement_argv,
                })?;
                contents = rewritten.0;
                additional_files.extend(rewritten.1);
                continue;
            }
            let start = usize::try_from(location.start_byte)
                .map_err(|_| "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: span is too large")?;
            let end = usize::try_from(location.end_byte)
                .map_err(|_| "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: span is too large")?;
            let fragment = contents.get(start..end).ok_or_else(|| {
                format!(
                    "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {} span is outside its host file",
                    path
                )
            })?;
            let replacement =
                rewrite_static_process_call(&path, fragment, retiring_source, &replacement_argv)?;
            contents.splice(start..end, replacement);
        }
        patches.push(generator_patch(root, &path, contents, 0o644)?);
        for file in additional_files {
            patches.push(generator_patch(
                root,
                &file.path,
                file.bytes,
                file.permissions,
            )?);
        }
    }
    Ok(patches)
}

fn require_regular_project_file(root: &Path, path: &str, context: &str) -> Result<(), String> {
    let absolute = crate::project::project_file_path(root, path)
        .map_err(|_| format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {context} requires {path}"))?;
    let metadata = absolute
        .symlink_metadata()
        .map_err(|_| format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {context} requires {path}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {context} requires a regular {path}"
        ));
    }
    Ok(())
}

fn rewrite_static_process_call(
    path: &str,
    fragment: &[u8],
    retiring_source: &str,
    replacement_argv: &[String],
) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(fragment)
        .map_err(|_| format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} is not UTF-8"))?;
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".py") {
        rewrite_static_python_call(path, text, retiring_source, replacement_argv)
    } else if [".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        rewrite_static_javascript_call(path, text, retiring_source, replacement_argv)
    } else if is_dockerfile_path(path) {
        rewrite_docker_exec_call(path, text, retiring_source, replacement_argv)
    } else if is_make_or_package_path(path) {
        validate_thin_project_interface(path, text, retiring_source)?;
        Ok(Vec::new())
    } else {
        Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} process syntax is not yet rewritable"
        ))
    }
}

/// The inputs of [`rewrite_github_run_call_site`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`rewrite_github_run_call_site`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct RewriteGithubRunCallSiteArgs<'a> {
    path: &'a str,
    contents: &'a [u8],
    location: &'a Location,
    retiring_source: &'a str,
    replacement_argv: &'a [String],
}

fn rewrite_github_run_call_site(
    parts: RewriteGithubRunCallSiteArgs<'_>,
) -> Result<(Vec<u8>, Vec<HostFile>), String> {
    // Destructured without `..`: see `RewriteGithubRunCallSiteArgs`.
    let RewriteGithubRunCallSiteArgs {
        path,
        contents,
        location,
        retiring_source,
        replacement_argv,
    } = parts;
    let start = usize::try_from(location.start_byte)
        .map_err(|_| "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: span is too large")?;
    let end = usize::try_from(location.end_byte)
        .map_err(|_| "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: span is too large")?;
    let fragment = contents.get(start..end).ok_or_else(|| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} span is outside its host file")
    })?;

    let line_start = contents[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let first_line_end = contents[line_start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(contents.len(), |index| line_start + index);
    let first_line = std::str::from_utf8(&contents[line_start..first_line_end]).map_err(|_| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} run line is not UTF-8")
    })?;
    let search_end = if start == line_start {
        first_line.len()
    } else {
        start - line_start
    };
    let key = first_line[..search_end].rfind("run:").ok_or_else(|| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} reference is not a run step")
    })?;
    let key_end = key + "run:".len();
    let scalar_header = first_line[key_end..].trim();
    let block_scalar = matches!(scalar_header, "|" | "|-" | "|+" | ">" | ">-" | ">+");
    let command = if block_scalar {
        if start != line_start || first_line_end >= end {
            return Err(format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} block scalar source map is inconsistent"
            ));
        }
        let indentation = first_line.len() - first_line.trim_start().len();
        let body = std::str::from_utf8(&contents[first_line_end + 1..end]).map_err(|_| {
            format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} block scalar is not UTF-8")
        })?;
        if body.lines().any(|line| {
            !line.trim().is_empty()
                && line.len().saturating_sub(line.trim_start().len()) <= indentation
        }) {
            return Err(format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} block scalar indentation is inconsistent"
            ));
        }
        let lines = body.lines().map(str::trim_start).collect::<Vec<_>>();
        if scalar_header.starts_with('>') {
            lines.join(" ")
        } else {
            lines.join("\n")
        }
    } else {
        let command = std::str::from_utf8(fragment)
            .map_err(|_| format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} is not UTF-8"))?;
        let value_start = start - line_start;
        if first_line_end < end
            || !first_line.as_bytes()[key_end..value_start]
                .iter()
                .all(u8::is_ascii_whitespace)
            || !first_line.as_bytes()[value_start + command.len()..]
                .iter()
                .all(u8::is_ascii_whitespace)
        {
            return Err(format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} run step has unsupported YAML syntax"
            ));
        }
        command.to_owned()
    };
    validate_thin_project_interface(path, &command, retiring_source)?;

    let identity = format!(
        "{path}\0{}\0{}\0{retiring_source}\0{}",
        location.start_byte,
        location.end_byte,
        replacement_argv.join("\0")
    );
    let action_digest = crate::digest::sha256(identity.as_bytes());
    let action_directory = format!(".github/actions/deshell-{}", &action_digest[..12]);
    let replacement = format!("uses: ./{action_directory}");
    let key_start = line_start + key;
    let mut rewritten = Vec::with_capacity(contents.len() - (end - key_start) + replacement.len());
    rewritten.extend_from_slice(&contents[..key_start]);
    rewritten.extend_from_slice(replacement.as_bytes());
    rewritten.extend_from_slice(&contents[end..]);

    let program = serde_json::to_string(&replacement_argv[0])
        .map_err(|error| format!("cannot encode GitHub action program: {error}"))?;
    let arguments = serde_json::to_string(&replacement_argv[1..])
        .map_err(|error| format!("cannot encode GitHub action arguments: {error}"))?;
    let javascript = format!(
        concat!(
            "// Generated by de-shell; project-native code with no de-shell runtime.\n",
            "const {{ spawnSync }} = require(\"node:child_process\");\n",
            "const result = spawnSync({program}, {arguments}, {{ stdio: \"inherit\", shell: false }});\n",
            "if (result.error) throw result.error;\n",
            "if (result.signal) process.kill(process.pid, result.signal);\n",
            "process.exitCode = result.status === null ? 1 : result.status;\n",
        ),
        program = program,
        arguments = arguments,
    );
    let action = concat!(
        "name: de-shell generated action\n",
        "description: Project-native replacement for a retired shell step\n",
        "runs:\n",
        "  using: node24\n",
        "  main: index.js\n",
    );
    Ok((
        rewritten,
        vec![
            HostFile {
                path: format!("{action_directory}/action.yml"),
                bytes: action.as_bytes().to_vec(),
                permissions: 0o644,
            },
            HostFile {
                path: format!("{action_directory}/index.js"),
                bytes: javascript.into_bytes(),
                permissions: 0o644,
            },
        ],
    ))
}

fn rewrite_docker_exec_call(
    path: &str,
    text: &str,
    retiring_source: &str,
    replacement_argv: &[String],
) -> Result<Vec<u8>, String> {
    let trimmed = text.trim_start();
    let (instruction, value) = trimmed.split_once(char::is_whitespace).ok_or_else(|| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed exec-form instruction in {path}")
    })?;
    if !["run", "cmd", "entrypoint"]
        .iter()
        .any(|expected| instruction.eq_ignore_ascii_case(expected))
    {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: unsupported Docker instruction in {path}"
        ));
    }
    let original_argv = serde_json::from_str::<Vec<String>>(value.trim()).map_err(|error| {
        format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed Docker exec argv in {path}: {error}"
        )
    })?;
    let target_index = direct_target_index(path, &original_argv, retiring_source)?;
    let rewritten = replacement_argv
        .iter()
        .chain(original_argv.iter().skip(target_index + 1))
        .cloned()
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&rewritten).map_err(|error| error.to_string())?;
    let indentation = &text[..text.len() - trimmed.len()];
    Ok(format!("{indentation}{instruction} {encoded}").into_bytes())
}

fn validate_thin_project_interface(
    path: &str,
    command: &str,
    retiring_source: &str,
) -> Result<(), String> {
    let mut argv = static_shell_words(command)
        .map_err(|message| format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} {message}"))?;
    if let Some(program) = argv.first_mut() {
        *program = program.trim_start_matches(['@', '-', '+']).to_owned();
    }
    let target_index = direct_target_index(path, &argv, retiring_source)?;
    if target_index + 1 != argv.len() {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} passes fixed arguments to {retiring_source}"
        ));
    }
    if path.to_ascii_lowercase().ends_with("package.json") && target_index != 1 {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} must name an explicit shell interpreter before {retiring_source}"
        ));
    }
    Ok(())
}

fn static_shell_words(command: &str) -> Result<Vec<String>, &'static str> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }

    let mut output = Vec::new();
    let mut word = String::new();
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut started = false;
    for character in command.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
            continue;
        }
        match quote {
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::None;
                } else {
                    word.push(character);
                }
                started = true;
            }
            Quote::Double => match character {
                '"' => {
                    quote = Quote::None;
                    started = true;
                }
                '\\' => escaped = true,
                '$' | '`' => return Err("contains a dynamic quoted expansion"),
                _ => {
                    word.push(character);
                    started = true;
                }
            },
            Quote::None => match character {
                '\'' => {
                    quote = Quote::Single;
                    started = true;
                }
                '"' => {
                    quote = Quote::Double;
                    started = true;
                }
                '\\' => escaped = true,
                value if value.is_whitespace() => {
                    if started {
                        output.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                '$' | '`' | ';' | '|' | '&' | '<' | '>' | '*' | '?' | '[' | ']' | '{' | '}'
                | '(' | ')' => return Err("contains shell expansion or a control operator"),
                _ => {
                    word.push(character);
                    started = true;
                }
            },
        }
    }
    if escaped || quote != Quote::None {
        return Err("contains an unterminated quote or escape");
    }
    if started {
        output.push(word);
    }
    if output.is_empty() {
        return Err("contains no static command");
    }
    Ok(output)
}

fn rewrite_static_python_call(
    path: &str,
    text: &str,
    retiring_source: &str,
    replacement_argv: &[String],
) -> Result<Vec<u8>, String> {
    let open = text.find('(').ok_or_else(|| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed Python call in {path}")
    })?;
    if !text.ends_with(')') {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed Python call in {path}"
        ));
    }
    let arguments = &text[open + 1..text.len() - 1];
    let values = crate::scanner::split_top_level_arguments(arguments);
    let original_argv = values
        .first()
        .and_then(|value| crate::scanner::static_argv_literals(value.trim()))
        .ok_or_else(|| {
            format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} requires a static Python argv collection"
            )
        })?;
    if values.iter().skip(1).any(|value| {
        value
            .split_once('=')
            .is_some_and(|(name, _)| matches!(name.trim(), "cwd" | "env" | "shell"))
    }) {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} uses cwd, env, or shell process options"
        ));
    }
    let target_index = direct_target_index(path, &original_argv, retiring_source)?;
    let argv = replacement_argv
        .iter()
        .chain(original_argv.iter().skip(target_index + 1))
        .map(|argument| serde_json::to_string(argument).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let mut replacement = format!("{}([{}]", &text[..open], argv);
    for value in values.iter().skip(1) {
        replacement.push(',');
        replacement.push_str(value);
    }
    replacement.push(')');
    Ok(replacement.into_bytes())
}

fn rewrite_static_javascript_call(
    path: &str,
    text: &str,
    retiring_source: &str,
    replacement_argv: &[String],
) -> Result<Vec<u8>, String> {
    let open = text.find('(').ok_or_else(|| {
        format!("DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed JavaScript call in {path}")
    })?;
    if !text.ends_with(')') {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: malformed JavaScript call in {path}"
        ));
    }
    let values = crate::scanner::split_top_level_arguments(&text[open + 1..text.len() - 1]);
    let program = values
        .first()
        .and_then(|value| crate::scanner::quoted_literal(value.trim()))
        .ok_or_else(|| {
            format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} requires a static JavaScript program"
            )
        })?;
    let arguments = match values.get(1) {
        Some(value) => crate::scanner::static_argv_literals(value.trim()).ok_or_else(|| {
            format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} requires a static JavaScript argv collection"
            )
        })?,
        None => Vec::new(),
    };
    if values.iter().skip(2).any(|value| {
        let compact = value
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        ["cwd:", "env:", "shell:"]
            .iter()
            .any(|key| compact.contains(key))
    }) {
        return Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} uses cwd, env, or shell process options"
        ));
    }
    let mut original_argv = vec![program];
    original_argv.extend(arguments);
    let target_index = direct_target_index(path, &original_argv, retiring_source)?;
    let rewritten = replacement_argv
        .iter()
        .chain(original_argv.iter().skip(target_index + 1))
        .collect::<Vec<_>>();
    let program = serde_json::to_string(rewritten[0]).map_err(|error| error.to_string())?;
    let arguments = rewritten
        .iter()
        .skip(1)
        .map(|argument| serde_json::to_string(argument).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    let mut replacement = format!("{}({}, [{}]", &text[..open], program, arguments);
    for value in values.iter().skip(2) {
        replacement.push(',');
        replacement.push_str(value);
    }
    replacement.push(')');
    Ok(replacement.into_bytes())
}

fn direct_target_index(
    path: &str,
    argv: &[String],
    retiring_source: &str,
) -> Result<usize, String> {
    let target_index = argv
        .iter()
        .position(|argument| argument.strip_prefix("./").unwrap_or(argument) == retiring_source)
        .ok_or_else(|| {
            format!(
                "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} no longer contains {retiring_source}"
            )
        })?;
    let supported_position = target_index == 0
        || target_index == 1
            && argv.first().is_some_and(|program| {
                matches!(
                    program.rsplit(['/', '\\']).next().unwrap_or(program),
                    "sh" | "bash" | "zsh" | "fish" | "pwsh" | "powershell" | "cmd" | "nu"
                )
            });
    if supported_position {
        Ok(target_index)
    } else {
        Err(format!(
            "DESHELL_BLOCKER_UNSUPPORTED_CALL_SITE: {path} does not execute {retiring_source} directly"
        ))
    }
}

fn external_target_hint(
    target: crate::config::MigrationTarget,
    module_root: &str,
    stem: &str,
    source_path: &str,
) -> String {
    match target {
        crate::config::MigrationTarget::Rust => {
            format!("{module_root}/{}.rs", rust_binary_name(stem))
        }
        crate::config::MigrationTarget::Go => format!("{module_root}/{stem}.go"),
        crate::config::MigrationTarget::Host => source_path.into(),
        crate::config::MigrationTarget::Agent => format!("{module_root}/{stem}"),
    }
}

fn rust_build_argv(target: &str, output: &str, _operating_system: &str) -> Vec<String> {
    let mut argv = vec![
        "rustc".into(),
        target.into(),
        "--edition=2024".into(),
        "-D".into(),
        "warnings".into(),
        "-Ccodegen-units=1".into(),
    ];
    argv.extend(["-o".into(), output.into()]);
    argv
}

fn verification_binary_path(stem: &str, operating_system: &str) -> String {
    let suffix = if operating_system == "windows" {
        ".exe"
    } else {
        ""
    };
    format!(".deshell/verification/{stem}{suffix}")
}

fn current_target_digest(root: &Path, target: &str) -> Result<Option<String>, String> {
    let path = safe_future_target(&canonical_root(root)?, target)?;
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(crate::digest::sha256(&std::fs::read(&path).map_err(
                |error| format!("cannot read generator target {target}: {error}"),
            )?)))
        }
        Ok(_) => Err(format!(
            "generator target is not a regular non-symlink file: {target}"
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot inspect generator target {target}: {error}")),
    }
}

/// The inputs of [`invoke_external_generator`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`invoke_external_generator`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct InvokeExternalGeneratorArgs<'a> {
    root: &'a Path,
    config: &'a crate::config::ProjectConfig,
    registration: &'a crate::config::ExternalGenerator,
    request: &'a MigrationRequest,
    target_path: &'a str,
    expected_digest: Option<String>,
    task: &'a crate::ir::Task,
}

fn invoke_external_generator(parts: InvokeExternalGeneratorArgs<'_>) -> Result<Proposal, String> {
    // Destructured without `..`: see `InvokeExternalGeneratorArgs`.
    let InvokeExternalGeneratorArgs {
        root,
        config,
        registration,
        request,
        target_path,
        expected_digest,
        task,
    } = parts;
    if !config.migration.allow_agent_network {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_NETWORK_POLICY: external generator execution requires explicit allow_agent_network because this host has no enforced network sandbox"
                .into(),
        );
    }
    if !config.migration.allow_source_send {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_SOURCE_POLICY: external generator execution requires explicit allow_source_send because this host has no enforced filesystem read sandbox"
                .into(),
        );
    }
    let executable = crate::project::project_file_path(root, &registration.executable)?;
    let (executable_bytes, executable_digest) = crate::digest::file_sha256(&executable)?;
    if executable_bytes == 0 || executable_bytes > 64 * 1024 * 1024 {
        return Err("external generator executable must be between 1 byte and 64 MiB".into());
    }
    let actual_digest = format!("sha256:{executable_digest}");
    if actual_digest != registration.digest {
        return Err(format!(
            "external generator executable digest mismatch for {}",
            registration.name
        ));
    }

    let isolated = tempfile::Builder::new()
        .prefix("deshell-generator-")
        .tempdir()
        .map_err(|error| format!("cannot create isolated generator directory: {error}"))?;
    #[cfg(windows)]
    let copied = isolated.path().join("generator.exe");
    #[cfg(not(windows))]
    let copied = isolated.path().join("generator");
    crate::patch::scratch::copy(&executable, &copied)
        .map_err(|error| format!("cannot copy external generator into isolation: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        crate::patch::scratch::set_permissions(&copied, std::fs::Permissions::from_mode(0o500))
            .map_err(|error| format!("cannot make isolated generator executable: {error}"))?;
    }
    let baseline = isolated_tree_digest(isolated.path())?;
    let project_baseline = guarded_project_tree_digest(root)?;
    let handshake_request = serde_json::json!({
        "id": "handshake",
        "jsonrpc": "2.0",
        "method": "deshell.handshake",
        "params": {"protocol_version": 1}
    });
    let handshake_result = execute_external_rpc(ExecuteExternalRpcArgs {
        root: isolated.path(),
        executable: &copied,
        request: &handshake_request,
        id: &serde_json::json!("handshake"),
        project_limits: config.limits,
        frame_limit: crate::protocol::MAX_MESSAGE_BYTES,
    });
    ensure_isolated_tree_unchanged(isolated.path(), &baseline)?;
    ensure_guarded_project_tree_unchanged(root, &project_baseline)?;
    let handshake_value = handshake_result?;
    let handshake: GeneratorHandshake = serde_json::from_value(handshake_value)
        .map_err(|error| format!("invalid Generator Protocol v1 handshake: {error}"))?;
    validate_external_handshake(&handshake, registration, &actual_digest, request.target)?;

    let validation = config
        .validation_commands
        .iter()
        .map(|command| command.argv.clone())
        .collect::<Vec<_>>();
    let propose_request = serde_json::json!({
        "id": "proposal",
        "jsonrpc": "2.0",
        "method": "generator.propose",
        "params": {
            "expected_digest": expected_digest,
            "request": request,
            "target_path": target_path,
            "validation": validation
        }
    });
    let proposal_result = execute_external_rpc(ExecuteExternalRpcArgs {
        root: isolated.path(),
        executable: &copied,
        request: &propose_request,
        id: &serde_json::json!("proposal"),
        project_limits: config.limits,
        frame_limit: handshake.max_frame_bytes as usize,
    });
    ensure_isolated_tree_unchanged(isolated.path(), &baseline)?;
    ensure_guarded_project_tree_unchanged(root, &project_baseline)?;
    let result = proposal_result?;
    let proposal: Proposal = serde_json::from_value(result)
        .map_err(|error| format!("external generator returned an invalid Proposal v1: {error}"))?;
    validate_external_proposal(ValidateExternalProposalArgs {
        root,
        registration,
        request,
        task,
        validation: &validation,
        proposal: &proposal,
    })?;
    Ok(proposal)
}

fn validate_external_handshake(
    handshake: &GeneratorHandshake,
    registration: &crate::config::ExternalGenerator,
    actual_digest: &str,
    target: crate::config::MigrationTarget,
) -> Result<(), String> {
    if handshake.schema_version != 1 || handshake.protocol != "deshell.generator.v1" {
        return Err("external generator selected an unsupported protocol".into());
    }
    if handshake.generator.name != registration.name
        || handshake.generator.version.trim().is_empty()
        || handshake.generator.digest != registration.digest
        || handshake.generator.digest != actual_digest
    {
        return Err("external generator handshake identity or digest mismatch".into());
    }
    if handshake.max_frame_bytes < 1024
        || handshake.max_frame_bytes > crate::protocol::MAX_MESSAGE_BYTES as u64
    {
        return Err("external generator advertised an unsafe frame limit".into());
    }
    let capabilities = handshake
        .generator
        .capabilities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if capabilities.len() != handshake.generator.capabilities.len()
        || !capabilities.contains(&target)
        || !registration.capabilities.contains(&target)
    {
        return Err("external generator handshake capability mismatch".into());
    }
    Ok(())
}

/// The inputs of [`execute_external_rpc`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`execute_external_rpc`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ExecuteExternalRpcArgs<'a> {
    root: &'a Path,
    executable: &'a Path,
    request: &'a serde_json::Value,
    id: &'a serde_json::Value,
    project_limits: crate::config::ResourceLimits,
    frame_limit: usize,
}

fn execute_external_rpc(parts: ExecuteExternalRpcArgs<'_>) -> Result<serde_json::Value, String> {
    // Destructured without `..`: see `ExecuteExternalRpcArgs`.
    let ExecuteExternalRpcArgs {
        root,
        executable,
        request,
        id,
        project_limits,
        frame_limit,
    } = parts;
    let mut input = crate::canonical_json::canonical_bytes(request)?;
    if input.len() > frame_limit || input.len() > crate::protocol::MAX_MESSAGE_BYTES {
        return Err("external generator request exceeds its negotiated frame limit".into());
    }
    input.push(b'\n');
    let mut limits: crate::agent_process::Limits = project_limits.into();
    limits.stdout_bytes = limits
        .stdout_bytes
        .min((crate::protocol::MAX_MESSAGE_BYTES as u64).saturating_mul(2));
    limits.stderr_bytes = limits.stderr_bytes.min(1024 * 1024);
    let outcome = crate::agent_process::execute(
        root,
        crate::agent_process::Request {
            argv: vec![executable.to_string_lossy().into_owned()],
            environment: Vec::new(),
            working_directory: None,
            stdin: input,
            limits,
        },
    )?;
    if outcome.exit_code != 0
        || outcome.signal.is_some()
        || outcome.timed_out
        || outcome.limit_exceeded.is_some()
    {
        return Err(format!(
            "external generator process failed with exit {}: {}",
            outcome.exit_code,
            String::from_utf8_lossy(&outcome.stderr)
        ));
    }
    if !outcome.stderr.is_empty() {
        return Err("external generator wrote unframed stderr output".into());
    }
    let frames = outcome
        .stdout
        .split(|byte| *byte == b'\n')
        .map(|frame| frame.strip_suffix(b"\r").unwrap_or(frame))
        .filter(|frame| !frame.is_empty())
        .collect::<Vec<_>>();
    if frames.len() != 1 {
        return Err("external generator must return exactly one JSON-RPC frame".into());
    }
    if frames[0].len() > frame_limit || frames[0].len() > crate::protocol::MAX_MESSAGE_BYTES {
        return Err("external generator response exceeds its negotiated frame limit".into());
    }
    crate::protocol::decode_response(frames[0], id)
}

/// The inputs of [`validate_external_proposal`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`validate_external_proposal`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ValidateExternalProposalArgs<'a> {
    root: &'a Path,
    registration: &'a crate::config::ExternalGenerator,
    request: &'a MigrationRequest,
    task: &'a crate::ir::Task,
    validation: &'a [Vec<String>],
    proposal: &'a Proposal,
}

fn validate_external_proposal(parts: ValidateExternalProposalArgs<'_>) -> Result<(), String> {
    // Destructured without `..`: see `ValidateExternalProposalArgs`.
    let ValidateExternalProposalArgs {
        root,
        registration,
        request,
        task,
        validation,
        proposal,
    } = parts;
    validate_proposal(proposal)?;
    if proposal.request_digest != request.request_id
        || proposal.generator_digest != registration.digest
        || proposal.validation != validation
    {
        return Err(
            "external Proposal is not bound to its request, generator, and validation".into(),
        );
    }
    for patch in &proposal.patches {
        if patch.path == request.source.location.path
            || patch.path == ".deshell"
            || patch.path.starts_with(".deshell/")
            || patch.path == ".git"
            || patch.path.starts_with(".git/")
        {
            return Err(format!(
                "external generator may not replace a retiring source or internal metadata: {}",
                patch.path
            ));
        }
        validate_external_patch_state(root, patch)?;
    }
    let mut expected_nodes = Vec::new();
    collect_node_ids(&task.body, &mut expected_nodes);
    let expected_nodes = expected_nodes.into_iter().collect::<BTreeSet<_>>();
    let mapped_nodes = proposal
        .source_map
        .iter()
        .map(|span| span.ir_node.clone())
        .collect::<BTreeSet<_>>();
    if mapped_nodes != expected_nodes {
        return Err("external Proposal source map does not cover every Effect IR node".into());
    }
    Ok(())
}

fn validate_external_patch_state(root: &Path, patch: &GeneratorPatch) -> Result<(), String> {
    let expected = current_target_digest(root, &patch.path)?;
    match (patch.operation, expected) {
        (PatchOperation::Create, None) => Ok(()),
        (PatchOperation::Update, Some(current))
            if patch.expected_digest.as_deref() == Some(current.as_str()) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "external Proposal target state is stale or forged for {}",
            patch.path
        )),
    }
}

fn isolated_tree_digest(root: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry =
            entry.map_err(|error| format!("cannot inspect generator isolation: {error}"))?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| "generator isolation path escaped its root")?
            .to_str()
            .ok_or("generator isolation path is not UTF-8")?
            .replace('\\', "/");
        let metadata = entry
            .path()
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect isolated generator path: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("external generator isolation contains a symlink".into());
        }
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt as _;
            metadata.permissions().mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        if metadata.file_type().is_dir() {
            entries.push((relative, "directory", String::new(), mode));
        } else if metadata.file_type().is_file() {
            let (_, digest) = crate::digest::file_sha256(entry.path())?;
            entries.push((relative, "file", digest, mode));
        } else {
            return Err("external generator isolation contains a special file".into());
        }
    }
    entries.sort();
    canonical_digest(&entries)
}

fn ensure_isolated_tree_unchanged(root: &Path, expected: &str) -> Result<(), String> {
    if isolated_tree_digest(root)? != expected {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_DIRECT_MUTATION: external generator mutated its isolated workspace"
                .into(),
        );
    }
    Ok(())
}

fn guarded_project_tree_digest(root: &Path) -> Result<String, String> {
    const IGNORED_DIRECTORIES: &[&str] = &[
        ".git",
        ".hg",
        ".svn",
        "_build",
        "_opam",
        "build",
        "node_modules",
        "target",
        "vendor",
    ];

    let root = canonical_root(root)?;
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !IGNORED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref())
        })
    {
        let entry = entry.map_err(|error| {
            format!("cannot inspect project tree before external generator execution: {error}")
        })?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| "guarded project path escaped its root")?
            .to_str()
            .ok_or("guarded project path is not UTF-8")?
            .replace('\\', "/");
        let metadata = entry
            .path()
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect guarded project path: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "guarded project tree contains a symlink: {relative}"
            ));
        }
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt as _;
            metadata.permissions().mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = u32::from(metadata.permissions().readonly());
        if metadata.file_type().is_dir() {
            entries.push((relative, "directory", String::new(), mode));
        } else if metadata.file_type().is_file() {
            let (_, digest) = crate::digest::file_sha256(entry.path())?;
            entries.push((relative, "file", digest, mode));
        } else {
            return Err("guarded project tree contains a special file".into());
        }
    }
    entries.sort();
    canonical_digest(&entries)
}

fn ensure_guarded_project_tree_unchanged(root: &Path, expected: &str) -> Result<(), String> {
    match guarded_project_tree_digest(root) {
        Ok(actual) if actual == expected => Ok(()),
        Ok(_) => Err(
            "DESHELL_BLOCKER_GENERATOR_DIRECT_MUTATION: external generator mutated the live project"
                .into(),
        ),
        Err(error) => Err(format!(
            "DESHELL_BLOCKER_GENERATOR_DIRECT_MUTATION: external generator left the live project unverifiable: {error}"
        )),
    }
}

fn external_generator_blocker(message: String) -> String {
    if message.contains("DESHELL_BLOCKER_") {
        message
    } else {
        format!("DESHELL_BLOCKER_GENERATOR_PROTOCOL: {message}")
    }
}

pub(crate) fn generator_propose(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let request: MigrationRequest = serde_json::from_value(
        parameters
            .get("request")
            .cloned()
            .ok_or("params.request is required")?,
    )
    .map_err(|error| format!("params.request is invalid: {error}"))?;
    validate_migration_request(&request)?;
    let target_path = parameters
        .get("target_path")
        .and_then(serde_json::Value::as_str)
        .ok_or("params.target_path must be a string")?;
    let normalized = crate::ir::normalize_path(target_path)?;
    if normalized != target_path {
        return Err("params.target_path is not normalized".into());
    }
    let expected_digest = match parameters.get("expected_digest") {
        Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(value)) if crate::digest::valid_sha256(value) => {
            Some(value.clone())
        }
        _ => return Err("params.expected_digest must be null or a SHA-256 digest".into()),
    };
    let validation: Vec<Vec<String>> = serde_json::from_value(
        parameters
            .get("validation")
            .cloned()
            .ok_or("params.validation is required")?,
    )
    .map_err(|error| format!("params.validation must be exact argv arrays: {error}"))?;
    for command in &validation {
        validate_exact_argv(command)?;
    }
    let plan: crate::ir::Plan = serde_json::from_value(request.effect_ir.clone())
        .map_err(|error| format!("request.effect_ir is invalid: {error}"))?;
    plan.validate().map_err(|errors| errors.join("; "))?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or("request Effect IR entrypoint task is missing")?;
    let stem = source_stem(&request.source.location.path);
    let expected_target = match request.target {
        crate::config::MigrationTarget::Rust => {
            format!("{}/{}.rs", request.module_root, rust_binary_name(&stem))
        }
        crate::config::MigrationTarget::Go => format!("{}/{stem}.go", request.module_root),
        crate::config::MigrationTarget::Host | crate::config::MigrationTarget::Agent => {
            return Err("official generator RPC currently accepts Rust or Go requests".into());
        }
    };
    if target_path != expected_target {
        return Err(format!(
            "params.target_path must be {expected_target} for this request"
        ));
    }
    let generated = match request.target {
        crate::config::MigrationTarget::Rust => generate_rust(&plan)?,
        crate::config::MigrationTarget::Go => generate_go(&plan)?,
        crate::config::MigrationTarget::Host | crate::config::MigrationTarget::Agent => {
            unreachable!()
        }
    };
    let verification_output = verification_binary_path(&stem, std::env::consts::OS);
    let (build_argv, run_argv) = match request.target {
        crate::config::MigrationTarget::Rust => (
            rust_build_argv(target_path, &verification_output, std::env::consts::OS),
            vec![verification_output],
        ),
        crate::config::MigrationTarget::Go => (
            vec![
                "go".into(),
                "build".into(),
                "-p=1".into(),
                "-o".into(),
                verification_output.clone(),
                target_path.into(),
            ],
            vec![verification_output],
        ),
        crate::config::MigrationTarget::Host | crate::config::MigrationTarget::Agent => {
            unreachable!()
        }
    };
    let mut node_ids = Vec::new();
    collect_node_ids(&task.body, &mut node_ids);
    let generated_len = generated.len() as u64;
    let content_digest = crate::digest::sha256(&generated);
    let mut proposal = Proposal {
        schema_version: 1,
        proposal_digest: ZERO_DIGEST.into(),
        request_digest: request.request_id,
        generator_digest: official_generator_digest(),
        patches: vec![GeneratorPatch {
            operation: if expected_digest.is_some() {
                PatchOperation::Update
            } else {
                PatchOperation::Create
            },
            path: target_path.into(),
            expected_digest,
            content_base64: base64::engine::general_purpose::STANDARD.encode(generated),
            content_digest,
            permissions: 0o644,
        }],
        build_argv,
        run_argv,
        validation,
        dependencies: Vec::new(),
        source_map: node_ids
            .into_iter()
            .map(|ir_node| GeneratedSpan {
                ir_node,
                generated: Location {
                    path: target_path.into(),
                    start_byte: 0,
                    end_byte: generated_len,
                },
            })
            .collect(),
    };
    proposal.proposal_digest = canonical_digest(&proposal)?;
    validate_proposal(&proposal)?;
    serde_json::to_value(proposal).map_err(|error| error.to_string())
}

fn validate_migration_request(request: &MigrationRequest) -> Result<(), String> {
    if request.schema_version != 1
        || !crate::digest::valid_sha256(&request.source.content_digest)
        || !crate::digest::valid_sha256(&request.effect_ir_digest)
        || request.source.interpreter.trim().is_empty()
    {
        return Err("Migration Request version, digest, or interpreter is invalid".into());
    }
    let mut unsigned = request.clone();
    unsigned.request_id = ZERO_DIGEST.into();
    if canonical_digest(&unsigned)? != request.request_id {
        return Err("Migration Request digest mismatch".into());
    }
    let path = crate::ir::normalize_path(&request.source.location.path)?;
    if path != request.source.location.path
        || request.source.location.start_byte > request.source.location.end_byte
    {
        return Err("Migration Request source location is invalid".into());
    }
    let module_root = crate::ir::normalize_path(&request.module_root)?;
    if module_root != request.module_root {
        return Err("Migration Request module root is invalid".into());
    }
    if canonical_digest(&request.effect_ir)? != request.effect_ir_digest {
        return Err("Migration Request Effect IR digest mismatch".into());
    }
    for call_site in &request.call_sites {
        let path = crate::ir::normalize_path(&call_site.path)?;
        if path != call_site.path || call_site.start_byte > call_site.end_byte {
            return Err("Migration Request call site is invalid".into());
        }
    }
    Ok(())
}

struct HostGeneration {
    bytes: Vec<u8>,
    build_argv: Vec<String>,
    run_argv: Vec<String>,
    generated_span: Location,
    additional_files: Vec<HostFile>,
}

struct HostFile {
    path: String,
    bytes: Vec<u8>,
    permissions: u32,
}

fn generate_structured_host(
    root: &Path,
    finding: &crate::scanner::Finding,
    plan: &crate::ir::Plan,
) -> Result<HostGeneration, String> {
    let name = finding.path.rsplit('/').next().unwrap_or(&finding.path);
    let lower = name.to_ascii_lowercase();
    if name.eq_ignore_ascii_case("Dockerfile")
        || lower.starts_with("dockerfile.")
        || lower.ends_with(".dockerfile")
    {
        let (bytes, run_argv, generated_span) = generate_docker_host(root, finding, plan)?;
        return Ok(HostGeneration {
            bytes,
            build_argv: vec!["true".into()],
            run_argv,
            generated_span,
            additional_files: Vec::new(),
        });
    }
    if lower.ends_with(".py") {
        return generate_python_host(root, finding, plan);
    }
    if lower.ends_with(".js") || lower.ends_with(".cjs") {
        return generate_javascript_host(root, finding, plan);
    }
    if finding.path.starts_with(".github/workflows/")
        && (lower.ends_with(".yml") || lower.ends_with(".yaml"))
    {
        return generate_github_action_host(root, finding, plan);
    }
    Err(format!(
        "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: structured host generator does not support {}",
        finding.path
    ))
}

fn generate_github_action_host(
    root: &Path,
    finding: &crate::scanner::Finding,
    plan: &crate::ir::Plan,
) -> Result<HostGeneration, String> {
    let argv = literal_exec_argv(plan, "GitHub local action argv")?;
    let (_, path) = crate::project::resolve_entry(root, &finding.path)?;
    let host = std::fs::read(&path)
        .map_err(|error| format!("cannot read structured host {}: {error}", finding.path))?;
    let (start, end, original) = structured_host_span(&host, finding)?;
    let line_start = host[..start]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let line_start = if start == line_start && start != 0 {
        start
    } else {
        line_start
    };
    let first_line_end = host[line_start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(host.len(), |index| line_start + index);
    let first_line = std::str::from_utf8(&host[line_start..first_line_end])
        .map_err(|_| "GitHub workflow run line is not UTF-8")?;
    let key = first_line
        .rfind("run:")
        .ok_or("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: GitHub shell span is not a run step")?;
    let key_start = line_start + key;
    let scalar_header = first_line[key + "run:".len()..].trim();
    let block_scalar = matches!(scalar_header, "|" | "|-" | "|+" | ">" | ">-" | ">+");
    if block_scalar {
        if start != line_start
            || !original
                .as_bytes()
                .starts_with(&host[line_start..first_line_end])
        {
            return Err(
                "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: GitHub block scalar source map is inconsistent"
                    .into(),
            );
        }
    } else {
        if original.as_bytes() != finding.source {
            return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: GitHub inline scalar source map is inconsistent".into());
        }
        let value_prefix = &host[key_start + "run:".len()..start];
        if !value_prefix.iter().all(u8::is_ascii_whitespace)
            || !host[end..first_line_end]
                .iter()
                .all(u8::is_ascii_whitespace)
        {
            return Err(
                "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: GitHub run scalar has trailing syntax"
                    .into(),
            );
        }
    }
    let action_id = &finding.content_digest[..12];
    let action_directory = format!(".github/actions/deshell-{action_id}");
    let uses = format!("uses: ./{action_directory}");
    let (bytes, _workflow_span) = replace_structured_host_span(ReplaceStructuredHostSpanArgs {
        host: &host,
        finding,
        start: key_start,
        end,
        replacement: uses.as_bytes(),
    });
    let program = serde_json::to_string(&argv[0]).map_err(|error| error.to_string())?;
    let arguments = serde_json::to_string(&argv[1..]).map_err(|error| error.to_string())?;
    let javascript = format!(
        concat!(
            "// Generated by de-shell; project-native code with no de-shell runtime.\n",
            "const {{ spawnSync }} = require(\"node:child_process\");\n",
            "const result = spawnSync({program}, {arguments}, {{ stdio: \"inherit\", shell: false }});\n",
            "if (result.error) throw result.error;\n",
            "if (result.signal) process.kill(process.pid, result.signal);\n",
            "process.exitCode = result.status === null ? 1 : result.status;\n",
        ),
        program = program,
        arguments = arguments,
    );
    let action = concat!(
        "name: de-shell generated action\n",
        "description: Project-native replacement for a retired shell step\n",
        "runs:\n",
        "  using: node24\n",
        "  main: index.js\n",
    );
    let index_path = format!("{action_directory}/index.js");
    let generated_span = Location {
        path: index_path.clone(),
        start_byte: 0,
        end_byte: javascript.len() as u64,
    };
    Ok(HostGeneration {
        bytes,
        build_argv: vec!["node".into(), "--check".into(), index_path.clone()],
        run_argv: argv,
        generated_span,
        additional_files: vec![
            HostFile {
                path: format!("{action_directory}/action.yml"),
                bytes: action.as_bytes().to_vec(),
                permissions: 0o644,
            },
            HostFile {
                path: index_path,
                bytes: javascript.into_bytes(),
                permissions: 0o644,
            },
        ],
    })
}

fn generate_javascript_host(
    root: &Path,
    finding: &crate::scanner::Finding,
    plan: &crate::ir::Plan,
) -> Result<HostGeneration, String> {
    let argv = literal_exec_argv(plan, "JavaScript execFileSync argv")?;
    let (_, path) = crate::project::resolve_entry(root, &finding.path)?;
    let host = std::fs::read(&path)
        .map_err(|error| format!("cannot read structured host {}: {error}", finding.path))?;
    let (start, end, original) = structured_host_span(&host, finding)?;
    let call = original.trim();
    let arguments = call
        .strip_prefix("child_process.execSync")
        .map(str::trim_start)
        .and_then(|value| value.strip_prefix('('))
        .and_then(|value| value.strip_suffix(')'))
        .ok_or("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: JavaScript host rewrite requires child_process.execSync")?;
    let values = crate::scanner::split_top_level_arguments(arguments);
    if values.len() != 2 || crate::scanner::quoted_literal(values[0].trim()).is_none() {
        return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: JavaScript execSync requires one quoted command and inherited stdio options".into());
    }
    let options = values[1].trim();
    let compact_options = options
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if !matches!(
        compact_options.as_str(),
        "{stdio:\"inherit\"}" | "{stdio:'inherit'}"
    ) {
        return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: JavaScript execSync migration requires exactly {stdio: \"inherit\"}".into());
    }
    let program = serde_json::to_string(&argv[0]).map_err(|error| error.to_string())?;
    let arguments = serde_json::to_string(&argv[1..]).map_err(|error| error.to_string())?;
    let replacement = format!("child_process.execFileSync({program},{arguments}, {options})");
    let (bytes, generated_span) = replace_structured_host_span(ReplaceStructuredHostSpanArgs {
        host: &host,
        finding,
        start,
        end,
        replacement: replacement.as_bytes(),
    });
    Ok(HostGeneration {
        bytes,
        build_argv: vec!["node".into(), "--check".into(), finding.path.clone()],
        run_argv: argv,
        generated_span,
        additional_files: Vec::new(),
    })
}

fn generate_python_host(
    root: &Path,
    finding: &crate::scanner::Finding,
    plan: &crate::ir::Plan,
) -> Result<HostGeneration, String> {
    let argv = literal_exec_argv(plan, "Python subprocess argv")?;
    let (_, path) = crate::project::resolve_entry(root, &finding.path)?;
    let host = std::fs::read(&path)
        .map_err(|error| format!("cannot read structured host {}: {error}", finding.path))?;
    let (start, end, original) = structured_host_span(&host, finding)?;
    let call = original.trim();
    let arguments = call
        .strip_prefix("subprocess.run")
        .map(str::trim_start)
        .and_then(|value| value.strip_prefix('('))
        .and_then(|value| value.strip_suffix(')'))
        .ok_or(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Python host rewrite requires subprocess.run",
        )?;
    let values = crate::scanner::split_top_level_arguments(arguments);
    if values.is_empty() || crate::scanner::quoted_literal(values[0].trim()).is_none() {
        return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Python subprocess command must be one quoted literal".into());
    }
    let mut shell = None;
    let mut check = None;
    for value in values.iter().skip(1) {
        let Some((name, value)) = value.split_once('=') else {
            return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Python subprocess positional options are not supported".into());
        };
        match (name.trim(), value.trim()) {
            ("shell", "True") if shell.replace(true).is_none() => {}
            ("shell", "False") if shell.replace(false).is_none() => {}
            ("check", "True") if check.replace(true).is_none() => {}
            ("check", "False") if check.replace(false).is_none() => {}
            ("shell" | "check", _) => {
                return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: duplicate or dynamic Python subprocess option".into());
            }
            _ => {
                return Err(format!(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Python subprocess option {} is not modeled",
                    name.trim()
                ));
            }
        }
    }
    if shell != Some(true) {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Python host rewrite requires shell=True".into(),
        );
    }
    let argv_literal = serde_json::to_string(&argv).map_err(|error| error.to_string())?;
    let replacement = format!(
        "subprocess.run({argv_literal}, shell=False, check={})",
        if check.unwrap_or(false) {
            "True"
        } else {
            "False"
        }
    );
    let (bytes, generated_span) = replace_structured_host_span(ReplaceStructuredHostSpanArgs {
        host: &host,
        finding,
        start,
        end,
        replacement: replacement.as_bytes(),
    });
    Ok(HostGeneration {
        bytes,
        build_argv: vec![
            "python3".into(),
            "-m".into(),
            "py_compile".into(),
            finding.path.clone(),
        ],
        run_argv: argv,
        generated_span,
        additional_files: Vec::new(),
    })
}

fn literal_exec_argv(plan: &crate::ir::Plan, context: &str) -> Result<Vec<String>, String> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or("host generator entrypoint task is missing")?;
    let crate::ir::Operation::Exec {
        argv,
        environment,
        working_directory,
    } = &task.body.operation
    else {
        return Err(format!(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: {context} requires one Exec node"
        ));
    };
    if !environment.is_empty() || working_directory.is_some() {
        return Err(format!(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: {context} cannot preserve per-command environment or cwd"
        ));
    }
    let argv = argv
        .iter()
        .map(literal_text_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if argv.is_empty() {
        return Err(format!(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: {context} is empty"
        ));
    }
    Ok(argv)
}

fn structured_host_span<'a>(
    host: &'a [u8],
    finding: &crate::scanner::Finding,
) -> Result<(usize, usize, &'a str), String> {
    let start = usize::try_from(finding.span.start_byte)
        .map_err(|_| "structured host start offset is too large")?;
    let end = usize::try_from(finding.span.end_byte)
        .map_err(|_| "structured host end offset is too large")?;
    if start > end || end > host.len() {
        return Err("structured host source span is outside the document".into());
    }
    let original = std::str::from_utf8(&host[start..end])
        .map_err(|_| "structured host source span is not UTF-8")?;
    Ok((start, end, original))
}

/// The inputs of [`replace_structured_host_span`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`replace_structured_host_span`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ReplaceStructuredHostSpanArgs<'a> {
    host: &'a [u8],
    finding: &'a crate::scanner::Finding,
    start: usize,
    end: usize,
    replacement: &'a [u8],
}

fn replace_structured_host_span(parts: ReplaceStructuredHostSpanArgs<'_>) -> (Vec<u8>, Location) {
    // Destructured without `..`: see `ReplaceStructuredHostSpanArgs`.
    let ReplaceStructuredHostSpanArgs {
        host,
        finding,
        start,
        end,
        replacement,
    } = parts;
    let mut generated = Vec::with_capacity(host.len() - (end - start) + replacement.len());
    generated.extend_from_slice(&host[..start]);
    generated.extend_from_slice(replacement);
    generated.extend_from_slice(&host[end..]);
    (
        generated,
        Location {
            path: finding.path.clone(),
            start_byte: start as u64,
            end_byte: (start + replacement.len()) as u64,
        },
    )
}

fn generate_docker_host(
    root: &Path,
    finding: &crate::scanner::Finding,
    plan: &crate::ir::Plan,
) -> Result<(Vec<u8>, Vec<String>, Location), String> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or("host generator entrypoint task is missing")?;
    let crate::ir::Operation::Exec {
        argv,
        environment,
        working_directory,
    } = &task.body.operation
    else {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Docker exec-form requires one Exec node".into(),
        );
    };
    if !environment.is_empty() || working_directory.is_some() {
        return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Docker exec-form cannot preserve per-command environment or cwd".into());
    }
    let argv = argv
        .iter()
        .map(literal_text_expression)
        .collect::<Result<Vec<_>, _>>()?;
    if argv.is_empty() {
        return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: Docker exec-form argv is empty".into());
    }
    let (_, path) = crate::project::resolve_entry(root, &finding.path)?;
    let host = std::fs::read(&path)
        .map_err(|error| format!("cannot read structured host {}: {error}", finding.path))?;
    let start = usize::try_from(finding.span.start_byte)
        .map_err(|_| "structured host start offset is too large")?;
    let end = usize::try_from(finding.span.end_byte)
        .map_err(|_| "structured host end offset is too large")?;
    if start > end || end > host.len() {
        return Err("structured host source span is outside the document".into());
    }
    let original = std::str::from_utf8(&host[start..end])
        .map_err(|_| "structured host source span is not UTF-8")?;
    let indentation = &original[..original.len() - original.trim_start().len()];
    let json = serde_json::to_string(&argv).map_err(|error| error.to_string())?;
    let replacement = format!("{indentation}RUN {json}");
    let mut generated = Vec::with_capacity(host.len() - (end - start) + replacement.len());
    generated.extend_from_slice(&host[..start]);
    generated.extend_from_slice(replacement.as_bytes());
    generated.extend_from_slice(&host[end..]);
    Ok((
        generated,
        argv,
        Location {
            path: finding.path.clone(),
            start_byte: start as u64,
            end_byte: (start + replacement.len()) as u64,
        },
    ))
}

fn literal_text_expression(expression: &crate::ir::TextExpression) -> Result<String, String> {
    let mut output = String::new();
    for part in &expression.parts {
        match part {
            crate::ir::TextPart::Literal { value } => output.push_str(value),
            crate::ir::TextPart::Variable { .. }
            | crate::ir::TextPart::Argument { .. }
            | crate::ir::TextPart::DefaultValue { .. } => {
                return Err("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: structured host argv cannot preserve shell expansion".into());
            }
        }
    }
    Ok(output)
}

fn lower_finding(
    finding: &crate::scanner::Finding,
    policy: crate::config::UnknownInterpreter,
) -> Result<crate::ir::Plan, String> {
    let interpreter = resolved_finding_interpreter(finding)?;
    crate::frontend::lower_with_interpreter(&finding.path, &finding.source, policy, &interpreter)
}

fn resolved_finding_interpreter(finding: &crate::scanner::Finding) -> Result<String, String> {
    let configured = finding.interpreter.as_deref().ok_or_else(|| {
        format!(
            "DESHELL_BLOCKER_UNKNOWN_INTERPRETER: {} has no resolved interpreter",
            finding.path
        )
    })?;
    if configured == "package-shell" {
        let command = std::str::from_utf8(&finding.source).map_err(
            |_| "DESHELL_BLOCKER_UNIMPLEMENTED_HOST_INTERFACE: package script is not UTF-8",
        )?;
        let argv = static_shell_words(command).map_err(|message| {
            format!("DESHELL_BLOCKER_UNIMPLEMENTED_HOST_INTERFACE: package script {message}")
        })?;
        let program = argv[0]
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&argv[0])
            .to_ascii_lowercase();
        match program.as_str() {
            "sh" | "bash" | "zsh" | "fish" | "cmd" | "nu" => Ok(program),
            "pwsh" | "powershell" => Ok("powershell".into()),
            _ => Err(format!(
                "DESHELL_BLOCKER_UNIMPLEMENTED_HOST_INTERFACE: package script in {} does not declare a supported interpreter",
                finding.path
            )),
        }
    } else {
        Ok(configured.into())
    }
}

fn add_scenario_input_coverage_blockers(
    plan: &crate::ir::Plan,
    scenarios: &BTreeMap<String, ApprovedScenario>,
    location: &Location,
    blockers: &mut Vec<Blocker>,
) {
    if scenarios.is_empty() {
        return;
    }
    let covered_arguments = scenarios
        .values()
        .flat_map(|scenario| scenario.arguments.iter().map(|value| value.name.as_str()))
        .collect::<BTreeSet<_>>();
    let covered_environment = scenarios
        .values()
        .flat_map(|scenario| scenario.environment.iter().map(|value| value.name.as_str()))
        .collect::<BTreeSet<_>>();
    for task in &plan.tasks {
        for input in &task.inputs {
            if !covered_arguments.contains(input.name.as_str()) {
                blockers.push(Blocker {
                    code: "DESHELL_BLOCKER_SCENARIO_INPUT_COVERAGE".into(),
                    message: format!(
                        "approved scenarios do not cover argument {} required by {}",
                        input.name, location.path
                    ),
                    location: Some(location.clone()),
                });
            }
        }
        for name in &task.environment {
            if !covered_environment.contains(name.as_str()) {
                blockers.push(Blocker {
                    code: "DESHELL_BLOCKER_SCENARIO_INPUT_COVERAGE".into(),
                    message: format!(
                        "approved scenarios do not cover environment {name} required by {}",
                        location.path
                    ),
                    location: Some(location.clone()),
                });
            }
        }
    }
}

fn approved_cells(
    root: &Path,
    config: &crate::config::ProjectConfig,
) -> Result<Vec<CellRequirement>, String> {
    let mut output = Vec::new();
    for cell in &config.platform_cells {
        let Some(approval_digest) = crate::approval::matrix_approval(root, cell)? else {
            continue;
        };
        output.push(CellRequirement {
            id: cell.id.clone(),
            platform_fingerprint: crate::digest::sha256(
                format!(
                    "deshell-platform-v1:{}:{}",
                    cell.operating_system, cell.architecture
                )
                .as_bytes(),
            ),
            runtime_fingerprint: crate::digest::sha256(
                format!("deshell-runtime-v1:{}", cell.runtime).as_bytes(),
            ),
            approval_digest,
        });
    }
    output.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(output)
}

fn classify_coverage(plan: &crate::ir::Plan, source_bytes: usize) -> Coverage {
    let mut bytes = vec![0_u8; source_bytes];
    fn mark(node: &crate::ir::Node, bytes: &mut [u8]) {
        if let Some(span) = &node.source {
            let start = usize::try_from(span.start_byte).unwrap_or(usize::MAX);
            let end = usize::try_from(span.end_byte).unwrap_or(usize::MAX);
            if start <= end && end <= bytes.len() {
                let level = match node.guarantee {
                    crate::ir::Guarantee::Native { .. } => 1,
                    crate::ir::Guarantee::Delegated { .. } => 2,
                    crate::ir::Guarantee::Residual { .. } => 3,
                };
                for byte in &mut bytes[start..end] {
                    *byte = (*byte).max(level);
                }
            }
        }
        visit_node(node, |child| mark(child, bytes));
    }
    for task in &plan.tasks {
        mark(&task.body, &mut bytes);
    }
    let mut coverage = Coverage {
        total_bytes: source_bytes as u64,
        ..Coverage::default()
    };
    for byte in bytes {
        match byte {
            0 => coverage.trivia_bytes += 1,
            1 => coverage.native_bytes += 1,
            2 => coverage.delegated_bytes += 1,
            3 => coverage.residual_bytes += 1,
            _ => unreachable!(),
        }
    }
    coverage
}

fn guarantee_counts(plan: &crate::ir::Plan) -> (usize, usize, usize) {
    let mut counts = (0, 0, 0);
    fn count(node: &crate::ir::Node, counts: &mut (usize, usize, usize)) {
        match node.guarantee {
            crate::ir::Guarantee::Native { .. } => counts.0 += 1,
            crate::ir::Guarantee::Delegated { .. } => counts.1 += 1,
            crate::ir::Guarantee::Residual { .. } => counts.2 += 1,
        }
        visit_node(node, |child| count(child, counts));
    }
    for task in &plan.tasks {
        count(&task.body, &mut counts);
    }
    counts
}

fn delegated_reasons(plan: &crate::ir::Plan) -> Vec<String> {
    let mut reasons = BTreeSet::new();
    fn collect(node: &crate::ir::Node, reasons: &mut BTreeSet<String>) {
        if let crate::ir::Guarantee::Delegated { reason } = &node.guarantee {
            reasons.insert(reason.clone());
        }
        visit_node(node, |child| collect(child, reasons));
    }
    for task in &plan.tasks {
        collect(&task.body, &mut reasons);
    }
    reasons.into_iter().collect()
}

fn delegated_blocker_code(reasons: &[String]) -> &'static str {
    if reasons.iter().any(|reason| {
        reason.contains("dynamic shell evaluation") || reason.contains("dynamic evaluation")
    }) {
        "DESHELL_BLOCKER_DYNAMIC_EVAL"
    } else if reasons.iter().any(|reason| {
        reason.contains("unterminated")
            || reason.contains("parse error")
            || reason.contains("invalid syntax")
    }) {
        "DESHELL_BLOCKER_PARSE_ERROR"
    } else if reasons.iter().any(|reason| {
        reason.contains("parser unavailable") || reason.contains("runtime unavailable")
    }) {
        "DESHELL_BLOCKER_PARSER_UNAVAILABLE"
    } else {
        "DESHELL_BLOCKER_UNIMPLEMENTED_SEMANTIC"
    }
}

fn delegated_blocker_location(reasons: &[String], source: &Location, kind: SourceKind) -> Location {
    if kind != SourceKind::ShellFile {
        return source.clone();
    }
    let source_len = source.end_byte.saturating_sub(source.start_byte);
    for reason in reasons {
        let Some((_, suffix)) = reason.split_once(" at bytes ") else {
            continue;
        };
        let range = suffix.split_once(" (").map_or(suffix, |(range, _)| range);
        let Some((start, end)) = range.split_once("..") else {
            continue;
        };
        let (Ok(start), Ok(end)) = (start.parse::<u64>(), end.parse::<u64>()) else {
            continue;
        };
        if start <= end && end <= source_len {
            return Location {
                path: source.path.clone(),
                start_byte: source.start_byte + start,
                end_byte: source.start_byte + end,
            };
        }
    }
    source.clone()
}

fn visit_node(node: &crate::ir::Node, mut visit: impl FnMut(&crate::ir::Node)) {
    match &node.operation {
        crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Sequence { nodes, .. }
        | crate::ir::Operation::Parallel { nodes } => {
            for child in nodes {
                visit(child);
            }
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            visit(predicate);
            visit(if_true);
            if let Some(child) = if_false {
                visit(child);
            }
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            for case in cases {
                visit(&case.body);
            }
            if let Some(child) = default {
                visit(child);
            }
        }
        crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::Not { body }
        | crate::ir::Operation::Redirect { body, .. }
        | crate::ir::Operation::Scope { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. }
        | crate::ir::Operation::Spawn { body, .. } => visit(body),
        crate::ir::Operation::While { condition, body } => {
            visit(condition);
            visit(body);
        }
        crate::ir::Operation::TryFinally { body, finalizer } => {
            visit(body);
            visit(finalizer);
        }
        // Listed rather than matched with `_`: a new operation that carries a
        // node has to be added here, and a wildcard would instead walk past its
        // children in silence. `While` reached this walk only after the
        // wildcard was removed, and until then every node inside a loop was
        // invisible to every caller.
        crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::ExpandWords { .. }
        | crate::ir::Operation::NoOp
        | crate::ir::Operation::WriteStdout { .. }
        | crate::ir::Operation::Test { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::SetEnvironment { .. }
        | crate::ir::Operation::SetWorkingDirectory { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::SendSignal { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::FileMetadata { .. }
        | crate::ir::Operation::FileSetMetadata { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::TaskCall { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::Exit { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => {}
    }
}

fn collect_node_ids(node: &crate::ir::Node, output: &mut Vec<String>) {
    output.push(node.id.clone());
    visit_node(node, |child| collect_node_ids(child, output));
}

#[derive(Clone, Debug)]
struct ReplayRequest {
    method: String,
    uri: String,
    body: Vec<u8>,
}

fn load_network_replay(root: &Path) -> Result<(crate::replay::ReplayStore, String), String> {
    let path = crate::project::project_file_path(root, ".deshell/replay.json")
        .map_err(|_| ".deshell/replay.json is missing or unsafe".to_owned())?;
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect network replay: {error}"))?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.len() > 16 * 1024 * 1024
    {
        return Err("network replay must be a regular file of at most 16 MiB".into());
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("cannot read network replay {}: {error}", path.display()))?;
    let replay = crate::replay::ReplayStore::decode(&bytes).map_err(|errors| errors.join("; "))?;
    if replay.encode_pretty()? != bytes {
        return Err("network replay is not canonical persisted JSON".into());
    }
    Ok((replay, crate::digest::sha256(&bytes)))
}

fn bind_network_replay(root: &Path, plan: &crate::ir::Plan) -> Result<String, String> {
    let requests = network_replay_requests(plan)?;
    if requests.is_empty() {
        return Err("network effects could not be reduced to static HTTP requests".into());
    }
    let (replay, digest) = load_network_replay(root)?;
    for request in requests {
        replay.lookup_entry_prevalidated(&request.method, &request.uri, &request.body)?;
    }
    Ok(digest)
}

fn network_replay_requests(plan: &crate::ir::Plan) -> Result<Vec<ReplayRequest>, String> {
    fn collect(node: &crate::ir::Node, output: &mut Vec<ReplayRequest>) -> Result<(), String> {
        match &node.operation {
            crate::ir::Operation::NetworkRequest { method, uri } => {
                let method = literal_text_value(method)
                    .ok_or("network replay requires a literal HTTP method")?;
                let uri =
                    literal_text_value(uri).ok_or("network replay requires a literal HTTP URI")?;
                require_replayable_http_uri(&uri)?;
                output.push(ReplayRequest {
                    method: method.to_ascii_uppercase(),
                    uri,
                    body: Vec::new(),
                });
            }
            crate::ir::Operation::Exec { argv, .. } => {
                let Some(program) = argv.first().and_then(literal_text_value) else {
                    visit_node(node, |_| {});
                    return Ok(());
                };
                let name = program
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&program)
                    .to_ascii_lowercase();
                if name == "curl" {
                    let argv = argv
                        .iter()
                        .map(|argument| {
                            literal_text_value(argument)
                                .ok_or("curl replay requires a fully literal argv")
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    output.push(parse_curl_replay_request(&argv)?);
                } else if [
                    "wget", "ftp", "nc", "ncat", "netcat", "socat", "ssh", "scp", "sftp",
                ]
                .contains(&name.as_str())
                {
                    return Err(format!("network replay does not support {name}"));
                }
            }
            _ => {}
        }
        let mut error = None;
        visit_node(node, |child| {
            if error.is_none() {
                error = collect(child, output).err();
            }
        });
        error.map_or(Ok(()), Err)
    }

    let mut output = Vec::new();
    for task in &plan.tasks {
        collect(&task.body, &mut output)?;
    }
    Ok(output)
}

fn parse_curl_replay_request(argv: &[String]) -> Result<ReplayRequest, String> {
    let mut method = "GET".to_owned();
    let mut uri = None;
    let mut body = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "-s" | "--silent" | "-S" | "--show-error" => index += 1,
            "-X" | "--request" => {
                let value = argv
                    .get(index + 1)
                    .ok_or("curl replay request option is missing its value")?;
                method = value.to_ascii_uppercase();
                index += 2;
            }
            "-d" | "--data" | "--data-raw" => {
                let value = argv
                    .get(index + 1)
                    .ok_or("curl replay data option is missing its value")?;
                body = value.as_bytes().to_vec();
                if method == "GET" {
                    method = "POST".into();
                }
                index += 2;
            }
            value if value.starts_with("http://") || value.starts_with("https://") => {
                if uri.replace(value.to_owned()).is_some() {
                    return Err("curl replay supports exactly one URI".into());
                }
                index += 1;
            }
            option if option.starts_with('-') => {
                return Err(format!("curl replay does not support option {option}"));
            }
            value => {
                return Err(format!(
                    "curl replay does not support positional value {value}"
                ));
            }
        }
    }
    let uri = uri.ok_or("curl replay request omitted its URI")?;
    require_replayable_http_uri(&uri)?;
    Ok(ReplayRequest { method, uri, body })
}

fn require_replayable_http_uri(uri: &str) -> Result<(), String> {
    if uri.starts_with("http://") {
        Ok(())
    } else if uri.starts_with("https://") {
        Err("HTTPS replay requires a trusted interception transport and is unavailable".into())
    } else {
        Err("network replay URI must use absolute HTTP".into())
    }
}

fn network_effects(plan: &crate::ir::Plan) -> Vec<(String, Option<Location>)> {
    fn collect(node: &crate::ir::Node, output: &mut BTreeSet<(String, Option<Location>)>) {
        let effect = match &node.operation {
            crate::ir::Operation::NetworkRequest { .. } => Some("network_request".to_owned()),
            crate::ir::Operation::Exec { argv, .. } => argv
                .first()
                .and_then(literal_text_value)
                .and_then(|program| {
                    let name = program
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or(&program)
                        .to_ascii_lowercase();
                    [
                        "curl", "wget", "ftp", "nc", "ncat", "netcat", "socat", "ssh", "scp",
                        "sftp",
                    ]
                    .contains(&name.as_str())
                    .then_some(name)
                }),
            _ => None,
        };
        if let Some(effect) = effect {
            output.insert((
                effect,
                node.source.as_ref().map(|span| Location {
                    path: span.file.clone(),
                    start_byte: span.start_byte,
                    end_byte: span.end_byte,
                }),
            ));
        }
        visit_node(node, |child| collect(child, output));
    }

    let mut output = BTreeSet::new();
    for task in &plan.tasks {
        collect(&task.body, &mut output);
    }
    output.into_iter().collect()
}

fn collect_ir_script_references(
    node: &crate::ir::Node,
    retiring_paths: &BTreeSet<String>,
    output: &mut Vec<(String, Option<Location>)>,
) {
    if let crate::ir::Operation::Exec { argv, .. } = &node.operation {
        for argument in argv {
            let Some(value) = literal_text_value(argument) else {
                continue;
            };
            let normalized = value.strip_prefix("./").unwrap_or(&value);
            if let Some(target) = retiring_paths.get(normalized) {
                output.push((
                    target.clone(),
                    node.source.as_ref().map(|span| Location {
                        path: span.file.clone(),
                        start_byte: span.start_byte,
                        end_byte: span.end_byte,
                    }),
                ));
            }
        }
    }
    visit_node(node, |child| {
        collect_ir_script_references(child, retiring_paths, output)
    });
}

fn literal_text_value(expression: &crate::ir::TextExpression) -> Option<String> {
    let mut output = String::new();
    for part in &expression.parts {
        if let crate::ir::TextPart::Literal { value } = part {
            output.push_str(value);
        } else {
            return None;
        }
    }
    Some(output)
}

fn generate_rust(plan: &crate::ir::Plan) -> Result<Vec<u8>, String> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| "generator entrypoint task is missing".to_owned())?;
    for task in &plan.tasks {
        if task.invocation.is_some() || !task.outputs.is_empty() {
            return Err("generator does not support invocation metadata or task outputs".into());
        }
    }
    // A shell function becomes a function, so the helpers a plan needs are
    // decided over every task rather than over the entry alone: a pipeline
    // inside a function would otherwise call a helper the generator had chosen
    // not to define.
    let bodies: Vec<&crate::ir::Node> = plan.tasks.iter().map(|task| &task.body).collect();
    let any =
        |predicate: &dyn Fn(&crate::ir::Node) -> bool| bodies.iter().any(|body| predicate(body));
    let mut functions = String::new();
    for other in plan.tasks.iter().filter(|other| other.name != task.name) {
        if !rust_function_name(&other.name) {
            return Err(format!(
                "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: task name is not a Rust identifier: {}",
                other.name
            ));
        }
        let mut other_body = String::new();
        emit_rust_node(&other.body, &mut other_body, 2)?;
        // The parameter is named the same as the entry's binding, so `$1` in a
        // function body reads the call's arguments with no special case and the
        // body is emitted by the same code either way.
        functions.push_str(&format!(
            concat!(
                "/// `{name}` from the retired shell, taking the arguments the\n",
                "/// call site passed it.\n",
                "fn {name}(deshell_args: &[String]) -> i32 {{\n",
                "{locals}",
                "{body}\n",
                "}}\n\n"
            ),
            name = other.name,
            locals = rust_variable_binding(&other.body),
            body = other_body,
        ));
    }
    let pipeline = plan
        .tasks
        .iter()
        .any(|task| rust_node_uses_pipeline(&task.body));
    let arguments = rust_node_uses_arguments(&task.body);
    let mut body = String::new();
    emit_rust_node(&task.body, &mut body, 2)?;
    // `Stdio` is named by a pipeline and by a redirection, which are the two
    // places a command's descriptors are set to something other than the
    // parent's.
    let import = if pipeline
        || plan
            .tasks
            .iter()
            .any(|task| rust_node_redirects(&task.body))
    {
        "use std::process::{Command, Stdio};\n\n"
    } else if plan
        .tasks
        .iter()
        .any(|task| rust_node_starts_a_process(&task.body))
    {
        "use std::process::Command;\n\n"
    } else {
        ""
    };
    let pipeline_helper = if pipeline {
        concat!(
            "fn deshell_run_pipeline(mut commands: Vec<Command>, pipefail: bool) -> i32 {\n",
            "    let count = commands.len();\n",
            "    let mut children = Vec::with_capacity(count);\n",
            "    let mut previous = None;\n",
            "    for (index, mut command) in commands.drain(..).enumerate() {\n",
            "        if let Some(input) = previous.take() { command.stdin(Stdio::from(input)); }\n",
            "        if index + 1 < count { command.stdout(Stdio::piped()); }\n",
            "        match command.spawn() {\n",
            "            Ok(mut child) => { previous = child.stdout.take(); children.push(child); }\n",
            "            Err(error) => {\n",
            "                eprintln!(\"{error}\");\n",
            "                for child in &mut children { let _ = child.kill(); }\n",
            "                return 127;\n",
            "            }\n",
            "        }\n",
            "    }\n",
            "    let mut statuses = Vec::with_capacity(count);\n",
            "    for mut child in children {\n",
            "        statuses.push(child.wait().map(|status| status.code().unwrap_or(128)).unwrap_or(127));\n",
            "    }\n",
            "    if pipefail { statuses.into_iter().rev().find(|status| *status != 0).unwrap_or(0) }\n",
            "    else { statuses.last().copied().unwrap_or(0) }\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let argument_binding = if arguments {
        "    let deshell_args: Vec<String> = std::env::args().skip(1).collect();\n"
    } else {
        ""
    };
    // A shell-local name is not an environment variable: the rest of the script
    // sees it and the processes it starts do not. The generated program keeps
    // them in their own map, and an expansion consults it before falling back to
    // the environment, which is the order the shell looks in.
    // The helper and the map travel together: an expansion needs somewhere to
    // look, and an assignment needs somewhere to write. Emitting one without the
    // other produces code that does not compile, which is a defect this
    // repository has now produced three times.
    // Each helper is emitted only if something calls it: an unused function is an
    // error under the generated code's `-D warnings` gate, so "might be needed"
    // is not good enough. `deshell_lookup` is written in terms of
    // `deshell_lookup_opt`, so a plain expansion needs both and a default
    // expansion needs only the second.
    let uses_plain_expansion = any(&|body| {
        node_has_expression_part(body, &|part| {
            matches!(part, crate::ir::TextPart::Variable { .. })
        })
    });
    let uses_default_expansion = any(&|body| {
        node_has_expression_part(body, &|part| {
            matches!(part, crate::ir::TextPart::DefaultValue { .. })
        })
    });
    let lookup_helper = if uses_plain_expansion {
        concat!(
            "fn deshell_lookup_opt(\n",
            "    name: &str,\n",
            "    locals: &std::collections::BTreeMap<String, String>,\n",
            ") -> Option<String> {\n",
            "    locals.get(name).cloned().or_else(|| std::env::var(name).ok())\n",
            "}\n\n",
            "fn deshell_lookup(name: &str, locals: &std::collections::BTreeMap<String, String>) -> String {\n",
            "    deshell_lookup_opt(name, locals).unwrap_or_default()\n",
            "}\n\n"
        )
    } else if uses_default_expansion {
        concat!(
            "fn deshell_lookup_opt(\n",
            "    name: &str,\n",
            "    locals: &std::collections::BTreeMap<String, String>,\n",
            ") -> Option<String> {\n",
            "    locals.get(name).cloned().or_else(|| std::env::var(name).ok())\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let variable_binding = rust_variable_binding(&task.body);
    let source = format!(
        concat!(
            "//! Generated by de-shell from a retired shell script.\n",
            "//!\n",
            "//! This file has no de-shell runtime dependency, and nothing here\n",
            "//! refers back to the tool that wrote it.\n\n",
            "{import}",
            "{pipeline_helper}",
            "{lookup_helper}",
            "{functions}",
            "fn main() {{\n",
            "{argument_binding}",
            "{variable_binding}",
            "{binding}",
            "{body}",
            ";\n",
            "{ending}",
            "}}\n"
        ),
        import = import,
        pipeline_helper = pipeline_helper,
        lookup_helper = lookup_helper,
        functions = functions,
        argument_binding = argument_binding,
        variable_binding = variable_binding,
        // A body that always exits leaves nothing after it to run, and the
        // generated program's own `-D warnings` build rejects an unreachable
        // statement. The status is the body's either way.
        binding = if node_always_exits(&task.body) {
            "    let _: i32 =\n"
        } else {
            "    let deshell_status =\n"
        },
        ending = if node_always_exits(&task.body) {
            ""
        } else {
            "    std::process::exit(deshell_status);\n"
        },
        body = body,
    );
    rustfmt_generated(source.as_bytes())
}

/// Whether a task name can be a Rust function name.
///
/// A shell function name may hold characters Rust will not, and renaming one
/// would put a name in the reader's program that is not in the script it
/// replaced.
fn rust_function_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// The shell-local map a body needs, if it needs one.
///
/// A function has its own: the shell's locals are the caller's, but a body that
/// only reads them still needs somewhere to look, and `mut` on a map nothing
/// writes is a warning the generated code is checked against.
fn rust_variable_binding(body: &crate::ir::Node) -> String {
    let reads = node_has_expression_part(body, &|part| {
        matches!(
            part,
            crate::ir::TextPart::Variable { .. } | crate::ir::TextPart::DefaultValue { .. }
        )
    });
    let writes = rust_node_sets_variables(body);
    match (writes, reads) {
        (false, false) => String::new(),
        (true, _) => "    let mut deshell_vars: std::collections::BTreeMap<String, String> =\n        std::collections::BTreeMap::new();\n".to_owned(),
        (false, true) => "    let deshell_vars: std::collections::BTreeMap<String, String> =\n        std::collections::BTreeMap::new();\n".to_owned(),
    }
}

fn rustfmt_generated(source: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = std::process::Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("official Rust generator requires rustfmt: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("cannot open rustfmt stdin")?
        .write_all(source)
        .map_err(|error| format!("cannot write generated Rust to rustfmt: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot wait for rustfmt: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "rustfmt rejected generated Rust: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn emit_rust_node(node: &crate::ir::Node, output: &mut String, depth: usize) -> Result<(), String> {
    let indent = "    ".repeat(depth);
    match &node.operation {
        crate::ir::Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            output.push_str(&format!("{indent}{{\n"));
            emit_rust_command(EmitRustCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshell_command",
                force_mutable: true,
                output,
                depth: depth + 1,
            })?;
            output.push_str(&format!(
                concat!(
                    "{indent}    match deshell_command.status() {{\n",
                    "{indent}        Ok(status) => status.code().unwrap_or(128),\n",
                    "{indent}        Err(error) => {{ eprintln!(\"{{error}}\"); 127 }},\n",
                    "{indent}    }}\n",
                    "{indent}}}"
                ),
                indent = indent
            ));
        }
        crate::ir::Operation::While { condition, body } => {
            output.push_str(&format!("{indent}{{\n"));
            output.push_str(&format!("{indent}    let mut deshell_last = 0;\n"));
            output.push_str(&format!("{indent}    loop {{\n"));
            output.push_str(&format!("{indent}        if (\n"));
            emit_rust_node(condition, output, depth + 3)?;
            output.push_str(&format!("\n{indent}        ) != 0 {{ break; }}\n"));
            output.push_str(&format!("{indent}        deshell_last =\n"));
            emit_rust_node(body, output, depth + 3)?;
            output.push_str(&format!(";\n{indent}    }}\n"));
            output.push_str(&format!("{indent}    deshell_last\n{indent}}}"));
        }
        crate::ir::Operation::Not { body } => {
            // `!` inverts to a boolean, so a body exiting 2 yields 0 just as one
            // exiting 1 does.
            output.push_str(&format!("{indent}i32::from(\n"));
            emit_rust_node(body, output, depth + 1)?;
            output.push_str(&format!("\n{indent}    == 0)"));
        }
        crate::ir::Operation::Test { predicate } => {
            // `test` succeeds with 0 and fails with 1, so the predicate becomes a
            // bool and the node becomes its exit status.
            let condition = match predicate {
                crate::ir::TestPredicate::NonEmpty { value } => {
                    format!("!{}.is_empty()", rust_expression(value)?)
                }
                crate::ir::TestPredicate::Empty { value } => {
                    format!("{}.is_empty()", rust_expression(value)?)
                }
                crate::ir::TestPredicate::StringEqual { left, right } => {
                    format!("{} == {}", rust_expression(left)?, rust_expression(right)?)
                }
                crate::ir::TestPredicate::StringNotEqual { left, right } => {
                    format!("{} != {}", rust_expression(left)?, rust_expression(right)?)
                }
                crate::ir::TestPredicate::StartsWith { value, prefix } => {
                    format!("{}.starts_with({prefix:?})", rust_expression(value)?)
                }
                crate::ir::TestPredicate::EndsWith { value, suffix } => {
                    format!("{}.ends_with({suffix:?})", rust_expression(value)?)
                }
                crate::ir::TestPredicate::Contains { value, infix } => {
                    format!("{}.contains({infix:?})", rust_expression(value)?)
                }
            };
            output.push_str(&format!("{indent}i32::from(!({condition}))"));
        }
        // Running nothing succeeds, which is what the shell reports for an arm
        // whose body is empty.
        crate::ir::Operation::NoOp => output.push_str(&format!("{indent}0")),
        // `Operation::Match` compares for equality — the frontend refuses a
        // glob pattern precisely so that it does — so the arms become a chain
        // of comparisons rather than a `match`, whose patterns must be literals
        // Rust can see at compile time. A `case` with no matching arm and no
        // `*` runs nothing and succeeds, which is the trailing `0`.
        crate::ir::Operation::Match {
            value,
            cases,
            default,
        } => {
            // A pattern that matches anything compiles to `true`, which reads
            // nothing — so a `case` whose every arm is one leaves the subject
            // unused, and the generated program's own `-D warnings` build
            // rejects an unused binding. The word is still expanded, because the
            // shell expands it.
            let reads_subject = cases
                .iter()
                .any(|case| !matches!(pattern_shape(&case.pattern), PatternShape::Anything));
            output.push_str(&format!(
                "{indent}{{\n{indent}    let {}deshell_subject = {};\n",
                if reads_subject { "" } else { "_" },
                rust_expression(value)?
            ));
            for case in cases {
                output.push_str(&format!(
                    "{indent}    if {} {{\n",
                    rust_pattern_test(&case.pattern, "deshell_subject")?
                ));
                emit_rust_node(&case.body, output, depth + 2)?;
                output.push_str(&format!("\n{indent}    }} else "));
            }
            output.push_str(&format!("{indent}    {{\n"));
            match default {
                Some(default) => emit_rust_node(default, output, depth + 2)?,
                None => output.push_str(&format!("{indent}        0")),
            }
            output.push_str(&format!("\n{indent}    }}\n{indent}}}"));
        }
        // Only a redirect around a single command is emitted. The shell can
        // redirect a whole compound statement, and reproducing that means
        // holding the descriptors open across everything inside — so the
        // general case is refused rather than narrowed silently.
        crate::ir::Operation::Redirect { redirections, body } => {
            let crate::ir::Operation::Exec {
                argv,
                environment,
                working_directory,
            } = &body.operation
            else {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: redirect supports only a simple command"
                        .into(),
                );
            };
            output.push_str(&format!("{indent}{{\n"));
            emit_rust_command(EmitRustCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshell_command",
                output,
                depth: depth + 1,
                force_mutable: true,
            })?;
            let mut closers = 0_usize;
            for redirection in redirections {
                let (fd, path, append) = match redirection {
                    crate::ir::Redirection::Write { fd, path, append } => (fd, path, *append),
                    crate::ir::Redirection::Read { .. }
                    | crate::ir::Redirection::Duplicate { .. }
                    | crate::ir::Redirection::Close { .. } => {
                        return Err(
                            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: only an output redirection is emitted"
                                .into(),
                        );
                    }
                };
                let sink = match fd {
                    1 => "stdout",
                    2 => "stderr",
                    _ => {
                        return Err(
                            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: only fd 1 and 2 are redirected"
                                .into(),
                        );
                    }
                };
                // A file that will not open is the shell's exit 1, and the
                // block is an expression, so the status is carried rather than
                // returned: `return` here would leave `main`.
                output.push_str(&format!(
                    concat!(
                        "{indent}    let deshell_opened = std::fs::OpenOptions::new()\n",
                        "{indent}        .write(true)\n",
                        "{indent}        .create(true)\n",
                        "{indent}        .append({append})\n",
                        "{indent}        .truncate({truncate})\n",
                        "{indent}        .open({path});\n",
                        "{indent}    match deshell_opened {{\n",
                        "{indent}        Err(error) => {{\n",
                        "{indent}            eprintln!(\"{{error}}\");\n",
                        "{indent}            1\n",
                        "{indent}        }}\n",
                        "{indent}        Ok(deshell_file) => {{\n",
                        "{indent}            deshell_command.{sink}(Stdio::from(deshell_file));\n"
                    ),
                    indent = indent,
                    append = append,
                    truncate = !append,
                    path = rust_expression_borrowed(path)?,
                    sink = sink,
                ));
                closers += 1;
            }
            output.push_str(&format!(
                concat!(
                    "{indent}            match deshell_command.status() {{\n",
                    "{indent}                Ok(status) => status.code().unwrap_or(128),\n",
                    "{indent}                Err(error) => {{\n",
                    "{indent}                    eprintln!(\"{{error}}\");\n",
                    "{indent}                    127\n",
                    "{indent}                }}\n",
                    "{indent}            }}\n"
                ),
                indent = indent
            ));
            for _ in 0..closers {
                output.push_str(&format!("{indent}        }}\n{indent}    }}\n"));
            }
            output.push_str(&format!("{indent}}}"));
        }
        // A shell function is a function: the definition and its call sites
        // stay a definition and calls, because that is where "these are the
        // same check" is written down.
        crate::ir::Operation::TaskCall {
            task,
            arguments,
            positional,
        } => {
            if !arguments.is_empty() {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: a task call with named arguments"
                        .into(),
                );
            }
            let mut values = Vec::new();
            for value in positional {
                values.push(rust_expression(value)?);
            }
            output.push_str(&format!("{indent}{task}(&[{}])", values.join(", ")));
        }
        // `process::exit` returns `!`, which is why it stands where the
        // surrounding expression wants an `i32`.
        crate::ir::Operation::Exit {
            status,
            non_numeric,
        } => match non_numeric {
            crate::ir::NonNumericStatus::Unreachable => output.push_str(&format!(
                "{indent}std::process::exit({})",
                rust_exit_status(status)?
            )),
            // The status arrives at run time. Every shell reduces a decimal one
            // modulo 256 and agrees; outside that they do not agree at all, so
            // the program stops and says which value it was rather than picking
            // one shell's answer.
            crate::ir::NonNumericStatus::Refuse => output.push_str(&format!(
                concat!(
                    "{indent}{{\n",
                    "{indent}    let deshell_status = {status};\n",
                    "{indent}    match deshell_status.trim().parse::<i64>() {{\n",
                    "{indent}        Ok(status) => std::process::exit(\n",
                    "{indent}            i32::try_from(status.rem_euclid(256)).unwrap_or(255),\n",
                    "{indent}        ),\n",
                    "{indent}        Err(_) => {{\n",
                    "{indent}            eprintln!(\n",
                    "{indent}                \"exit status {{deshell_status:?}} is not a number; \\\n",
                    "{indent}                 the shells do not agree on what that means\"\n",
                    "{indent}            );\n",
                    "{indent}            std::process::exit(70)\n",
                    "{indent}        }}\n",
                    "{indent}    }}\n",
                    "{indent}}}"
                ),
                indent = indent,
                status = rust_expression(status)?
            )),
        },
        // `echo` returns 1 when the write fails, so the status is the write's.
        // The `use` is local to the block: the import list is decided before the
        // body is walked, and an unconditional `std::io::Write` would be unused
        // in every plan that prints nothing.
        crate::ir::Operation::WriteStdout { contents } => {
            output.push_str(&format!(
                concat!(
                    "{indent}{{\n",
                    "{indent}    use std::io::Write as _;\n",
                    "{indent}    let deshell_bytes = {contents};\n",
                    "{indent}    i32::from(\n",
                    "{indent}        std::io::stdout()\n",
                    "{indent}            .write_all(deshell_bytes.as_bytes())\n",
                    "{indent}            .is_err(),\n",
                    "{indent}    )\n",
                    "{indent}}}"
                ),
                indent = indent,
                contents = rust_expression(contents)?
            ));
        }
        crate::ir::Operation::Sequence { nodes, .. } => {
            let Some((last, preceding)) = reachable_nodes(nodes).split_last() else {
                return Err("generator received an empty sequence".into());
            };
            output.push_str(&format!("{indent}{{\n"));
            for child in preceding {
                output.push_str(&format!("{indent}    let _ =\n"));
                emit_rust_node(child, output, depth + 2)?;
                output.push_str(";\n");
            }
            emit_rust_node(last, output, depth + 1)?;
            output.push_str(&format!("\n{indent}}}"));
        }
        crate::ir::Operation::Pipeline { nodes, status } => {
            output.push_str(&format!("{indent}{{\n"));
            output.push_str(&format!(
                "{indent}    let mut deshell_commands = Vec::new();\n"
            ));
            for child in nodes {
                let crate::ir::Operation::Exec {
                    argv,
                    environment,
                    working_directory,
                } = &child.operation
                else {
                    return Err("generator pipeline supports only Exec stages".into());
                };
                emit_rust_command(EmitRustCommandArgs {
                    argv,
                    environment,
                    working_directory: working_directory.as_ref(),
                    variable: "deshell_stage",
                    force_mutable: false,
                    output,
                    depth: depth + 1,
                })?;
                output.push_str(&format!(
                    "{indent}    deshell_commands.push(deshell_stage);\n"
                ));
            }
            output.push_str(&format!(
                "{indent}    deshell_run_pipeline(deshell_commands, {})\n{indent}}}",
                matches!(status, crate::ir::PipelineStatus::Pipefail)
            ));
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            output.push_str(&format!(
                "{indent}{{\n{indent}    let deshell_predicate =\n"
            ));
            emit_rust_node(predicate, output, depth + 2)?;
            output.push_str(&format!(";\n{indent}    if deshell_predicate == 0 {{\n"));
            emit_rust_node(if_true, output, depth + 2)?;
            output.push('\n');
            if let Some(if_false) = if_false {
                output.push_str(&format!("{indent}    }} else {{\n"));
                emit_rust_node(if_false, output, depth + 2)?;
                output.push('\n');
            } else {
                output.push_str(&format!(
                    "{indent}    }} else {{\n{indent}        deshell_predicate\n"
                ));
            }
            output.push_str(&format!("{indent}    }}\n{indent}}}"));
        }
        crate::ir::Operation::SetVariable { name, value, .. } => {
            output.push_str(&format!(
                "{indent}{{\n{indent}    deshell_vars.insert({name:?}.to_owned(), {});\n{indent}    0\n{indent}}}",
                rust_expression(value)?
            ));
        }
        crate::ir::Operation::CaptureStdout { name, body, .. } => {
            let crate::ir::Operation::Exec {
                argv,
                environment,
                working_directory,
            } = &body.operation
            else {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: capture supports only a simple command"
                        .into(),
                );
            };
            output.push_str(&format!("{indent}{{\n"));
            emit_rust_command(EmitRustCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshell_command",
                output,
                depth: depth + 1,
                force_mutable: true,
            })?;
            // The shell strips trailing newlines from a substitution and nothing
            // else, so `trim_end_matches` is right where `trim` would be wrong.
            output.push_str(&format!(
                concat!(
                    "{indent}    let deshell_output = deshell_command.output();\n",
                    "{indent}    let deshell_code = match &deshell_output {{\n",
                    "{indent}        Ok(done) => done.status.code().unwrap_or(128),\n",
                    "{indent}        Err(error) => {{ eprintln!(\"{{error}}\"); 127 }},\n",
                    "{indent}    }};\n",
                    "{indent}    let deshell_captured = deshell_output\n",
                    "{indent}        .map(|done| String::from_utf8_lossy(&done.stdout).into_owned())\n",
                    "{indent}        .unwrap_or_default();\n",
                    "{indent}    deshell_vars.insert(\n",
                    "{indent}        {name:?}.to_owned(),\n",
                    "{indent}        deshell_captured.trim_end_matches('\\n').to_owned(),\n",
                    "{indent}    );\n",
                    "{indent}    deshell_code\n",
                    "{indent}}}"
                ),
                indent = indent,
                name = name
            ));
        }
        other => {
            return Err(format!(
                "generator cannot preserve {} semantics yet",
                other.name()
            ));
        }
    }
    Ok(())
}

/// Whether the generated Go will need the `strings` package.
///
/// A capture trims the output; a pattern predicate calls `HasPrefix` and friends.
/// Go rejects an unused import outright, so this has to be exact rather than
/// generous.
fn node_captures_stdout(node: &crate::ir::Node) -> bool {
    if matches!(node.operation, crate::ir::Operation::CaptureStdout { .. })
        || matches!(
            &node.operation,
            crate::ir::Operation::Test { predicate }
                if matches!(
                    predicate,
                    crate::ir::TestPredicate::StartsWith { .. }
                        | crate::ir::TestPredicate::EndsWith { .. }
                        | crate::ir::TestPredicate::Contains { .. }
                )
        )
    {
        return true;
    }
    match &node.operation {
        crate::ir::Operation::NoOp
        | crate::ir::Operation::WriteStdout { .. }
        | crate::ir::Operation::Exit { .. } => false,
        crate::ir::Operation::Sequence { nodes, .. }
        | crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Parallel { nodes } => nodes.iter().any(node_captures_stdout),
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            node_captures_stdout(predicate)
                || node_captures_stdout(if_true)
                || if_false.as_deref().is_some_and(node_captures_stdout)
        }
        crate::ir::Operation::While { condition, body } => {
            node_captures_stdout(condition) || node_captures_stdout(body)
        }
        crate::ir::Operation::TryFinally { body, finalizer } => {
            node_captures_stdout(body) || node_captures_stdout(finalizer)
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            cases.iter().any(|case| node_captures_stdout(&case.body))
                || default.as_deref().is_some_and(node_captures_stdout)
        }
        crate::ir::Operation::Not { body }
        | crate::ir::Operation::Redirect { body, .. }
        | crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::Scope { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. }
        | crate::ir::Operation::Spawn { body, .. } => node_captures_stdout(body),
        crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::Test { .. }
        | crate::ir::Operation::ExpandWords { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::SetEnvironment { .. }
        | crate::ir::Operation::SetWorkingDirectory { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::FileMetadata { .. }
        | crate::ir::Operation::FileSetMetadata { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::SendSignal { .. }
        | crate::ir::Operation::TaskCall { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => false,
    }
}

/// Whether the tree assigns to a shell-local name.
fn rust_node_sets_variables(node: &crate::ir::Node) -> bool {
    if matches!(
        node.operation,
        crate::ir::Operation::SetVariable { .. } | crate::ir::Operation::CaptureStdout { .. }
    ) {
        return true;
    }
    match &node.operation {
        crate::ir::Operation::NoOp
        | crate::ir::Operation::WriteStdout { .. }
        | crate::ir::Operation::Exit { .. } => false,
        crate::ir::Operation::Sequence { nodes, .. }
        | crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Parallel { nodes } => nodes.iter().any(rust_node_sets_variables),
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            rust_node_sets_variables(predicate)
                || rust_node_sets_variables(if_true)
                || if_false.as_deref().is_some_and(rust_node_sets_variables)
        }
        crate::ir::Operation::While { condition, body } => {
            rust_node_sets_variables(condition) || rust_node_sets_variables(body)
        }
        crate::ir::Operation::TryFinally { body, finalizer } => {
            rust_node_sets_variables(body) || rust_node_sets_variables(finalizer)
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            cases
                .iter()
                .any(|case| rust_node_sets_variables(&case.body))
                || default.as_deref().is_some_and(rust_node_sets_variables)
        }
        crate::ir::Operation::Not { body }
        | crate::ir::Operation::Redirect { body, .. }
        | crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::Scope { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. }
        | crate::ir::Operation::Spawn { body, .. } => rust_node_sets_variables(body),
        crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::Test { .. }
        | crate::ir::Operation::ExpandWords { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::SetEnvironment { .. }
        | crate::ir::Operation::SetWorkingDirectory { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::FileMetadata { .. }
        | crate::ir::Operation::FileSetMetadata { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::SendSignal { .. }
        | crate::ir::Operation::TaskCall { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => false,
    }
}

/// The shape a `case` pattern reduces to.
///
/// Every pattern in the modelled subset is one of these, and each becomes a
/// single expression in the generated program. A generic matcher walking a list
/// of pieces would be code the reader of the migration has to maintain and the
/// script never needed — and a plan that used `*` and not `?` would carry a
/// branch of it that nothing reaches.
enum PatternShape<'a> {
    /// `abc` — one exact string.
    Exact(String),
    /// `*` — anything, including nothing.
    Anything,
    /// `*abc*` — the text appears somewhere.
    Contains(String),
    /// `abc*` — the text is at the front.
    Prefix(String),
    /// `*abc` — the text is at the back.
    Suffix(String),
    /// `abc*def` — a front and a back that do not overlap.
    PrefixAndSuffix { prefix: String, suffix: String },
    /// `a?c` — a front, a back, and exactly `gap` characters between them.
    FixedGap {
        prefix: String,
        suffix: String,
        gap: usize,
    },
    /// Anything else, which is refused rather than compiled.
    Unsupported(&'a str),
}

/// Reduce a pattern to the shape that describes it.
///
/// The `?` in `a?c` is a character rather than a byte, so a shape carrying one
/// counts characters; the shapes that do not carry a `?` compare bytes, which
/// is the same answer and a shorter expression.
///
/// A pattern with a literal between two runs — `a*b*c` — has no single
/// expression, because finding the middle needs a search that can back up. It
/// is refused, which says so out loud rather than emitting a machine to do it.
fn pattern_shape(pattern: &crate::ir::PatternExpression) -> PatternShape<'_> {
    let mut literals: Vec<String> = Vec::new();
    let mut runs = 0_usize;
    let mut gap = 0_usize;
    let mut leading_run = false;
    let mut trailing_run = false;
    let mut previous_was_literal = false;
    for (index, piece) in pattern.pieces.iter().enumerate() {
        let last = index + 1 == pattern.pieces.len();
        match piece {
            crate::ir::PatternPiece::Literal { value } => {
                let [crate::ir::TextPart::Literal { value }] = value.parts.as_slice() else {
                    return PatternShape::Unsupported("a pattern piece is not literal text");
                };
                if previous_was_literal {
                    // Two literals in a row cannot happen in a canonical
                    // pattern, and joining them here would hide it if it did.
                    return PatternShape::Unsupported("a pattern has adjacent literals");
                }
                previous_was_literal = true;
                literals.push(value.clone());
            }
            crate::ir::PatternPiece::AnyRun => {
                previous_was_literal = false;
                runs += 1;
                if index == 0 {
                    leading_run = true;
                }
                if last {
                    trailing_run = true;
                }
            }
            crate::ir::PatternPiece::AnyCharacter => {
                previous_was_literal = false;
                gap += 1;
            }
        }
    }
    if runs > 0 && gap > 0 {
        return PatternShape::Unsupported("a pattern mixes `*` and `?`");
    }
    if runs > 1 && !(leading_run && trailing_run && runs == 2) {
        return PatternShape::Unsupported("a pattern has a literal between two runs");
    }
    match (runs, gap, literals.as_slice()) {
        (0, 0, []) => PatternShape::Exact(String::new()),
        (0, 0, [exact]) => PatternShape::Exact(exact.clone()),
        (1, 0, []) => PatternShape::Anything,
        (2, 0, [infix]) => PatternShape::Contains(infix.clone()),
        (1, 0, [text]) if leading_run => PatternShape::Suffix(text.clone()),
        (1, 0, [text]) if trailing_run => PatternShape::Prefix(text.clone()),
        (1, 0, [prefix, suffix]) => PatternShape::PrefixAndSuffix {
            prefix: prefix.clone(),
            suffix: suffix.clone(),
        },
        (0, gap, []) if gap > 0 => PatternShape::FixedGap {
            prefix: String::new(),
            suffix: String::new(),
            gap,
        },
        (0, gap, [prefix]) if gap > 0 => PatternShape::FixedGap {
            prefix: prefix.clone(),
            suffix: String::new(),
            gap,
        },
        (0, gap, [prefix, suffix]) if gap > 0 => PatternShape::FixedGap {
            prefix: prefix.clone(),
            suffix: suffix.clone(),
            gap,
        },
        _ => PatternShape::Unsupported("a pattern this does not reduce to one expression"),
    }
}

/// A needle for `contains`, `starts_with` and `ends_with`.
///
/// A one-character needle is written as a `char`. The string form compiles, but
/// `clippy::pedantic` rejects it — and the project receiving this code decides
/// which lints it runs, not the generator.
fn rust_needle(text: &str) -> String {
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(single), None) => format!("{single:?}"),
        _ => format!("{text:?}"),
    }
}

/// The Rust expression that tests `subject` against a `case` pattern.
fn rust_pattern_test(
    pattern: &crate::ir::PatternExpression,
    subject: &str,
) -> Result<String, String> {
    Ok(match pattern_shape(pattern) {
        PatternShape::Exact(exact) => format!("{subject} == {exact:?}"),
        PatternShape::Anything => "true".to_owned(),
        PatternShape::Contains(infix) => {
            format!("{subject}.contains({})", rust_needle(&infix))
        }
        PatternShape::Prefix(prefix) => {
            format!("{subject}.starts_with({})", rust_needle(&prefix))
        }
        PatternShape::Suffix(suffix) => {
            format!("{subject}.ends_with({})", rust_needle(&suffix))
        }
        // The length test is what stops `ab` from matching `a*b` by using its
        // one `b` as both ends.
        PatternShape::PrefixAndSuffix { prefix, suffix } => format!(
            "{subject}.len() >= {} && {subject}.starts_with({}) && {subject}.ends_with({})",
            prefix.len() + suffix.len(),
            rust_needle(&prefix),
            rust_needle(&suffix)
        ),
        PatternShape::FixedGap {
            prefix,
            suffix,
            gap,
        } => format!(
            "{subject}.chars().count() == {} && {subject}.starts_with({}) && {subject}.ends_with({})",
            prefix.chars().count() + suffix.chars().count() + gap,
            rust_needle(&prefix),
            rust_needle(&suffix)
        ),
        PatternShape::Unsupported(reason) => {
            return Err(format!("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: {reason}"));
        }
    })
}

/// The Go expression that tests `subject` against a `case` pattern.
fn go_pattern_test(
    pattern: &crate::ir::PatternExpression,
    subject: &str,
) -> Result<String, String> {
    Ok(match pattern_shape(pattern) {
        PatternShape::Exact(exact) => format!("{subject} == {}", go_string(&exact)?),
        PatternShape::Anything => "true".to_owned(),
        PatternShape::Contains(infix) => {
            format!("strings.Contains({subject}, {})", go_string(&infix)?)
        }
        PatternShape::Prefix(prefix) => {
            format!("strings.HasPrefix({subject}, {})", go_string(&prefix)?)
        }
        PatternShape::Suffix(suffix) => {
            format!("strings.HasSuffix({subject}, {})", go_string(&suffix)?)
        }
        PatternShape::PrefixAndSuffix { prefix, suffix } => format!(
            "len({subject}) >= {} && strings.HasPrefix({subject}, {}) && strings.HasSuffix({subject}, {})",
            prefix.len() + suffix.len(),
            go_string(&prefix)?,
            go_string(&suffix)?
        ),
        PatternShape::FixedGap {
            prefix,
            suffix,
            gap,
        } => format!(
            "utf8.RuneCountInString({subject}) == {} && strings.HasPrefix({subject}, {}) && strings.HasSuffix({subject}, {})",
            prefix.chars().count() + suffix.chars().count() + gap,
            go_string(&prefix)?,
            go_string(&suffix)?
        ),
        PatternShape::Unsupported(reason) => {
            return Err(format!("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: {reason}"));
        }
    })
}

/// Which packages the compiled `case` patterns under this node need.
///
/// A pattern becomes an expression rather than a call into a matcher, so what
/// the generated Go imports follows from the shapes those expressions take:
/// every shape but an exact comparison calls `strings`, and only a `?` counts
/// characters.
fn go_pattern_packages(node: &crate::ir::Node) -> (bool, bool) {
    let mut strings = false;
    let mut utf8 = false;
    if let crate::ir::Operation::Match { cases, .. } = &node.operation {
        for case in cases {
            match pattern_shape(&case.pattern) {
                PatternShape::Exact(_) | PatternShape::Anything | PatternShape::Unsupported(_) => {}
                PatternShape::FixedGap { .. } => {
                    strings = true;
                    utf8 = true;
                }
                PatternShape::Contains(_)
                | PatternShape::Prefix(_)
                | PatternShape::Suffix(_)
                | PatternShape::PrefixAndSuffix { .. } => strings = true,
            }
        }
    }
    visit_node(node, |child| {
        let (child_strings, child_utf8) = go_pattern_packages(child);
        strings |= child_strings;
        utf8 |= child_utf8;
    });
    (strings, utf8)
}

/// Whether any command under this node has a descriptor redirected.
fn rust_node_redirects(node: &crate::ir::Node) -> bool {
    if matches!(node.operation, crate::ir::Operation::Redirect { .. }) {
        return true;
    }
    let mut found = false;
    visit_node(node, |child| found |= rust_node_redirects(child));
    found
}

/// Whether every path through this node ends the task.
///
/// A statement after one cannot run, and emitting it anyway produces code the
/// generator's own `-D warnings` gate rejects as unreachable. Suppressing the
/// warning would keep the dead statement in the output; dropping it is what the
/// shell does at run time, so the two programs still agree.
///
/// Conservative in the safe direction: `false` for anything this cannot prove,
/// which at worst emits a statement that does run.
fn node_always_exits(node: &crate::ir::Node) -> bool {
    match &node.operation {
        crate::ir::Operation::Exit { .. } => true,
        // A sequence ends the task if any of its statements does, whatever
        // `on_failure` says: the ones after it are the unreachable part.
        crate::ir::Operation::Sequence { nodes, .. } => nodes.iter().any(node_always_exits),
        // Every arm has to end it, and an absent default is an arm that runs
        // nothing and returns.
        crate::ir::Operation::Match { cases, default, .. } => {
            default.as_deref().is_some_and(node_always_exits)
                && cases.iter().all(|case| node_always_exits(&case.body))
        }
        // Without an `else` the false branch returns.
        crate::ir::Operation::Condition {
            if_true, if_false, ..
        } => node_always_exits(if_true) && if_false.as_deref().is_some_and(node_always_exits),
        crate::ir::Operation::Not { body }
        | crate::ir::Operation::Redirect { body, .. }
        | crate::ir::Operation::Scope { body, .. } => node_always_exits(body),
        // A loop may run its body no times, a substitution and a pipeline stage
        // run in their own process, and the rest start no task to end.
        crate::ir::Operation::While { .. }
        | crate::ir::Operation::Foreach { .. }
        | crate::ir::Operation::Pipeline { .. }
        | crate::ir::Operation::Parallel { .. }
        | crate::ir::Operation::CaptureStdout { .. }
        | crate::ir::Operation::Spawn { .. }
        | crate::ir::Operation::TryFinally { .. }
        | crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::ExpandWords { .. }
        | crate::ir::Operation::NoOp
        | crate::ir::Operation::WriteStdout { .. }
        | crate::ir::Operation::Test { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::SetEnvironment { .. }
        | crate::ir::Operation::SetWorkingDirectory { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::SendSignal { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::FileMetadata { .. }
        | crate::ir::Operation::FileSetMetadata { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::TaskCall { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => false,
    }
}

/// The statements of a sequence that can run.
///
/// Everything after the first one that always ends the task is unreachable.
fn reachable_nodes(nodes: &[crate::ir::Node]) -> &[crate::ir::Node] {
    match nodes.iter().position(node_always_exits) {
        Some(index) => &nodes[..=index],
        None => nodes,
    }
}

/// The literal status an `Operation::Exit` ends with.
///
/// The frontend refuses a status it cannot read, so a plan that reaches a
/// generator carries a decimal integer. Reducing it here rather than in the
/// generated program keeps the arithmetic in one place, and every measured
/// shell reduces modulo 256.
fn rust_exit_status(status: &crate::ir::TextExpression) -> Result<i32, String> {
    let [crate::ir::TextPart::Literal { value }] = status.parts.as_slice() else {
        return Err(
            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: exit status must be a literal integer".into(),
        );
    };
    let parsed: i64 = value
        .trim()
        .parse()
        .map_err(|error| format!("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: exit status: {error}"))?;
    i32::try_from(parsed.rem_euclid(256))
        .map_err(|error| format!("DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: exit status: {error}"))
}

/// Whether the generated program will name `Command` at all.
///
/// A plan made only of `Test` nodes starts no process, and an unused import is a
/// hard error under the `-D warnings` gate the generated Rust is checked with —
/// so emitting the import unconditionally made such a plan ungeneratable.
fn rust_node_starts_a_process(node: &crate::ir::Node) -> bool {
    match &node.operation {
        crate::ir::Operation::NoOp
        | crate::ir::Operation::WriteStdout { .. }
        | crate::ir::Operation::Exit { .. } => false,
        crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::Pipeline { .. }
        | crate::ir::Operation::InterpreterCall { .. } => true,
        crate::ir::Operation::Sequence { nodes, .. } | crate::ir::Operation::Parallel { nodes } => {
            nodes.iter().any(rust_node_starts_a_process)
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            rust_node_starts_a_process(predicate)
                || rust_node_starts_a_process(if_true)
                || if_false
                    .as_ref()
                    .is_some_and(|node| rust_node_starts_a_process(node))
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            cases
                .iter()
                .any(|case| rust_node_starts_a_process(&case.body))
                || default
                    .as_ref()
                    .is_some_and(|node| rust_node_starts_a_process(node))
        }
        crate::ir::Operation::Redirect { body, .. }
        | crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::Scope { body, .. }
        | crate::ir::Operation::Not { body }
        | crate::ir::Operation::CaptureStdout { body, .. }
        | crate::ir::Operation::Spawn { body, .. } => rust_node_starts_a_process(body),
        crate::ir::Operation::While {
            condition: first,
            body: second,
        }
        | crate::ir::Operation::TryFinally {
            body: first,
            finalizer: second,
        } => rust_node_starts_a_process(first) || rust_node_starts_a_process(second),
        // Listed rather than left to a wildcard: a `_` arm here answered "no" for
        // `While`, and the generated program then named `Command` without
        // importing it. A new operation must be classified, not defaulted.
        crate::ir::Operation::Test { .. }
        | crate::ir::Operation::ExpandWords { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::SetEnvironment { .. }
        | crate::ir::Operation::SetWorkingDirectory { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::FileMetadata { .. }
        | crate::ir::Operation::FileSetMetadata { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::OpaqueCapsule { .. }
        | crate::ir::Operation::SendSignal { .. } => false,
        // A call runs whatever the callee runs, and the callee is a task of the
        // same plan — so the decision is made over every task rather than
        // followed from here. Assuming `true` was right while a plan had one
        // task; with functions it puts an import in a program that starts no
        // process, which its own `-D warnings` build rejects.
        crate::ir::Operation::TaskCall { .. } => false,
    }
}

/// Whether the generated program will call the pipeline helper.
///
/// Written over the shared walk rather than over a hand-listed few operations:
/// the hand-listed version knew about `Sequence` and `Condition` only, so a
/// pipeline inside a loop or a `case` arm emitted a call to a helper the
/// generator had decided not to define.
fn rust_node_uses_pipeline(node: &crate::ir::Node) -> bool {
    if matches!(node.operation, crate::ir::Operation::Pipeline { .. }) {
        return true;
    }
    let mut found = false;
    visit_node(node, |child| found |= rust_node_uses_pipeline(child));
    found
}

fn rust_node_uses_arguments(node: &crate::ir::Node) -> bool {
    node_has_expression_part(node, &|part| {
        matches!(part, crate::ir::TextPart::Argument { .. })
    })
}

/// Whether any expression under `node` has a part the predicate accepts.
fn node_has_expression_part(
    node: &crate::ir::Node,
    wanted: &dyn Fn(&crate::ir::TextPart) -> bool,
) -> bool {
    let expression = |value: &crate::ir::TextExpression| value.parts.iter().any(wanted);
    match &node.operation {
        crate::ir::Operation::NoOp => false,
        crate::ir::Operation::WriteStdout { contents } => expression(contents),
        crate::ir::Operation::Exit { status, .. } => expression(status),
        crate::ir::Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            argv.iter().any(expression)
                || environment.iter().any(|value| expression(&value.value))
                || working_directory.as_ref().is_some_and(expression)
        }
        crate::ir::Operation::Sequence { nodes, .. }
        | crate::ir::Operation::Pipeline { nodes, .. } => nodes
            .iter()
            .any(|child| node_has_expression_part(child, wanted)),
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            node_has_expression_part(predicate, wanted)
                || node_has_expression_part(if_true, wanted)
                || if_false
                    .as_ref()
                    .is_some_and(|child| node_has_expression_part(child, wanted))
        }
        crate::ir::Operation::While { condition, body } => {
            node_has_expression_part(condition, wanted) || node_has_expression_part(body, wanted)
        }
        crate::ir::Operation::Test { predicate } => match predicate {
            crate::ir::TestPredicate::NonEmpty { value }
            | crate::ir::TestPredicate::Empty { value } => value.parts.iter().any(wanted),
            crate::ir::TestPredicate::StringEqual { left, right }
            | crate::ir::TestPredicate::StringNotEqual { left, right } => {
                left.parts.iter().any(wanted) || right.parts.iter().any(wanted)
            }
            crate::ir::TestPredicate::StartsWith { value, .. }
            | crate::ir::TestPredicate::EndsWith { value, .. }
            | crate::ir::TestPredicate::Contains { value, .. } => value.parts.iter().any(wanted),
        },
        crate::ir::Operation::Not { body }
        | crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::Scope { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. }
        | crate::ir::Operation::Spawn { body, .. } => node_has_expression_part(body, wanted),
        // Destructured without `..`: the subject is an expression, and reading
        // it with a wildcard is how `case "$1" in` produced a program that
        // named `deshell_args` without binding it. A pattern piece carries one
        // too.
        crate::ir::Operation::Match {
            value,
            cases,
            default,
        } => {
            expression(value)
                || cases.iter().any(|case| {
                    case.pattern.pieces.iter().any(|piece| match piece {
                        crate::ir::PatternPiece::Literal { value } => expression(value),
                        crate::ir::PatternPiece::AnyRun | crate::ir::PatternPiece::AnyCharacter => {
                            false
                        }
                    }) || node_has_expression_part(&case.body, wanted)
                })
                || default
                    .as_ref()
                    .is_some_and(|child| node_has_expression_part(child, wanted))
        }
        crate::ir::Operation::TryFinally { body, finalizer } => {
            node_has_expression_part(body, wanted) || node_has_expression_part(finalizer, wanted)
        }
        crate::ir::Operation::Parallel { nodes } => nodes
            .iter()
            .any(|child| node_has_expression_part(child, wanted)),
        // Each of these carries an expression, and answering `false` for the
        // lot of them is how `x="$1"` produced a program that named
        // `deshell_args` without binding it. A `_` arm had already done the
        // same thing for `While`; listing the operations did not help while the
        // list said "no" for operations that hold expressions.
        crate::ir::Operation::ExpandWords {
            value,
            field_splitting,
            ..
        } => {
            expression(value)
                || match field_splitting {
                    crate::ir::FieldSplitting::PosixIfs { ifs } => expression(ifs),
                    crate::ir::FieldSplitting::None => false,
                }
        }
        crate::ir::Operation::SetVariable { value, .. } => expression(value),
        // The value is optional: `unset` removes the variable and holds no
        // expression to walk.
        crate::ir::Operation::SetEnvironment { value, .. } => match value {
            Some(value) => expression(value),
            None => false,
        },
        crate::ir::Operation::SetWorkingDirectory { path } => expression(path),
        crate::ir::Operation::FileRead { path }
        | crate::ir::Operation::FileRemove { path }
        | crate::ir::Operation::FileMetadata { path, .. }
        | crate::ir::Operation::FileSetMetadata { path, .. } => expression(path),
        crate::ir::Operation::FileWrite { path, contents, .. } => {
            expression(path) || expression(contents)
        }
        crate::ir::Operation::NetworkRequest { method, uri } => {
            expression(method) || expression(uri)
        }
        // A call's arguments are expressions evaluated by the caller, so `f
        // "$1"` reads the caller's arguments even though the callee's body does
        // not. Answering `false` here left the generated `main` naming a
        // binding it had decided not to emit.
        // Destructured without `..`: a redirection's target is an expression,
        // and `>>"${OUT}"` produced a program that named `deshell_vars` without
        // declaring it.
        crate::ir::Operation::Redirect { redirections, body } => {
            redirections.iter().any(|redirection| match redirection {
                crate::ir::Redirection::Read { path, .. }
                | crate::ir::Redirection::Write { path, .. } => expression(path),
                crate::ir::Redirection::Duplicate { .. } | crate::ir::Redirection::Close { .. } => {
                    false
                }
            }) || node_has_expression_part(body, wanted)
        }
        crate::ir::Operation::TaskCall {
            task: _,
            arguments,
            positional,
        } => {
            arguments.iter().any(|argument| expression(&argument.value))
                || positional.iter().any(expression)
        }
        // These hold no expression at all, which is why they can say no.
        crate::ir::Operation::ClockRead { .. }
        | crate::ir::Operation::RandomBytes { .. }
        | crate::ir::Operation::Wait { .. }
        | crate::ir::Operation::SendSignal { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => false,
    }
}

/// The inputs of [`emit_rust_command`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`emit_rust_command`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct EmitRustCommandArgs<'a> {
    argv: &'a [crate::ir::TextExpression],
    environment: &'a [crate::ir::NamedExpression],
    working_directory: Option<&'a crate::ir::TextExpression>,
    variable: &'a str,
    force_mutable: bool,
    output: &'a mut String,
    depth: usize,
}

fn emit_rust_command(parts: EmitRustCommandArgs<'_>) -> Result<(), String> {
    // Destructured without `..`: see `EmitRustCommandArgs`.
    let EmitRustCommandArgs {
        argv,
        environment,
        working_directory,
        variable,
        force_mutable,
        output,
        depth,
    } = parts;
    let indent = "    ".repeat(depth);
    let program = argv
        .first()
        .ok_or_else(|| "generator received empty Exec argv".to_owned())?;
    let mutable =
        force_mutable || argv.len() > 1 || !environment.is_empty() || working_directory.is_some();
    output.push_str(&format!(
        "{indent}let {mutable}{variable} = Command::new({});\n",
        rust_expression_borrowed(program)?,
        mutable = if mutable { "mut " } else { "" },
    ));
    for argument in &argv[1..] {
        output.push_str(&format!(
            "{indent}{variable}.arg({});\n",
            rust_expression(argument)?
        ));
    }
    for value in environment {
        output.push_str(&format!(
            "{indent}{variable}.env({:?}, {});\n",
            value.name,
            rust_expression(&value.value)?
        ));
    }
    if let Some(directory) = working_directory {
        output.push_str(&format!(
            "{indent}{variable}.current_dir({});\n",
            rust_expression(directory)?
        ));
    }
    Ok(())
}

/// The expression, as something that borrows rather than owns where it can.
///
/// `Command::new` and its friends take `impl AsRef<_>`, so a literal needs no
/// `to_owned` — and `clippy` says so. Everywhere a `String` is required,
/// [`rust_expression`] is the one to call.
fn rust_expression_borrowed(expression: &crate::ir::TextExpression) -> Result<String, String> {
    if let [crate::ir::TextPart::Literal { value }] = expression.parts.as_slice() {
        return Ok(format!("{value:?}"));
    }
    rust_expression(expression)
}

fn rust_expression(expression: &crate::ir::TextExpression) -> Result<String, String> {
    // An expression that is one literal is that literal. Building it with a
    // `String::new()` and a `push_str` produces the same bytes and gives the
    // reader of the migration a block to read where a string would do.
    if let [crate::ir::TextPart::Literal { value }] = expression.parts.as_slice() {
        return Ok(format!("{value:?}.to_owned()"));
    }
    let mut output = "{ let mut deshell_value = String::new();".to_owned();
    for part in &expression.parts {
        match part {
            crate::ir::TextPart::Literal { value } => {
                // `push_str` with a one-character literal is a hard error under
                // the `-D warnings` gate the generated Rust is checked with, so
                // the single-character case uses `push`. Only a literal that is
                // one *char* qualifies: `"é"` is two bytes and one char, and
                // `push` takes a char.
                let mut chars = value.chars();
                output.push_str(&match (chars.next(), chars.next()) {
                    (Some(single), None) => format!(" deshell_value.push({single:?});"),
                    _ => format!(" deshell_value.push_str({value:?});"),
                });
            }
            crate::ir::TextPart::Variable { name } => {
                // Locals shadow the environment, the way the shell resolves a
                // name. `deshell_vars` exists only when the tree assigns to
                // something, so the lookup is written to compile either way.
                output.push_str(&format!(
                    " deshell_value.push_str(&deshell_lookup({name:?}, &deshell_vars));"
                ));
            }
            crate::ir::TextPart::DefaultValue {
                name,
                fallback,
                empty_is_unset,
            } => {
                // `:-` substitutes an empty value as well as an unset one, so the
                // two forms cannot share a lookup: `unwrap_or_default` would turn
                // `${x-d}` with `x=""` into `d`, which the shell does not.
                // Locals shadow the environment here too: `${x:-d}` after `x=v`
                // is `v`, and reading only the environment would give `d`.
                let lookup = if *empty_is_unset {
                    format!(
                        "deshell_lookup_opt({name:?}, &deshell_vars).filter(|deshell_set| !deshell_set.is_empty()).unwrap_or_else(|| {fallback:?}.to_owned())"
                    )
                } else {
                    format!(
                        "deshell_lookup_opt({name:?}, &deshell_vars).unwrap_or_else(|| {fallback:?}.to_owned())"
                    )
                };
                output.push_str(&format!(" deshell_value.push_str(&{lookup});"));
            }
            crate::ir::TextPart::Argument { name } => {
                let index = name
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .ok_or_else(|| format!("generator cannot bind named argument {name}"))?;
                let lookup = if index == 0 {
                    "deshell_args.first()".to_owned()
                } else {
                    format!("deshell_args.get({index})")
                };
                output.push_str(&format!(
                    " deshell_value.push_str({lookup}.map_or(\"\", String::as_str));"
                ));
            }
        }
    }
    output.push_str(" deshell_value }");
    Ok(output)
}

fn generate_go(plan: &crate::ir::Plan) -> Result<Vec<u8>, String> {
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| "generator entrypoint task is missing".to_owned())?;
    for task in &plan.tasks {
        if task.invocation.is_some() || !task.outputs.is_empty() {
            return Err("generator does not support invocation metadata or task outputs".into());
        }
    }
    let mut body = String::new();
    emit_go_node(&task.body, &mut body, 1)?;
    // A shell function becomes a function, so the helpers a plan needs are
    // decided over every task rather than over the entry alone.
    let mut functions = String::new();
    for other in plan.tasks.iter().filter(|other| other.name != task.name) {
        if !rust_function_name(&other.name) {
            return Err(format!(
                "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: task name is not a Go identifier: {}",
                other.name
            ));
        }
        let mut other_body = String::new();
        emit_go_node(&other.body, &mut other_body, 1)?;
        functions.push_str(&format!(
            concat!(
                "// {name} is `{name}` from the retired shell, taking the\n",
                "// arguments the call site passed it.\n",
                "func {name}(deshellArgs []string) int {{\n",
                "{locals}",
                "\tdeshellLast := 0\n",
                "{body}",
                "\treturn deshellLast\n",
                "}}\n\n"
            ),
            name = other.name,
            locals = go_variable_binding(&other.body),
            body = other_body,
        ));
    }
    let pipeline_helper = if plan
        .tasks
        .iter()
        .any(|task| rust_node_uses_pipeline(&task.body))
    {
        concat!(
            "func deshellRunPipeline(commands []*exec.Cmd, pipefail bool) int {\n",
            "\tif len(commands) == 0 {\n\t\treturn 0\n\t}\n",
            "\tpipes := make([]*os.File, 0, 2*(len(commands)-1))\n",
            "\tfor index := 0; index+1 < len(commands); index++ {\n",
            "\t\treader, writer, err := os.Pipe()\n",
            "\t\tif err != nil {\n\t\t\tfmt.Fprintln(os.Stderr, err)\n\t\t\treturn 127\n\t\t}\n",
            "\t\tcommands[index].Stdout = writer\n",
            "\t\tcommands[index+1].Stdin = reader\n",
            "\t\tpipes = append(pipes, reader, writer)\n",
            "\t}\n",
            "\tif commands[0].Stdin == nil {\n\t\tcommands[0].Stdin = os.Stdin\n\t}\n",
            "\tif commands[len(commands)-1].Stdout == nil {\n\t\tcommands[len(commands)-1].Stdout = os.Stdout\n\t}\n",
            "\tfor _, command := range commands {\n\t\tcommand.Stderr = os.Stderr\n\t}\n",
            "\tstarted := 0\n",
            "\tfor _, command := range commands {\n",
            "\t\tif err := command.Start(); err != nil {\n",
            "\t\t\tfmt.Fprintln(os.Stderr, err)\n",
            "\t\t\tfor index := 0; index < started; index++ {\n\t\t\t\t_ = commands[index].Process.Kill()\n\t\t\t}\n",
            "\t\t\tfor _, pipe := range pipes {\n\t\t\t\t_ = pipe.Close()\n\t\t\t}\n",
            "\t\t\treturn 127\n\t\t}\n",
            "\t\tstarted++\n\t}\n",
            "\tfor _, pipe := range pipes {\n\t\t_ = pipe.Close()\n\t}\n",
            "\tstatuses := make([]int, len(commands))\n",
            "\tfor index, command := range commands {\n\t\tstatuses[index] = deshellExitCode(command.Wait())\n\t}\n",
            "\tif pipefail {\n",
            "\t\tfor index := len(statuses) - 1; index >= 0; index-- {\n",
            "\t\t\tif statuses[index] != 0 {\n\t\t\t\treturn statuses[index]\n\t\t\t}\n\t\t}\n",
            "\t\treturn 0\n\t}\n",
            "\treturn statuses[len(statuses)-1]\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let uses_arguments = rust_node_uses_arguments(&task.body);
    let argument_helper = if uses_arguments {
        concat!(
            "func deshellArgument(arguments []string, index int) string {\n",
            "\tif len(arguments) > index {\n\t\treturn arguments[index]\n\t}\n",
            "\treturn \"\"\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let argument_binding = if uses_arguments {
        "\tdeshellArgs := os.Args[1:]\n"
    } else {
        ""
    };
    // A shell-local name is not an environment variable, so the generated program
    // keeps its own map and consults it first — the order the shell resolves in.
    // `os.Getenv` cannot tell unset from empty, which is the distinction `${x-d}`
    // turns on, so the two default forms stay separate.
    let go_uses_plain = plan.tasks.iter().any(|task| {
        node_has_expression_part(&task.body, &|part| {
            matches!(part, crate::ir::TextPart::Variable { .. })
        })
    });
    let go_uses_defaults = plan
        .tasks
        .iter()
        .any(|task| go_node_uses_defaults(&task.body));
    let go_sets_variables = plan
        .tasks
        .iter()
        .any(|task| rust_node_sets_variables(&task.body));
    let lookup_helper = if go_uses_plain || go_uses_defaults {
        concat!(
            "func deshellLookup(locals map[string]string, name string) (string, bool) {\n",
            "\tif value, ok := locals[name]; ok {\n\t\treturn value, true\n\t}\n",
            "\treturn os.LookupEnv(name)\n",
            "}\n\n",
            "func deshellValue(locals map[string]string, name string) string {\n",
            "\tvalue, _ := deshellLookup(locals, name)\n",
            "\treturn value\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let default_helper = if go_uses_defaults {
        concat!(
            "func deshellDefaultEmpty(locals map[string]string, name string, fallback string) string {\n",
            "\tif value, _ := deshellLookup(locals, name); value != \"\" {\n\t\treturn value\n\t}\n",
            "\treturn fallback\n",
            "}\n\n",
            "func deshellDefaultUnset(locals map[string]string, name string, fallback string) string {\n",
            "\tif value, ok := deshellLookup(locals, name); ok {\n\t\treturn value\n\t}\n",
            "\treturn fallback\n",
            "}\n\n"
        )
    } else {
        ""
    };
    let variable_binding = if go_uses_plain || go_uses_defaults || go_sets_variables {
        "\tdeshellVars := map[string]string{}\n"
    } else {
        ""
    };
    // Go rejects an unused import outright, and it rejects a missing one, so
    // the list is decided from what the body needs rather than from a default.
    // `strings` is used by a capture, which trims, and by the pattern matcher's
    // prefix test; `unicode/utf8` only by the matcher, which steps over one
    // character at a time.
    let go_captures = plan
        .tasks
        .iter()
        .any(|task| node_captures_stdout(&task.body));
    // A run-time exit status is parsed and trimmed, which is the only place the
    // generated Go needs `strconv`.
    let go_parses_status = plan
        .tasks
        .iter()
        .any(|task| go_node_checks_exit_status(&task.body));
    let (pattern_strings, pattern_utf8) =
        plan.tasks
            .iter()
            .fold((false, false), |(strings, utf8), task| {
                let (task_strings, task_utf8) = go_pattern_packages(&task.body);
                (strings || task_strings, utf8 || task_utf8)
            });
    // Go rejects an unused import and a missing one alike, so the list is built
    // from what the body needs rather than chosen from a few shapes.
    let imports = {
        let mut names = vec!["fmt", "os", "os/exec"];
        if go_parses_status {
            names.push("strconv");
        }
        if go_captures || pattern_strings || go_parses_status {
            names.push("strings");
        }
        if pattern_utf8 {
            names.push("unicode/utf8");
        }
        names.sort_unstable();
        format!(
            "import (\n{}\n)\n\n",
            names
                .into_iter()
                .map(|name| format!("\t\"{name}\""))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let source = format!(
        concat!(
            "// Code generated by de-shell. This file has no de-shell runtime dependency.\n",
            "package main\n\n",
            "{imports}",
            "func deshellExitCode(err error) int {{\n",
            "\tif err == nil {{\n\t\treturn 0\n\t}}\n",
            "\tif exit, ok := err.(*exec.ExitError); ok {{\n\t\treturn exit.ExitCode()\n\t}}\n",
            "\tfmt.Fprintln(os.Stderr, err)\n",
            "\treturn 127\n",
            "}}\n\n",
            "{pipeline_helper}",
            "{argument_helper}{lookup_helper}{default_helper}",
            "{functions}",
            "func main() {{\n",
            "{argument_binding}",
            "{variable_binding}",
            "\tdeshellLast := 0\n",
            "{body}",
            "\tos.Exit(deshellLast)\n",
            "}}\n"
        ),
        imports = imports,
        pipeline_helper = pipeline_helper,
        argument_helper = argument_helper,
        lookup_helper = lookup_helper,
        default_helper = default_helper,
        functions = functions,
        argument_binding = argument_binding,
        variable_binding = variable_binding,
        body = body,
    );
    Ok(source.into_bytes())
}

/// Whether any `exit` under this node reads its status at run time.
fn go_node_checks_exit_status(node: &crate::ir::Node) -> bool {
    if let crate::ir::Operation::Exit { non_numeric, .. } = &node.operation
        && *non_numeric == crate::ir::NonNumericStatus::Refuse
    {
        return true;
    }
    let mut found = false;
    visit_node(node, |child| found |= go_node_checks_exit_status(child));
    found
}

/// The shell-local map a Go function body needs, if it needs one.
fn go_variable_binding(body: &crate::ir::Node) -> String {
    let reads = node_has_expression_part(body, &|part| {
        matches!(
            part,
            crate::ir::TextPart::Variable { .. } | crate::ir::TextPart::DefaultValue { .. }
        )
    });
    if reads || rust_node_sets_variables(body) {
        "\tdeshellVars := map[string]string{}\n".to_owned()
    } else {
        String::new()
    }
}

fn emit_go_node(node: &crate::ir::Node, output: &mut String, depth: usize) -> Result<(), String> {
    let indent = "\t".repeat(depth);
    match &node.operation {
        crate::ir::Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            output.push_str(&format!("{indent}{{\n"));
            emit_go_command(EmitGoCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshellCommand",
                output,
                depth: depth + 1,
            })?;
            output.push_str(&format!(
                "{indent}\tdeshellCommand.Stdin, deshellCommand.Stdout, deshellCommand.Stderr = os.Stdin, os.Stdout, os.Stderr\n"
            ));
            output.push_str(&format!(
                concat!(
                    "{indent}\tdeshellLast = deshellExitCode(deshellCommand.Run())\n",
                    "{indent}}}\n"
                ),
                indent = indent
            ));
        }
        crate::ir::Operation::While { condition, body } => {
            output.push_str(&format!("{indent}for {{\n"));
            emit_go_node(condition, output, depth + 1)?;
            output.push_str(&format!(
                "{indent}\tif deshellLast != 0 {{\n{indent}\t\tdeshellLast = 0\n{indent}\t\tbreak\n{indent}\t}}\n"
            ));
            emit_go_node(body, output, depth + 1)?;
            output.push_str(&format!("{indent}}}\n"));
        }
        crate::ir::Operation::Not { body } => {
            emit_go_node(body, output, depth)?;
            output.push_str(&format!(
                concat!(
                    "{indent}if deshellLast == 0 {{\n",
                    "{indent}\tdeshellLast = 1\n",
                    "{indent}}} else {{\n",
                    "{indent}\tdeshellLast = 0\n",
                    "{indent}}}\n"
                ),
                indent = indent
            ));
        }
        crate::ir::Operation::Test { predicate } => {
            // `test` succeeds with 0 and fails with 1, so the predicate becomes a
            // bool and the node sets the running status from it.
            let condition = match predicate {
                crate::ir::TestPredicate::NonEmpty { value } => {
                    format!("{} != \"\"", go_expression(value)?)
                }
                crate::ir::TestPredicate::Empty { value } => {
                    format!("{} == \"\"", go_expression(value)?)
                }
                crate::ir::TestPredicate::StringEqual { left, right } => {
                    format!("{} == {}", go_expression(left)?, go_expression(right)?)
                }
                crate::ir::TestPredicate::StringNotEqual { left, right } => {
                    format!("{} != {}", go_expression(left)?, go_expression(right)?)
                }
                crate::ir::TestPredicate::StartsWith { value, prefix } => format!(
                    "strings.HasPrefix({}, {})",
                    go_expression(value)?,
                    go_string(prefix)?
                ),
                crate::ir::TestPredicate::EndsWith { value, suffix } => format!(
                    "strings.HasSuffix({}, {})",
                    go_expression(value)?,
                    go_string(suffix)?
                ),
                crate::ir::TestPredicate::Contains { value, infix } => format!(
                    "strings.Contains({}, {})",
                    go_expression(value)?,
                    go_string(infix)?
                ),
            };
            output.push_str(&format!(
                concat!(
                    "{indent}if {condition} {{\n",
                    "{indent}\tdeshellLast = 0\n",
                    "{indent}}} else {{\n",
                    "{indent}\tdeshellLast = 1\n",
                    "{indent}}}\n"
                ),
                indent = indent,
                condition = condition
            ));
        }
        // Running nothing succeeds, which is what the shell reports for an arm
        // whose body is empty.
        crate::ir::Operation::NoOp => {
            output.push_str(&format!("{indent}deshellLast = 0\n"));
        }
        // A chain rather than a `switch` on the subject: an arm can be a
        // pattern, and `switch` compares for equality. `switch { case cond: }`
        // keeps the arms in one construct and reads the same way.
        // A `case` with no matching arm and no `*` runs nothing and succeeds,
        // which is the `default` clause.
        crate::ir::Operation::Match {
            value,
            cases,
            default,
        } => {
            // Go rejects an unused binding outright, and a `case` whose every
            // arm matches anything leaves the subject unread. The word is still
            // expanded, because the shell expands it.
            let reads_subject = cases
                .iter()
                .any(|case| !matches!(pattern_shape(&case.pattern), PatternShape::Anything));
            output.push_str(&format!(
                "{indent}{}deshellSubject {}= {}\n{indent}switch {{\n",
                if reads_subject { "" } else { "_ = " },
                if reads_subject { ":" } else { "" },
                go_expression(value)?
            ));
            for case in cases {
                output.push_str(&format!(
                    "{indent}case {}:\n",
                    go_pattern_test(&case.pattern, "deshellSubject")?
                ));
                emit_go_node(&case.body, output, depth + 1)?;
            }
            output.push_str(&format!("{indent}default:\n"));
            match default {
                Some(default) => emit_go_node(default, output, depth + 1)?,
                None => output.push_str(&format!("{indent}\tdeshellLast = 0\n")),
            }
            output.push_str(&format!("{indent}}}\n"));
        }
        crate::ir::Operation::Exit {
            status,
            non_numeric,
        } => match non_numeric {
            crate::ir::NonNumericStatus::Unreachable => {
                output.push_str(&format!("{indent}os.Exit({})\n", rust_exit_status(status)?))
            }
            crate::ir::NonNumericStatus::Refuse => output.push_str(&format!(
                concat!(
                    "{indent}{{\n",
                    "{indent}\tdeshellStatus := {status}\n",
                    "{indent}\tdeshellCode, deshellErr := strconv.ParseInt(",
                    "strings.TrimSpace(deshellStatus), 10, 64)\n",
                    "{indent}\tif deshellErr != nil {{\n",
                    "{indent}\t\tfmt.Fprintf(os.Stderr,\n",
                    "{indent}\t\t\t\"exit status %q is not a number; \"+\n",
                    "{indent}\t\t\t\t\"the shells do not agree on what that means\\n\",\n",
                    "{indent}\t\t\tdeshellStatus)\n",
                    "{indent}\t\tos.Exit(70)\n",
                    "{indent}\t}}\n",
                    "{indent}\tdeshellCode = ((deshellCode % 256) + 256) % 256\n",
                    "{indent}\tos.Exit(int(deshellCode))\n",
                    "{indent}}}\n"
                ),
                indent = indent,
                status = go_expression(status)?
            )),
        },
        crate::ir::Operation::TaskCall {
            task,
            arguments,
            positional,
        } => {
            if !arguments.is_empty() {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: a task call with named arguments"
                        .into(),
                );
            }
            let mut values = Vec::new();
            for value in positional {
                values.push(go_expression(value)?);
            }
            output.push_str(&format!(
                "{indent}deshellLast = {task}([]string{{{}}})\n",
                values.join(", ")
            ));
        }
        // Only a redirect around a single command: redirecting a compound
        // statement means holding the descriptors open across everything
        // inside, which is refused rather than narrowed silently.
        crate::ir::Operation::Redirect { redirections, body } => {
            let crate::ir::Operation::Exec {
                argv,
                environment,
                working_directory,
            } = &body.operation
            else {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: redirect supports only a simple command"
                        .into(),
                );
            };
            output.push_str(&format!("{indent}{{\n"));
            emit_go_command(EmitGoCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshellCommand",
                output,
                depth: depth + 1,
            })?;
            for redirection in redirections {
                let (fd, path, append) = match redirection {
                    crate::ir::Redirection::Write { fd, path, append } => (fd, path, *append),
                    crate::ir::Redirection::Read { .. }
                    | crate::ir::Redirection::Duplicate { .. }
                    | crate::ir::Redirection::Close { .. } => {
                        return Err(
                            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: only an output redirection is emitted"
                                .into(),
                        );
                    }
                };
                let sink = match fd {
                    1 => "Stdout",
                    2 => "Stderr",
                    _ => {
                        return Err(
                            "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: only fd 1 and 2 are redirected"
                                .into(),
                        );
                    }
                };
                output.push_str(&format!(
                    concat!(
                        "{indent}\tdeshellFile, deshellErr := os.OpenFile(\n",
                        "{indent}\t\t{path},\n",
                        "{indent}\t\tos.O_WRONLY|os.O_CREATE|{mode},\n",
                        "{indent}\t\t0o644,\n",
                        "{indent}\t)\n",
                        "{indent}\tif deshellErr != nil {{\n",
                        "{indent}\t\tfmt.Fprintln(os.Stderr, deshellErr)\n",
                        "{indent}\t\tdeshellLast = 1\n",
                        "{indent}\t}} else {{\n",
                        "{indent}\t\tdefer deshellFile.Close()\n",
                        "{indent}\t\tdeshellCommand.{sink} = deshellFile\n",
                        "{indent}\t}}\n"
                    ),
                    indent = indent,
                    path = go_expression(path)?,
                    mode = if append { "os.O_APPEND" } else { "os.O_TRUNC" },
                    sink = sink,
                ));
            }
            output.push_str(&format!(
                "{indent}\tdeshellLast = deshellExitCode(deshellCommand.Run())\n{indent}}}\n"
            ));
        }
        // `echo` returns 1 when the write fails, so the status is the write's.
        crate::ir::Operation::WriteStdout { contents } => {
            output.push_str(&format!(
                concat!(
                    "{indent}{{\n",
                    "{indent}\tdeshellLast = 0\n",
                    "{indent}\tif _, deshellWriteErr := os.Stdout.WriteString({contents}); ",
                    "deshellWriteErr != nil {{\n",
                    "{indent}\t\tdeshellLast = 1\n",
                    "{indent}\t}}\n",
                    "{indent}}}\n"
                ),
                indent = indent,
                contents = go_expression(contents)?
            ));
        }
        crate::ir::Operation::Sequence { nodes, .. } => {
            for child in reachable_nodes(nodes) {
                emit_go_node(child, output, depth)?;
            }
        }
        crate::ir::Operation::Pipeline { nodes, status } => {
            output.push_str(&format!("{indent}{{\n"));
            output.push_str(&format!(
                "{indent}\tdeshellCommands := make([]*exec.Cmd, 0, {})\n",
                nodes.len()
            ));
            for (index, child) in nodes.iter().enumerate() {
                let crate::ir::Operation::Exec {
                    argv,
                    environment,
                    working_directory,
                } = &child.operation
                else {
                    return Err("generator pipeline supports only Exec stages".into());
                };
                let variable = format!("deshellStage{index}");
                emit_go_command(EmitGoCommandArgs {
                    argv,
                    environment,
                    working_directory: working_directory.as_ref(),
                    variable: &variable,
                    output,
                    depth: depth + 1,
                })?;
                output.push_str(&format!(
                    "{indent}\tdeshellCommands = append(deshellCommands, {variable})\n"
                ));
            }
            output.push_str(&format!(
                "{indent}\tdeshellLast = deshellRunPipeline(deshellCommands, {})\n{indent}}}\n",
                matches!(status, crate::ir::PipelineStatus::Pipefail)
            ));
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            emit_go_node(predicate, output, depth)?;
            output.push_str(&format!("{indent}if deshellLast == 0 {{\n"));
            emit_go_node(if_true, output, depth + 1)?;
            if let Some(if_false) = if_false {
                output.push_str(&format!("{indent}}} else {{\n"));
                emit_go_node(if_false, output, depth + 1)?;
            }
            output.push_str(&format!("{indent}}}\n"));
        }
        crate::ir::Operation::SetVariable { name, value, .. } => {
            output.push_str(&format!(
                "{indent}deshellVars[{}] = {}\n{indent}deshellLast = 0\n",
                go_string(name)?,
                go_expression(value)?
            ));
        }
        crate::ir::Operation::CaptureStdout { name, body, .. } => {
            let crate::ir::Operation::Exec {
                argv,
                environment,
                working_directory,
            } = &body.operation
            else {
                return Err(
                    "DESHELL_BLOCKER_GENERATOR_UNSUPPORTED: capture supports only a simple command"
                        .into(),
                );
            };
            output.push_str(&format!("{indent}{{\n"));
            emit_go_command(EmitGoCommandArgs {
                argv,
                environment,
                working_directory: working_directory.as_ref(),
                variable: "deshellCommand",
                output,
                depth: depth + 1,
            })?;
            // The shell strips trailing newlines from a substitution and nothing
            // else, so `TrimRight(.., "\n")` is right where `TrimSpace` is not.
            output.push_str(&format!(
                concat!(
                    "{indent}\tdeshellCommand.Stderr = os.Stderr\n",
                    "{indent}\tdeshellOut, deshellErr := deshellCommand.Output()\n",
                    "{indent}\tdeshellLast = deshellExitCode(deshellErr)\n",
                    "{indent}\tdeshellVars[{name}] = strings.TrimRight(string(deshellOut), \"\\n\")\n",
                    "{indent}}}\n"
                ),
                indent = indent,
                name = go_string(name)?
            ));
        }
        other => {
            return Err(format!(
                "generator cannot preserve {} semantics yet",
                other.name()
            ));
        }
    }
    Ok(())
}

/// The inputs of [`emit_go_command`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`emit_go_command`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct EmitGoCommandArgs<'a> {
    argv: &'a [crate::ir::TextExpression],
    environment: &'a [crate::ir::NamedExpression],
    working_directory: Option<&'a crate::ir::TextExpression>,
    variable: &'a str,
    output: &'a mut String,
    depth: usize,
}

fn emit_go_command(parts: EmitGoCommandArgs<'_>) -> Result<(), String> {
    // Destructured without `..`: see `EmitGoCommandArgs`.
    let EmitGoCommandArgs {
        argv,
        environment,
        working_directory,
        variable,
        output,
        depth,
    } = parts;
    let indent = "\t".repeat(depth);
    let program = argv
        .first()
        .ok_or_else(|| "generator received empty Exec argv".to_owned())?;
    let arguments = argv[1..]
        .iter()
        .map(go_expression)
        .collect::<Result<Vec<_>, _>>()?;
    output.push_str(&format!(
        "{indent}{variable} := exec.Command({}{})\n",
        go_expression(program)?,
        if arguments.is_empty() {
            String::new()
        } else {
            format!(", {}", arguments.join(", "))
        }
    ));
    if !environment.is_empty() {
        output.push_str(&format!("{indent}{variable}.Env = os.Environ()\n"));
        for value in environment {
            output.push_str(&format!(
                "{indent}{variable}.Env = append({variable}.Env, {} + \"=\" + {})\n",
                go_string(&value.name)?,
                go_expression(&value.value)?
            ));
        }
    }
    if let Some(directory) = working_directory {
        output.push_str(&format!(
            "{indent}{variable}.Dir = {}\n",
            go_expression(directory)?
        ));
    }
    Ok(())
}

/// Whether any expression in the tree carries a `${name:-fallback}`.
///
/// Reuses the walk `rust_node_uses_arguments` performs, since the question has
/// the same shape: does any expression anywhere under this node contain a part
/// of a given kind.
fn go_node_uses_defaults(node: &crate::ir::Node) -> bool {
    node_has_expression_part(node, &|part| {
        matches!(part, crate::ir::TextPart::DefaultValue { .. })
    })
}

fn go_expression(expression: &crate::ir::TextExpression) -> Result<String, String> {
    let mut parts = Vec::new();
    for part in &expression.parts {
        parts.push(match part {
            crate::ir::TextPart::Literal { value } => go_string(value)?,
            crate::ir::TextPart::Variable { name } => {
                format!("deshellValue(deshellVars, {})", go_string(name)?)
            }
            crate::ir::TextPart::DefaultValue {
                name,
                fallback,
                empty_is_unset,
            } => {
                // `os.Getenv` cannot tell unset from empty, so `-` needs LookupEnv.
                if *empty_is_unset {
                    format!(
                        "deshellDefaultEmpty(deshellVars, {}, {})",
                        go_string(name)?,
                        go_string(fallback)?
                    )
                } else {
                    format!(
                        "deshellDefaultUnset(deshellVars, {}, {})",
                        go_string(name)?,
                        go_string(fallback)?
                    )
                }
            }
            crate::ir::TextPart::Argument { name } => {
                let index = name
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .ok_or_else(|| format!("generator cannot bind named argument {name}"))?;
                format!("deshellArgument(deshellArgs, {index})")
            }
        });
    }
    Ok(if parts.is_empty() {
        "\"\"".into()
    } else {
        parts.join(" + ")
    })
}

fn go_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn proposal_diff(proposals: &[Proposal]) -> Result<String, String> {
    let mut output = String::new();
    for proposal in proposals {
        for patch in &proposal.patches {
            let contents = patch.contents()?;
            let text = std::str::from_utf8(&contents)
                .map_err(|_| format!("generated patch is not UTF-8: {}", patch.path))?;
            match patch.operation {
                PatchOperation::Create => {
                    output.push_str(&format!("--- /dev/null\n+++ b/{}\n", patch.path));
                    for line in text.split_inclusive('\n') {
                        output.push('+');
                        output.push_str(line);
                    }
                    if !text.ends_with('\n') {
                        output.push('\n');
                    }
                }
                PatchOperation::Update => {
                    output.push_str(&format!(
                        "--- a/{0}\n+++ b/{0}\n@@ replacement @@\n",
                        patch.path
                    ));
                    for line in text.split_inclusive('\n') {
                        output.push('+');
                        output.push_str(line);
                    }
                    if !text.ends_with('\n') {
                        output.push('\n');
                    }
                }
            }
        }
    }
    Ok(output)
}

fn persist_plan(
    root: &Path,
    plan: &MigrationPlan,
    artifacts: &PlannedArtifacts,
    diff: &str,
) -> Result<(), String> {
    let root = canonical_root(root)?;
    let migrations = ensure_safe_directory(&root, ".deshell/migrations")?;
    let sha256 = ensure_child_directory(&migrations, "sha256")?;
    let directory = ensure_child_directory(&sha256, &plan.plan_digest)?;
    let request_directory = ensure_child_directory(&directory, "requests")?;
    let ir_directory = ensure_child_directory(&directory, "ir")?;
    let proposal_directory = ensure_child_directory(&directory, "proposals")?;
    let evidence_directory = ensure_child_directory(&directory, "evidence")?;
    let _ = evidence_directory;
    for request in &artifacts.requests {
        persist_immutable(
            &request_directory.join(format!("{}.json", request.request_id)),
            pretty_bytes(request)?,
        )?;
    }
    for (digest, ir) in &artifacts.plans {
        persist_immutable(
            &ir_directory.join(format!("{digest}.json")),
            ir.encode_pretty()?,
        )?;
    }
    for proposal in &artifacts.proposals {
        persist_immutable(
            &proposal_directory.join(format!("{}.json", proposal.proposal_digest)),
            pretty_bytes(proposal)?,
        )?;
    }
    persist_immutable(&directory.join("diff.patch"), diff.as_bytes().to_vec())?;
    persist_immutable(&directory.join("plan.json"), pretty_bytes(plan)?)
}

fn persist_active_index(root: &Path, digest: &str) -> Result<(), String> {
    if !crate::digest::valid_sha256(digest) {
        return Err("active migration plan digest is invalid".into());
    }
    let root = canonical_root(root)?;
    let migrations = ensure_safe_directory(&root, ".deshell/migrations")?;
    let path = migrations.join("active.json");
    let index = MigrationIndex {
        schema_version: 1,
        active_plan_digest: digest.into(),
    };
    let bytes = pretty_bytes(&index)?;
    let proposal = match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            crate::patch::prepare(&path, bytes)?
        }
        Ok(_) => {
            return Err(format!(
                "migration index is not a regular file: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::patch::prepare_create(&path, bytes, 0o644)?
        }
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
    };
    crate::patch::apply_all(&[proposal])
}

fn load_active_index(root: &Path) -> Result<Option<MigrationIndex>, String> {
    let root = canonical_root(root)?;
    let path = root.join(".deshell/migrations/active.json");
    let metadata = match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "migration index is not a regular file: {}",
            path.display()
        ));
    }
    let index: MigrationIndex = crate::strict_json::decode(
        &std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    )?;
    if index.schema_version != 1 || !crate::digest::valid_sha256(&index.active_plan_digest) {
        return Err("migration index must be strict Migration Index v1".into());
    }
    Ok(Some(index))
}

fn pretty_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    crate::canonical_json::pretty_bytes(
        &serde_json::to_value(value).map_err(|error| error.to_string())?,
    )
}

fn persist_immutable(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let current = std::fs::read(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            if current == bytes {
                Ok(())
            } else {
                Err(format!(
                    "content-addressed artifact bytes differ at {}",
                    path.display()
                ))
            }
        }
        Ok(_) => Err(format!(
            "content-addressed artifact is not a regular file: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let proposal = crate::patch::prepare_create(path, bytes, 0o644)?;
            crate::patch::apply_all(&[proposal])
        }
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

impl MigrationPlan {
    fn computed_digest(&self) -> Result<String, String> {
        let mut unsigned = self.clone();
        unsigned.plan_digest = ZERO_DIGEST.into();
        canonical_digest(&unsigned)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.kind != PlanKind::Migration {
            return Err("migration plan must use fresh schema version 1 and kind=migration".into());
        }
        if self.computed_digest()? != self.plan_digest {
            return Err("migration plan digest mismatch".into());
        }
        if !crate::digest::valid_sha256(&self.inventory_digest) {
            return Err("migration plan inventory digest is invalid".into());
        }
        let proposal_set = self.proposals.iter().collect::<BTreeSet<_>>();
        if proposal_set.len() != self.proposals.len() {
            return Err("migration plan proposal digests must be unique".into());
        }
        for digest in self
            .proposals
            .iter()
            .chain(self.sources.iter().map(|source| &source.content_digest))
            .chain(self.sources.iter().map(|source| &source.ir_digest))
            .chain(
                self.required_scenarios
                    .iter()
                    .map(|scenario| &scenario.digest),
            )
        {
            if !crate::digest::valid_sha256(digest) {
                return Err("migration plan contains an invalid SHA-256 digest".into());
            }
        }
        let mut source_locations = BTreeSet::new();
        for source in &self.sources {
            let normalized = crate::ir::normalize_path(&source.location.path)
                .map_err(|error| format!("migration plan source location is invalid: {error}"))?;
            if normalized != source.location.path
                || source.location.start_byte >= source.location.end_byte
                || !source_locations.insert((
                    source.location.path.as_str(),
                    source.location.start_byte,
                    source.location.end_byte,
                ))
            {
                return Err(
                    "migration plan source locations must be normalized, non-empty, and unique"
                        .into(),
                );
            }
            if source
                .proposal_digest
                .as_ref()
                .is_some_and(|digest| !proposal_set.contains(digest))
            {
                return Err("migration plan source proposal is not present in proposals".into());
            }
        }
        let mut scenario_names = BTreeSet::new();
        for scenario in &self.required_scenarios {
            if scenario.name.trim().is_empty()
                || !scenario_names.insert(scenario.name.as_str())
                || !crate::digest::valid_pinned_sha256(&scenario.approval_digest)
            {
                return Err("migration plan scenario names must be non-empty and unique".into());
            }
        }
        let mut cell_ids = BTreeSet::new();
        for cell in &self.required_cells {
            let portable_id = !cell.id.is_empty()
                && cell.id.len() <= 128
                && cell.id.bytes().enumerate().all(|(index, byte)| {
                    byte.is_ascii_alphanumeric()
                        || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
                });
            if !portable_id
                || !cell_ids.insert(cell.id.as_str())
                || !crate::digest::valid_sha256(&cell.platform_fingerprint)
                || !crate::digest::valid_sha256(&cell.runtime_fingerprint)
                || !crate::digest::valid_pinned_sha256(&cell.approval_digest)
            {
                return Err(
                    "migration plan platform cells must have unique portable ids and valid fingerprints"
                        .into(),
                );
            }
        }
        let mut command_names = BTreeSet::new();
        for command in &self.validation_commands {
            if command.name.trim().is_empty() || !command_names.insert(command.name.as_str()) {
                return Err(
                    "migration plan validation command names must be non-empty and unique".into(),
                );
            }
            validate_exact_argv(&command.argv).map_err(|error| {
                format!("migration plan validation command is invalid: {error}")
            })?;
        }
        if self
            .network_replay_digest
            .as_ref()
            .is_some_and(|digest| !crate::digest::valid_sha256(digest))
        {
            return Err("migration plan contains an invalid network replay digest".into());
        }
        if self.sources.iter().any(|source| {
            !matches!(
                source.interpreter.as_str(),
                "sh" | "bash" | "zsh" | "fish" | "powershell" | "cmd" | "nu"
            )
        }) {
            return Err("migration plan contains an unknown source interpreter".into());
        }
        if self.coverage.total_bytes
            != self.coverage.native_bytes
                + self.coverage.delegated_bytes
                + self.coverage.residual_bytes
                + self.coverage.trivia_bytes
        {
            return Err("migration plan source coverage does not sum to total bytes".into());
        }
        let mut limit_errors = Vec::new();
        self.validation_limits.validate(&mut limit_errors);
        if !limit_errors.is_empty() {
            return Err(format!(
                "migration plan validation limits are invalid: {}",
                limit_errors.join("; ")
            ));
        }
        Ok(())
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        let plan: Self = crate::strict_json::decode(bytes)?;
        plan.validate()?;
        Ok(plan)
    }
}

fn validate_proposal(proposal: &Proposal) -> Result<(), String> {
    if proposal.schema_version != 1 || !crate::digest::valid_sha256(&proposal.request_digest) {
        return Err("proposal version or request digest is invalid".into());
    }
    let mut unsigned = proposal.clone();
    unsigned.proposal_digest = ZERO_DIGEST.into();
    if canonical_digest(&unsigned)? != proposal.proposal_digest {
        return Err("proposal digest mismatch".into());
    }
    if !crate::digest::valid_pinned_sha256(&proposal.generator_digest) {
        return Err("proposal generator digest is not pinned".into());
    }
    if proposal.patches.is_empty() {
        return Err("proposal contains no create/update patch".into());
    }
    let mut paths = BTreeSet::new();
    let mut generated_lengths = BTreeMap::new();
    let mut total_bytes = 0_usize;
    for patch in &proposal.patches {
        let normalized = crate::ir::normalize_path(&patch.path)?;
        if normalized != patch.path || !paths.insert(&patch.path) {
            return Err(format!(
                "invalid or duplicate proposal path: {}",
                patch.path
            ));
        }
        match patch.operation {
            PatchOperation::Create if patch.expected_digest.is_some() => {
                return Err("create proposal must not carry expected_digest".into());
            }
            PatchOperation::Update
                if !patch
                    .expected_digest
                    .as_deref()
                    .is_some_and(crate::digest::valid_sha256) =>
            {
                return Err("update proposal requires expected_digest".into());
            }
            PatchOperation::Create | PatchOperation::Update => {}
        }
        if patch.permissions > 0o777 {
            return Err(format!(
                "proposal permissions exceed 0777 for {}",
                patch.path
            ));
        }
        let contents = patch.contents()?;
        if contents.len() > 4 * 1024 * 1024 {
            return Err(format!(
                "proposal patch exceeds the 4 MiB frame limit: {}",
                patch.path
            ));
        }
        total_bytes = total_bytes
            .checked_add(contents.len())
            .ok_or("proposal patch size overflow")?;
        if total_bytes > 16 * 1024 * 1024 {
            return Err("proposal patches exceed the 16 MiB aggregate limit".into());
        }
        generated_lengths.insert(patch.path.as_str(), contents.len() as u64);
    }
    validate_exact_argv(&proposal.build_argv)?;
    validate_exact_argv(&proposal.run_argv)?;
    for command in &proposal.validation {
        validate_exact_argv(command)?;
    }
    let mut dependencies = BTreeSet::new();
    for dependency in &proposal.dependencies {
        if dependency.name.trim().is_empty()
            || dependency.name.chars().any(char::is_whitespace)
            || !dependencies.insert((dependency.ecosystem, dependency.name.as_str()))
        {
            return Err("proposal dependency names must be non-empty and unique".into());
        }
        if !exact_dependency_pin(&dependency.version) {
            return Err(format!(
                "proposal dependency requires an exact pin: {} {}",
                dependency.name, dependency.version
            ));
        }
    }
    if proposal.source_map.is_empty() {
        return Err("proposal source map must not be empty".into());
    }
    let mut spans = BTreeSet::new();
    for span in &proposal.source_map {
        if span.ir_node.len() != 32
            || !span
                .ir_node
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("proposal source map contains an invalid IR node id".into());
        }
        let normalized = crate::ir::normalize_path(&span.generated.path)?;
        if normalized != span.generated.path {
            return Err("proposal source map path is not normalized".into());
        }
        let Some(length) = generated_lengths.get(span.generated.path.as_str()) else {
            return Err(format!(
                "proposal source map path has no patch: {}",
                span.generated.path
            ));
        };
        if span.generated.start_byte > span.generated.end_byte || span.generated.end_byte > *length
        {
            return Err(format!(
                "proposal generated span is outside {}",
                span.generated.path
            ));
        }
        if !spans.insert((
            span.ir_node.as_str(),
            span.generated.path.as_str(),
            span.generated.start_byte,
            span.generated.end_byte,
        )) {
            return Err("proposal contains a duplicate generated span".into());
        }
    }
    Ok(())
}

fn exact_dependency_pin(version: &str) -> bool {
    !version.is_empty()
        && version.chars().any(|character| character.is_ascii_digit())
        && !version.chars().any(|character| {
            character.is_whitespace() || matches!(character, '*' | '^' | '~' | '<' | '>' | ',')
        })
}

fn validate_exact_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() || argv[0].is_empty() || argv[0].starts_with('-') {
        return Err("exact argv requires a non-option argv[0]".into());
    }
    if argv.iter().any(|argument| argument.contains('\0')) {
        return Err("exact argv must not contain NUL".into());
    }
    Ok(())
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    Ok(crate::digest::sha256(
        &crate::canonical_json::canonical_bytes(&value)?,
    ))
}

fn blocker_code(message: &str, fallback: &str) -> String {
    message
        .split(|character: char| character.is_whitespace() || character == ':')
        .find(|part| part.starts_with("DESHELL_BLOCKER_"))
        .unwrap_or(fallback)
        .into()
}

fn source_stem(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    let mut output = String::new();
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            output.push(character.to_ascii_lowercase());
        } else if !output.ends_with('_') {
            output.push('_');
        }
    }
    let output = output.trim_matches('_');
    if output.is_empty() {
        "entry".into()
    } else {
        output.into()
    }
}

fn rust_binary_name(stem: &str) -> String {
    format!("deshell_{stem}")
}

fn canonical_root(root: &Path) -> Result<PathBuf, String> {
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect project root {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "project root is not a regular non-symlink directory: {}",
            root.display()
        ));
    }
    root.canonicalize()
        .map_err(|error| format!("cannot resolve project root {}: {error}", root.display()))
}

fn safe_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!("generator target is not normalized: {relative}"));
    }
    if let Some((parent, filename)) = relative.rsplit_once('/') {
        let parent = crate::project::project_directory_path(root, parent)?;
        Ok(parent.join(filename))
    } else {
        Ok(root.join(relative))
    }
}

fn safe_future_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!("generator target is not normalized: {relative}"));
    }
    let components = relative.split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    let mut missing_parent = false;
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component);
        if missing_parent {
            continue;
        }
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => {
                return Err(format!(
                    "generator target parent is not a regular directory: {}",
                    current.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing_parent = true;
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect generator target parent {}: {error}",
                    current.display()
                ));
            }
        }
    }
    Ok(root.join(relative))
}

fn generator_patch(
    root: &Path,
    target: &str,
    bytes: Vec<u8>,
    permissions: u32,
) -> Result<GeneratorPatch, String> {
    let absolute = safe_future_target(root, target)?;
    let (operation, expected_digest) = match absolute.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => (
            PatchOperation::Update,
            Some(crate::digest::sha256(&std::fs::read(&absolute).map_err(
                |error| format!("cannot read {}: {error}", absolute.display()),
            )?)),
        ),
        Ok(_) => return Err(format!("generator target is not a regular file: {target}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (PatchOperation::Create, None)
        }
        Err(error) => return Err(format!("cannot inspect generator target {target}: {error}")),
    };
    Ok(GeneratorPatch {
        operation,
        path: target.into(),
        expected_digest,
        content_digest: crate::digest::sha256(&bytes),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        permissions,
    })
}

fn ensure_safe_directory(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!("directory path is not normalized: {relative}"));
    }
    let mut current = root.to_path_buf();
    for component in relative.split('/') {
        current = ensure_child_directory(&current, component)?;
    }
    Ok(current)
}

fn ensure_child_directory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(format!("invalid directory component: {name}"));
    }
    let target = parent.join(name);
    match target.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(format!(
                "migration directory is not a regular directory: {}",
                target.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match crate::patch::ensure_directory(&target) {
                Ok(
                    crate::patch::DirectoryState::Created | crate::patch::DirectoryState::Existing,
                ) => {}
                Err(crate::patch::DirectoryError::Occupied) => {
                    return Err(format!(
                        "migration directory is not a regular directory: {}",
                        target.display()
                    ));
                }
                Err(crate::patch::DirectoryError::Io(error)) => {
                    return Err(format!("cannot create {}: {error}", target.display()));
                }
            }
        }
        Err(error) => return Err(format!("cannot inspect {}: {error}", target.display())),
    }
    target
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", target.display()))
}

pub(crate) fn verify(
    root: &Path,
    digest: &str,
    cell_id: &str,
) -> Result<MigrationEvidence, String> {
    let (directory, plan) = load_plan(root, digest)?;
    if !plan.blockers.is_empty() {
        return Err(plan
            .blockers
            .iter()
            .map(|blocker| format!("{}: {}", blocker.code, blocker.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    validate_current_plan_policy(root, &plan)?;
    let approval_digests = plan_approval_digests(&plan);
    let cell = plan
        .required_cells
        .iter()
        .find(|cell| cell.id == cell_id)
        .ok_or_else(|| format!("DESHELL_BLOCKER_CELL_NOT_APPROVED: {cell_id}"))?;
    let scenarios = load_approved_scenario_values(root)?;
    let toolchain_digest = crate::digest::sha256(
        &std::fs::read(crate::project::project_file_path(root, "deshell.lock")?)
            .map_err(|error| format!("cannot read deshell.lock: {error}"))?,
    );
    let local_platform_fingerprint = crate::digest::sha256(
        format!(
            "deshell-platform-v1:{}:{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
        .as_bytes(),
    );
    let local_runtime_fingerprint = crate::digest::sha256(b"deshell-runtime-v1:native");
    if cell.platform_fingerprint != local_platform_fingerprint
        || cell.runtime_fingerprint != local_runtime_fingerprint
    {
        let error =
            format!("foreign platform/runtime cell {cell_id} is unavailable to this verifier");
        let mut checks = Vec::new();
        for source in &plan.sources {
            let proposal_digest = source.proposal_digest.as_deref().ok_or_else(|| {
                format!(
                    "DESHELL_BLOCKER_PROPOSAL_MISSING: {}@{}..{}",
                    source.location.path, source.location.start_byte, source.location.end_byte
                )
            })?;
            let proposal = load_proposal(&directory, proposal_digest)?;
            validate_current_source(root, source)?;
            for requirement in &plan.required_scenarios {
                scenarios
                    .get(&requirement.name)
                    .filter(|scenario| {
                        scenario.digest().ok().as_deref() == Some(&requirement.digest)
                            && scenario.approval_digest == requirement.approval_digest
                    })
                    .ok_or_else(|| {
                        format!(
                            "DESHELL_BLOCKER_STALE_SCENARIO: {} no longer matches {}",
                            requirement.name, requirement.digest
                        )
                    })?;
                checks.push(EvidenceCheck {
                    source: source.location.clone(),
                    scenario: requirement.name.clone(),
                    key: EvidenceKey {
                        source_digest: source.content_digest.clone(),
                        ir_digest: source.ir_digest.clone(),
                        proposal_digest: proposal.proposal_digest.clone(),
                        generator_digest: proposal
                            .generator_digest
                            .strip_prefix("sha256:")
                            .unwrap_or(&proposal.generator_digest)
                            .into(),
                        toolchain_digest: toolchain_digest.clone(),
                        scenario_digest: requirement.digest.clone(),
                        platform_fingerprint: cell.platform_fingerprint.clone(),
                        runtime_fingerprint: cell.runtime_fingerprint.clone(),
                    },
                    status: EvidenceStatus::Unavailable,
                    error: Some(error.clone()),
                    covered_nodes: Vec::new(),
                    comparisons: Vec::new(),
                });
            }
        }
        let evidence = MigrationEvidence {
            schema_version: 1,
            plan_digest: plan.plan_digest,
            approval_digests,
            cell: cell_id.into(),
            status: EvidenceStatus::Unavailable,
            repetitions: 2,
            checks,
            validation: Vec::new(),
        };
        validate_evidence_document(&evidence)?;
        return Ok(evidence);
    }
    let replay = if plan.network_replay_digest.is_some() {
        Some(load_network_replay(root)?.0)
    } else {
        None
    };
    let mut checks = Vec::new();
    for source in &plan.sources {
        let proposal_digest = source.proposal_digest.as_deref().ok_or_else(|| {
            format!(
                "DESHELL_BLOCKER_PROPOSAL_MISSING: {}@{}..{}",
                source.location.path, source.location.start_byte, source.location.end_byte
            )
        })?;
        let proposal = load_proposal(&directory, proposal_digest)?;
        let ir = load_ir(&directory, &source.ir_digest)?;
        validate_current_source(root, source)?;
        for requirement in &plan.required_scenarios {
            let scenario = scenarios
                .get(&requirement.name)
                .filter(|scenario| {
                    scenario.digest().ok().as_deref() == Some(&requirement.digest)
                        && scenario.approval_digest == requirement.approval_digest
                })
                .ok_or_else(|| {
                    format!(
                        "DESHELL_BLOCKER_STALE_SCENARIO: {} no longer matches {}",
                        requirement.name, requirement.digest
                    )
                })?;
            let mut comparisons = Vec::new();
            let mut covered_nodes = BTreeSet::new();
            for _ in 0..2 {
                let original = observe_original(root, source, scenario, replay.as_ref())?;
                let (ir_observation, visited) = observe_ir(root, &ir, scenario, replay.as_ref())?;
                covered_nodes.extend(visited);
                let replacement = observe_replacement(root, &proposal, scenario, replay.as_ref())?;
                let differences = compare_three(&original, &ir_observation, &replacement);
                comparisons.push(TripleComparison {
                    original,
                    ir: ir_observation,
                    replacement,
                    differences,
                });
            }
            let nondeterministic = subject_changed(&comparisons, |value| &value.original)
                || subject_changed(&comparisons, |value| &value.ir)
                || subject_changed(&comparisons, |value| &value.replacement);
            let status = if nondeterministic {
                EvidenceStatus::Nondeterministic
            } else if comparisons
                .iter()
                .any(|comparison| !comparison.differences.is_empty())
            {
                EvidenceStatus::Different
            } else {
                EvidenceStatus::Verified
            };
            checks.push(EvidenceCheck {
                source: source.location.clone(),
                scenario: scenario.name.clone(),
                key: EvidenceKey {
                    source_digest: source.content_digest.clone(),
                    ir_digest: source.ir_digest.clone(),
                    proposal_digest: proposal.proposal_digest.clone(),
                    generator_digest: proposal
                        .generator_digest
                        .strip_prefix("sha256:")
                        .unwrap_or(&proposal.generator_digest)
                        .into(),
                    toolchain_digest: toolchain_digest.clone(),
                    scenario_digest: requirement.digest.clone(),
                    platform_fingerprint: cell.platform_fingerprint.clone(),
                    runtime_fingerprint: cell.runtime_fingerprint.clone(),
                },
                status,
                error: None,
                covered_nodes: covered_nodes.into_iter().collect(),
                comparisons,
            });
        }
    }
    let validation = verify_validation_commands(root, &directory, &plan)?;
    let status = if checks
        .iter()
        .any(|check| check.status == EvidenceStatus::Nondeterministic)
    {
        EvidenceStatus::Nondeterministic
    } else if checks
        .iter()
        .any(|check| check.status == EvidenceStatus::Different)
    {
        EvidenceStatus::Different
    } else if validation.iter().any(|command| command.exit_code != 0) {
        EvidenceStatus::Failed
    } else {
        EvidenceStatus::Verified
    };
    let evidence = MigrationEvidence {
        schema_version: 1,
        plan_digest: plan.plan_digest,
        approval_digests,
        cell: cell_id.into(),
        status,
        repetitions: 2,
        checks,
        validation,
    };
    validate_evidence_document(&evidence)?;
    Ok(evidence)
}

fn plan_approval_digests(plan: &MigrationPlan) -> Vec<String> {
    let mut digests = plan
        .required_scenarios
        .iter()
        .map(|scenario| scenario.approval_digest.clone())
        .chain(
            plan.required_cells
                .iter()
                .map(|cell| cell.approval_digest.clone()),
        )
        .collect::<Vec<_>>();
    digests.sort();
    digests.dedup();
    digests
}

#[derive(Clone, Debug)]
struct ApprovedScenario {
    scenario: crate::config::Scenario,
    approval_digest: String,
}

impl std::ops::Deref for ApprovedScenario {
    type Target = crate::config::Scenario;

    fn deref(&self) -> &Self::Target {
        &self.scenario
    }
}

fn load_approved_scenario_values(
    root: &Path,
) -> Result<BTreeMap<String, ApprovedScenario>, String> {
    let directory = crate::project::project_directory_path(root, ".deshell/scenarios")?;
    let mut paths = std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut output = BTreeMap::new();
    for path in paths {
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(format!("unsafe scenario file: {}", path.display()));
        }
        let scenario = crate::config::Scenario::decode(
            &std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
        )
        .map_err(|errors| errors.join("; "))?;
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("scenario filename is not UTF-8: {}", path.display()))?;
        let relative = format!(".deshell/scenarios/{filename}");
        if let Some(approval_digest) =
            crate::approval::scenario_approval(root, &relative, &scenario)?
        {
            let approved = ApprovedScenario {
                scenario,
                approval_digest,
            };
            if output.insert(approved.name.clone(), approved).is_some() {
                return Err("duplicate approved scenario name".into());
            }
        }
    }
    Ok(output)
}

fn load_proposal(directory: &Path, digest: &str) -> Result<Proposal, String> {
    if !crate::digest::valid_sha256(digest) {
        return Err("proposal selector is not a SHA-256 digest".into());
    }
    let path = directory.join(format!("proposals/{digest}.json"));
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!("proposal path is unsafe: {}", path.display()));
    }
    let proposal: Proposal = crate::strict_json::decode(
        &std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    )?;
    validate_proposal(&proposal)?;
    if proposal.proposal_digest != digest {
        return Err("proposal filename digest does not match content".into());
    }
    Ok(proposal)
}

fn load_ir(directory: &Path, digest: &str) -> Result<crate::ir::Plan, String> {
    if !crate::digest::valid_sha256(digest) {
        return Err("IR selector is not a SHA-256 digest".into());
    }
    let path = directory.join(format!("ir/{digest}.json"));
    let bytes =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let plan = crate::ir::Plan::decode(&bytes).map_err(|errors| errors.join("; "))?;
    if canonical_digest(&plan)? != digest {
        return Err("IR filename digest does not match content".into());
    }
    Ok(plan)
}

fn validate_current_source(root: &Path, source: &PlanSource) -> Result<(), String> {
    if source.kind == SourceKind::EmbeddedShell {
        current_embedded_source(root, source)?;
        return Ok(());
    }
    current_shell_source(root, source)?;
    Ok(())
}

fn current_shell_source(root: &Path, source: &PlanSource) -> Result<Vec<u8>, String> {
    let inventory = crate::project::scan(root)?;
    let finding = inventory.findings.iter().find(|finding| {
        finding.kind == crate::scanner::FindingKind::ShellFile
            && finding.path == source.location.path
            && finding.span.start_byte == source.location.start_byte
            && finding.span.end_byte == source.location.end_byte
            && finding.content_digest == source.content_digest
            && finding.interpreter.as_deref() == Some(source.interpreter.as_str())
    });
    finding.map(|finding| finding.source.clone()).ok_or_else(|| {
        format!(
            "DESHELL_BLOCKER_STALE_SOURCE: source or interpreter for {} changed after plan creation",
            source.location.path
        )
    })
}

fn current_embedded_source(root: &Path, source: &PlanSource) -> Result<(Vec<u8>, String), String> {
    let inventory = crate::project::scan(root)?;
    let finding = inventory.findings.iter().find(|finding| {
        finding.kind == crate::scanner::FindingKind::EmbeddedShell
            && finding.path == source.location.path
            && finding.span.start_byte == source.location.start_byte
            && finding.span.end_byte == source.location.end_byte
            && finding.content_digest == source.content_digest
            && resolved_finding_interpreter(finding).ok().as_deref()
                == Some(source.interpreter.as_str())
    });
    let Some(finding) = finding else {
        return Err(format!(
            "DESHELL_BLOCKER_STALE_SOURCE: embedded shell {}@{}..{} changed after plan creation",
            source.location.path, source.location.start_byte, source.location.end_byte
        ));
    };
    Ok((finding.source.clone(), source.interpreter.clone()))
}

fn observe_original(
    root: &Path,
    source: &PlanSource,
    scenario: &crate::config::Scenario,
    replay: Option<&crate::replay::ReplayStore>,
) -> Result<Observation, String> {
    let workspace = prepared_workspace(root, scenario)?;
    let before = crate::workspace::capture(workspace.path())?;
    let proxy = start_replay_proxy(replay, scenario.limits)?;
    let environment = replay_environment(scenario_environment(scenario), proxy.as_ref());
    if source.kind == SourceKind::EmbeddedShell {
        let (snippet, interpreter) = current_embedded_source(workspace.path(), source)?;
        let mut invocation = embedded_original_invocation(&interpreter, snippet)?;
        invocation.argv.extend(scenario.argv.clone());
        let outcome = crate::agent_process::execute(
            workspace.path(),
            crate::agent_process::Request {
                argv: invocation.argv,
                environment,
                working_directory: scenario.cwd.clone(),
                stdin: scenario_stdin(scenario)?,
                limits: scenario.limits.into(),
            },
        )?;
        validate_expectations(scenario, &outcome)?;
        let network = finish_replay_proxy(proxy)?;
        return observation_from_outcome(workspace.path(), &before, outcome, network);
    }
    let _source_bytes = current_shell_source(workspace.path(), source)?;
    let absolute_source = workspace
        .path()
        .join(&source.location.path)
        .to_string_lossy()
        .into_owned();
    let mut argv = original_script_argv(&source.interpreter, &absolute_source)?;
    argv.extend(scenario.argv.clone());
    let outcome = crate::agent_process::execute(
        workspace.path(),
        crate::agent_process::Request {
            argv,
            environment,
            working_directory: scenario.cwd.clone(),
            stdin: scenario_stdin(scenario)?,
            limits: scenario.limits.into(),
        },
    )?;
    validate_expectations(scenario, &outcome)?;
    let network = finish_replay_proxy(proxy)?;
    observation_from_outcome(workspace.path(), &before, outcome, network)
}

struct EmbeddedOriginalInvocation {
    argv: Vec<String>,
    _script_directory: Option<tempfile::TempDir>,
}

fn embedded_original_invocation(
    interpreter: &str,
    snippet: Vec<u8>,
) -> Result<EmbeddedOriginalInvocation, String> {
    let snippet = String::from_utf8(snippet)
        .map_err(|_| "embedded shell verification requires UTF-8 source")?;
    if interpreter == "cmd" {
        // cmd.exe does not reliably treat embedded newlines in its /C command
        // argument as batch separators. Host runners execute multiline `run:`
        // content through a temporary script as well, so preserve the decoded
        // bytes in an isolated .cmd file and use the exact batch argv contract.
        let directory = tempfile::Builder::new()
            .prefix("deshell-original-cmd-")
            .tempdir()
            .map_err(|error| format!("cannot create embedded cmd script directory: {error}"))?;
        let script = directory.path().join("snippet.cmd");
        crate::patch::scratch::write(&script, snippet.as_bytes())
            .map_err(|error| format!("cannot write embedded cmd script: {error}"))?;
        let script = script
            .to_str()
            .ok_or("embedded cmd script path is not UTF-8")?;
        return Ok(EmbeddedOriginalInvocation {
            argv: original_script_argv("cmd", script)?,
            _script_directory: Some(directory),
        });
    }
    let (executable, flag) = match interpreter {
        "sh" => ("sh", "-c"),
        "bash" => ("bash", "-c"),
        "zsh" => ("zsh", "-c"),
        "fish" => ("fish", "-c"),
        "powershell" => ("pwsh", "-Command"),
        "nu" => ("nu", "-c"),
        other => return Err(format!("unknown embedded interpreter: {other}")),
    };
    Ok(EmbeddedOriginalInvocation {
        argv: vec![executable.into(), flag.into(), snippet],
        _script_directory: None,
    })
}

fn original_script_argv(interpreter: &str, source: &str) -> Result<Vec<String>, String> {
    match interpreter {
        "sh" | "bash" | "zsh" | "fish" | "nu" => Ok(vec![interpreter.into(), source.into()]),
        "powershell" => Ok(vec!["pwsh".into(), source.into()]),
        "cmd" => Ok(vec![
            "cmd".into(),
            "/D".into(),
            "/S".into(),
            "/C".into(),
            source.into(),
        ]),
        name => Err(format!("unknown original interpreter: {name}")),
    }
}

fn observe_replacement(
    root: &Path,
    proposal: &Proposal,
    scenario: &crate::config::Scenario,
    replay: Option<&crate::replay::ReplayStore>,
) -> Result<Observation, String> {
    let workspace = prepared_workspace(root, scenario)?;
    apply_generator_patches(workspace.path(), proposal)?;
    ensure_safe_directory(workspace.path(), ".deshell/verification")?;
    let build_environment = verification_build_environment(workspace.path(), &proposal.build_argv);
    let build_limits = verification_build_limits(&proposal.build_argv, scenario.limits);
    let build = execute_exact(ExecuteExactArgs {
        root: workspace.path(),
        argv: &proposal.build_argv,
        environment: &build_environment,
        cwd: None,
        stdin: &[],
        limits: build_limits,
    })?;
    if let Some(reason) = classify_build(&build, build_limits.timeout_ms) {
        return Err(format!(
            "{reason}: {}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    let before = crate::workspace::capture(workspace.path())?;
    let proxy = start_replay_proxy(replay, scenario.limits)?;
    let environment = replay_environment(scenario_environment(scenario), proxy.as_ref());
    let mut argv = proposal.run_argv.clone();
    argv.extend(scenario.argv.clone());
    let outcome = execute_exact(ExecuteExactArgs {
        root: workspace.path(),
        argv: &argv,
        environment: &environment,
        cwd: scenario.cwd.clone(),
        stdin: &scenario_stdin(scenario)?,
        limits: scenario.limits,
    })?;
    let network = finish_replay_proxy(proxy)?;
    observation_from_outcome(workspace.path(), &before, outcome, network)
}

fn verification_build_environment(root: &Path, argv: &[String]) -> Vec<(String, String)> {
    if argv.first().is_some_and(|program| program == "go") {
        vec![
            (
                "GOCACHE".into(),
                root.join(".deshell/verification/go-cache")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "GOMODCACHE".into(),
                root.join(".deshell/verification/go-mod-cache")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("GOENV".into(), "off".into()),
            ("GOMAXPROCS".into(), "2".into()),
        ]
    } else {
        Vec::new()
    }
}

fn verification_validation_environment(root: &Path) -> Vec<(String, String)> {
    verification_build_environment(root, &["go".into()])
}

/// Why the replacement build produced no comparison, or `None` if it built.
///
/// A build that ran out of budget is not a build that failed. Both leave a
/// non-zero status — a timeout leaves 124 — so the status alone cannot tell
/// them apart, and reporting one as the other says the generated code is wrong
/// when what happened is that nothing was learned about it. The budget is
/// checked before the status for exactly that reason.
fn classify_build(build: &crate::agent_process::Outcome, timeout_ms: u64) -> Option<String> {
    if build.timed_out {
        return Some(format!(
            "replacement build reached its {timeout_ms} ms budget, so nothing was compared"
        ));
    }
    if let Some(limit) = &build.limit_exceeded {
        return Some(format!(
            "replacement build reached its {limit} limit, so nothing was compared"
        ));
    }
    if build.exit_code != 0 {
        return Some(format!(
            "replacement build failed with exit {}",
            build.exit_code
        ));
    }
    None
}

/// The budget for building the replacement.
///
/// A scenario's limits bound the script under verification. Building the
/// replacement is de-shell's own toolchain invocation, which happens to share
/// the field — so the two were already separated for memory and for process
/// count, and the omission of time was the one that showed: a `cargo build` on
/// a busy machine ran out of the script's budget and was reported as the
/// replacement failing to build.
fn verification_build_limits(
    argv: &[String],
    mut limits: crate::config::ResourceLimits,
) -> crate::config::ResourceLimits {
    if argv
        .first()
        .is_some_and(|program| matches!(program.as_str(), "cargo" | "go" | "node" | "rustc"))
    {
        limits.memory_bytes = limits.memory_bytes.max(8 * 1024 * 1024 * 1024);
        limits.processes = 60_000;
        limits.timeout_ms = limits.timeout_ms.max(TOOLCHAIN_BUILD_TIMEOUT_MS);
    }
    limits
}

/// How long a compiler is given to build the replacement.
///
/// Not a threshold tuned until a test passed: a scenario's timeout describes
/// the script, and this describes a toolchain. Ten minutes is the same budget
/// the workflows give a build step.
const TOOLCHAIN_BUILD_TIMEOUT_MS: u64 = 600_000;

fn observe_ir(
    root: &Path,
    plan: &crate::ir::Plan,
    scenario: &crate::config::Scenario,
    replay: Option<&crate::replay::ReplayStore>,
) -> Result<(Observation, BTreeSet<String>), String> {
    let workspace = prepared_workspace(root, scenario)?;
    let before = crate::workspace::capture(workspace.path())?;
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| "IR entrypoint task is missing".to_owned())?;
    let proxy = start_replay_proxy(replay, scenario.limits)?;
    let environment = replay_environment(scenario_environment(scenario), proxy.as_ref());
    let variables = environment.iter().cloned().collect::<BTreeMap<_, _>>();
    let arguments = scenario
        .arguments
        .iter()
        .map(|value| (value.name.clone(), value.value.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let outcome = execute_ir_node(
        workspace.path(),
        &task.body,
        &variables,
        &arguments,
        &scenario.argv,
        &scenario_stdin(scenario)?,
        scenario.cwd.as_deref(),
        scenario.limits,
        &mut visited,
    )?;
    let network = finish_replay_proxy(proxy)?;
    Ok((
        observation_from_outcome(workspace.path(), &before, outcome, network)?,
        visited,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "the arguments are a contract record; grouping them into a struct would hide which fields the caller must supply"
)]
fn execute_ir_node(
    root: &Path,
    node: &crate::ir::Node,
    variables: &BTreeMap<String, String>,
    arguments: &BTreeMap<String, String>,
    positional: &[String],
    stdin: &[u8],
    default_cwd: Option<&str>,
    limits: crate::config::ResourceLimits,
    visited: &mut BTreeSet<String>,
) -> Result<crate::agent_process::Outcome, String> {
    visited.insert(node.id.clone());
    match &node.operation {
        crate::ir::Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            let argv = argv
                .iter()
                .map(|value| value.evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty))
                .collect::<Result<Vec<_>, _>>()?;
            let mut process_environment = variables.clone();
            for value in environment {
                process_environment.insert(
                    value.name.clone(),
                    value
                        .value
                        .evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty)?,
                );
            }
            let working_directory = working_directory
                .as_ref()
                .map(|value| value.evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty))
                .transpose()?
                .or_else(|| default_cwd.map(str::to_owned));
            let _ = positional;
            crate::agent_process::execute(
                root,
                crate::agent_process::Request {
                    argv,
                    environment: process_environment.into_iter().collect(),
                    working_directory,
                    stdin: stdin.to_vec(),
                    limits: limits.into(),
                },
            )
        }
        crate::ir::Operation::Sequence { nodes, .. } => {
            let mut aggregate = crate::agent_process::Outcome {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: false,
                limit_exceeded: None,
                signal: None,
            };
            for (index, child) in nodes.iter().enumerate() {
                let result = execute_ir_node(
                    root,
                    child,
                    variables,
                    arguments,
                    positional,
                    if index == 0 { stdin } else { &[] },
                    default_cwd,
                    limits,
                    visited,
                )?;
                aggregate.stdout.extend(result.stdout);
                aggregate.stderr.extend(result.stderr);
                aggregate.exit_code = result.exit_code;
                aggregate.timed_out |= result.timed_out;
                aggregate.signal = result.signal;
                aggregate.limit_exceeded = result.limit_exceeded;
            }
            Ok(aggregate)
        }
        crate::ir::Operation::Pipeline { nodes, status } => {
            let mut requests = Vec::new();
            for (index, child) in nodes.iter().enumerate() {
                visited.insert(child.id.clone());
                requests.push(ir_exec_request(IrExecRequestArgs {
                    _root: root,
                    node: child,
                    variables,
                    arguments,
                    stdin: if index == 0 { stdin } else { &[] },
                    default_cwd,
                    limits,
                })?);
            }
            let outcomes = crate::agent_process::execute_pipeline(root, requests)?;
            let selected = match status {
                crate::ir::PipelineStatus::Last => outcomes.len().saturating_sub(1),
                crate::ir::PipelineStatus::Pipefail => outcomes
                    .iter()
                    .rposition(|outcome| outcome.exit_code != 0)
                    .unwrap_or_else(|| outcomes.len().saturating_sub(1)),
            };
            let mut aggregate = crate::agent_process::Outcome {
                exit_code: outcomes
                    .get(selected)
                    .map_or(0, |outcome| outcome.exit_code),
                stdout: outcomes
                    .last()
                    .map_or_else(Vec::new, |outcome| outcome.stdout.clone()),
                stderr: Vec::new(),
                timed_out: outcomes.iter().any(|outcome| outcome.timed_out),
                limit_exceeded: outcomes
                    .iter()
                    .find_map(|outcome| outcome.limit_exceeded.clone()),
                signal: outcomes.get(selected).and_then(|outcome| outcome.signal),
            };
            for outcome in outcomes {
                aggregate.stderr.extend(outcome.stderr);
            }
            Ok(aggregate)
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            let mut aggregate = execute_ir_node(
                root,
                predicate,
                variables,
                arguments,
                positional,
                stdin,
                default_cwd,
                limits,
                visited,
            )?;
            let branch = if aggregate.exit_code == 0 {
                Some(if_true.as_ref())
            } else {
                if_false.as_deref()
            };
            if let Some(branch) = branch {
                let outcome = execute_ir_node(
                    root,
                    branch,
                    variables,
                    arguments,
                    positional,
                    &[],
                    default_cwd,
                    limits,
                    visited,
                )?;
                aggregate.stdout.extend(outcome.stdout);
                aggregate.stderr.extend(outcome.stderr);
                aggregate.exit_code = outcome.exit_code;
                aggregate.timed_out |= outcome.timed_out;
                aggregate.limit_exceeded = outcome.limit_exceeded;
                aggregate.signal = outcome.signal;
            }
            Ok(aggregate)
        }
        other => Err(format!(
            "independent IR verifier does not support {}",
            other.name()
        )),
    }
}

/// The inputs of [`ir_exec_request`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`ir_exec_request`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct IrExecRequestArgs<'a> {
    _root: &'a Path,
    node: &'a crate::ir::Node,
    variables: &'a BTreeMap<String, String>,
    arguments: &'a BTreeMap<String, String>,
    stdin: &'a [u8],
    default_cwd: Option<&'a str>,
    limits: crate::config::ResourceLimits,
}

fn ir_exec_request(parts: IrExecRequestArgs<'_>) -> Result<crate::agent_process::Request, String> {
    // Destructured without `..`: see `IrExecRequestArgs`.
    let IrExecRequestArgs {
        _root,
        node,
        variables,
        arguments,
        stdin,
        default_cwd,
        limits,
    } = parts;
    let crate::ir::Operation::Exec {
        argv,
        environment,
        working_directory,
    } = &node.operation
    else {
        return Err("independent IR verifier pipeline supports only Exec stages".into());
    };
    let argv = argv
        .iter()
        .map(|value| value.evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty))
        .collect::<Result<Vec<_>, _>>()?;
    let mut process_environment = variables.clone();
    for value in environment {
        process_environment.insert(
            value.name.clone(),
            value
                .value
                .evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty)?,
        );
    }
    let working_directory = working_directory
        .as_ref()
        .map(|value| value.evaluate(variables, arguments, crate::ir::UnsetPolicy::Empty))
        .transpose()?
        .or_else(|| default_cwd.map(str::to_owned));
    Ok(crate::agent_process::Request {
        argv,
        environment: process_environment.into_iter().collect(),
        working_directory,
        stdin: stdin.to_vec(),
        limits: limits.into(),
    })
}

fn prepared_workspace(
    root: &Path,
    scenario: &crate::config::Scenario,
) -> Result<crate::workspace::PrivateWorkspace, String> {
    let workspace = crate::workspace::private_snapshot(root)?;
    crate::workspace::materialize(workspace.path(), &scenario.fixtures)?;
    Ok(workspace)
}

fn start_replay_proxy(
    replay: Option<&crate::replay::ReplayStore>,
    limits: crate::config::ResourceLimits,
) -> Result<Option<crate::replay_proxy::ReplayProxy>, String> {
    replay
        .map(|replay| crate::replay_proxy::ReplayProxy::start(replay, limits.timeout_ms))
        .transpose()
}

fn replay_environment(
    mut environment: Vec<(String, String)>,
    proxy: Option<&crate::replay_proxy::ReplayProxy>,
) -> Vec<(String, String)> {
    let Some(proxy) = proxy else {
        return environment;
    };
    environment.retain(|(name, _)| {
        !["http_proxy", "https_proxy", "no_proxy"]
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
    });
    environment.extend(proxy.environment());
    environment
}

fn finish_replay_proxy(
    proxy: Option<crate::replay_proxy::ReplayProxy>,
) -> Result<Vec<crate::replay::NetworkExchange>, String> {
    proxy.map_or_else(|| Ok(Vec::new()), crate::replay_proxy::ReplayProxy::finish)
}

fn scenario_environment(scenario: &crate::config::Scenario) -> Vec<(String, String)> {
    scenario
        .environment
        .iter()
        .map(|value| (value.name.clone(), value.value.clone()))
        .collect()
}

fn scenario_stdin(scenario: &crate::config::Scenario) -> Result<Vec<u8>, String> {
    scenario
        .stdin
        .as_ref()
        .map(crate::config::BinaryData::bytes)
        .transpose()
        .map(|value| value.unwrap_or_default())
}

/// The inputs of [`execute_exact`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`execute_exact`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ExecuteExactArgs<'a> {
    root: &'a Path,
    argv: &'a [String],
    environment: &'a [(String, String)],
    cwd: Option<String>,
    stdin: &'a [u8],
    limits: crate::config::ResourceLimits,
}

fn execute_exact(parts: ExecuteExactArgs<'_>) -> Result<crate::agent_process::Outcome, String> {
    // Destructured without `..`: see `ExecuteExactArgs`.
    let ExecuteExactArgs {
        root,
        argv,
        environment,
        cwd,
        stdin,
        limits,
    } = parts;
    validate_exact_argv(argv)?;
    let mut argv = argv.to_vec();
    if argv[0].contains('/') && !Path::new(&argv[0]).is_absolute() {
        argv[0] = root.join(&argv[0]).to_string_lossy().into_owned();
    }
    crate::agent_process::execute(
        root,
        crate::agent_process::Request {
            argv,
            environment: environment.to_vec(),
            working_directory: cwd,
            stdin: stdin.to_vec(),
            limits: limits.into(),
        },
    )
}

fn apply_generator_patches(root: &Path, proposal: &Proposal) -> Result<(), String> {
    let canonical = canonical_root(root)?;
    let created_directories = ensure_patch_directories(&canonical, &proposal.patches)?;
    let result = (|| {
        let mut patches = Vec::new();
        for patch in &proposal.patches {
            let path = safe_target(&canonical, &patch.path)?;
            let contents = patch.contents()?;
            patches.push(match patch.operation {
                PatchOperation::Create => {
                    crate::patch::prepare_create(&path, contents, patch.permissions)?
                }
                PatchOperation::Update => crate::patch::prepare_expected(
                    &path,
                    patch
                        .expected_digest
                        .as_deref()
                        .ok_or("update digest missing")?,
                    contents,
                )?,
            });
        }
        crate::patch::apply_all(&patches)
    })();
    if result.is_err() {
        cleanup_empty_directories(&created_directories);
    }
    result
}

fn validate_expectations(
    scenario: &crate::config::Scenario,
    outcome: &crate::agent_process::Outcome,
) -> Result<(), String> {
    if let Some(expected) = scenario.expect.exit_code
        && expected != outcome.exit_code
    {
        return Err(format!(
            "original run for scenario {} expected exit {expected}, found {}: {}",
            scenario.name,
            outcome.exit_code,
            String::from_utf8_lossy(&outcome.stderr).trim()
        ));
    }
    if let Some(expected) = &scenario.expect.stdout
        && expected.bytes()? != outcome.stdout
    {
        return Err("original run did not satisfy expected stdout".into());
    }
    if let Some(expected) = &scenario.expect.stderr
        && expected.bytes()? != outcome.stderr
    {
        return Err("original run did not satisfy expected stderr".into());
    }
    Ok(())
}

fn observation_from_outcome(
    root: &Path,
    before: &crate::workspace::Snapshot,
    outcome: crate::agent_process::Outcome,
    network: Vec<crate::replay::NetworkExchange>,
) -> Result<Observation, String> {
    let after = crate::workspace::capture(root)?;
    Ok(Observation {
        exit_code: outcome.exit_code,
        signal: outcome.signal,
        timed_out: outcome.timed_out,
        stdout_base64: base64::engine::general_purpose::STANDARD.encode(outcome.stdout),
        stderr_base64: base64::engine::general_purpose::STANDARD.encode(outcome.stderr),
        files: file_changes(before, &after),
        network,
    })
}

fn file_changes(
    before: &crate::workspace::Snapshot,
    after: &crate::workspace::Snapshot,
) -> Vec<FileChange> {
    let before = before
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect::<BTreeMap<_, _>>();
    before
        .keys()
        .chain(after.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| match (before.get(path), after.get(path)) {
            (None, Some(file)) => Some(FileChange {
                path: path.into(),
                kind: FileChangeKind::Created,
                before_sha256: None,
                after_sha256: Some(file.sha256.clone()),
                before_executable: None,
                after_executable: Some(file.executable),
            }),
            (Some(file), None) => Some(FileChange {
                path: path.into(),
                kind: FileChangeKind::Removed,
                before_sha256: Some(file.sha256.clone()),
                after_sha256: None,
                before_executable: Some(file.executable),
                after_executable: None,
            }),
            (Some(left), Some(right))
                if left.sha256 != right.sha256 || left.executable != right.executable =>
            {
                Some(FileChange {
                    path: path.into(),
                    kind: FileChangeKind::Modified,
                    before_sha256: Some(left.sha256.clone()),
                    after_sha256: Some(right.sha256.clone()),
                    before_executable: Some(left.executable),
                    after_executable: Some(right.executable),
                })
            }
            _ => None,
        })
        .collect()
}

fn compare_three(
    original: &Observation,
    ir: &Observation,
    replacement: &Observation,
) -> Vec<String> {
    let mut output = Vec::new();
    for (name, left, right) in [("ir", original, ir), ("replacement", original, replacement)] {
        if left.exit_code != right.exit_code {
            output.push(format!("{name}.exit_code"));
        }
        if left.signal != right.signal {
            output.push(format!("{name}.signal"));
        }
        if left.timed_out != right.timed_out {
            output.push(format!("{name}.timeout"));
        }
        if left.stdout_base64 != right.stdout_base64 {
            output.push(format!("{name}.stdout"));
        }
        if left.stderr_base64 != right.stderr_base64 {
            output.push(format!("{name}.stderr"));
        }
        if left.files != right.files {
            output.push(format!("{name}.files"));
        }
        if left.network != right.network {
            output.push(format!("{name}.network"));
        }
    }
    output
}

fn subject_changed<'a>(
    comparisons: &'a [TripleComparison],
    select: impl Fn(&'a TripleComparison) -> &'a Observation,
) -> bool {
    comparisons.first().is_some_and(|first| {
        comparisons
            .iter()
            .skip(1)
            .any(|next| select(next) != select(first))
    })
}

fn verify_validation_commands(
    root: &Path,
    directory: &Path,
    plan: &MigrationPlan,
) -> Result<Vec<ValidationEvidence>, String> {
    if plan.validation_commands.is_empty() {
        return Ok(Vec::new());
    }
    let workspace = crate::workspace::private_snapshot(root)?;
    let _ = ensure_retirement_directories(workspace.path())?;
    let _ = ensure_plan_patch_directories(workspace.path(), directory, plan)?;
    let retirement = prepare_retirement(workspace.path(), directory, plan)?;
    crate::patch::apply_all(&retirement)?;
    require_shell_free_tree(workspace.path(), "validation")?;
    ensure_safe_directory(workspace.path(), ".deshell/verification")?;
    // A project-native validation entrypoint can transitively invoke the generated Go
    // command (for example a rewritten Python call site), so the isolated Go caches
    // belong to the validation cell rather than only commands whose argv starts with Go.
    let validation_environment = verification_validation_environment(workspace.path());
    let mut output = Vec::new();
    for command in &plan.validation_commands {
        let outcome = execute_exact(ExecuteExactArgs {
            root: workspace.path(),
            argv: &command.argv,
            environment: &validation_environment,
            cwd: None,
            stdin: &[],
            limits: plan.validation_limits,
        })?;
        output.push(ValidationEvidence {
            name: command.name.clone(),
            argv: command.argv.clone(),
            exit_code: outcome.exit_code,
            stdout_digest: crate::digest::sha256(&outcome.stdout),
            stderr_digest: crate::digest::sha256(&outcome.stderr),
        });
    }
    Ok(output)
}

fn validate_evidence_document(evidence: &MigrationEvidence) -> Result<(), String> {
    if evidence.schema_version != 1 || evidence.repetitions < 2 {
        return Err(
            "migration Evidence requires schema version 1 and at least two repetitions".into(),
        );
    }
    if !crate::digest::valid_sha256(&evidence.plan_digest) || evidence.cell.trim().is_empty() {
        return Err("migration Evidence plan digest or cell is invalid".into());
    }
    if evidence.approval_digests.is_empty()
        || evidence
            .approval_digests
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        || evidence
            .approval_digests
            .iter()
            .any(|digest| !crate::digest::valid_pinned_sha256(digest))
    {
        return Err(
            "migration Evidence approval digests must be non-empty, sorted, unique, and pinned"
                .into(),
        );
    }
    if evidence.checks.is_empty() {
        return Err("migration Evidence contains no source/scenario check".into());
    }
    for check in &evidence.checks {
        match check.status {
            EvidenceStatus::Unavailable | EvidenceStatus::Failed => {
                if !check.comparisons.is_empty() || check.error.as_deref().is_none_or(str::is_empty)
                {
                    return Err("unavailable/failed migration Evidence requires an error and no fabricated comparisons".into());
                }
            }
            EvidenceStatus::Verified
            | EvidenceStatus::Different
            | EvidenceStatus::Nondeterministic => {
                if check.comparisons.len() != evidence.repetitions as usize || check.error.is_some()
                {
                    return Err(
                        "migration Evidence repetition count or error does not match status".into(),
                    );
                }
            }
        }
        for digest in [
            &check.key.source_digest,
            &check.key.ir_digest,
            &check.key.proposal_digest,
            &check.key.generator_digest,
            &check.key.toolchain_digest,
            &check.key.scenario_digest,
            &check.key.platform_fingerprint,
            &check.key.runtime_fingerprint,
        ] {
            if !crate::digest::valid_sha256(digest) {
                return Err("migration Evidence key contains an invalid digest".into());
            }
        }
        if check.status == EvidenceStatus::Verified
            && check
                .comparisons
                .iter()
                .any(|comparison| !comparison.differences.is_empty())
        {
            return Err("verified migration Evidence contains differences".into());
        }
        for comparison in &check.comparisons {
            for observation in [
                &comparison.original,
                &comparison.ir,
                &comparison.replacement,
            ] {
                validate_network_exchanges(&observation.network)?;
            }
        }
    }
    Ok(())
}

fn validate_network_exchanges(exchanges: &[crate::replay::NetworkExchange]) -> Result<(), String> {
    for (sequence, exchange) in exchanges.iter().enumerate() {
        if exchange.sequence != sequence as u64
            || exchange.method.is_empty()
            || !exchange
                .method
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
            || !exchange.uri.starts_with("http://")
            || !crate::digest::valid_sha256(&exchange.request_body_sha256)
            || !crate::digest::valid_sha256(&exchange.response_body_sha256)
            || !(100..=599).contains(&exchange.status)
        {
            return Err("migration Evidence contains an invalid network replay sequence".into());
        }
    }
    Ok(())
}

impl MigrationEvidence {
    pub(crate) fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        validate_evidence_document(self)?;
        pretty_bytes(self)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        let evidence: Self = crate::strict_json::decode(bytes)?;
        validate_evidence_document(&evidence)?;
        Ok(evidence)
    }
}

pub(crate) fn import_evidence(
    root: &Path,
    digest: &str,
    files: &[PathBuf],
) -> Result<Vec<String>, String> {
    let (directory, plan) = load_plan(root, digest)?;
    let evidence_directory = directory.join("evidence");
    let metadata = evidence_directory
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", evidence_directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err("migration Evidence directory is unsafe".into());
    }
    let mut imported = Vec::new();
    for file in files {
        let metadata = file
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect Evidence {}: {error}", file.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > 16 * 1024 * 1024
        {
            return Err(format!(
                "Evidence input must be a regular non-symlink file of at most 16 MiB: {}",
                file.display()
            ));
        }
        let bytes = std::fs::read(file)
            .map_err(|error| format!("cannot read Evidence {}: {error}", file.display()))?;
        let evidence = MigrationEvidence::decode(&bytes)?;
        validate_evidence_against(root, &directory, &plan, &evidence)?;
        let canonical = evidence.encode_pretty()?;
        reject_conflicting_cell_evidence(&evidence_directory, &evidence, &canonical)?;
        let evidence_digest = crate::digest::sha256(&canonical);
        persist_immutable(
            &evidence_directory.join(format!("{}-{evidence_digest}.json", evidence.cell)),
            canonical,
        )?;
        imported.push(evidence_digest);
    }
    Ok(imported)
}

fn reject_conflicting_cell_evidence(
    directory: &Path,
    incoming: &MigrationEvidence,
    canonical: &[u8],
) -> Result<(), String> {
    let mut paths = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect Evidence {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > 16 * 1024 * 1024
        {
            return Err("DESHELL_BLOCKER_EVIDENCE_INVALID: imported Evidence is unsafe".into());
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read Evidence {}: {error}", path.display()))?;
        let existing = MigrationEvidence::decode(&bytes)
            .map_err(|error| format!("DESHELL_BLOCKER_EVIDENCE_INVALID: {error}"))?;
        if existing.cell == incoming.cell && bytes != canonical {
            return Err(format!(
                "DESHELL_BLOCKER_EVIDENCE_CONFLICT: cell {} already has different verified Evidence",
                incoming.cell
            ));
        }
    }
    Ok(())
}

fn validate_evidence_against(
    root: &Path,
    directory: &Path,
    plan: &MigrationPlan,
    evidence: &MigrationEvidence,
) -> Result<(), String> {
    validate_current_plan_policy(root, plan)?;
    if evidence.plan_digest != plan.plan_digest {
        return Err("Evidence plan digest does not match the selected plan".into());
    }
    if evidence.approval_digests != plan_approval_digests(plan) {
        return Err("Evidence approval digests do not match the selected plan".into());
    }
    let cell = plan
        .required_cells
        .iter()
        .find(|cell| cell.id == evidence.cell)
        .ok_or("Evidence cell is not approved by the selected plan")?;
    let scenarios = load_approved_scenario_values(root)?;
    let toolchain_digest = crate::digest::sha256(
        &std::fs::read(crate::project::project_file_path(root, "deshell.lock")?)
            .map_err(|error| format!("cannot read deshell.lock: {error}"))?,
    );
    let mut expected = BTreeSet::new();
    for source in &plan.sources {
        let proposal_digest = source
            .proposal_digest
            .as_deref()
            .ok_or("plan source has no proposal")?;
        let proposal = load_proposal(directory, proposal_digest)?;
        validate_current_source(root, source)?;
        for scenario in &plan.required_scenarios {
            let current = scenarios
                .get(&scenario.name)
                .ok_or_else(|| format!("approved scenario disappeared: {}", scenario.name))?;
            if current.digest()? != scenario.digest {
                return Err(format!("approved scenario became stale: {}", scenario.name));
            }
            if current.approval_digest != scenario.approval_digest {
                return Err(format!(
                    "approved scenario approval became stale: {}",
                    scenario.name
                ));
            }
            expected.insert((source.location.clone(), scenario.name.clone()));
            let check = evidence
                .checks
                .iter()
                .find(|check| check.source == source.location && check.scenario == scenario.name)
                .ok_or_else(|| {
                    format!(
                        "Evidence omitted {} for scenario {}",
                        source.location.path, scenario.name
                    )
                })?;
            let generator_digest = proposal
                .generator_digest
                .strip_prefix("sha256:")
                .unwrap_or(&proposal.generator_digest);
            let expected_key = EvidenceKey {
                source_digest: source.content_digest.clone(),
                ir_digest: source.ir_digest.clone(),
                proposal_digest: proposal_digest.into(),
                generator_digest: generator_digest.into(),
                toolchain_digest: toolchain_digest.clone(),
                scenario_digest: scenario.digest.clone(),
                platform_fingerprint: cell.platform_fingerprint.clone(),
                runtime_fingerprint: cell.runtime_fingerprint.clone(),
            };
            if check.key != expected_key {
                return Err(format!(
                    "Evidence key is stale for {} and scenario {}",
                    source.location.path, scenario.name
                ));
            }
        }
    }
    let actual = evidence
        .checks
        .iter()
        .map(|check| (check.source.clone(), check.scenario.clone()))
        .collect::<BTreeSet<_>>();
    if actual != expected || actual.len() != evidence.checks.len() {
        return Err(
            "Evidence contains missing, duplicate, or unexpected source/scenario checks".into(),
        );
    }
    let expected_validation = plan
        .validation_commands
        .iter()
        .map(|command| (command.name.as_str(), command.argv.as_slice()))
        .collect::<Vec<_>>();
    let actual_validation = evidence
        .validation
        .iter()
        .map(|command| (command.name.as_str(), command.argv.as_slice()))
        .collect::<Vec<_>>();
    if expected_validation != actual_validation {
        return Err("Evidence validation command set does not match the plan".into());
    }
    Ok(())
}

pub(crate) fn load_plan(root: &Path, digest: &str) -> Result<(PathBuf, MigrationPlan), String> {
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    if !crate::digest::valid_sha256(digest) {
        return Err("migration plan selector must be a SHA-256 digest".into());
    }
    let root = canonical_root(root)?;
    let directory = root.join(format!(".deshell/migrations/sha256/{digest}"));
    let metadata = directory
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect migration plan {digest}: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!("migration plan directory is unsafe: {digest}"));
    }
    let bytes = std::fs::read(directory.join("plan.json"))
        .map_err(|error| format!("cannot read migration plan {digest}: {error}"))?;
    let plan = MigrationPlan::decode(&bytes)?;
    if plan.plan_digest != digest {
        return Err("selected migration plan digest does not match its content".into());
    }
    Ok((directory, plan))
}

pub(crate) fn apply(root: &Path, digest: &str) -> Result<(), String> {
    let (directory, plan) = load_plan(root, digest)?;
    if !plan.blockers.is_empty() {
        return Err(plan
            .blockers
            .iter()
            .map(|blocker| format!("{}: {}", blocker.code, blocker.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    validate_current_plan_policy(root, &plan)?;
    let evidence = load_complete_evidence(root, &directory, &plan)?;
    validate_node_coverage(&directory, &plan, &evidence)?;

    // Rehearse the exact retirement on a private snapshot before touching the
    // live tree. The scanner sees generated project-native code but ignores
    // the content-addressed non-execution archive under .deshell.
    let staged = crate::workspace::private_snapshot(root)?;
    let _ = ensure_retirement_directories(staged.path())?;
    let _ = ensure_plan_patch_directories(staged.path(), &directory, &plan)?;
    let staged_proposals = prepare_retirement(staged.path(), &directory, &plan)?;
    crate::patch::apply_all(&staged_proposals)?;
    require_shell_free_tree(staged.path(), "staged")?;

    let canonical = canonical_root(root)?;
    let mut created_directories = ensure_retirement_directories(&canonical)?;
    match ensure_plan_patch_directories(&canonical, &directory, &plan) {
        Ok(mut created) => created_directories.append(&mut created),
        Err(error) => {
            cleanup_empty_directories(&created_directories);
            return Err(error);
        }
    }
    let proposals = match prepare_retirement(&canonical, &directory, &plan) {
        Ok(proposals) => proposals,
        Err(error) => {
            cleanup_empty_directories(&created_directories);
            return Err(error);
        }
    };
    let before = capture_before_states(&proposals)?;
    if let Err(error) = crate::patch::apply_all(&proposals) {
        cleanup_empty_directories(&created_directories);
        return Err(error);
    }
    if let Err(scan_error) = require_shell_free_tree(&canonical, "post-apply") {
        let rollback_error = rollback_retirement(&proposals, &before).err();
        cleanup_empty_directories(&created_directories);
        return Err(match rollback_error {
            Some(rollback) => format!("{scan_error}; rollback failed: {rollback}"),
            None => format!("{scan_error}; all retirement changes were rolled back"),
        });
    }
    Ok(())
}

fn validate_current_plan_policy(root: &Path, plan: &MigrationPlan) -> Result<(), String> {
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    let validation_commands = config
        .validation_commands
        .iter()
        .map(|command| ExactCommand {
            name: command.name.clone(),
            kind: command.kind,
            argv: command.argv.clone(),
        })
        .collect::<Vec<_>>();
    if validation_commands != plan.validation_commands {
        return Err(
            "DESHELL_BLOCKER_STALE_VALIDATION_POLICY: exact build/test argv changed after plan creation"
                .into(),
        );
    }
    if config.limits != plan.validation_limits {
        return Err(
            "DESHELL_BLOCKER_STALE_VALIDATION_LIMITS: validation resource limits changed after plan creation"
                .into(),
        );
    }
    if approved_cells(root, &config)? != plan.required_cells {
        return Err(
            "DESHELL_BLOCKER_STALE_PLATFORM_MATRIX: approved platform/runtime cells changed after plan creation"
                .into(),
        );
    }
    if let Some(expected) = &plan.network_replay_digest {
        let (_, actual) = load_network_replay(root)
            .map_err(|message| format!("DESHELL_BLOCKER_STALE_NETWORK_REPLAY: {message}"))?;
        if &actual != expected {
            return Err(
                "DESHELL_BLOCKER_STALE_NETWORK_REPLAY: replay bytes changed after plan creation"
                    .into(),
            );
        }
    }
    Ok(())
}

fn load_complete_evidence(
    root: &Path,
    directory: &Path,
    plan: &MigrationPlan,
) -> Result<Vec<MigrationEvidence>, String> {
    let evidence_directory = directory.join("evidence");
    let mut paths = std::fs::read_dir(&evidence_directory)
        .map_err(|error| format!("cannot read {}: {error}", evidence_directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut documents = Vec::new();
    let mut cell_documents = BTreeMap::new();
    for path in paths {
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect Evidence {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > 16 * 1024 * 1024
        {
            return Err(format!(
                "DESHELL_BLOCKER_EVIDENCE_INVALID: imported Evidence is unsafe: {}",
                path.display()
            ));
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read Evidence {}: {error}", path.display()))?;
        let document = MigrationEvidence::decode(&bytes)?;
        let canonical = document.encode_pretty()?;
        let content_digest = crate::digest::sha256(&canonical);
        let expected_name = format!("{}-{content_digest}.json", document.cell);
        if canonical != bytes
            || path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
        {
            return Err(format!(
                "DESHELL_BLOCKER_EVIDENCE_INVALID: Evidence content address is invalid for {}",
                path.display()
            ));
        }
        if cell_documents
            .insert(document.cell.clone(), content_digest)
            .is_some()
        {
            return Err(format!(
                "DESHELL_BLOCKER_EVIDENCE_CONFLICT: cell {} has multiple verified Evidence documents",
                document.cell
            ));
        }
        validate_evidence_against(root, directory, plan, &document)?;
        if document.repetitions < 2
            || document.status != EvidenceStatus::Verified
            || document.checks.iter().any(|check| {
                check.status != EvidenceStatus::Verified
                    || check
                        .comparisons
                        .iter()
                        .any(|comparison| !comparison.differences.is_empty())
            })
            || document
                .validation
                .iter()
                .any(|command| command.exit_code != 0)
        {
            return Err(format!(
                "DESHELL_BLOCKER_EVIDENCE_DIFFERENCE: cell {} is not completely verified",
                document.cell
            ));
        }
        documents.push(document);
    }
    let present = documents
        .iter()
        .map(|document| document.cell.as_str())
        .collect::<BTreeSet<_>>();
    let missing = plan
        .required_cells
        .iter()
        .filter(|cell| !present.contains(cell.id.as_str()))
        .map(|cell| cell.id.clone())
        .collect::<Vec<_>>();
    if documents.is_empty() || !missing.is_empty() {
        return Err(format!(
            "DESHELL_BLOCKER_EVIDENCE_INCOMPLETE: missing verified cells: {}",
            if missing.is_empty() {
                "<all>".into()
            } else {
                missing.join(", ")
            }
        ));
    }
    Ok(documents)
}

fn validate_node_coverage(
    directory: &Path,
    plan: &MigrationPlan,
    documents: &[MigrationEvidence],
) -> Result<(), String> {
    for cell in &plan.required_cells {
        for source in &plan.sources {
            let ir = load_ir(directory, &source.ir_digest)?;
            let task = ir
                .tasks
                .iter()
                .find(|task| task.name == ir.entrypoint)
                .ok_or("IR entrypoint task is missing")?;
            let mut required = Vec::new();
            collect_node_ids(&task.body, &mut required);
            let covered = documents
                .iter()
                .filter(|document| document.cell == cell.id)
                .flat_map(|document| document.checks.iter())
                .filter(|check| check.source == source.location)
                .flat_map(|check| check.covered_nodes.iter().cloned())
                .collect::<BTreeSet<_>>();
            let missing = required
                .into_iter()
                .filter(|node| !covered.contains(node))
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(format!(
                    "DESHELL_BLOCKER_COVERAGE_INCOMPLETE: {} in cell {} omitted nodes {}",
                    source.location.path,
                    cell.id,
                    missing.join(", ")
                ));
            }
        }
    }
    Ok(())
}

fn ensure_retirement_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let deshell = root.join(".deshell");
    let metadata = deshell
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", deshell.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(".deshell is not a regular directory".into());
    }
    let mut created = Vec::new();
    for path in [deshell.join("archive"), deshell.join("archive/sha256")] {
        match path.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => {
                cleanup_empty_directories(&created);
                return Err(format!(
                    "retirement directory is not a regular directory: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match crate::patch::ensure_directory(&path) {
                    Ok(crate::patch::DirectoryState::Created) => created.push(path),
                    // A concurrent writer created it, so rollback must leave it alone.
                    Ok(crate::patch::DirectoryState::Existing) => {}
                    Err(crate::patch::DirectoryError::Occupied) => {
                        cleanup_empty_directories(&created);
                        return Err(format!(
                            "retirement directory is not a regular directory: {}",
                            path.display()
                        ));
                    }
                    Err(crate::patch::DirectoryError::Io(error)) => {
                        cleanup_empty_directories(&created);
                        return Err(format!("cannot create {}: {error}", path.display()));
                    }
                }
            }
            Err(error) => {
                cleanup_empty_directories(&created);
                return Err(format!("cannot inspect {}: {error}", path.display()));
            }
        }
    }
    Ok(created)
}

fn ensure_patch_directories(
    root: &Path,
    patches: &[GeneratorPatch],
) -> Result<Vec<PathBuf>, String> {
    let mut created = Vec::new();
    let mut seen = BTreeSet::new();
    for patch in patches {
        let normalized = crate::ir::normalize_path(&patch.path)?;
        if normalized != patch.path {
            cleanup_empty_directories(&created);
            return Err(format!(
                "generator patch path is not normalized: {}",
                patch.path
            ));
        }
        let components = patch.path.split('/').collect::<Vec<_>>();
        let mut current = root.to_path_buf();
        for component in components.iter().take(components.len().saturating_sub(1)) {
            current.push(component);
            if !seen.insert(current.clone()) {
                continue;
            }
            match current.symlink_metadata() {
                Ok(metadata)
                    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => {
                    cleanup_empty_directories(&created);
                    return Err(format!(
                        "generator patch parent is not a regular directory: {}",
                        current.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match crate::patch::ensure_directory(&current) {
                        Ok(crate::patch::DirectoryState::Created) => created.push(current.clone()),
                        // A concurrent writer created it, so rollback must leave it alone.
                        Ok(crate::patch::DirectoryState::Existing) => {}
                        Err(crate::patch::DirectoryError::Occupied) => {
                            cleanup_empty_directories(&created);
                            return Err(format!(
                                "generator patch parent is not a regular directory: {}",
                                current.display()
                            ));
                        }
                        Err(crate::patch::DirectoryError::Io(error)) => {
                            cleanup_empty_directories(&created);
                            return Err(format!("cannot create {}: {error}", current.display()));
                        }
                    }
                }
                Err(error) => {
                    cleanup_empty_directories(&created);
                    return Err(format!("cannot inspect {}: {error}", current.display()));
                }
            }
        }
    }
    Ok(created)
}

fn ensure_plan_patch_directories(
    root: &Path,
    plan_directory: &Path,
    plan: &MigrationPlan,
) -> Result<Vec<PathBuf>, String> {
    let mut patches = Vec::new();
    for digest in &plan.proposals {
        patches.extend(load_proposal(plan_directory, digest)?.patches);
    }
    ensure_patch_directories(root, &patches)
}

fn cleanup_empty_directories(paths: &[PathBuf]) {
    for path in paths.iter().rev() {
        crate::patch::remove_empty_directory(path);
    }
}

fn prepare_retirement(
    root: &Path,
    plan_directory: &Path,
    plan: &MigrationPlan,
) -> Result<Vec<crate::patch::Proposal>, String> {
    let root = canonical_root(root)?;
    let mut proposals = Vec::new();
    for digest in &plan.proposals {
        let proposal = load_proposal(plan_directory, digest)?;
        for patch in &proposal.patches {
            let target = safe_target(&root, &patch.path)?;
            let contents = patch.contents()?;
            proposals.push(match patch.operation {
                PatchOperation::Create => {
                    crate::patch::prepare_create(&target, contents, patch.permissions)?
                }
                PatchOperation::Update => crate::patch::prepare_expected(
                    &target,
                    patch
                        .expected_digest
                        .as_deref()
                        .ok_or("update proposal omitted expected digest")?,
                    contents,
                )?,
            });
        }
    }

    let mut archive = load_archive_manifest(&root, &plan.plan_digest)?;
    archive.plan_digest = plan.plan_digest.clone();
    let mut retired_paths = BTreeSet::new();
    let mut retired_locations = BTreeSet::new();
    let mut scheduled_archive_blobs = BTreeSet::new();
    for source in &plan.sources {
        if !retired_locations.insert(source.location.clone()) {
            return Err(format!(
                "duplicate retirement source: {}@{}..{}",
                source.location.path, source.location.start_byte, source.location.end_byte
            ));
        }
        retired_paths.insert(source.location.path.clone());
        let (_, source_path) = crate::project::resolve_entry(&root, &source.location.path)?;
        let bytes = match source.kind {
            SourceKind::ShellFile => {
                let bytes = std::fs::read(&source_path)
                    .map_err(|error| format!("cannot read {}: {error}", source_path.display()))?;
                if source.location.start_byte != 0
                    || source.location.end_byte != bytes.len() as u64
                    || crate::digest::sha256(&bytes) != source.content_digest
                {
                    return Err(format!(
                        "DESHELL_BLOCKER_STALE_SOURCE: {} changed after planning",
                        source.location.path
                    ));
                }
                bytes
            }
            SourceKind::EmbeddedShell => current_embedded_source(&root, source)?.0,
        };
        let archive_relative = format!(".deshell/archive/sha256/{}", source.content_digest);
        let archive_path = root.join(&archive_relative);
        match archive_path.symlink_metadata() {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                let existing = std::fs::read(&archive_path)
                    .map_err(|error| format!("cannot read {}: {error}", archive_path.display()))?;
                if existing != bytes {
                    return Err(format!(
                        "content-addressed archive collision at {}",
                        archive_path.display()
                    ));
                }
            }
            Ok(_) => {
                return Err(format!(
                    "archive target is not a regular file: {}",
                    archive_path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if scheduled_archive_blobs.insert(archive_relative.clone()) {
                    proposals.push(crate::patch::prepare_create(&archive_path, bytes, 0o444)?);
                }
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect archive target {}: {error}",
                    archive_path.display()
                ));
            }
        }
        let entry = ArchiveEntry {
            original: source.location.clone(),
            plan_digest: plan.plan_digest.clone(),
            kind: source.kind,
            content_digest: source.content_digest.clone(),
            archive_path: archive_relative,
        };
        match archive
            .entries
            .iter()
            .find(|existing| existing.original == entry.original)
        {
            Some(existing) if existing == &entry => {}
            Some(_) => {
                return Err(format!(
                    "archive manifest has conflicting source location: {}",
                    source.location.path
                ));
            }
            None => archive.entries.push(entry),
        }
        if source.kind == SourceKind::ShellFile {
            proposals.push(crate::patch::prepare_delete(&source_path)?);
        }
    }
    archive.plan_digest.clone_from(&plan.plan_digest);
    archive.entries.sort_by(|left, right| {
        left.original
            .cmp(&right.original)
            .then_with(|| left.content_digest.cmp(&right.content_digest))
    });
    validate_archive_manifest(&archive)?;
    prepare_changed_or_created(
        &root.join(".deshell/archive/manifest.json"),
        pretty_bytes(&archive)?,
        0o644,
        &mut proposals,
    )?;

    let mut config = crate::project::load_config(&root).map_err(|errors| errors.join("; "))?;
    config
        .entrypoints
        .retain(|entry| !retired_paths.contains(entry));
    config
        .location_overrides
        .retain(|entry| !retired_paths.contains(&entry.path));
    prepare_changed_or_created(
        &root.join(".deshell/project.toml"),
        config.encode_pretty()?,
        0o644,
        &mut proposals,
    )?;

    let mut manifest = crate::project::load_manifest(&root).map_err(|errors| errors.join("; "))?;
    manifest
        .entries
        .retain(|entry| !retired_paths.contains(&entry.entrypoint));
    prepare_changed_or_created(
        &root.join(".deshell/manifest.json"),
        manifest.encode_pretty()?,
        0o644,
        &mut proposals,
    )?;

    let marker = pretty_bytes(&serde_json::json!({
        "schema_version": 1,
        "plan_digest": plan.plan_digest,
    }))?;
    for name in ["verified.json", "retired.json"] {
        prepare_changed_or_created(
            &root.join(format!(
                ".deshell/migrations/sha256/{}/{name}",
                plan.plan_digest
            )),
            marker.clone(),
            0o444,
            &mut proposals,
        )?;
    }
    Ok(proposals)
}

fn load_archive_manifest(root: &Path, plan_digest: &str) -> Result<ArchiveManifest, String> {
    let path = root.join(".deshell/archive/manifest.json");
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let manifest: ArchiveManifest = crate::strict_json::decode(
                &std::fs::read(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
            )?;
            validate_archive_manifest(&manifest)?;
            Ok(manifest)
        }
        Ok(_) => Err(format!(
            "archive manifest is not a regular file: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ArchiveManifest {
            schema_version: 1,
            plan_digest: plan_digest.into(),
            entries: Vec::new(),
        }),
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn validate_archive_manifest(manifest: &ArchiveManifest) -> Result<(), String> {
    if manifest.schema_version != 1 || !crate::digest::valid_sha256(&manifest.plan_digest) {
        return Err("archive manifest version or plan digest is invalid".into());
    }
    let mut locations = BTreeSet::new();
    for entry in &manifest.entries {
        if !locations.insert(entry.original.clone())
            || !crate::digest::valid_sha256(&entry.plan_digest)
            || !crate::digest::valid_sha256(&entry.content_digest)
            || entry.archive_path != format!(".deshell/archive/sha256/{}", entry.content_digest)
        {
            return Err("archive manifest contains an invalid or duplicate entry".into());
        }
    }
    Ok(())
}

fn prepare_changed_or_created(
    path: &Path,
    replacement: Vec<u8>,
    permissions: u32,
    proposals: &mut Vec<crate::patch::Proposal>,
) -> Result<(), String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let current = std::fs::read(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            if current != replacement {
                proposals.push(crate::patch::prepare_expected(
                    path,
                    &crate::digest::sha256(&current),
                    replacement,
                )?);
            }
            Ok(())
        }
        Ok(_) => Err(format!(
            "retirement target is not a regular file: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            proposals.push(crate::patch::prepare_create(
                path,
                replacement,
                permissions,
            )?);
            Ok(())
        }
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn require_shell_free_tree(root: &Path, phase: &str) -> Result<(), String> {
    #[cfg(test)]
    if phase == "post-apply" && FORCE_POST_SCAN_FAILURE.with(|flag| flag.replace(false)) {
        return Err("DESHELL_BLOCKER_POST_SCAN: injected post-apply scan failure".into());
    }
    let inventory = crate::project::scan(root)?;
    if inventory.findings.is_empty() && inventory.skipped.is_empty() && inventory.errors.is_empty()
    {
        return Ok(());
    }
    Err(format!(
        "DESHELL_BLOCKER_POST_SCAN: {phase} scan found {} shell findings, {} unresolved candidates, and {} errors",
        inventory.findings.len(),
        inventory.skipped.len(),
        inventory.errors.len()
    ))
}

#[cfg(test)]
std::thread_local! {
    static FORCE_POST_SCAN_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(test, unix))]
fn force_post_scan_failure_once() {
    FORCE_POST_SCAN_FAILURE.with(|flag| flag.set(true));
}

#[derive(Clone)]
enum BeforeState {
    Missing,
    Existing { bytes: Vec<u8>, permissions: u32 },
}

fn capture_before_states(
    proposals: &[crate::patch::Proposal],
) -> Result<BTreeMap<PathBuf, BeforeState>, String> {
    let mut states = BTreeMap::new();
    for proposal in proposals {
        let state = match proposal.path.symlink_metadata() {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                BeforeState::Existing {
                    bytes: std::fs::read(&proposal.path).map_err(|error| {
                        format!("cannot read {}: {error}", proposal.path.display())
                    })?,
                    permissions: retirement_permissions(&metadata),
                }
            }
            Ok(_) => {
                return Err(format!(
                    "retirement target is not a regular file: {}",
                    proposal.path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BeforeState::Missing,
            Err(error) => {
                return Err(format!(
                    "cannot inspect {}: {error}",
                    proposal.path.display()
                ));
            }
        };
        states.insert(proposal.path.clone(), state);
    }
    Ok(states)
}

fn rollback_retirement(
    applied: &[crate::patch::Proposal],
    before: &BTreeMap<PathBuf, BeforeState>,
) -> Result<(), String> {
    for proposal in applied {
        if proposal.deletes() {
            if proposal.path.symlink_metadata().is_ok() {
                return Err(format!(
                    "concurrent edit appeared at deleted path {}",
                    proposal.path.display()
                ));
            }
        } else {
            let metadata = proposal.path.symlink_metadata().map_err(|error| {
                format!(
                    "applied path disappeared before rollback {}: {error}",
                    proposal.path.display()
                )
            })?;
            if metadata.file_type().is_symlink()
                || !metadata.file_type().is_file()
                || crate::digest::sha256(
                    &std::fs::read(&proposal.path).map_err(|error| {
                        format!("cannot read {}: {error}", proposal.path.display())
                    })?,
                ) != crate::digest::sha256(&proposal.replacement)
            {
                return Err(format!(
                    "concurrent edit detected at {}",
                    proposal.path.display()
                ));
            }
        }
    }
    let mut rollback = Vec::new();
    for (path, state) in before {
        match state {
            BeforeState::Missing => {
                if path.symlink_metadata().is_ok() {
                    rollback.push(crate::patch::prepare_delete(path)?);
                }
            }
            BeforeState::Existing { bytes, permissions } => match path.symlink_metadata() {
                Ok(_) => {
                    let mut proposal = crate::patch::prepare(path, bytes.clone())?;
                    proposal.permissions = *permissions;
                    rollback.push(proposal);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => rollback.push(
                    crate::patch::prepare_create(path, bytes.clone(), *permissions)?,
                ),
                Err(error) => {
                    return Err(format!("cannot inspect {}: {error}", path.display()));
                }
            },
        }
    }
    crate::patch::apply_all(&rollback)
}

#[cfg(unix)]
fn retirement_permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn retirement_permissions(_metadata: &std::fs::Metadata) -> u32 {
    0o644
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActiveState {
    #[default]
    None,
    Blocked,
    Planned,
    Verified,
    Retired,
    Stale,
}

impl ActiveState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Blocked => "blocked",
            Self::Planned => "planned",
            Self::Verified => "verified",
            Self::Retired => "retired",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Status {
    pub schema_version: u32,
    pub live: usize,
    pub active_plan: Option<String>,
    pub active_state: ActiveState,
    pub blocked: usize,
    pub planned: usize,
    pub verified: usize,
    pub retired: usize,
    pub stale: usize,
    pub history: usize,
    pub superseded: usize,
    pub archived: usize,
    pub required_cells: Vec<String>,
    pub remaining_evidence: usize,
    pub next: String,
    pub next_argv: Vec<String>,
}

pub(crate) fn status(root: &Path) -> Result<Status, String> {
    let inventory = crate::project::scan(root)?;
    let canonical = canonical_root(root)?;
    let base = canonical.join(".deshell/migrations/sha256");
    let mut status = Status {
        schema_version: 1,
        live: inventory.findings.len(),
        next: String::new(),
        ..Status::default()
    };
    let mut next_cell = None;
    let mut needs_scenario_approval = false;
    let mut needs_matrix_approval = false;
    let mut plan_count = 0_usize;
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let Ok(bytes) = std::fs::read(path.join("plan.json")) else {
                continue;
            };
            let Ok(_plan) = MigrationPlan::decode(&bytes) else {
                continue;
            };
            plan_count += 1;
        }
    }
    status.history = plan_count;
    if let Some(index) = load_active_index(root)? {
        let (directory, plan) = load_plan(root, &index.active_plan_digest)?;
        status.active_plan = Some(plan.plan_digest.clone());
        status.required_cells = plan
            .required_cells
            .iter()
            .map(|cell| cell.id.clone())
            .collect();
        needs_scenario_approval = plan
            .blockers
            .iter()
            .any(|blocker| blocker.code == "DESHELL_BLOCKER_UNAPPROVED_SCENARIO");
        needs_matrix_approval = plan.blockers.iter().any(|blocker| {
            matches!(
                blocker.code.as_str(),
                "DESHELL_BLOCKER_PLATFORM_MATRIX_EMPTY" | "DESHELL_BLOCKER_UNAPPROVED_MATRIX_CELL"
            )
        });
        status.superseded = plan_count.saturating_sub(1);
        status.active_state = if directory.join("retired.json").is_file() {
            ActiveState::Retired
        } else if !plan.blockers.is_empty() {
            ActiveState::Blocked
        } else if active_plan_stale(root, &inventory, &plan) {
            ActiveState::Stale
        } else if directory.join("verified.json").is_file()
            || load_complete_evidence(&canonical, &directory, &plan)
                .and_then(|evidence| validate_node_coverage(&directory, &plan, &evidence))
                .is_ok()
        {
            ActiveState::Verified
        } else {
            ActiveState::Planned
        };
        let present = imported_verified_cells(&directory, &plan.plan_digest);
        status.remaining_evidence = plan
            .required_cells
            .iter()
            .filter(|cell| !present.contains(cell.id.as_str()))
            .count();
        if status.active_state == ActiveState::Planned {
            next_cell = plan
                .required_cells
                .iter()
                .find(|cell| !present.contains(cell.id.as_str()))
                .or_else(|| plan.required_cells.first())
                .map(|cell| cell.id.clone());
        }
        status.blocked = usize::from(status.active_state == ActiveState::Blocked);
        status.planned = usize::from(status.active_state == ActiveState::Planned);
        status.verified = usize::from(status.active_state == ActiveState::Verified);
        status.retired = usize::from(status.active_state == ActiveState::Retired);
        status.stale = usize::from(status.active_state == ActiveState::Stale);
    } else {
        status.superseded = plan_count;
    }
    let archive = canonical.join(".deshell/archive/manifest.json");
    if archive.is_file()
        && let Ok(value) = crate::strict_json::parse(&std::fs::read(archive).unwrap_or_default())
    {
        status.archived = value["entries"].as_array().map_or(0, Vec::len);
    }
    let root_value = root.to_string_lossy().into_owned();
    let pending_approval_argv = if needs_scenario_approval {
        crate::approval::scenario_reviews(root)
            .ok()
            .and_then(|reviews| {
                reviews
                    .into_iter()
                    .find(|review| review.status != crate::approval::ReviewStatus::Approved)
            })
            .map(|review| {
                vec![
                    "deshell".into(),
                    "scenario".into(),
                    "approve".into(),
                    "--root".into(),
                    root_value.clone(),
                    "--name".into(),
                    review.name,
                    "--digest".into(),
                    review.digest,
                ]
            })
    } else {
        None
    }
    .or_else(|| {
        needs_matrix_approval
            .then(|| crate::approval::matrix_reviews(root).ok())
            .flatten()
            .and_then(|reviews| {
                reviews
                    .into_iter()
                    .find(|review| review.status != crate::approval::ReviewStatus::Approved)
            })
            .map(|review| {
                vec![
                    "deshell".into(),
                    "matrix".into(),
                    "approve".into(),
                    "--root".into(),
                    root_value.clone(),
                    "--cell".into(),
                    review.name,
                    "--digest".into(),
                    review.digest,
                ]
            })
    });
    status.next_argv = if status.live == 0 {
        vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root_value,
            "--require".into(),
            "shell-free".into(),
        ]
    } else if status.active_state == ActiveState::Blocked && pending_approval_argv.is_some() {
        pending_approval_argv.expect("checked pending approval argv")
    } else {
        match status.active_state {
            ActiveState::Blocked
            | ActiveState::Stale
            | ActiveState::None
            | ActiveState::Retired => {
                vec![
                    "deshell".into(),
                    "migrate".into(),
                    "plan".into(),
                    "--root".into(),
                    root_value,
                ]
            }
            ActiveState::Planned => {
                let digest = status.active_plan.clone().unwrap_or_default();
                let cell = next_cell.unwrap_or_else(|| "<approved-cell>".into());
                let output = format!(".deshell/{digest}-{cell}-evidence.json");
                vec![
                    "deshell".into(),
                    "migrate".into(),
                    "verify".into(),
                    "--root".into(),
                    root_value,
                    "--plan".into(),
                    digest,
                    "--cell".into(),
                    cell,
                    "--output".into(),
                    output,
                ]
            }
            ActiveState::Verified => vec![
                "deshell".into(),
                "migrate".into(),
                "apply".into(),
                "--root".into(),
                root_value,
                "--plan".into(),
                status.active_plan.clone().unwrap_or_default(),
            ],
        }
    };
    status.next = status.next_argv.join(" ");
    Ok(status)
}

fn imported_verified_cells(directory: &Path, plan_digest: &str) -> BTreeSet<String> {
    let evidence = directory.join("evidence");
    let Ok(entries) = std::fs::read_dir(evidence) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read(entry.path()).ok())
        .filter_map(|bytes| MigrationEvidence::decode(&bytes).ok())
        .filter(|evidence| {
            evidence.plan_digest == plan_digest && evidence.status == EvidenceStatus::Verified
        })
        .map(|evidence| evidence.cell)
        .collect()
}

fn active_plan_stale(
    root: &Path,
    inventory: &crate::scanner::Inventory,
    plan: &MigrationPlan,
) -> bool {
    if canonical_digest(inventory).ok().as_deref() != Some(&plan.inventory_digest) {
        return true;
    }
    if validate_current_plan_policy(root, plan).is_err() {
        return true;
    }
    let Ok(scenarios) = load_approved_scenario_values(root) else {
        return true;
    };
    plan.required_scenarios.iter().any(|requirement| {
        scenarios.get(&requirement.name).is_none_or(|scenario| {
            scenario.digest().ok().as_deref() != Some(&requirement.digest)
                || scenario.approval_digest != requirement.approval_digest
        })
    })
}

pub(crate) fn verify_integrity(root: &Path) -> Result<(), String> {
    let root = canonical_root(root)?;
    let migrations = root.join(".deshell/migrations/sha256");
    let mut retired_sources = BTreeSet::new();
    let mut retired_plans = BTreeSet::new();
    match migrations.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            let mut directories = std::fs::read_dir(&migrations)
                .map_err(|error| format!("cannot read {}: {error}", migrations.display()))?
                .map(|entry| {
                    entry
                        .map(|entry| entry.path())
                        .map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            directories.sort();
            for directory in directories {
                let metadata = directory.symlink_metadata().map_err(|error| {
                    format!(
                        "cannot inspect migration artifact {}: {error}",
                        directory.display()
                    )
                })?;
                if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                    return Err(format!(
                        "DESHELL_EVIDENCE_TAMPERED: migration artifact is unsafe: {}",
                        directory.display()
                    ));
                }
                let name = directory
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or("DESHELL_EVIDENCE_TAMPERED: migration digest path is not UTF-8")?;
                if !crate::digest::valid_sha256(name) {
                    return Err(format!(
                        "DESHELL_EVIDENCE_TAMPERED: invalid migration digest directory {name}"
                    ));
                }
                let plan_bytes = std::fs::read(directory.join("plan.json")).map_err(|error| {
                    format!("DESHELL_EVIDENCE_TAMPERED: cannot read plan {name}: {error}")
                })?;
                let plan = MigrationPlan::decode(&plan_bytes)
                    .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
                if plan.plan_digest != name {
                    return Err(format!(
                        "DESHELL_EVIDENCE_TAMPERED: plan directory digest mismatch for {name}"
                    ));
                }
                for proposal in &plan.proposals {
                    load_proposal(&directory, proposal)
                        .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
                }
                for source in &plan.sources {
                    load_ir(&directory, &source.ir_digest)
                        .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
                }
                verify_evidence_directory(&directory, &plan)?;
                if directory.join("retired.json").is_file() {
                    verify_retirement_marker(&directory.join("retired.json"), &plan.plan_digest)?;
                    verify_retirement_marker(&directory.join("verified.json"), &plan.plan_digest)?;
                    retired_plans.insert(plan.plan_digest.clone());
                    for source in &plan.sources {
                        retired_sources.insert((
                            source.location.clone(),
                            plan.plan_digest.clone(),
                            source.content_digest.clone(),
                            match source.kind {
                                SourceKind::ShellFile => "shell_file",
                                SourceKind::EmbeddedShell => "embedded_shell",
                            },
                        ));
                    }
                }
            }
        }
        Ok(_) => {
            return Err("DESHELL_EVIDENCE_TAMPERED: migrations/sha256 is not a directory".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect {}: {error}", migrations.display())),
    }

    let archive_path = root.join(".deshell/archive/manifest.json");
    if retired_sources.is_empty() {
        if archive_path.symlink_metadata().is_ok() {
            return Err("DESHELL_ARCHIVE_TAMPERED: archive exists without a retired plan".into());
        }
        return Ok(());
    }
    let metadata = archive_path
        .symlink_metadata()
        .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: cannot inspect manifest: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("DESHELL_ARCHIVE_TAMPERED: archive manifest is unsafe".into());
    }
    let manifest: ArchiveManifest = crate::strict_json::decode(
        &std::fs::read(&archive_path)
            .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?,
    )
    .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?;
    validate_archive_manifest(&manifest)
        .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?;
    if !retired_plans.contains(&manifest.plan_digest) {
        return Err("DESHELL_ARCHIVE_TAMPERED: manifest names an unretired plan".into());
    }
    let actual = manifest
        .entries
        .iter()
        .map(|entry| {
            (
                entry.original.clone(),
                entry.plan_digest.clone(),
                entry.content_digest.clone(),
                match entry.kind {
                    SourceKind::ShellFile => "shell_file",
                    SourceKind::EmbeddedShell => "embedded_shell",
                },
            )
        })
        .collect::<BTreeSet<_>>();
    if actual != retired_sources || actual.len() != manifest.entries.len() {
        return Err(
            "DESHELL_ARCHIVE_TAMPERED: manifest does not exactly match retired sources".into(),
        );
    }
    for entry in &manifest.entries {
        let path = crate::project::project_file_path(&root, &entry.archive_path)
            .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?;
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || archive_executable(&metadata)
        {
            return Err(format!(
                "DESHELL_ARCHIVE_TAMPERED: archive is unsafe or executable: {}",
                entry.archive_path
            ));
        }
        let bytes =
            std::fs::read(&path).map_err(|error| format!("DESHELL_ARCHIVE_TAMPERED: {error}"))?;
        if crate::digest::sha256(&bytes) != entry.content_digest {
            return Err(format!(
                "DESHELL_ARCHIVE_TAMPERED: content digest mismatch for {}",
                entry.archive_path
            ));
        }
    }
    Ok(())
}

fn verify_evidence_directory(directory: &Path, plan: &MigrationPlan) -> Result<(), String> {
    let evidence_directory = directory.join("evidence");
    let metadata = evidence_directory.symlink_metadata().map_err(|error| {
        format!("DESHELL_EVIDENCE_TAMPERED: cannot inspect Evidence directory: {error}")
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err("DESHELL_EVIDENCE_TAMPERED: Evidence directory is unsafe".into());
    }
    let mut paths = std::fs::read_dir(&evidence_directory)
        .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    for path in paths {
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > 16 * 1024 * 1024
        {
            return Err("DESHELL_EVIDENCE_TAMPERED: Evidence file is unsafe".into());
        }
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_suffix(".json"))
            .ok_or("DESHELL_EVIDENCE_TAMPERED: invalid Evidence filename")?;
        let (cell, digest) = filename
            .rsplit_once('-')
            .ok_or("DESHELL_EVIDENCE_TAMPERED: invalid Evidence filename")?;
        if cell.is_empty() || !crate::digest::valid_sha256(digest) {
            return Err("DESHELL_EVIDENCE_TAMPERED: invalid Evidence filename".into());
        }
        let bytes =
            std::fs::read(&path).map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
        let evidence = MigrationEvidence::decode(&bytes)
            .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
        let canonical = evidence
            .encode_pretty()
            .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
        if evidence.plan_digest != plan.plan_digest
            || evidence.cell != cell
            || canonical != bytes
            || crate::digest::sha256(&canonical) != digest
        {
            return Err(format!(
                "DESHELL_EVIDENCE_TAMPERED: content address mismatch for {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn verify_retirement_marker(path: &Path, plan_digest: &str) -> Result<(), String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("DESHELL_EVIDENCE_TAMPERED: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("DESHELL_EVIDENCE_TAMPERED: retirement marker is unsafe".into());
    }
    let expected = pretty_bytes(&serde_json::json!({
        "schema_version": 1,
        "plan_digest": plan_digest,
    }))?;
    if std::fs::read(path).map_err(|error| error.to_string())? != expected {
        return Err("DESHELL_EVIDENCE_TAMPERED: retirement marker changed".into());
    }
    Ok(())
}

#[cfg(unix)]
fn archive_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn archive_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
// Tests reach for the raw APIs on purpose: they stage corrupt trees, race two
// writers against one path, and assert on what the transactional layer does with
// the result. Constructing those situations is precisely what the production ban
// exists to prevent, so the ban is lifted here and nowhere else.
#[expect(
    clippy::disallowed_methods,
    reason = "tests construct the races and corrupt trees the production ban prevents"
)]
mod tests {
    use super::*;

    #[test]
    fn go_build_environment_is_private_complete_and_not_applied_to_other_tools() {
        let root = tempfile::tempdir().unwrap();
        let environment = verification_build_environment(
            root.path(),
            &["go".into(), "build".into(), "./...".into()],
        );
        let values: BTreeMap<_, _> = environment.into_iter().collect();
        assert_eq!(values.get("GOENV").map(String::as_str), Some("off"));
        assert_eq!(values.get("GOMAXPROCS").map(String::as_str), Some("2"));
        assert_eq!(
            values.get("GOCACHE").map(String::as_str),
            Some(
                root.path()
                    .join(".deshell/verification/go-cache")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            values.get("GOMODCACHE").map(String::as_str),
            Some(
                root.path()
                    .join(".deshell/verification/go-mod-cache")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert!(verification_build_environment(root.path(), &["cargo".into()]).is_empty());
        assert_eq!(
            verification_validation_environment(root.path())
                .into_iter()
                .find(|(name, _)| name == "GOMODCACHE")
                .map(|(_, value)| value),
            Some(
                root.path()
                    .join(".deshell/verification/go-mod-cache")
                    .to_string_lossy()
                    .into_owned()
            )
        );
    }

    fn signed_proposal(contents: Vec<u8>) -> Proposal {
        let digest = crate::digest::sha256(&contents);
        let mut proposal = Proposal {
            schema_version: 1,
            proposal_digest: ZERO_DIGEST.into(),
            request_digest: ZERO_DIGEST.into(),
            generator_digest: format!("sha256:{ZERO_DIGEST}"),
            patches: vec![GeneratorPatch {
                operation: PatchOperation::Create,
                path: "src/bin/generated.rs".into(),
                expected_digest: None,
                content_base64: base64::engine::general_purpose::STANDARD.encode(&contents),
                content_digest: digest,
                permissions: 0o644,
            }],
            build_argv: vec!["rustc".into(), "src/bin/generated.rs".into()],
            run_argv: vec!["target/generated".into()],
            validation: Vec::new(),
            dependencies: Vec::new(),
            source_map: vec![GeneratedSpan {
                ir_node: "00000000000000000000000000000000".into(),
                generated: Location {
                    path: "src/bin/generated.rs".into(),
                    start_byte: 0,
                    end_byte: contents.len() as u64,
                },
            }],
        };
        proposal.proposal_digest = canonical_digest(&proposal).unwrap();
        proposal
    }

    fn resign(proposal: &mut Proposal) {
        proposal.proposal_digest = ZERO_DIGEST.into();
        proposal.proposal_digest = canonical_digest(proposal).unwrap();
    }

    #[test]
    fn official_generator_proposals_have_no_delete_operation() {
        assert!(
            !serde_json::to_string(&PatchOperation::Create)
                .unwrap()
                .contains("delete")
        );
        assert!(
            !serde_json::to_string(&PatchOperation::Update)
                .unwrap()
                .contains("delete")
        );
    }

    #[test]
    fn rust_generator_build_argv_is_platform_exact_without_foreign_linker_flags() {
        let windows = rust_build_argv("src/bin/build.rs", ".deshell/verification/build", "windows");
        assert_eq!(
            windows,
            [
                "rustc",
                "src/bin/build.rs",
                "--edition=2024",
                "-D",
                "warnings",
                "-Ccodegen-units=1",
                "-o",
                ".deshell/verification/build"
            ]
        );
        let linux = rust_build_argv("src/bin/build.rs", ".deshell/verification/build", "linux");
        assert_eq!(
            linux,
            [
                "rustc",
                "src/bin/build.rs",
                "--edition=2024",
                "-D",
                "warnings",
                "-Ccodegen-units=1",
                "-o",
                ".deshell/verification/build"
            ],
            "generator build argv must not assume a target linker's private thread flag"
        );
    }

    #[test]
    fn original_cmd_script_uses_noninteractive_exact_batch_argv() {
        assert_eq!(
            original_script_argv("cmd", r"C:\workspace\corpus.cmd").unwrap(),
            ["cmd", "/D", "/S", "/C", r"C:\workspace\corpus.cmd"]
        );
    }

    #[test]
    fn embedded_cmd_snippet_is_materialized_as_an_exact_batch_file() {
        let snippet = b"@echo off\ntarget\\deshell-corpus-helper.exe branch".to_vec();
        let invocation = embedded_original_invocation("cmd", snippet.clone()).unwrap();
        assert!(invocation._script_directory.is_some());
        assert_eq!(&invocation.argv[..4], ["cmd", "/D", "/S", "/C"]);
        assert_eq!(invocation.argv.len(), 5);
        let script = Path::new(&invocation.argv[4]);
        assert_eq!(
            script.extension().and_then(|value| value.to_str()),
            Some("cmd")
        );
        assert_eq!(std::fs::read(script).unwrap(), snippet);
    }

    #[test]
    fn verification_binary_path_uses_the_platform_executable_suffix() {
        assert_eq!(
            verification_binary_path("build", "windows"),
            ".deshell/verification/build.exe"
        );
        assert_eq!(
            verification_binary_path("build", "linux"),
            ".deshell/verification/build"
        );
    }

    #[test]
    fn generator_proposal_rejects_oversize_permissions_ranges_and_false_source_maps() {
        let oversized = signed_proposal(vec![b'x'; 4 * 1024 * 1024 + 1]);
        assert!(validate_proposal(&oversized).unwrap_err().contains("4 MiB"));

        let mut permissions = signed_proposal(b"fn main() {}\n".to_vec());
        permissions.patches[0].permissions = 0o4755;
        resign(&mut permissions);
        assert!(
            validate_proposal(&permissions)
                .unwrap_err()
                .contains("permissions")
        );

        let mut dependency = signed_proposal(b"fn main() {}\n".to_vec());
        dependency.dependencies.push(Dependency {
            ecosystem: DependencyEcosystem::Cargo,
            name: "example".into(),
            version: "^1.2".into(),
        });
        resign(&mut dependency);
        assert!(
            validate_proposal(&dependency)
                .unwrap_err()
                .contains("exact pin")
        );

        let mut outside = signed_proposal(b"fn main() {}\n".to_vec());
        outside.source_map[0].generated.end_byte += 1;
        resign(&mut outside);
        assert!(
            validate_proposal(&outside)
                .unwrap_err()
                .contains("generated span")
        );

        let mut false_path = signed_proposal(b"fn main() {}\n".to_vec());
        false_path.source_map[0].generated.path = "src/bin/other.rs".into();
        resign(&mut false_path);
        assert!(
            validate_proposal(&false_path)
                .unwrap_err()
                .contains("source map path")
        );
    }

    #[test]
    fn generator_proposal_rejects_traversal_and_duplicate_patch_paths() {
        let mut traversal = signed_proposal(b"fn main() {}\n".to_vec());
        traversal.patches[0].path = "../escape.rs".into();
        traversal.source_map[0].generated.path = "../escape.rs".into();
        resign(&mut traversal);
        assert!(validate_proposal(&traversal).is_err());

        let mut duplicate = signed_proposal(b"fn main() {}\n".to_vec());
        duplicate.patches.push(duplicate.patches[0].clone());
        resign(&mut duplicate);
        assert!(
            validate_proposal(&duplicate)
                .unwrap_err()
                .contains("duplicate proposal path")
        );
    }

    #[cfg(unix)]
    #[test]
    fn external_patch_state_rejects_symlink_targets() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::create_dir_all(directory.path().join("src/bin")).unwrap();
        symlink(
            outside.path(),
            directory.path().join("src/bin/generated.rs"),
        )
        .unwrap();
        let patch = &signed_proposal(b"fn main() {}\n".to_vec()).patches[0];

        let error = validate_external_patch_state(directory.path(), patch).unwrap_err();
        assert!(error.contains("regular non-symlink file"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn external_rpc_rejects_an_oversized_actual_response_frame() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let generator = directory.path().join("generator.py");
        std::fs::write(
            &generator,
            br##"#!/usr/bin/python3
import json
print(json.dumps({"id": "proposal", "jsonrpc": "2.0", "result": "x" * 2048}))
"##,
        )
        .unwrap();
        std::fs::set_permissions(&generator, std::fs::Permissions::from_mode(0o500)).unwrap();

        let error = execute_external_rpc(ExecuteExternalRpcArgs {
            root: directory.path(),
            executable: &generator,
            request: &serde_json::json!({"id": "proposal", "jsonrpc": "2.0", "method": "test"}),
            id: &serde_json::json!("proposal"),
            project_limits: crate::config::ResourceLimits::DEFAULT,
            frame_limit: 1024,
        })
        .unwrap_err();
        assert!(error.contains("negotiated frame limit"), "{error}");
    }

    #[test]
    fn external_generator_handshake_cannot_spoof_its_pin_or_capability() {
        let registration = crate::config::ExternalGenerator {
            name: "fixture".into(),
            executable: "tools/fixture".into(),
            digest: format!("sha256:{}", "a".repeat(64)),
            capabilities: vec![crate::config::MigrationTarget::Agent],
        };
        let mut handshake = GeneratorHandshake {
            schema_version: 1,
            protocol: "deshell.generator.v1".into(),
            generator: GeneratorIdentity {
                name: "fixture".into(),
                version: "1".into(),
                digest: registration.digest.clone(),
                capabilities: vec![crate::config::MigrationTarget::Agent],
            },
            max_frame_bytes: crate::protocol::MAX_MESSAGE_BYTES as u64,
        };
        assert!(
            validate_external_handshake(
                &handshake,
                &registration,
                &registration.digest,
                crate::config::MigrationTarget::Agent,
            )
            .is_ok()
        );
        handshake.generator.digest = format!("sha256:{}", "b".repeat(64));
        assert!(
            validate_external_handshake(
                &handshake,
                &registration,
                &registration.digest,
                crate::config::MigrationTarget::Agent,
            )
            .unwrap_err()
            .contains("identity or digest")
        );
        handshake.generator.digest = registration.digest.clone();
        handshake.generator.capabilities = vec![crate::config::MigrationTarget::Rust];
        assert!(
            validate_external_handshake(
                &handshake,
                &registration,
                &registration.digest,
                crate::config::MigrationTarget::Agent,
            )
            .unwrap_err()
            .contains("capability")
        );
    }

    #[test]
    fn external_generator_isolation_detects_direct_mutation() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("generator"), b"pinned").unwrap();
        let baseline = isolated_tree_digest(directory.path()).unwrap();
        std::fs::write(directory.path().join("unexpected"), b"mutation").unwrap();
        let error = ensure_isolated_tree_unchanged(directory.path(), &baseline).unwrap_err();
        assert!(error.contains("DESHELL_BLOCKER_GENERATOR_DIRECT_MUTATION"));
    }

    #[test]
    fn external_generator_guard_detects_live_project_mutation() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("source.txt"), b"before").unwrap();
        let baseline = guarded_project_tree_digest(directory.path()).unwrap();
        std::fs::write(directory.path().join("source.txt"), b"after").unwrap();

        let error = ensure_guarded_project_tree_unchanged(directory.path(), &baseline).unwrap_err();
        assert!(error.contains("DESHELL_BLOCKER_GENERATOR_DIRECT_MUTATION"));
        assert!(error.contains("live project"));
    }

    #[test]
    fn proposal_call_site_updates_are_resolved_against_an_isolated_reference_graph() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("src/bin")).unwrap();
        std::fs::write(
            directory.path().join("build.sh"),
            b"#!/bin/sh\n/usr/bin/printf migrated\n",
        )
        .unwrap();
        let original = b"import subprocess\nsubprocess.run([\"sh\", \"build.sh\"], check=True)\n";
        std::fs::write(directory.path().join("caller.py"), original).unwrap();
        let targets = BTreeSet::from(["build.sh".to_owned()]);

        let before = crate::scanner::static_script_references(directory.path(), &targets).unwrap();
        assert_eq!(before.len(), 1);
        let replacement =
            b"import subprocess\nsubprocess.run([\"native-build\"], check=True)\n".to_vec();
        let mut proposal = signed_proposal(b"fn main() {}\n".to_vec());
        proposal
            .patches
            .push(generator_patch(directory.path(), "caller.py", replacement, 0o644).unwrap());
        resign(&mut proposal);

        let after = remaining_static_references_after_proposals(
            directory.path(),
            &targets,
            std::slice::from_ref(&proposal),
        )
        .unwrap();
        assert!(after.is_empty(), "{after:#?}");
        assert_eq!(
            std::fs::read(directory.path().join("caller.py")).unwrap(),
            original
        );
    }

    #[cfg(unix)]
    #[test]
    fn post_scan_failure_rolls_back_source_generation_archive_and_metadata() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = b"#!/usr/bin/env bash\n/usr/bin/printf rollback\n";
        std::fs::write(directory.path().join("rollback.sh"), source).unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("entrypoints = []", "entrypoints = [\"rollback.sh\"]")
            .replace(
                "platform_cells = []",
                &format!(
                    "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}]",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            );
        std::fs::write(&config_path, config).unwrap();
        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("approval = \"draft\"", "approval = \"approved\"");
        std::fs::write(scenario_path, scenario).unwrap();
        std::fs::create_dir_all(directory.path().join("src/bin")).unwrap();

        let planned = create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        let evidence = verify(directory.path(), &planned.digest, "host").unwrap();
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        let config_before = std::fs::read(&config_path).unwrap();
        let manifest_path = directory.path().join(".deshell/manifest.json");
        let manifest_before = std::fs::read(&manifest_path).unwrap();

        force_post_scan_failure_once();
        let error = apply(directory.path(), &planned.digest).unwrap_err();
        assert!(
            error.contains("all retirement changes were rolled back"),
            "{error}"
        );
        assert_eq!(
            std::fs::read(directory.path().join("rollback.sh")).unwrap(),
            source
        );
        assert!(
            !directory
                .path()
                .join("src/bin/deshell_rollback.rs")
                .exists()
        );
        assert!(!directory.path().join(".deshell/archive").exists());
        assert_eq!(std::fs::read(config_path).unwrap(), config_before);
        assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_before);
        let plan_directory = directory
            .path()
            .join(format!(".deshell/migrations/sha256/{}", planned.digest));
        assert!(!plan_directory.join("verified.json").exists());
        assert!(!plan_directory.join("retired.json").exists());
    }

    fn plan_with_body(body: crate::ir::Node) -> crate::ir::Plan {
        let mut plan = crate::frontend::lower_with_interpreter(
            "build.sh",
            b"true\n",
            crate::config::UnknownInterpreter::Reject,
            "sh",
        )
        .unwrap();
        plan.tasks[0].body = body;
        plan
    }

    fn node(operation: crate::ir::Operation) -> crate::ir::Node {
        crate::ir::Node {
            id: "00000000000000000000000000000000".into(),
            operation,
            guarantee: crate::ir::Guarantee::Native {
                semantic_model: "test-v1".into(),
            },
            source: None,
        }
    }

    fn exec(argv: Vec<crate::ir::TextExpression>) -> crate::ir::Node {
        node(crate::ir::Operation::Exec {
            argv,
            environment: Vec::new(),
            working_directory: None,
        })
    }

    #[test]
    fn effect_tree_walkers_cover_every_recursive_shape_and_guarantee_class() {
        let leaf = || exec(vec![crate::ir::TextExpression::literal("true")]);
        let tree = node(crate::ir::Operation::Sequence {
            nodes: vec![
                node(crate::ir::Operation::Pipeline {
                    nodes: vec![leaf(), leaf()],
                    status: crate::ir::PipelineStatus::Last,
                }),
                node(crate::ir::Operation::Parallel {
                    nodes: vec![leaf()],
                }),
                node(crate::ir::Operation::Condition {
                    predicate: Box::new(leaf()),
                    if_true: Box::new(leaf()),
                    if_false: Some(Box::new(leaf())),
                }),
                node(crate::ir::Operation::Match {
                    value: crate::ir::TextExpression::literal("value"),
                    cases: vec![crate::ir::MatchCase {
                        pattern: crate::ir::PatternExpression::literal("pattern"),
                        body: leaf(),
                    }],
                    default: Some(Box::new(leaf())),
                }),
                node(crate::ir::Operation::Foreach {
                    variable: "item".into(),
                    items: vec![crate::ir::TextExpression::literal("one")],
                    body: Box::new(leaf()),
                }),
                node(crate::ir::Operation::Redirect {
                    redirections: vec![],
                    body: Box::new(leaf()),
                }),
                node(crate::ir::Operation::Scope {
                    variables: vec![],
                    environment: vec![],
                    working_directory: None,
                    body: Box::new(leaf()),
                }),
                node(crate::ir::Operation::CaptureStdout {
                    name: "capture".into(),
                    value_type: crate::ir::PrimitiveType::Bytes,
                    body: Box::new(leaf()),
                }),
                node(crate::ir::Operation::Spawn {
                    handle: "child".into(),
                    body: Box::new(leaf()),
                }),
                node(crate::ir::Operation::TryFinally {
                    body: Box::new(leaf()),
                    finalizer: Box::new(leaf()),
                }),
            ],
            on_failure: crate::ir::SequenceFailure::Continue,
        });
        let plan = plan_with_body(tree);
        let mut ids = Vec::new();
        collect_node_ids(&plan.tasks[0].body, &mut ids);
        assert_eq!(ids.len(), 26);
        assert_eq!(guarantee_counts(&plan), (26, 0, 0));

        let span = |start, end| crate::ir::SourceSpan {
            file: "build.sh".into(),
            start_line: 1,
            start_column: start,
            end_line: 1,
            end_column: end,
            start_byte: start,
            end_byte: end,
        };
        let mut native = leaf();
        native.source = Some(span(0, 3));
        let mut delegated = leaf();
        delegated.source = Some(span(3, 6));
        delegated.guarantee = crate::ir::Guarantee::Delegated {
            reason: "parse error".into(),
        };
        let mut residual = leaf();
        residual.source = Some(span(6, 9));
        residual.guarantee = crate::ir::Guarantee::Residual {
            reason: "opaque".into(),
        };
        let plan = plan_with_body(node(crate::ir::Operation::Sequence {
            nodes: vec![native, delegated, residual],
            on_failure: crate::ir::SequenceFailure::Continue,
        }));
        assert_eq!(
            classify_coverage(&plan, 10),
            Coverage {
                total_bytes: 10,
                native_bytes: 3,
                delegated_bytes: 3,
                residual_bytes: 3,
                trivia_bytes: 1,
            }
        );
        assert_eq!(guarantee_counts(&plan), (2, 1, 1));
        assert_eq!(delegated_reasons(&plan), vec!["parse error"]);

        assert_eq!(
            delegated_blocker_code(&["dynamic evaluation".into()]),
            "DESHELL_BLOCKER_DYNAMIC_EVAL"
        );
        assert_eq!(
            delegated_blocker_code(&["unterminated quote".into()]),
            "DESHELL_BLOCKER_PARSE_ERROR"
        );
        assert_eq!(
            delegated_blocker_code(&["parser unavailable".into()]),
            "DESHELL_BLOCKER_PARSER_UNAVAILABLE"
        );
        assert_eq!(
            delegated_blocker_code(&["other".into()]),
            "DESHELL_BLOCKER_UNIMPLEMENTED_SEMANTIC"
        );
        let source = Location {
            path: "host.sh".into(),
            start_byte: 10,
            end_byte: 30,
        };
        assert_eq!(
            delegated_blocker_location(
                &["failure at bytes 2..5 (detail)".into()],
                &source,
                SourceKind::ShellFile
            ),
            Location {
                path: "host.sh".into(),
                start_byte: 12,
                end_byte: 15
            }
        );
        assert_eq!(
            delegated_blocker_location(
                &["failure at bytes bad..5".into()],
                &source,
                SourceKind::EmbeddedShell
            ),
            source
        );
    }

    #[test]
    fn network_replay_reduction_accepts_exact_http_and_rejects_every_ambiguous_curl_shape() {
        let request = parse_curl_replay_request(&[
            "curl".into(),
            "--silent".into(),
            "--show-error".into(),
            "--request".into(),
            "put".into(),
            "--data-raw".into(),
            "body".into(),
            "http://example.test/data".into(),
        ])
        .unwrap();
        assert_eq!(request.method, "PUT");
        assert_eq!(request.uri, "http://example.test/data");
        assert_eq!(request.body, b"body");
        let post = parse_curl_replay_request(&[
            "curl".into(),
            "-d".into(),
            "body".into(),
            "http://example.test".into(),
        ])
        .unwrap();
        assert_eq!(post.method, "POST");

        let cases = [
            (vec!["curl", "-X"], "missing its value"),
            (vec!["curl", "-d"], "missing its value"),
            (vec!["curl", "--header", "x"], "does not support option"),
            (vec!["curl", "value"], "positional value"),
            (vec!["curl"], "omitted its URI"),
            (vec!["curl", "http://one", "http://two"], "exactly one URI"),
            (vec!["curl", "https://example.test"], "HTTPS replay"),
        ];
        for (argv, expected) in cases {
            let argv = argv.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let error = parse_curl_replay_request(&argv).unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }
        assert!(require_replayable_http_uri("http://example.test").is_ok());
        assert!(
            require_replayable_http_uri("file:///tmp/value")
                .unwrap_err()
                .contains("absolute HTTP")
        );

        let network = node(crate::ir::Operation::NetworkRequest {
            method: crate::ir::TextExpression::literal("get"),
            uri: crate::ir::TextExpression::literal("http://example.test/data"),
        });
        let curl = exec(vec![
            crate::ir::TextExpression::literal("/usr/bin/curl"),
            crate::ir::TextExpression::literal("http://example.test/other"),
        ]);
        let plan = plan_with_body(node(crate::ir::Operation::Sequence {
            nodes: vec![network, curl],
            on_failure: crate::ir::SequenceFailure::Continue,
        }));
        let requests = network_replay_requests(&plan).unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(network_effects(&plan).len(), 2);

        let unsupported = plan_with_body(exec(vec![crate::ir::TextExpression::literal("wget")]));
        assert!(
            network_replay_requests(&unsupported)
                .unwrap_err()
                .contains("does not support wget")
        );
        let dynamic = plan_with_body(node(crate::ir::Operation::NetworkRequest {
            method: crate::ir::TextExpression {
                parts: vec![crate::ir::TextPart::Variable {
                    name: "METHOD".into(),
                }],
            },
            uri: crate::ir::TextExpression::literal("http://example.test"),
        }));
        assert!(
            network_replay_requests(&dynamic)
                .unwrap_err()
                .contains("literal HTTP method")
        );
    }

    #[test]
    fn official_generators_emit_literal_variable_argument_environment_cwd_pipeline_and_condition() {
        let expression = crate::ir::TextExpression {
            parts: vec![
                crate::ir::TextPart::Literal {
                    value: "prefix-".into(),
                },
                crate::ir::TextPart::Variable {
                    name: "VALUE".into(),
                },
                crate::ir::TextPart::Argument { name: "1".into() },
            ],
        };
        let mut command = exec(vec![
            crate::ir::TextExpression::literal("/usr/bin/printf"),
            expression,
        ]);
        if let crate::ir::Operation::Exec {
            environment,
            working_directory,
            ..
        } = &mut command.operation
        {
            environment.push(crate::ir::NamedExpression {
                name: "LOCAL".into(),
                value: crate::ir::TextExpression::literal("value"),
            });
            *working_directory = Some(crate::ir::TextExpression::literal("work"));
        }
        let pipeline = node(crate::ir::Operation::Pipeline {
            nodes: vec![
                exec(vec![crate::ir::TextExpression::literal("first")]),
                exec(vec![crate::ir::TextExpression::literal("second")]),
            ],
            status: crate::ir::PipelineStatus::Pipefail,
        });
        let condition = node(crate::ir::Operation::Condition {
            predicate: Box::new(exec(vec![crate::ir::TextExpression::literal("predicate")])),
            if_true: Box::new(exec(vec![crate::ir::TextExpression::literal("yes")])),
            if_false: Some(Box::new(exec(vec![crate::ir::TextExpression::literal(
                "no",
            )]))),
        });
        let plan = plan_with_body(node(crate::ir::Operation::Sequence {
            nodes: vec![command, pipeline, condition],
            on_failure: crate::ir::SequenceFailure::Continue,
        }));
        let rust = String::from_utf8(generate_rust(&plan).unwrap()).unwrap();
        let go = String::from_utf8(generate_go(&plan).unwrap()).unwrap();
        for expected in [
            "VALUE",
            "deshell_args.first()",
            ".env(\"LOCAL\"",
            ".current_dir",
            "deshell_run_pipeline",
            "if deshell_predicate == 0",
        ] {
            assert!(
                rust.contains(expected),
                "missing {expected:?} in Rust output"
            );
        }
        for expected in [
            // Locals shadow the environment, so an expansion goes through the
            // lookup rather than straight to `os.Getenv`.
            "deshellValue(deshellVars, \"VALUE\")",
            "deshellArgument(deshellArgs, 0)",
            ".Env = append",
            ".Dir =",
            "deshellRunPipeline",
            "if deshellLast == 0",
        ] {
            assert!(go.contains(expected), "missing {expected:?} in Go output");
        }

        assert_eq!(
            go_expression(&crate::ir::TextExpression { parts: vec![] }).unwrap(),
            "\"\""
        );
        let named = crate::ir::TextExpression {
            parts: vec![crate::ir::TextPart::Argument {
                name: "named".into(),
            }],
        };
        assert!(
            rust_expression(&named)
                .unwrap_err()
                .contains("named argument")
        );
        assert!(
            go_expression(&named)
                .unwrap_err()
                .contains("named argument")
        );

        let empty_exec = plan_with_body(exec(vec![]));
        assert!(
            generate_rust(&empty_exec)
                .unwrap_err()
                .contains("empty Exec argv")
        );
        assert!(
            generate_go(&empty_exec)
                .unwrap_err()
                .contains("empty Exec argv")
        );
        let unsupported = plan_with_body(node(crate::ir::Operation::FileRead {
            path: crate::ir::TextExpression::literal("file"),
        }));
        assert!(
            generate_rust(&unsupported)
                .unwrap_err()
                .contains("cannot preserve")
        );
        assert!(
            generate_go(&unsupported)
                .unwrap_err()
                .contains("cannot preserve")
        );
        let bad_pipeline = plan_with_body(node(crate::ir::Operation::Pipeline {
            nodes: vec![node(crate::ir::Operation::FileRead {
                path: crate::ir::TextExpression::literal("file"),
            })],
            status: crate::ir::PipelineStatus::Last,
        }));
        assert!(
            generate_rust(&bad_pipeline)
                .unwrap_err()
                .contains("only Exec stages")
        );
        assert!(
            generate_go(&bad_pipeline)
                .unwrap_err()
                .contains("only Exec stages")
        );

        let mut metadata = plan.clone();
        metadata.tasks[0].outputs.push(crate::ir::Binding {
            name: "result".into(),
            value_type: crate::ir::ValueType::Primitive(crate::ir::PrimitiveType::Text),
        });
        assert!(
            generate_rust(&metadata)
                .unwrap_err()
                .contains("task outputs")
        );
        assert!(generate_go(&metadata).unwrap_err().contains("task outputs"));
        metadata.entrypoint = "missing".into();
        assert!(
            generate_rust(&metadata)
                .unwrap_err()
                .contains("entrypoint task is missing")
        );
        assert!(
            generate_go(&metadata)
                .unwrap_err()
                .contains("entrypoint task is missing")
        );
    }

    #[test]
    fn official_rust_and_go_generators_pass_formatter_build_and_lint_gates() {
        let literal_plan = plan_with_body(exec(vec![
            crate::ir::TextExpression::literal("/usr/bin/printf"),
            crate::ir::TextExpression::literal("literal"),
        ]));
        let literal_rust = generate_rust(&literal_plan).unwrap();
        let literal_go = generate_go(&literal_plan).unwrap();
        let literal_rust_text = String::from_utf8(literal_rust.clone()).unwrap();
        let literal_go_text = String::from_utf8(literal_go.clone()).unwrap();
        assert!(!literal_rust_text.contains("Stdio"));
        assert!(!literal_rust_text.contains("deshell_args"));
        assert!(!literal_rust_text.contains("deshell_run_pipeline"));
        assert!(!literal_go_text.contains("deshellArgs"));
        assert!(!literal_go_text.contains("deshellRunPipeline"));

        let simple = exec(vec![
            crate::ir::TextExpression::literal("/usr/bin/printf"),
            crate::ir::TextExpression {
                parts: vec![crate::ir::TextPart::Argument { name: "1".into() }],
            },
        ]);
        let pipeline = node(crate::ir::Operation::Pipeline {
            nodes: vec![
                exec(vec![crate::ir::TextExpression::literal("first")]),
                exec(vec![crate::ir::TextExpression::literal("second")]),
            ],
            status: crate::ir::PipelineStatus::Pipefail,
        });
        let condition = node(crate::ir::Operation::Condition {
            predicate: Box::new(exec(vec![crate::ir::TextExpression::literal("predicate")])),
            if_true: Box::new(exec(vec![crate::ir::TextExpression::literal("yes")])),
            if_false: Some(Box::new(exec(vec![crate::ir::TextExpression::literal(
                "no",
            )]))),
        });
        let plan = plan_with_body(node(crate::ir::Operation::Sequence {
            nodes: vec![simple, pipeline, condition],
            on_failure: crate::ir::SequenceFailure::Continue,
        }));
        let directory = tempfile::tempdir().unwrap();
        let literal_rust_path = directory.path().join("literal.rs");
        let literal_go_path = directory.path().join("literal.go");
        std::fs::write(&literal_rust_path, literal_rust).unwrap();
        std::fs::write(&literal_go_path, literal_go).unwrap();
        let literal_rustc = std::process::Command::new("rustc")
            .arg(&literal_rust_path)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("literal-rust"))
            .output()
            .unwrap();
        assert!(
            literal_rustc.status.success(),
            "rustc rejected simple generated source:\n{}",
            String::from_utf8_lossy(&literal_rustc.stderr)
        );
        let literal_gofmt = std::process::Command::new("gofmt")
            .arg("-d")
            .arg(&literal_go_path)
            .output()
            .unwrap();
        assert!(literal_gofmt.status.success() && literal_gofmt.stdout.is_empty());
        let literal_vet = std::process::Command::new("go")
            .args(["vet", "literal.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            literal_vet.status.success(),
            "go vet rejected simple generated source:\n{}",
            String::from_utf8_lossy(&literal_vet.stderr)
        );
        let rust = directory.path().join("generated.rs");
        let go = directory.path().join("generated.go");
        std::fs::write(&rust, generate_rust(&plan).unwrap()).unwrap();
        std::fs::write(&go, generate_go(&plan).unwrap()).unwrap();

        let rustfmt = std::process::Command::new("rustfmt")
            .args(["--edition", "2024", "--check"])
            .arg(&rust)
            .output()
            .unwrap();
        assert!(
            rustfmt.status.success(),
            "rustfmt rejected generated source:\n{}",
            String::from_utf8_lossy(&rustfmt.stdout)
        );
        let rustc = std::process::Command::new("rustc")
            .arg(&rust)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("generated-rust"))
            .output()
            .unwrap();
        assert!(
            rustc.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        );
        let cargo_root = directory.path().join("rust-project");
        std::fs::create_dir_all(cargo_root.join("src")).unwrap();
        std::fs::write(
            cargo_root.join("Cargo.toml"),
            b"[package]\nname = \"deshell-generated-gate\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();
        std::fs::copy(&rust, cargo_root.join("src/main.rs")).unwrap();
        let clippy = std::process::Command::new("cargo")
            .args(["clippy", "--offline", "--quiet", "--", "-D", "warnings"])
            .current_dir(&cargo_root)
            .output()
            .unwrap();
        assert!(
            clippy.status.success(),
            "clippy rejected generated source:\n{}",
            String::from_utf8_lossy(&clippy.stderr)
        );

        let gofmt = std::process::Command::new("gofmt")
            .arg("-d")
            .arg(&go)
            .output()
            .unwrap();
        assert!(
            gofmt.status.success() && gofmt.stdout.is_empty(),
            "generated Go source was not gofmt canonical:\n{}",
            String::from_utf8_lossy(&gofmt.stdout)
        );
        for arguments in [
            vec!["build", "-o", "generated-go", "generated.go"],
            vec!["test", "generated.go"],
            vec!["vet", "generated.go"],
        ] {
            let output = std::process::Command::new("go")
                .args(&arguments)
                .current_dir(directory.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "go {} rejected generated source:\n{}",
                arguments[0],
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    /// A pipeline inside a loop or a `case` arm must still get its helper.
    ///
    /// The walk that decides whether to define `deshell_run_pipeline` listed
    /// `Sequence` and `Condition` by hand and fell through on everything else,
    /// so this plan generated a call to a function the generator had chosen not
    /// to emit — code its own `-D warnings` gate rejects. The plan also carries
    /// an empty `case` arm and an `echo`, which are the other two operations
    /// that reach the generators without starting a process.
    #[test]
    fn a_pipeline_nested_in_a_loop_or_a_case_arm_still_defines_its_helper() {
        let pipeline = || {
            node(crate::ir::Operation::Pipeline {
                nodes: vec![
                    exec(vec![crate::ir::TextExpression::literal("/bin/first")]),
                    exec(vec![crate::ir::TextExpression::literal("/bin/second")]),
                ],
                status: crate::ir::PipelineStatus::Pipefail,
            })
        };
        let loop_node = node(crate::ir::Operation::While {
            condition: Box::new(node(crate::ir::Operation::Test {
                predicate: crate::ir::TestPredicate::Empty {
                    value: crate::ir::TextExpression::literal(""),
                },
            })),
            body: Box::new(pipeline()),
        });
        let match_node = node(crate::ir::Operation::Match {
            value: crate::ir::TextExpression::literal("a"),
            cases: vec![
                crate::ir::MatchCase {
                    pattern: crate::ir::PatternExpression::literal("a"),
                    body: pipeline(),
                },
                crate::ir::MatchCase {
                    pattern: crate::ir::PatternExpression::literal("b"),
                    body: node(crate::ir::Operation::NoOp),
                },
            ],
            default: Some(Box::new(node(crate::ir::Operation::WriteStdout {
                contents: crate::ir::TextExpression::literal("::error::unknown\n"),
            }))),
        });
        let plan = plan_with_body(node(crate::ir::Operation::Sequence {
            nodes: vec![loop_node, match_node],
            on_failure: crate::ir::SequenceFailure::Stop,
        }));

        let directory = tempfile::tempdir().unwrap();
        let rust = directory.path().join("nested.rs");
        let go = directory.path().join("nested.go");
        std::fs::write(&rust, generate_rust(&plan).unwrap()).unwrap();
        std::fs::write(&go, generate_go(&plan).unwrap()).unwrap();

        let rustc = std::process::Command::new("rustc")
            .arg(&rust)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("nested-rust"))
            .output()
            .unwrap();
        assert!(
            rustc.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&rustc.stderr)
        );
        let vet = std::process::Command::new("go")
            .args(["vet", "nested.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            vet.status.success(),
            "go vet rejected generated source:\n{}",
            String::from_utf8_lossy(&vet.stderr)
        );
    }

    /// A status read at run time ends where the shells end, and stops where
    /// they part.
    ///
    /// `exit "${CODE}"` is the last thing standing between a real workflow and
    /// a migration, and it is the one model that claims less than the whole of
    /// its operation. Every measured shell reduces a decimal status modulo 256
    /// and agrees; outside that bash ends with 255 and zsh with 0, so there is
    /// nothing to reproduce. The generated programs match the shells inside the
    /// domain and stop loudly outside it.
    #[test]
    fn a_run_time_exit_status_matches_the_shells_and_stops_where_they_part() {
        let script = "exit \"${DESHELL_TEST_CODE}\"\n";
        let plan = crate::frontend::lower(
            "end.sh",
            format!("#!/bin/bash\n{script}").as_bytes(),
            crate::config::UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        let crate::ir::Operation::Exit {
            status,
            non_numeric,
        } = &plan.tasks[0].body.operation
        else {
            panic!("expected an exit: {plan:#?}")
        };
        assert_eq!(*non_numeric, crate::ir::NonNumericStatus::Refuse);
        assert_eq!(
            status.parts,
            [crate::ir::TextPart::Variable {
                name: "DESHELL_TEST_CODE".into()
            }]
        );

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("end.sh"), script).unwrap();
        std::fs::write(
            directory.path().join("end.rs"),
            generate_rust(&plan).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.path().join("end.go"), generate_go(&plan).unwrap()).unwrap();
        let built = std::process::Command::new("rustc")
            .arg("end.rs")
            .args(["--edition=2024", "-D", "warnings", "-o", "end-rust"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let built = std::process::Command::new("go")
            .args(["build", "-o", "end-go", "end.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "go build rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );

        // Inside the domain: the same status bash ends with, measured rather
        // than assumed, including the values that wrap.
        for code in ["0", "1", "2", "255", "256", "300", "-1", " 7 "] {
            let shell = std::process::Command::new("bash")
                .arg("end.sh")
                .env("DESHELL_TEST_CODE", code)
                .current_dir(directory.path())
                .status()
                .unwrap();
            for program in ["./end-rust", "./end-go"] {
                let ran = std::process::Command::new(program)
                    .env("DESHELL_TEST_CODE", code)
                    .current_dir(directory.path())
                    .status()
                    .unwrap();
                assert_eq!(
                    ran.code(),
                    shell.code(),
                    "{program} ended differently for {code:?}"
                );
            }
        }

        // Outside it: the shells part, so the programs stop and name the value
        // rather than picking one of them to imitate.
        for program in ["./end-rust", "./end-go"] {
            let ran = std::process::Command::new(program)
                .env("DESHELL_TEST_CODE", "not-a-number")
                .current_dir(directory.path())
                .output()
                .unwrap();
            assert_eq!(ran.status.code(), Some(70), "{program}");
            let stderr = String::from_utf8_lossy(&ran.stderr);
            assert!(
                stderr.contains("not-a-number") && stderr.contains("do not agree"),
                "{program}: {stderr}"
            );
        }
    }

    /// A redirection target is expanded, and the generated programs write where
    /// the shell writes.
    ///
    /// `>>"${GITHUB_OUTPUT}"` is how a workflow appends to a file whose path
    /// arrives in the environment. The target used to have to be a literal, so
    /// the most common redirection in CI was the one that delegated.
    #[test]
    fn a_redirection_target_is_expanded_like_any_other_word() {
        let script = "/bin/echo appended >>\"${DESHELL_TEST_OUT}\"\n";
        let plan = crate::frontend::lower(
            "write.sh",
            format!("#!/bin/bash\n{script}").as_bytes(),
            crate::config::UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        let crate::ir::Operation::Redirect { redirections, .. } = &plan.tasks[0].body.operation
        else {
            panic!("expected a redirect: {plan:#?}")
        };
        let [crate::ir::Redirection::Write { fd, path, append }] = redirections.as_slice() else {
            panic!("expected one write: {redirections:#?}")
        };
        assert_eq!(*fd, 1);
        assert!(*append);
        assert_eq!(
            path.parts,
            [crate::ir::TextPart::Variable {
                name: "DESHELL_TEST_OUT".into()
            }]
        );

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("write.sh"), script).unwrap();
        std::fs::write(
            directory.path().join("write.rs"),
            generate_rust(&plan).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("write.go"),
            generate_go(&plan).unwrap(),
        )
        .unwrap();
        let built = std::process::Command::new("rustc")
            .arg("write.rs")
            .args(["--edition=2024", "-D", "warnings", "-o", "write-rust"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let built = std::process::Command::new("go")
            .args(["build", "-o", "write-go", "write.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "go build rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );

        for (program, arguments) in [
            ("bash", vec!["write.sh"]),
            ("./write-rust", vec![]),
            ("./write-go", vec![]),
        ] {
            let target = directory
                .path()
                .join(format!("{}.out", program.replace(['.', '/'], "")));
            // Appended twice: the second run has to add to the first, which is
            // what separates `>>` from `>`.
            for _ in 0..2 {
                let status = std::process::Command::new(program)
                    .args(&arguments)
                    .env("DESHELL_TEST_OUT", &target)
                    .current_dir(directory.path())
                    .status()
                    .unwrap();
                assert!(status.success(), "{program}");
            }
            assert_eq!(
                std::fs::read_to_string(&target).unwrap(),
                "appended\nappended\n",
                "{program} wrote other bytes"
            );
        }
    }

    /// A shell function arrives as a function, and behaves like one.
    ///
    /// The point of not inlining is that the definition is where "these call
    /// sites are the same check" is written down. So this asserts both: that
    /// one function is emitted and called twice, and that the two programs
    /// agree with bash over inputs that take each path.
    #[test]
    fn a_shell_function_arrives_as_a_function_and_behaves_like_one() {
        let script = concat!(
            "reject() {\n",
            "  case \"$2\" in\n",
            "    *x*)\n",
            "      echo \"$1 holds an x\"\n",
            "      exit 2\n",
            "      ;;\n",
            "  esac\n",
            "}\n",
            "reject first \"$1\"\n",
            "reject second \"$2\"\n",
            "echo accepted\n"
        );
        let plan = crate::frontend::lower(
            "check.sh",
            format!("#!/bin/bash\n{script}").as_bytes(),
            crate::config::UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        assert_eq!(plan.tasks.len(), 2, "{plan:#?}");

        let rust = String::from_utf8(generate_rust(&plan).unwrap()).unwrap();
        let go = String::from_utf8(generate_go(&plan).unwrap()).unwrap();
        if std::env::var("DESHELL_SHOW_GENERATED").is_ok() {
            eprintln!("--- rust ---\n{rust}\n--- go ---\n{go}");
        }
        assert_eq!(
            rust.matches("fn reject(").count(),
            1,
            "one definition:\n{rust}"
        );
        assert_eq!(rust.matches("reject(&[").count(), 2, "two calls:\n{rust}");
        assert_eq!(go.matches("func reject(").count(), 1, "{go}");
        assert_eq!(go.matches("reject([]string{").count(), 2, "{go}");

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("check.sh"), script).unwrap();
        std::fs::write(directory.path().join("check.rs"), &rust).unwrap();
        std::fs::write(directory.path().join("check.go"), &go).unwrap();
        // The project receiving this code decides which lints it runs, and the
        // one this was written for denies `missing_docs` and runs
        // `clippy::pedantic`. Building under both here is how the generator
        // learns what a reader's settings would say, rather than what its own
        // gate happens to allow.
        let built = std::process::Command::new("rustc")
            .arg("check.rs")
            .args([
                "--edition=2024",
                "-D",
                "warnings",
                "-D",
                "missing_docs",
                "-o",
                "check-rust",
            ])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected generated source:\n{}\n{rust}",
            String::from_utf8_lossy(&built.stderr)
        );
        let pedantic = std::process::Command::new("clippy-driver")
            // de-shell's own `clippy.toml` is not the receiving project's:
            // without this the generated program is judged by the rules of the
            // tool that wrote it, which bans `std::process::exit` for reasons
            // that belong to a transactional file layer and not to a script.
            .env("CLIPPY_CONF_DIR", directory.path())
            .arg("check.rs")
            .args([
                "--edition=2024",
                "-D",
                "warnings",
                "-W",
                "clippy::pedantic",
                "--emit=metadata",
            ])
            .current_dir(directory.path())
            .output();
        match pedantic {
            Ok(pedantic) => assert!(
                pedantic.status.success(),
                "clippy::pedantic rejected generated source:\n{}\n{rust}",
                String::from_utf8_lossy(&pedantic.stderr)
            ),
            // A runner without clippy still checks everything above.
            Err(error) => eprintln!("clippy-driver unavailable, skipped: {error}"),
        }
        let built = std::process::Command::new("go")
            .args(["build", "-o", "check-go", "check.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "go build rejected generated source:\n{}\n{go}",
            String::from_utf8_lossy(&built.stderr)
        );

        for arguments in [["ok", "fine"], ["oops", "xyz"], ["bad", "ok"]] {
            let shell = std::process::Command::new("bash")
                .arg("check.sh")
                .args(arguments)
                .current_dir(directory.path())
                .output()
                .unwrap();
            for program in ["./check-rust", "./check-go"] {
                let ran = std::process::Command::new(program)
                    .args(arguments)
                    .current_dir(directory.path())
                    .output()
                    .unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&ran.stdout),
                    String::from_utf8_lossy(&shell.stdout),
                    "{program} wrote other bytes for {arguments:?}"
                );
                assert_eq!(
                    ran.status.code(),
                    shell.status.code(),
                    "{program} ended with another status for {arguments:?}"
                );
            }
        }
    }

    /// What a reader of the migration receives for a `case`.
    ///
    /// The generated program is read and maintained by the project being
    /// migrated, so a pattern becomes an expression rather than a call into a
    /// matcher: a generic machine is code its author never wrote and now has
    /// to keep, and a plan that used `*` and not `?` would carry a branch of it
    /// that nothing reaches. This asserts the shape of what comes out, not just
    /// that it compiles.
    #[test]
    fn a_compiled_case_pattern_is_an_expression_and_not_a_machine() {
        let arm = |pattern: crate::ir::PatternExpression| crate::ir::MatchCase {
            pattern,
            body: node(crate::ir::Operation::NoOp),
        };
        let run =
            |pieces: Vec<crate::ir::PatternPiece>| arm(crate::ir::PatternExpression { pieces });
        let literal = |value: &str| crate::ir::PatternPiece::Literal {
            value: crate::ir::TextExpression::literal(value),
        };
        let plan = plan_with_body(node(crate::ir::Operation::Match {
            value: crate::ir::TextExpression::literal("x"),
            cases: vec![
                run(vec![
                    crate::ir::PatternPiece::AnyRun,
                    literal("\n"),
                    crate::ir::PatternPiece::AnyRun,
                ]),
                run(vec![literal("v"), crate::ir::PatternPiece::AnyRun]),
                run(vec![crate::ir::PatternPiece::AnyRun, literal(".log")]),
                run(vec![literal("Linux/X64")]),
            ],
            default: None,
        }));
        let rust = String::from_utf8(generate_rust(&plan).unwrap()).unwrap();
        let go = String::from_utf8(generate_go(&plan).unwrap()).unwrap();

        for fragment in [
            // A one-character needle is a `char`: the string form compiles and
            // `clippy::pedantic` rejects it, and which lints run is the
            // receiving project's decision.
            "deshell_subject.contains('\\n')",
            "deshell_subject.starts_with('v')",
            "deshell_subject.ends_with(\".log\")",
            "deshell_subject == \"Linux/X64\"",
        ] {
            assert!(rust.contains(fragment), "missing {fragment}:\n{rust}");
        }
        for fragment in [
            "strings.Contains(deshellSubject, \"\\n\")",
            "strings.HasPrefix(deshellSubject, \"v\")",
            "strings.HasSuffix(deshellSubject, \".log\")",
            "deshellSubject == \"Linux/X64\"",
        ] {
            assert!(go.contains(fragment), "missing {fragment}:\n{go}");
        }

        // No machine, and no suppression: both are things the reader would
        // inherit without having asked for them.
        for (name, source) in [("rust", &rust), ("go", &go)] {
            for unwanted in ["DeshellPiece", "deshellPiece", "allow(", "expect(", "kind:"] {
                assert!(
                    !source.contains(unwanted),
                    "generated {name} carries {unwanted}:\n{source}"
                );
            }
        }

        // `unicode/utf8` appears only where a `?` counts characters, because Go
        // rejects an unused import.
        assert!(!go.contains("unicode/utf8"), "{go}");
        let counted = plan_with_body(node(crate::ir::Operation::Match {
            value: crate::ir::TextExpression::literal("x"),
            cases: vec![run(vec![
                literal("a"),
                crate::ir::PatternPiece::AnyCharacter,
                literal("c"),
            ])],
            default: None,
        }));
        let counted = String::from_utf8(generate_go(&counted).unwrap()).unwrap();
        assert!(counted.contains("unicode/utf8"), "{counted}");
        assert!(
            counted.contains("utf8.RuneCountInString(deshellSubject) == 3"),
            "{counted}"
        );
    }

    /// Every pattern the frontend lowers compiles to an expression that
    /// answers what the shells answer.
    ///
    /// The frontend's matcher and the generators' expressions are two separate
    /// implementations of the same rule, so one test reading the measurement is
    /// not enough: this builds a Rust program out of the compiled expressions
    /// and runs it over the same words, which is the only way to find a shape
    /// that reduces wrongly.
    #[test]
    fn every_compiled_pattern_answers_what_the_shells_answer() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/case-pattern-semantics-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let shells: Vec<&str> = corpus["shells"]
            .as_array()
            .expect("corpus lists shells")
            .iter()
            .map(|value| value.as_str().expect("shell is a string"))
            .collect();

        let mut checks = String::new();
        let mut expected = Vec::new();
        for case in corpus["cases"].as_array().expect("corpus has cases") {
            let id = case["id"].as_str().expect("case has an id");
            let script = case["script"].as_str().expect("case has a script");
            let Some(pattern) = script
                .strip_prefix("case \"$1\" in\n  ")
                .and_then(|rest| rest.split(") printf MATCH").next())
            else {
                continue;
            };
            let word = String::from_utf8(crate::frontend::decode_base64(
                case["word_base64"].as_str().expect("case has a word"),
            ))
            .expect("word is UTF-8");
            let answers: std::collections::BTreeSet<&str> = shells
                .iter()
                .map(|shell| case[*shell].as_str().expect("case records this shell"))
                .collect();
            let models: Option<Vec<crate::ir::PatternExpression>> = pattern
                .split('|')
                .map(crate::frontend::case_pattern_for_tests)
                .collect();
            let Some(models) = models else { continue };
            let tests: Result<Vec<String>, String> = models
                .iter()
                .map(|model| rust_pattern_test(model, "subject"))
                .collect();
            let Ok(tests) = tests else { continue };

            let answer = answers.iter().next().copied().expect("one answer");
            assert_eq!(answers.len(), 1, "{id}: the shells disagree");
            expected.push((id.to_owned(), answer == "MATCH"));
            checks.push_str(&format!(
                // `let _ = &subject` because a pattern that matches anything
                // compiles to `true` and reads nothing, and this program is
                // built under `-D warnings` like the generated ones are.
                "    {{ let subject = {word:?}; let _ = &subject; println!(\"{{}}\", {}); }}\n",
                tests.join(" || ")
            ));
        }
        assert!(expected.len() >= 20, "{} patterns compiled", expected.len());

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("patterns.rs");
        std::fs::write(&source, format!("fn main() {{\n{checks}}}\n")).unwrap();
        let built = std::process::Command::new("rustc")
            .arg(&source)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("patterns"))
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected the compiled patterns:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let ran = std::process::Command::new(directory.path().join("patterns"))
            .output()
            .unwrap();
        let answers: Vec<&str> = std::str::from_utf8(&ran.stdout).unwrap().lines().collect();
        assert_eq!(answers.len(), expected.len());
        for ((id, wanted), answer) in expected.iter().zip(answers) {
            assert_eq!(
                answer,
                if *wanted { "true" } else { "false" },
                "{id}: the compiled expression answers differently than the shells"
            );
        }
    }

    /// The generated programs match a `case` pattern the way the shell does.
    ///
    /// Both are built and run against the same words, because a generator that
    /// compared for equality where the shell matched a pattern would produce a
    /// program that compiles, runs, and takes the wrong arm. The pattern here
    /// is the one `action.yml` writes for a line-break guard, spelled with a
    /// real newline: `$'\n'` is resolved during word expansion, so what
    /// reaches the pattern is an ordinary character.
    #[test]
    fn generated_programs_match_a_case_pattern_the_way_the_shell_does() {
        let arm = |pattern: crate::ir::PatternExpression, text: &str| crate::ir::MatchCase {
            pattern,
            body: node(crate::ir::Operation::WriteStdout {
                contents: crate::ir::TextExpression::literal(text),
            }),
        };
        let any_newline = crate::ir::PatternExpression {
            pieces: vec![
                crate::ir::PatternPiece::AnyRun,
                crate::ir::PatternPiece::Literal {
                    value: crate::ir::TextExpression::literal("\n"),
                },
                crate::ir::PatternPiece::AnyRun,
            ],
        };
        let plan = plan_with_body(node(crate::ir::Operation::Match {
            value: crate::ir::TextExpression {
                parts: vec![crate::ir::TextPart::Argument { name: "1".into() }],
            },
            cases: vec![
                arm(any_newline, "REJECTED\n"),
                // A quoted star is a character: this arm is an equality test,
                // and the generator has to emit it as one.
                arm(crate::ir::PatternExpression::literal("a*c"), "STAR\n"),
            ],
            default: Some(Box::new(node(crate::ir::Operation::WriteStdout {
                contents: crate::ir::TextExpression::literal("ACCEPTED\n"),
            }))),
        }));

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("original.sh"),
            "case \"$1\" in\n  *'\n'*) printf 'REJECTED\\n' ;;\n  'a*c') printf 'STAR\\n' ;;\n  *) printf 'ACCEPTED\\n' ;;\nesac\n",
        )
        .unwrap();

        let rust = directory.path().join("matching.rs");
        std::fs::write(&rust, generate_rust(&plan).unwrap()).unwrap();
        let built = std::process::Command::new("rustc")
            .arg(&rust)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("matching-rust"))
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        std::fs::write(
            directory.path().join("matching.go"),
            generate_go(&plan).unwrap(),
        )
        .unwrap();
        let built = std::process::Command::new("go")
            .args(["build", "-o", "matching-go", "matching.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "go build rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );

        for word in ["a\nb", "abc", "a*c", "", "\n", "aXc"] {
            let shell = std::process::Command::new("bash")
                .arg("original.sh")
                .arg(word)
                .current_dir(directory.path())
                .output()
                .unwrap();
            for program in ["matching-rust", "matching-go"] {
                let ran = std::process::Command::new(directory.path().join(program))
                    .arg(word)
                    .output()
                    .unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&ran.stdout),
                    String::from_utf8_lossy(&shell.stdout),
                    "{program} took a different arm for {word:?}"
                );
            }
        }
    }

    /// The generated programs end where the shell ends, with the same bytes and
    /// the same status.
    ///
    /// Compiling is not enough for `exit`: a generator that emitted it as a
    /// status rather than as a jump would produce a program that compiles and
    /// keeps running. So both are built and run, and their output is compared
    /// against bash running the script they replaced.
    #[test]
    fn generated_programs_stop_at_exit_exactly_where_bash_does() {
        let plan = plan_with_body(node(crate::ir::Operation::Match {
            value: crate::ir::TextExpression::literal("1"),
            cases: vec![
                crate::ir::MatchCase {
                    pattern: crate::ir::PatternExpression::literal("0"),
                    body: node(crate::ir::Operation::NoOp),
                },
                crate::ir::MatchCase {
                    pattern: crate::ir::PatternExpression::literal("1"),
                    body: node(crate::ir::Operation::Sequence {
                        nodes: vec![
                            node(crate::ir::Operation::WriteStdout {
                                contents: crate::ir::TextExpression::literal("before\n"),
                            }),
                            node(crate::ir::Operation::Exit {
                                status: crate::ir::TextExpression::literal("3"),
                                non_numeric: crate::ir::NonNumericStatus::Unreachable,
                            }),
                            node(crate::ir::Operation::WriteStdout {
                                contents: crate::ir::TextExpression::literal("after\n"),
                            }),
                        ],
                        on_failure: crate::ir::SequenceFailure::Continue,
                    }),
                },
            ],
            default: None,
        }));
        let directory = tempfile::tempdir().unwrap();

        // What the script this replaces does, measured rather than assumed.
        std::fs::write(
            directory.path().join("original.sh"),
            "case '1' in\n  0) ;;\n  1)\n    echo before\n    exit 3\n    echo after\n    ;;\nesac\n",
        )
        .unwrap();
        let shell = std::process::Command::new("bash")
            .arg("original.sh")
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&shell.stdout), "before\n");
        assert_eq!(shell.status.code(), Some(3));

        let rust = directory.path().join("exiting.rs");
        std::fs::write(&rust, generate_rust(&plan).unwrap()).unwrap();
        let built = std::process::Command::new("rustc")
            .arg(&rust)
            .args(["--edition=2024", "-D", "warnings", "-o"])
            .arg(directory.path().join("exiting-rust"))
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "rustc rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let ran = std::process::Command::new(directory.path().join("exiting-rust"))
            .output()
            .unwrap();
        assert_eq!(ran.stdout, shell.stdout, "generated Rust wrote other bytes");
        assert_eq!(ran.status.code(), shell.status.code());

        std::fs::write(
            directory.path().join("exiting.go"),
            generate_go(&plan).unwrap(),
        )
        .unwrap();
        let vet = std::process::Command::new("go")
            .args(["vet", "exiting.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            vet.status.success(),
            "go vet rejected generated source:\n{}",
            String::from_utf8_lossy(&vet.stderr)
        );
        // Built rather than `go run`: `go run` reports 1 for any non-zero exit
        // and prints the real status to its own stderr, which would hide exactly
        // what this checks.
        let built = std::process::Command::new("go")
            .args(["build", "-o", "exiting-go", "exiting.go"])
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "go build rejected generated source:\n{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let ran = std::process::Command::new(directory.path().join("exiting-go"))
            .output()
            .unwrap();
        assert_eq!(ran.stdout, shell.stdout, "generated Go wrote other bytes");
        assert_eq!(ran.status.code(), shell.status.code());
    }

    /// A build that ran out of time is not a build that failed.
    ///
    /// A timeout leaves exit 124, so a check that reads the status first calls
    /// it a build failure — which is what it did, and what sent a reader to
    /// inspect generated code that had compiled fine the run before.
    #[test]
    fn a_build_that_ran_out_of_budget_is_not_reported_as_one_that_failed() {
        let outcome = |exit_code, timed_out, limit_exceeded| crate::agent_process::Outcome {
            exit_code,
            stdout: Vec::new(),
            stderr: Vec::new(),
            timed_out,
            limit_exceeded,
            signal: None,
        };

        assert_eq!(classify_build(&outcome(0, false, None), 1000), None);

        let timed_out = classify_build(&outcome(124, true, None), 30_000).unwrap();
        assert!(
            timed_out.contains("30000 ms budget") && !timed_out.contains("failed"),
            "{timed_out}"
        );

        let limited = classify_build(&outcome(137, false, Some("memory".into())), 1000).unwrap();
        assert!(
            limited.contains("memory limit") && !limited.contains("failed"),
            "{limited}"
        );

        // A real failure still reads as one, and still names the status.
        let failed = classify_build(&outcome(101, false, None), 1000).unwrap();
        assert!(failed.contains("failed with exit 101"), "{failed}");

        // A toolchain gets a build-sized budget; a script keeps the scenario's.
        let scenario = crate::config::ResourceLimits {
            timeout_ms: 30_000,
            memory_bytes: 1024,
            processes: 8,
            stdout_bytes: 1024,
            stderr_bytes: 1024,
        };
        let build = verification_build_limits(&["cargo".into(), "build".into()], scenario);
        assert_eq!(build.timeout_ms, TOOLCHAIN_BUILD_TIMEOUT_MS);
        let script = verification_build_limits(&["./build.sh".into()], scenario);
        assert_eq!(script.timeout_ms, scenario.timeout_ms);
    }

    #[test]
    fn proposal_diff_and_literal_reference_helpers_preserve_exact_bytes() {
        let mut proposal = signed_proposal(b"created\n".to_vec());
        let updated = b"updated-without-newline".to_vec();
        proposal.patches.push(GeneratorPatch {
            operation: PatchOperation::Update,
            path: "caller.py".into(),
            expected_digest: Some(crate::digest::sha256(b"before")),
            content_base64: base64::engine::general_purpose::STANDARD.encode(&updated),
            content_digest: crate::digest::sha256(&updated),
            permissions: 0o644,
        });
        resign(&mut proposal);
        let diff = proposal_diff(&[proposal]).unwrap();
        assert!(diff.contains("--- /dev/null\n+++ b/src/bin/generated.rs\n+created\n"));
        assert!(diff.contains(
            "--- a/caller.py\n+++ b/caller.py\n@@ replacement @@\n+updated-without-newline\n"
        ));

        let mut binary = signed_proposal(vec![0xff]);
        resign(&mut binary);
        assert!(proposal_diff(&[binary]).unwrap_err().contains("not UTF-8"));
        assert_eq!(source_stem("path/to/build.test.sh"), "build_test");
        assert_eq!(rust_binary_name("9_build"), "deshell_9_build");
        assert_eq!(
            blocker_code("DESHELL_BLOCKER_CODE: detail", "fallback"),
            "DESHELL_BLOCKER_CODE"
        );
        assert_eq!(blocker_code("no code", "fallback"), "fallback");
        assert!(exact_dependency_pin("1.2.3"));
        assert!(!exact_dependency_pin("^1.2"));
        for argv in [vec![], vec!["-program".into()], vec!["program\0bad".into()]] {
            assert!(validate_exact_argv(&argv).is_err());
        }
    }

    fn signed_migration_plan() -> MigrationPlan {
        let digest = crate::digest::sha256(b"bound");
        let mut plan = MigrationPlan {
            schema_version: 1,
            kind: PlanKind::Migration,
            plan_digest: ZERO_DIGEST.into(),
            inventory_digest: digest.clone(),
            sources: vec![PlanSource {
                location: Location {
                    path: "build.sh".into(),
                    start_byte: 0,
                    end_byte: 4,
                },
                kind: SourceKind::ShellFile,
                interpreter: "sh".into(),
                content_digest: digest.clone(),
                ir_digest: digest.clone(),
                proposal_digest: Some(digest.clone()),
            }],
            proposals: vec![digest.clone()],
            required_scenarios: vec![ScenarioRequirement {
                name: "default".into(),
                digest: digest.clone(),
                approval_digest: format!("sha256:{digest}"),
            }],
            required_cells: vec![CellRequirement {
                id: "linux-x86_64".into(),
                platform_fingerprint: digest.clone(),
                runtime_fingerprint: digest.clone(),
                approval_digest: format!("sha256:{digest}"),
            }],
            validation_commands: vec![ExactCommand {
                name: "test".into(),
                kind: crate::config::ValidationKind::Test,
                argv: vec!["cargo".into(), "test".into()],
            }],
            validation_limits: crate::config::ResourceLimits::DEFAULT,
            network_replay_digest: Some(digest),
            coverage: Coverage {
                total_bytes: 4,
                native_bytes: 4,
                ..Coverage::default()
            },
            blockers: Vec::new(),
        };
        plan.plan_digest = plan.computed_digest().unwrap();
        plan
    }

    fn resign_plan(plan: &mut MigrationPlan) {
        plan.plan_digest = plan.computed_digest().unwrap();
    }

    #[test]
    fn migration_plan_validation_binds_inventory_locations_matrix_and_exact_commands() {
        let plan = signed_migration_plan();
        plan.validate().unwrap();
        assert_eq!(
            MigrationPlan::decode(&pretty_bytes(&plan).unwrap()).unwrap(),
            plan
        );

        let mut cases: Vec<(MigrationPlan, &str)> = Vec::new();
        let mut candidate = signed_migration_plan();
        candidate.schema_version = 2;
        resign_plan(&mut candidate);
        cases.push((candidate, "fresh schema"));
        let mut candidate = signed_migration_plan();
        candidate.inventory_digest = "bad".into();
        resign_plan(&mut candidate);
        cases.push((candidate, "inventory digest"));
        let mut candidate = signed_migration_plan();
        candidate.proposals[0] = "bad".into();
        resign_plan(&mut candidate);
        cases.push((candidate, "invalid SHA-256"));
        let mut candidate = signed_migration_plan();
        candidate.network_replay_digest = Some("bad".into());
        resign_plan(&mut candidate);
        cases.push((candidate, "network replay digest"));
        let mut candidate = signed_migration_plan();
        candidate.sources[0].interpreter = "unknown".into();
        resign_plan(&mut candidate);
        cases.push((candidate, "unknown source interpreter"));
        let mut candidate = signed_migration_plan();
        candidate.sources[0].location.path = "../outside".into();
        resign_plan(&mut candidate);
        cases.push((candidate, "source location"));
        let mut candidate = signed_migration_plan();
        candidate.sources[0].proposal_digest = Some(crate::digest::sha256(b"other"));
        resign_plan(&mut candidate);
        cases.push((candidate, "source proposal"));
        let mut candidate = signed_migration_plan();
        candidate.required_cells[0].platform_fingerprint = "bad".into();
        resign_plan(&mut candidate);
        cases.push((candidate, "platform cell"));
        let mut candidate = signed_migration_plan();
        candidate.validation_commands[0].argv = vec!["-option".into()];
        resign_plan(&mut candidate);
        cases.push((candidate, "validation command"));
        let mut candidate = signed_migration_plan();
        candidate.coverage.total_bytes = 5;
        resign_plan(&mut candidate);
        cases.push((candidate, "coverage"));
        let mut candidate = signed_migration_plan();
        candidate.validation_limits.timeout_ms = 0;
        resign_plan(&mut candidate);
        cases.push((candidate, "validation limits"));

        for (candidate, expected) in cases {
            let error = candidate.validate().unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }
        let mut tampered = signed_migration_plan();
        tampered.coverage.native_bytes = 3;
        assert!(tampered.validate().unwrap_err().contains("digest mismatch"));
    }

    #[test]
    fn proposal_validation_rejects_all_unbound_patch_dependency_and_source_map_fields() {
        let mut cases: Vec<(Proposal, &str)> = Vec::new();
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.schema_version = 2;
        resign(&mut proposal);
        cases.push((proposal, "version or request digest"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.generator_digest = "bad".into();
        resign(&mut proposal);
        cases.push((proposal, "generator digest"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.patches.clear();
        resign(&mut proposal);
        cases.push((proposal, "no create/update"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.patches[0].expected_digest = Some(crate::digest::sha256(b"before"));
        resign(&mut proposal);
        cases.push((proposal, "create proposal"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.patches[0].operation = PatchOperation::Update;
        resign(&mut proposal);
        cases.push((proposal, "update proposal"));
        for field in 0..3 {
            let mut proposal = signed_proposal(b"value".to_vec());
            match field {
                0 => proposal.build_argv.clear(),
                1 => proposal.run_argv = vec!["-bad".into()],
                _ => proposal.validation.push(vec!["bad\0argv".into()]),
            }
            resign(&mut proposal);
            cases.push((proposal, "exact argv"));
        }
        for (name, version, duplicate, expected) in [
            ("", "1.0.0", false, "dependency names"),
            ("dep", "^1.0", false, "exact pin"),
            ("dep", "1.0.0", true, "dependency names"),
        ] {
            let mut proposal = signed_proposal(b"value".to_vec());
            let dependency = Dependency {
                ecosystem: DependencyEcosystem::Cargo,
                name: name.into(),
                version: version.into(),
            };
            proposal.dependencies.push(dependency.clone());
            if duplicate {
                proposal.dependencies.push(dependency);
            }
            resign(&mut proposal);
            cases.push((proposal, expected));
        }
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.source_map.clear();
        resign(&mut proposal);
        cases.push((proposal, "source map must not be empty"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.source_map[0].ir_node = "UPPERCASE00000000000000000000000".into();
        resign(&mut proposal);
        cases.push((proposal, "invalid IR node"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.source_map[0].generated.path = "other.rs".into();
        resign(&mut proposal);
        cases.push((proposal, "path has no patch"));
        let mut proposal = signed_proposal(b"value".to_vec());
        proposal.source_map.push(proposal.source_map[0].clone());
        resign(&mut proposal);
        cases.push((proposal, "duplicate generated span"));

        for (proposal, expected) in cases {
            let error = validate_proposal(&proposal).unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }
        let mut tampered = signed_proposal(b"value".to_vec());
        tampered.run_argv.push("changed".into());
        assert!(
            validate_proposal(&tampered)
                .unwrap_err()
                .contains("digest mismatch")
        );
    }

    fn observation() -> Observation {
        Observation {
            exit_code: 0,
            signal: None,
            timed_out: false,
            stdout_base64: String::new(),
            stderr_base64: String::new(),
            files: Vec::new(),
            network: Vec::new(),
        }
    }

    fn valid_migration_evidence() -> MigrationEvidence {
        let digest = crate::digest::sha256(b"bound");
        let comparison = TripleComparison {
            original: observation(),
            ir: observation(),
            replacement: observation(),
            differences: Vec::new(),
        };
        MigrationEvidence {
            schema_version: 1,
            plan_digest: digest.clone(),
            approval_digests: vec![format!("sha256:{digest}")],
            cell: "linux".into(),
            status: EvidenceStatus::Verified,
            repetitions: 2,
            checks: vec![EvidenceCheck {
                source: Location {
                    path: "build.sh".into(),
                    start_byte: 0,
                    end_byte: 4,
                },
                scenario: "default".into(),
                key: EvidenceKey {
                    source_digest: digest.clone(),
                    ir_digest: digest.clone(),
                    proposal_digest: digest.clone(),
                    generator_digest: digest.clone(),
                    toolchain_digest: digest.clone(),
                    scenario_digest: digest.clone(),
                    platform_fingerprint: digest.clone(),
                    runtime_fingerprint: digest,
                },
                status: EvidenceStatus::Verified,
                error: None,
                covered_nodes: vec!["00000000000000000000000000000000".into()],
                comparisons: vec![comparison.clone(), comparison],
            }],
            validation: Vec::new(),
        }
    }

    #[test]
    fn migration_evidence_validation_and_comparison_cover_every_observable_difference() {
        assert_eq!(
            [
                EvidenceStatus::Verified,
                EvidenceStatus::Different,
                EvidenceStatus::Unavailable,
                EvidenceStatus::Failed,
                EvidenceStatus::Nondeterministic
            ]
            .map(EvidenceStatus::as_str),
            [
                "verified",
                "different",
                "unavailable",
                "failed",
                "nondeterministic"
            ]
        );
        let evidence = valid_migration_evidence();
        validate_evidence_document(&evidence).unwrap();
        assert_eq!(
            MigrationEvidence::decode(&evidence.encode_pretty().unwrap()).unwrap(),
            evidence
        );

        let mut cases: Vec<(MigrationEvidence, &str)> = Vec::new();
        let mut value = valid_migration_evidence();
        value.repetitions = 1;
        cases.push((value, "at least two repetitions"));
        let mut value = valid_migration_evidence();
        value.plan_digest = "bad".into();
        cases.push((value, "plan digest or cell"));
        let mut value = valid_migration_evidence();
        value.checks.clear();
        cases.push((value, "contains no source"));
        let mut value = valid_migration_evidence();
        value.checks[0].status = EvidenceStatus::Unavailable;
        cases.push((value, "requires an error"));
        let mut value = valid_migration_evidence();
        value.checks[0].comparisons.pop();
        cases.push((value, "repetition count"));
        let mut value = valid_migration_evidence();
        value.checks[0].key.ir_digest = "bad".into();
        cases.push((value, "invalid digest"));
        let mut value = valid_migration_evidence();
        value.checks[0].comparisons[0]
            .differences
            .push("ir.stdout".into());
        cases.push((value, "contains differences"));
        for (value, expected) in cases {
            let error = validate_evidence_document(&value).unwrap_err();
            assert!(error.contains(expected), "missing {expected:?} in {error}");
        }

        let state = |path: &str, digest: &str, executable| crate::workspace::FileState {
            path: path.into(),
            sha256: digest.into(),
            executable,
        };
        let before = crate::workspace::Snapshot {
            files: vec![
                state("modified", "a", false),
                state("removed", "b", true),
                state("same", "c", false),
            ],
        };
        let after = crate::workspace::Snapshot {
            files: vec![
                state("created", "d", true),
                state("modified", "z", false),
                state("same", "c", false),
            ],
        };
        let changes = file_changes(&before, &after);
        assert_eq!(
            changes.iter().map(|change| change.kind).collect::<Vec<_>>(),
            vec![
                FileChangeKind::Created,
                FileChangeKind::Modified,
                FileChangeKind::Removed
            ]
        );

        let original = observation();
        let mut different = observation();
        different.exit_code = 1;
        different.signal = Some(9);
        different.timed_out = true;
        different.stdout_base64 = "stdout".into();
        different.stderr_base64 = "stderr".into();
        different.files = changes;
        different.network.push(crate::replay::NetworkExchange {
            sequence: 0,
            method: "GET".into(),
            uri: "http://example.test".into(),
            request_body_sha256: crate::digest::sha256(b""),
            status: 200,
            response_body_sha256: crate::digest::sha256(b"body"),
        });
        let differences = compare_three(&original, &different, &different);
        assert_eq!(differences.len(), 14);
        let comparisons = vec![
            TripleComparison {
                original: original.clone(),
                ir: original.clone(),
                replacement: original.clone(),
                differences: vec![],
            },
            TripleComparison {
                original: different.clone(),
                ir: original.clone(),
                replacement: original,
                differences: vec![],
            },
        ];
        assert!(subject_changed(&comparisons, |comparison| &comparison.original));
        assert!(!subject_changed(&comparisons, |comparison| &comparison.ir));
    }

    #[test]
    fn immutable_artifact_persistence_reuses_equal_bytes_and_rejects_collisions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("artifact");
        persist_immutable(&path, b"one".to_vec()).unwrap();
        persist_immutable(&path, b"one".to_vec()).unwrap();
        assert!(
            persist_immutable(&path, b"two".to_vec())
                .unwrap_err()
                .contains("bytes differ")
        );
        let directory = root.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        assert!(
            persist_immutable(&directory, b"value".to_vec())
                .unwrap_err()
                .contains("not a regular file")
        );
    }
}
