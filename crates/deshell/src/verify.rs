use crate::evidence::{Evidence, ObservationEvidence, ObservationStatus};
use crate::ir::{Guarantee, Node, Operation, Plan};
use crate::runner::{RunResult, TraceEvent};
use base64::Engine as _;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuditReport {
    pub formal: usize,
    pub exhaustive: usize,
    pub residual: usize,
    pub residual_reasons: Vec<String>,
    pub observations: usize,
}

pub(crate) fn audit(plan: &Plan, evidence: Option<&Evidence>) -> Result<AuditReport, Vec<String>> {
    plan.validate()?;
    let mut report = AuditReport {
        formal: 0,
        exhaustive: 0,
        residual: 0,
        residual_reasons: vec![],
        observations: evidence.map_or(0, |value| value.observations.len()),
    };
    for task in &plan.tasks {
        audit_node(&task.body, &mut report);
    }
    if let Some(evidence) = evidence {
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
    }
    Ok(report)
}

fn audit_node(node: &Node, report: &mut AuditReport) {
    match &node.guarantee {
        Guarantee::Formal { .. } => report.formal += 1,
        Guarantee::Exhaustive { .. } => report.exhaustive += 1,
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
        Operation::Pipeline { nodes }
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
        Operation::Foreach { body, .. } | Operation::CaptureStdout { body, .. } => visit(body),
        Operation::TryFinally { body, finalizer } => {
            visit(body);
            visit(finalizer);
        }
        Operation::Exec { .. }
        | Operation::TaskCall { .. }
        | Operation::SetVariable { .. }
        | Operation::FileRead { .. }
        | Operation::FileWrite { .. }
        | Operation::FileRemove { .. }
        | Operation::NetworkRequest { .. }
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
    comparison: &Comparison,
) -> Result<(), String> {
    let dimensions = comparison
        .differences
        .iter()
        .map(dimension)
        .collect::<Vec<_>>();
    evidence.append_observation(ObservationEvidence {
        scenarios: vec![scenario.into()],
        status: if comparison.equivalent {
            ObservationStatus::Verified
        } else {
            ObservationStatus::Different
        },
        provider: Some(provider.into()),
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
        TraceEvent::Opaque {
            interpreter,
            exit_code,
        } => {
            serde_json::json!({"exit_code": exit_code, "interpreter": interpreter, "type": "opaque"})
        }
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
            guarantee: Guarantee::Formal {
                basis: "test".into(),
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
                    guarantee: Guarantee::Exhaustive {
                        scenarios: vec!["default".into()],
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

    #[test]
    fn verifier_counts_only_static_guarantees_and_evidence_observations() {
        let plan = plan();
        let evidence = Evidence::from_plan(&plan, "build.sh", b"dynamic").unwrap();
        let report = audit(&plan, Some(&evidence)).unwrap();
        assert_eq!(
            (report.formal, report.exhaustive, report.residual),
            (1, 1, 1)
        );
        assert_eq!(report.observations, 0);
        assert_eq!(report.residual_reasons.len(), 1);
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
        record_comparison(&mut evidence, "default", "test-provider", &comparison).unwrap();
        assert_eq!(evidence.observations[0].status, ObservationStatus::Verified);
        assert_eq!(plan.encode_pretty().unwrap(), before);
    }

    #[test]
    fn differences_are_recorded_as_different_observations() {
        let plan = plan();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"dynamic").unwrap();
        let comparison = compare(&result(0, b"a"), &result(1, b"b")).unwrap();
        record_comparison(&mut evidence, "default", "test-provider", &comparison).unwrap();
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
