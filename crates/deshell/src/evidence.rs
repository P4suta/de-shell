use crate::ir::{Guarantee, Plan};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub schema_version: u32,
    pub plan_digest: String,
    pub source: EvidenceSource,
    pub nodes: Vec<NodeEvidence>,
    pub observations: Vec<ObservationEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceSource {
    pub path: String,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeEvidence {
    pub id: String,
    pub operation: String,
    pub guarantee: Guarantee,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationEvidence {
    pub scenario: String,
    pub key: ObservationKey,
    pub status: ObservationStatus,
    pub provider: String,
    pub reason: Option<String>,
    pub digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationKey {
    pub scenario_digest: String,
    pub provider_fingerprint: String,
    pub runtime_lock_digest: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObservationStatus {
    Verified,
    Different,
    Unavailable,
    Failed,
    Nondeterministic,
}

impl Evidence {
    pub(crate) fn from_plan(plan: &Plan, source_path: &str, source: &[u8]) -> Result<Self, String> {
        plan.validate().map_err(|errors| errors.join("; "))?;
        let normalized = crate::ir::normalize_path(source_path)?;
        if normalized != source_path {
            return Err(format!("source path is not normalized: {source_path}"));
        }
        let mut nodes = Vec::new();
        for task in &plan.tasks {
            collect_nodes(&task.body, &mut nodes);
        }
        Ok(Self {
            schema_version: 1,
            plan_digest: plan_digest(plan)?,
            source: EvidenceSource {
                path: normalized,
                content_hash: crate::digest::sha256(source),
            },
            nodes,
            observations: Vec::new(),
        })
    }

    pub(crate) fn decode(input: &[u8]) -> Result<Self, Vec<String>> {
        let evidence: Self = crate::strict_json::decode(input).map_err(|error| vec![error])?;
        evidence.validate_document()?;
        Ok(evidence)
    }

    pub(crate) fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        self.validate_document()
            .map_err(|errors| errors.join("; "))?;
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        crate::canonical_json::pretty_bytes(&value)
    }

    pub(crate) fn validate_against(
        &self,
        plan: &Plan,
        current_source: &[u8],
    ) -> Result<(), Vec<String>> {
        let mut errors = self.validate_document().err().unwrap_or_default();
        match plan_digest(plan) {
            Ok(digest) if digest != self.plan_digest => {
                errors.push("evidence plan digest mismatch".into())
            }
            Err(error) => errors.push(error),
            Ok(_) => {}
        }
        if crate::digest::sha256(current_source) != self.source.content_hash {
            errors.push(format!(
                "evidence source digest mismatch for {}",
                self.source.path
            ));
        }
        let mut expected = Vec::new();
        for task in &plan.tasks {
            collect_nodes(&task.body, &mut expected);
        }
        if expected != self.nodes {
            errors.push("evidence node inventory does not match plan".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub(crate) fn append_observation(
        &mut self,
        mut observation: ObservationEvidence,
    ) -> Result<ObservationStatus, String> {
        validate_observation(&observation).map_err(|errors| errors.join("; "))?;
        if let Some(previous) = self
            .observations
            .iter()
            .rev()
            .find(|previous| previous.key == observation.key)
        {
            if previous == &observation {
                return Ok(previous.status);
            }
            if observation_result(previous) != observation_result(&observation) {
                let current_status = observation.status;
                let current_digest = observation.digest.clone();
                observation.status = ObservationStatus::Nondeterministic;
                observation.reason = Some(format!(
                    "conflicting observations for the same scenario/provider/runtime key (previous status {:?}, digest {:?}; current status {:?}, digest {:?})",
                    previous.status, previous.digest, current_status, current_digest
                ));
                observation.digest = None;
            }
        }
        let status = observation.status;
        self.observations.push(observation);
        Ok(status)
    }

    fn validate_document(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != 1 {
            errors.push(format!(
                "evidence schema_version must be 1 (found {})",
                self.schema_version
            ));
        }
        if !crate::digest::valid_sha256(&self.plan_digest) {
            errors.push("evidence plan_digest is invalid".into());
        }
        if !crate::digest::valid_sha256(&self.source.content_hash) {
            errors.push("evidence source content_hash is invalid".into());
        }
        match crate::ir::normalize_path(&self.source.path) {
            Ok(path) if path == self.source.path => {}
            Ok(_) => errors.push("evidence source path is not normalized".into()),
            Err(error) => errors.push(format!("invalid evidence source path: {error}")),
        }
        let mut node_ids = std::collections::BTreeSet::new();
        for node in &self.nodes {
            if !node_ids.insert(&node.id) {
                errors.push(format!("duplicate evidence node id: {}", node.id));
            }
            if node.operation.is_empty() {
                errors.push(format!("evidence operation is empty for node {}", node.id));
            }
        }
        for observation in &self.observations {
            if let Err(mut observation_errors) = validate_observation(observation) {
                errors.append(&mut observation_errors);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn plan_digest(plan: &Plan) -> Result<String, String> {
    plan.validate().map_err(|errors| errors.join("; "))?;
    let value = serde_json::to_value(plan).map_err(|error| error.to_string())?;
    let canonical = crate::canonical_json::canonical_bytes(&value)?;
    Ok(crate::digest::sha256(&canonical))
}

fn collect_nodes(node: &crate::ir::Node, output: &mut Vec<NodeEvidence>) {
    output.push(NodeEvidence {
        id: node.id.clone(),
        operation: node.operation.name().into(),
        guarantee: node.guarantee.clone(),
    });
    match &node.operation {
        crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Sequence { nodes }
        | crate::ir::Operation::Parallel { nodes } => {
            for child in nodes {
                collect_nodes(child, output);
            }
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            collect_nodes(predicate, output);
            collect_nodes(if_true, output);
            if let Some(child) = if_false {
                collect_nodes(child, output);
            }
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            for case in cases {
                collect_nodes(&case.body, output);
            }
            if let Some(child) = default {
                collect_nodes(child, output);
            }
        }
        crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. } => collect_nodes(body, output),
        crate::ir::Operation::TryFinally { body, finalizer } => {
            collect_nodes(body, output);
            collect_nodes(finalizer, output);
        }
        crate::ir::Operation::Exec { .. }
        | crate::ir::Operation::TaskCall { .. }
        | crate::ir::Operation::SetVariable { .. }
        | crate::ir::Operation::FileRead { .. }
        | crate::ir::Operation::FileWrite { .. }
        | crate::ir::Operation::FileRemove { .. }
        | crate::ir::Operation::NetworkRequest { .. }
        | crate::ir::Operation::InterpreterCall { .. }
        | crate::ir::Operation::OpaqueCapsule { .. } => {}
    }
}

fn validate_observation(observation: &ObservationEvidence) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if observation.scenario.trim().is_empty() {
        errors.push("observation scenario must not be empty".into());
    }
    for (name, digest) in [
        ("scenario_digest", &observation.key.scenario_digest),
        (
            "provider_fingerprint",
            &observation.key.provider_fingerprint,
        ),
        ("runtime_lock_digest", &observation.key.runtime_lock_digest),
    ] {
        if !crate::digest::valid_sha256(digest) {
            errors.push(format!("observation {name} must be a SHA-256 digest"));
        }
    }
    if observation.provider.trim().is_empty() {
        errors.push("observation provider must not be empty".into());
    }
    match observation.status {
        ObservationStatus::Verified | ObservationStatus::Different => {
            if !observation
                .digest
                .as_deref()
                .is_some_and(crate::digest::valid_sha256)
            {
                errors.push("verified or different observation requires a SHA-256 digest".into());
            }
            if observation.status == ObservationStatus::Different
                && observation.reason.as_deref().is_none_or(str::is_empty)
            {
                errors.push("different observation requires a reason".into());
            }
        }
        ObservationStatus::Unavailable
        | ObservationStatus::Failed
        | ObservationStatus::Nondeterministic => {
            if observation.reason.as_deref().is_none_or(str::is_empty) {
                errors.push("unavailable or failed observation requires a reason".into());
            }
            if observation.digest.is_some() {
                errors.push("unavailable or failed observation must not have a digest".into());
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn observation_result(observation: &ObservationEvidence) -> (ObservationStatus, Option<&str>) {
    (observation.status, observation.digest.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Node, Operation, SourceSpan, Task, TextExpression};

    fn plan() -> Plan {
        let mut plan = Plan {
            schema_version: 1,
            generator: "deshell/0.1.0".into(),
            entrypoint: "main".into(),
            tasks: vec![Task {
                name: "main".into(),
                inputs: vec![],
                outputs: vec![],
                environment: vec![],
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                invocation: None,
                body: Node {
                    id: String::new(),
                    operation: Operation::Exec {
                        argv: vec![TextExpression::literal("true")],
                        environment: vec![],
                        working_directory: None,
                    },
                    guarantee: Guarantee::Native {
                        semantic_model: "literal-v1".into(),
                    },
                    source: Some(SourceSpan {
                        file: "build.sh".into(),
                        start_line: 1,
                        start_column: 0,
                        end_line: 1,
                        end_column: 4,
                        start_byte: 0,
                        end_byte: 4,
                    }),
                },
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    fn key() -> ObservationKey {
        ObservationKey {
            scenario_digest: "1".repeat(64),
            provider_fingerprint: "2".repeat(64),
            runtime_lock_digest: "3".repeat(64),
        }
    }

    #[test]
    fn evidence_binds_canonical_plan_and_raw_source_bytes() {
        let plan = plan();
        let evidence = Evidence::from_plan(&plan, "build.sh", b"true\n").unwrap();
        assert_eq!(evidence.schema_version, 1);
        assert_eq!(evidence.plan_digest.len(), 64);
        assert_eq!(evidence.source.content_hash.len(), 64);
        evidence.validate_against(&plan, b"true\n").unwrap();
        assert!(evidence.validate_against(&plan, b"false\n").is_err());
    }

    #[test]
    fn observation_is_appended_without_mutating_plan_bytes() {
        let plan = plan();
        let before = plan.encode_pretty().unwrap();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"true\n").unwrap();
        evidence
            .append_observation(ObservationEvidence {
                scenario: "default".into(),
                key: key(),
                status: ObservationStatus::Verified,
                provider: "local-test".into(),
                reason: None,
                digest: Some("a".repeat(64)),
            })
            .unwrap();
        assert_eq!(plan.encode_pretty().unwrap(), before);
        assert_eq!(evidence.observations.len(), 1);
        let encoded = evidence.encode_pretty().unwrap();
        assert_eq!(Evidence::decode(&encoded).unwrap(), evidence);
    }

    #[test]
    fn verified_observation_requires_digest_and_a_complete_key() {
        let plan = plan();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"true\n").unwrap();
        assert!(
            evidence
                .append_observation(ObservationEvidence {
                    scenario: "same".into(),
                    key: key(),
                    status: ObservationStatus::Verified,
                    provider: "test".into(),
                    reason: None,
                    digest: None,
                })
                .is_err()
        );
    }

    #[test]
    fn conflicting_results_for_the_same_key_become_nondeterministic() {
        let plan = plan();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"true\n").unwrap();
        for digest in ["a".repeat(64), "b".repeat(64)] {
            evidence
                .append_observation(ObservationEvidence {
                    scenario: "default".into(),
                    key: key(),
                    status: ObservationStatus::Verified,
                    provider: "test".into(),
                    reason: None,
                    digest: Some(digest),
                })
                .unwrap();
        }
        assert_eq!(
            evidence.observations.last().unwrap().status,
            ObservationStatus::Nondeterministic
        );
        assert!(evidence.observations.last().unwrap().digest.is_none());
    }

    #[test]
    fn tampering_and_unknown_fields_are_rejected() {
        let plan = plan();
        let evidence = Evidence::from_plan(&plan, "build.sh", b"true\n").unwrap();
        let encoded = String::from_utf8(evidence.encode_pretty().unwrap()).unwrap();
        let tampered = encoded.replacen(&evidence.plan_digest, &"0".repeat(64), 1);
        let decoded = Evidence::decode(tampered.as_bytes()).unwrap();
        assert!(decoded.validate_against(&plan, b"true\n").is_err());
        let unknown = encoded.replacen("{\n", "{\n  \"future\": true,\n", 1);
        assert!(Evidence::decode(unknown.as_bytes()).is_err());
    }
}
