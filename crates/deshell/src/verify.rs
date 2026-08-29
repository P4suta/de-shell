use crate::evidence::{Evidence, ObservationEvidence, ObservationStatus};
use crate::ir::{Guarantee, Node, Operation, Plan};
use crate::runner::{RunResult, TraceEvent};
use base64::Engine as _;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuditReport {
    pub native: usize,
    pub delegated: usize,
    pub residual: usize,
    pub residual_reasons: Vec<String>,
    pub observations: usize,
    pub verified: usize,
    pub different: usize,
    pub unavailable: usize,
    pub failed: usize,
    pub nondeterministic: usize,
    pub stale: usize,
    pub unobserved: usize,
    pub source_bytes: usize,
    pub native_bytes: usize,
    pub delegated_bytes: usize,
    pub residual_bytes: usize,
    pub uncovered_bytes: usize,
}

pub(crate) struct AuditContext<'a> {
    pub source_path: &'a str,
    pub source_bytes: usize,
    pub scenario_digests: &'a std::collections::BTreeMap<String, String>,
    pub runtime_lock_digest: &'a str,
    pub lab_image: &'a str,
    pub provider_fingerprint: &'a str,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn audit(plan: &Plan, evidence: Option<&Evidence>) -> Result<AuditReport, Vec<String>> {
    audit_inner(plan, evidence, None)
}

pub(crate) fn audit_current(
    plan: &Plan,
    evidence: &Evidence,
    context: AuditContext<'_>,
) -> Result<AuditReport, Vec<String>> {
    audit_inner(plan, Some(evidence), Some(context))
}

fn audit_inner(
    plan: &Plan,
    evidence: Option<&Evidence>,
    context: Option<AuditContext<'_>>,
) -> Result<AuditReport, Vec<String>> {
    plan.validate()?;
    let mut report = AuditReport {
        native: 0,
        delegated: 0,
        residual: 0,
        residual_reasons: vec![],
        observations: evidence.map_or(0, |value| value.observations.len()),
        verified: 0,
        different: 0,
        unavailable: 0,
        failed: 0,
        nondeterministic: 0,
        stale: 0,
        unobserved: 0,
        source_bytes: context.as_ref().map_or(0, |value| value.source_bytes),
        native_bytes: 0,
        delegated_bytes: 0,
        residual_bytes: 0,
        uncovered_bytes: 0,
    };
    for task in &plan.tasks {
        audit_node(&task.body, &mut report);
    }
    if let Some(context) = &context {
        audit_coverage(plan, context, &mut report)?;
    }
    if let Some(evidence) = evidence {
        let mut observed_scenarios = std::collections::BTreeSet::new();
        for observation in &evidence.observations {
            if context
                .as_ref()
                .is_some_and(|context| !current_observation(observation, context))
            {
                report.stale += 1;
                continue;
            }
            observed_scenarios.insert(observation.scenario.as_str());
            match observation.status {
                ObservationStatus::Verified => report.verified += 1,
                ObservationStatus::Different => report.different += 1,
                ObservationStatus::Unavailable => report.unavailable += 1,
                ObservationStatus::Failed => report.failed += 1,
                ObservationStatus::Nondeterministic => report.nondeterministic += 1,
            }
        }
        let mut expected = Vec::new();
        for task in &plan.tasks {
            collect_identity(&task.body, &mut expected);
        }
        let actual: Vec<_> = evidence
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.operation.as_str(), &node.guarantee))
            .collect();
        if expected != actual {
            return Err(vec!["evidence node inventory does not match plan".into()]);
        }
        if let Some(context) = &context {
            report.unobserved = context
                .scenario_digests
                .keys()
                .filter(|scenario| !observed_scenarios.contains(scenario.as_str()))
                .count();
        }
    } else if let Some(context) = &context {
        report.unobserved = context.scenario_digests.len();
    }
    Ok(report)
}

fn current_observation(observation: &ObservationEvidence, context: &AuditContext<'_>) -> bool {
    context
        .scenario_digests
        .get(&observation.scenario)
        .is_some_and(|digest| digest == &observation.key.scenario_digest)
        && observation.key.runtime_lock_digest == context.runtime_lock_digest
        && observation.key.provider_fingerprint == context.provider_fingerprint
        && observation.key.provider_fingerprint
            == crate::digest::sha256(
                format!(
                    "deshell-provider-v1:{}:{}",
                    observation.provider, context.lab_image
                )
                .as_bytes(),
            )
}

fn audit_coverage(
    plan: &Plan,
    context: &AuditContext<'_>,
    report: &mut AuditReport,
) -> Result<(), Vec<String>> {
    let mut coverage = vec![0_u8; context.source_bytes];
    let mut errors = Vec::new();
    for task in &plan.tasks {
        mark_coverage(&task.body, context, &mut coverage, &mut errors);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for value in coverage {
        match value {
            0 => report.uncovered_bytes += 1,
            1 => report.native_bytes += 1,
            2 => report.delegated_bytes += 1,
            3 => report.residual_bytes += 1,
            _ => unreachable!(),
        }
    }
    Ok(())
}

fn mark_coverage(
    node: &Node,
    context: &AuditContext<'_>,
    coverage: &mut [u8],
    errors: &mut Vec<String>,
) {
    if let Some(span) = &node.source
        && span.file == context.source_path
    {
        let Ok(start) = usize::try_from(span.start_byte) else {
            errors.push(format!("node {} source start is outside usize", node.id));
            return;
        };
        let Ok(end) = usize::try_from(span.end_byte) else {
            errors.push(format!("node {} source end is outside usize", node.id));
            return;
        };
        if start > end || end > coverage.len() {
            errors.push(format!(
                "node {} source span exceeds current source",
                node.id
            ));
        } else {
            let level = match node.guarantee {
                Guarantee::Native { .. } => 1,
                Guarantee::Delegated { .. } => 2,
                Guarantee::Residual { .. } => 3,
            };
            for byte in &mut coverage[start..end] {
                *byte = (*byte).max(level);
            }
        }
    }
    visit_children(node, |child| {
        mark_coverage(child, context, coverage, errors)
    });
}

fn audit_node(node: &Node, report: &mut AuditReport) {
    match &node.guarantee {
        Guarantee::Native { .. } => report.native += 1,
        Guarantee::Delegated { .. } => report.delegated += 1,
        Guarantee::Residual { reason } => {
            report.residual += 1;
            report
                .residual_reasons
                .push(format!("{}: {reason}", node.id));
        }
    }
    visit_children(node, |child| audit_node(child, report));
}

fn collect_identity<'a>(node: &'a Node, output: &mut Vec<(&'a str, &'a str, &'a Guarantee)>) {
    output.push((&node.id, node.operation.name(), &node.guarantee));
    visit_children(node, |child| collect_identity(child, output));
}

fn visit_children<'a>(node: &'a Node, mut visit: impl FnMut(&'a Node)) {
    match &node.operation {
        Operation::Pipeline { nodes, .. }
        | Operation::Sequence { nodes }
        | Operation::Parallel { nodes } => {
            for child in nodes {
                visit(child);
            }
        }
        Operation::Condition {
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
        Operation::Match { cases, default, .. } => {
            for case in cases {
                visit(&case.body);
            }
            if let Some(child) = default {
                visit(child);
            }
        }
        Operation::Foreach { body, .. }
        | Operation::Scope { body, .. }
        | Operation::Redirect { body, .. }
        | Operation::CaptureStdout { body, .. }
        | Operation::Spawn { body, .. } => visit(body),
        Operation::TryFinally { body, finalizer } => {
            visit(body);
            visit(finalizer);
        }
        Operation::Exec { .. }
        | Operation::ExpandWords { .. }
        | Operation::TaskCall { .. }
        | Operation::SetVariable { .. }
        | Operation::SetEnvironment { .. }
        | Operation::SetWorkingDirectory { .. }
        | Operation::Wait { .. }
        | Operation::SendSignal { .. }
        | Operation::FileRead { .. }
        | Operation::FileWrite { .. }
        | Operation::FileRemove { .. }
        | Operation::FileMetadata { .. }
        | Operation::FileSetMetadata { .. }
        | Operation::NetworkRequest { .. }
        | Operation::ClockRead { .. }
        | Operation::RandomBytes { .. }
        | Operation::InterpreterCall { .. }
        | Operation::OpaqueCapsule { .. } => {}
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Difference {
    ExitCode { expected: i32, actual: i32 },
    Stdout { expected: Vec<u8>, actual: Vec<u8> },
    Stderr { expected: Vec<u8>, actual: Vec<u8> },
    Trace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Comparison {
    pub equivalent: bool,
    pub differences: Vec<Difference>,
    pub actual_digest: String,
}

pub(crate) fn compare(expected: &RunResult, actual: &RunResult) -> Result<Comparison, String> {
    let mut differences = Vec::new();
    if expected.exit_code != actual.exit_code {
        differences.push(Difference::ExitCode {
            expected: expected.exit_code,
            actual: actual.exit_code,
        });
    }
    if expected.stdout != actual.stdout {
        differences.push(Difference::Stdout {
            expected: expected.stdout.clone(),
            actual: actual.stdout.clone(),
        });
    }
    if expected.stderr != actual.stderr {
        differences.push(Difference::Stderr {
            expected: expected.stderr.clone(),
            actual: actual.stderr.clone(),
        });
    }
    if expected.trace != actual.trace {
        differences.push(Difference::Trace);
    }
    let value = result_json(actual);
    let canonical = crate::canonical_json::canonical_bytes(&value)?;
    Ok(Comparison {
        equivalent: differences.is_empty(),
        differences,
        actual_digest: crate::digest::sha256(&canonical),
    })
}

pub(crate) fn record_comparison(
    evidence: &mut Evidence,
    scenario: &str,
    provider: &str,
    key: crate::evidence::ObservationKey,
    comparison: &Comparison,
) -> Result<ObservationStatus, String> {
    let dimensions = comparison
        .differences
        .iter()
        .map(dimension)
        .collect::<Vec<_>>();
    evidence.append_observation(ObservationEvidence {
        scenario: scenario.into(),
        key,
        status: if comparison.equivalent {
            ObservationStatus::Verified
        } else {
            ObservationStatus::Different
        },
        provider: provider.into(),
        reason: if comparison.equivalent {
            None
        } else {
            Some(format!("different dimensions: {}", dimensions.join(", ")))
        },
        digest: Some(comparison.actual_digest.clone()),
    })
}

fn dimension(difference: &Difference) -> &'static str {
    match difference {
        Difference::ExitCode { .. } => "exit_code",
        Difference::Stdout { .. } => "stdout",
        Difference::Stderr { .. } => "stderr",
        Difference::Trace => "trace",
    }
}

fn result_json(result: &RunResult) -> serde_json::Value {
    serde_json::json!({
        "exit_code": result.exit_code,
        "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&result.stderr),
        "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&result.stdout),
        "trace": result.trace.iter().map(trace_json).collect::<Vec<_>>()
    })
}

fn trace_json(event: &TraceEvent) -> serde_json::Value {
    match event {
        TraceEvent::Process { argv, exit_code } => {
            serde_json::json!({"argv": argv, "exit_code": exit_code, "type": "process"})
        }
        TraceEvent::FileRead { path } => serde_json::json!({"path": path, "type": "file_read"}),
        TraceEvent::FileWrite { path } => serde_json::json!({"path": path, "type": "file_write"}),
        TraceEvent::FileRemove { path } => serde_json::json!({"path": path, "type": "file_remove"}),
        TraceEvent::Network { method, uri } => {
            serde_json::json!({"method": method, "type": "network", "uri": uri})
        }
        TraceEvent::Delegated {
            interpreter,
            interpreter_pin,
            exit_code,
        } => serde_json::json!({
            "exit_code": exit_code,
            "interpreter": interpreter,
            "interpreter_pin": interpreter_pin,
            "type": "delegated"
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::Evidence;
    use crate::ir::{Guarantee, Node, Operation, Task, TextExpression};
    use crate::runner::TraceEvent;

    fn plan() -> Plan {
        let child = Node {
            id: String::new(),
            operation: Operation::Exec {
                argv: vec![TextExpression::literal("true")],
                environment: vec![],
                working_directory: None,
            },
            guarantee: Guarantee::Native {
                semantic_model: "test".into(),
            },
            source: None,
        };
        let residual = Node {
            id: String::new(),
            operation: Operation::OpaqueCapsule {
                interpreter: "sh".into(),
                source: crate::ir::SourceBytes::from_bytes(b"dynamic"),
                path: Some("build.sh".into()),
            },
            guarantee: Guarantee::Residual {
                reason: "dynamic".into(),
            },
            source: None,
        };
        let mut plan = Plan {
            schema_version: 1,
            generator: "test".into(),
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
                    operation: Operation::Sequence {
                        nodes: vec![child, residual],
                    },
                    guarantee: Guarantee::Native {
                        semantic_model: "test-sequence-v1".into(),
                    },
                    source: None,
                },
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    fn result(code: i32, stdout: &[u8]) -> RunResult {
        RunResult {
            exit_code: code,
            stdout: stdout.to_vec(),
            stderr: vec![],
            trace: vec![TraceEvent::Process {
                argv: vec!["true".into()],
                exit_code: code,
            }],
        }
    }

    fn observation_key() -> crate::evidence::ObservationKey {
        crate::evidence::ObservationKey {
            scenario_digest: "a".repeat(64),
            provider_fingerprint: "b".repeat(64),
            runtime_lock_digest: "c".repeat(64),
        }
    }

    #[test]
    fn verifier_counts_only_static_guarantees_and_evidence_observations() {
        let plan = plan();
        let evidence = Evidence::from_plan(&plan, "build.sh", b"dynamic").unwrap();
        let report = audit(&plan, Some(&evidence)).unwrap();
        assert_eq!(
            (report.native, report.delegated, report.residual),
            (2, 0, 1)
        );
        assert_eq!(report.observations, 0);
        assert_eq!(report.residual_reasons.len(), 1);
    }

    #[test]
    fn observations_from_a_no_longer_selected_provider_become_stale() {
        let source = b"/usr/bin/printf ok\n";
        let plan = crate::frontend::lower(
            "build.sh",
            source,
            crate::config::UnknownInterpreter::Reject,
        )
        .unwrap();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", source).unwrap();
        let scenario_digest = "a".repeat(64);
        let runtime_lock_digest = "b".repeat(64);
        let lab_image = concat!(
            "ghcr.io/deshell-lang/lab@sha256:",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        let old_provider = "unavailable";
        let old_fingerprint = crate::digest::sha256(
            format!("deshell-provider-v1:{old_provider}:{lab_image}").as_bytes(),
        );
        evidence
            .append_observation(ObservationEvidence {
                scenario: "default".into(),
                key: crate::evidence::ObservationKey {
                    scenario_digest: scenario_digest.clone(),
                    provider_fingerprint: old_fingerprint,
                    runtime_lock_digest: runtime_lock_digest.clone(),
                },
                status: ObservationStatus::Unavailable,
                provider: old_provider.into(),
                reason: Some("provider missing".into()),
                digest: None,
            })
            .unwrap();
        let scenarios = std::collections::BTreeMap::from([("default".to_owned(), scenario_digest)]);
        let current_fingerprint =
            crate::digest::sha256(format!("deshell-provider-v1:podman:{lab_image}").as_bytes());

        let report = audit_current(
            &plan,
            &evidence,
            AuditContext {
                source_path: "build.sh",
                source_bytes: source.len(),
                scenario_digests: &scenarios,
                runtime_lock_digest: &runtime_lock_digest,
                lab_image,
                provider_fingerprint: &current_fingerprint,
            },
        )
        .unwrap();
        assert_eq!(report.stale, 1);
        assert_eq!(report.unavailable, 0);
        assert_eq!(report.unobserved, 1);
    }

    #[test]
    fn differential_comparison_uses_raw_bytes_and_canonical_digest() {
        let expected = result(0, &[0, 0xff]);
        let actual = result(7, &[0, 0xfe]);
        let comparison = compare(&expected, &actual).unwrap();
        assert!(!comparison.equivalent);
        assert!(
            comparison
                .differences
                .iter()
                .any(|difference| matches!(difference, Difference::ExitCode { .. }))
        );
        assert!(
            comparison
                .differences
                .iter()
                .any(|difference| matches!(difference, Difference::Stdout { .. }))
        );
        assert_eq!(comparison.actual_digest.len(), 64);
        assert_eq!(comparison, compare(&expected, &actual).unwrap());
    }

    #[test]
    fn comparison_is_recorded_in_evidence_without_plan_mutation() {
        let plan = plan();
        let before = plan.encode_pretty().unwrap();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"dynamic").unwrap();
        let comparison = compare(&result(0, b"same"), &result(0, b"same")).unwrap();
        record_comparison(
            &mut evidence,
            "default",
            "test-provider",
            observation_key(),
            &comparison,
        )
        .unwrap();
        assert_eq!(evidence.observations[0].status, ObservationStatus::Verified);
        assert_eq!(plan.encode_pretty().unwrap(), before);
    }

    #[test]
    fn differences_are_recorded_as_different_observations() {
        let plan = plan();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"dynamic").unwrap();
        let comparison = compare(&result(0, b"a"), &result(1, b"b")).unwrap();
        record_comparison(
            &mut evidence,
            "default",
            "test-provider",
            observation_key(),
            &comparison,
        )
        .unwrap();
        assert_eq!(
            evidence.observations[0].status,
            ObservationStatus::Different
        );
        assert!(
            evidence.observations[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("exit_code")
        );
    }
}
