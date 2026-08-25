use crate::config::Scenario;
use crate::evidence::Evidence;
use crate::ir::Plan;
use crate::runner::{Backend, Policy, RunResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(crate) enum ProviderFailureKind {
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProviderFailure {
    pub kind: ProviderFailureKind,
    pub message: String,
}

pub(crate) trait Observer: Sync {
    fn observe(&self, scenario: &Scenario) -> Result<RunResult, ProviderFailure>;
    fn name(&self) -> &str;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Verified,
    Different,
    Unavailable,
    Failed,
}

pub(crate) fn evaluate(
    observer: &dyn Observer,
    backend: &dyn Backend,
    policy: Policy,
    plan: &Plan,
    scenario: &Scenario,
    evidence: &mut Evidence,
) -> Result<Outcome, String> {
    if observer.name().trim().is_empty() {
        return Err("observer provider name must not be empty".into());
    }
    let expected = match observer.observe(scenario) {
        Ok(result) => result,
        Err(failure) => {
            let (status, outcome) = match failure.kind {
                ProviderFailureKind::Unavailable => (
                    crate::evidence::ObservationStatus::Unavailable,
                    Outcome::Unavailable,
                ),
                ProviderFailureKind::Failed => {
                    (crate::evidence::ObservationStatus::Failed, Outcome::Failed)
                }
            };
            evidence.append_observation(crate::evidence::ObservationEvidence {
                scenarios: vec![scenario.name.clone()],
                status,
                provider: Some(observer.name().into()),
                reason: Some(failure.message),
                digest: None,
            })?;
            return Ok(outcome);
        }
    };
    if let Some(reason) = expectation_failure(scenario, &expected) {
        evidence.append_observation(crate::evidence::ObservationEvidence {
            scenarios: vec![scenario.name.clone()],
            status: crate::evidence::ObservationStatus::Failed,
            provider: Some(observer.name().into()),
            reason: Some(reason),
            digest: None,
        })?;
        return Ok(Outcome::Failed);
    }
    let host_environment = scenario
        .environment
        .iter()
        .map(|value| (value.name.clone(), value.value.clone()))
        .collect();
    let named_inputs = scenario
        .arguments
        .iter()
        .map(|value| (value.name.clone(), value.value.clone()))
        .collect();
    let actual = match crate::runner::run_plan(
        backend,
        policy,
        plan,
        &host_environment,
        &named_inputs,
        &scenario.argv,
    ) {
        Ok(result) => result,
        Err(error) => {
            evidence.append_observation(crate::evidence::ObservationEvidence {
                scenarios: vec![scenario.name.clone()],
                status: crate::evidence::ObservationStatus::Failed,
                provider: Some(observer.name().into()),
                reason: Some(format!(
                    "plan execution failed during differential observation: {}",
                    error.message
                )),
                digest: None,
            })?;
            return Ok(Outcome::Failed);
        }
    };
    let comparison = crate::verify::compare(&expected, &actual)?;
    crate::verify::record_comparison(evidence, &scenario.name, observer.name(), &comparison)?;
    Ok(if comparison.equivalent {
        Outcome::Verified
    } else {
        Outcome::Different
    })
}

fn expectation_failure(scenario: &Scenario, observed: &RunResult) -> Option<String> {
    if let Some(expected) = scenario.expect.exit_code
        && expected != observed.exit_code
    {
        return Some(format!(
            "scenario expected exit code {expected}, observed {}",
            observed.exit_code
        ));
    }
    if let Some(expected) = &scenario.expect.stdout
        && expected.as_bytes() != observed.stdout
    {
        return Some("scenario expected stdout did not match the original observation".into());
    }
    if let Some(expected) = &scenario.expect.stderr
        && expected.as_bytes() != observed.stderr
    {
        return Some("scenario expected stderr did not match the original observation".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::ObservationStatus;
    use crate::ir::{Guarantee, Node, Operation, Task, TextExpression};
    use crate::runner::{CapsuleRequest, ProcessRequest, ProcessResult, TraceEvent};

    struct MockObserver(Result<RunResult, ProviderFailure>);
    impl Observer for MockObserver {
        fn observe(&self, _scenario: &Scenario) -> Result<RunResult, ProviderFailure> {
            self.0.clone()
        }
        fn name(&self) -> &str {
            "mock-observer"
        }
    }

    struct MockBackend(RunResult);
    impl Backend for MockBackend {
        fn execute(&self, _request: ProcessRequest) -> Result<ProcessResult, String> {
            Ok(ProcessResult {
                exit_code: self.0.exit_code,
                stdout: self.0.stdout.clone(),
                stderr: self.0.stderr.clone(),
            })
        }
        fn execute_capsule(&self, _request: CapsuleRequest) -> Result<ProcessResult, String> {
            unreachable!()
        }
        fn read_file(&self, _path: &str) -> Result<Vec<u8>, String> {
            unreachable!()
        }
        fn write_file(&self, _path: &str, _contents: &[u8], _append: bool) -> Result<(), String> {
            unreachable!()
        }
        fn remove_file(&self, _path: &str) -> Result<(), String> {
            unreachable!()
        }
        fn network_request(&self, _method: &str, _uri: &str) -> Result<Vec<u8>, String> {
            unreachable!()
        }
    }

    fn result(exit_code: i32, stdout: &[u8]) -> RunResult {
        RunResult {
            exit_code,
            stdout: stdout.to_vec(),
            stderr: vec![],
            trace: vec![TraceEvent::Process {
                argv: vec!["emit".into()],
                exit_code,
            }],
        }
    }

    fn plan() -> Plan {
        let mut plan = Plan {
            schema_version: 1,
            generator: "test".into(),
            entrypoint: "main".into(),
            tasks: vec![Task {
                name: "main".into(),
                inputs: vec![],
                outputs: vec![],
                environment: vec!["VALUE".into()],
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                invocation: None,
                body: Node {
                    id: String::new(),
                    operation: Operation::Exec {
                        argv: vec![TextExpression::literal("emit")],
                        environment: vec![],
                        working_directory: None,
                    },
                    guarantee: Guarantee::Formal {
                        basis: "test".into(),
                    },
                    source: None,
                },
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    fn scenario() -> Scenario {
        let mut scenario = Scenario::decode(&Scenario::default_text()).unwrap();
        scenario.expect.stdout = None;
        scenario.expect.stderr = None;
        scenario
    }

    #[test]
    fn equivalent_runs_append_verified_evidence_without_changing_the_plan() {
        let plan = plan();
        let before = plan.encode_pretty().unwrap();
        let expected = result(0, b"");
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"emit").unwrap();
        let outcome = evaluate(
            &MockObserver(Ok(expected.clone())),
            &MockBackend(expected),
            Policy::default(),
            &plan,
            &scenario(),
            &mut evidence,
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Verified);
        assert_eq!(evidence.observations[0].status, ObservationStatus::Verified);
        assert_eq!(plan.encode_pretty().unwrap(), before);
    }

    #[test]
    fn raw_output_or_status_differences_are_observations_not_plan_guarantees() {
        let plan = plan();
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"emit").unwrap();
        let outcome = evaluate(
            &MockObserver(Ok(result(0, &[0xff]))),
            &MockBackend(result(7, &[0xfe])),
            Policy::default(),
            &plan,
            &scenario(),
            &mut evidence,
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Different);
        assert_eq!(
            evidence.observations[0].status,
            ObservationStatus::Different
        );
        assert!(
            evidence.observations[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("stdout")
        );
    }

    #[test]
    fn unavailable_and_failed_providers_are_recorded_without_fake_digests() {
        for (kind, status, outcome) in [
            (
                ProviderFailureKind::Unavailable,
                ObservationStatus::Unavailable,
                Outcome::Unavailable,
            ),
            (
                ProviderFailureKind::Failed,
                ObservationStatus::Failed,
                Outcome::Failed,
            ),
        ] {
            let plan = plan();
            let mut evidence = Evidence::from_plan(&plan, "build.sh", b"emit").unwrap();
            let observer = MockObserver(Err(ProviderFailure {
                kind,
                message: "provider stopped".into(),
            }));
            assert_eq!(
                evaluate(
                    &observer,
                    &MockBackend(result(0, b"")),
                    Policy::default(),
                    &plan,
                    &scenario(),
                    &mut evidence
                )
                .unwrap(),
                outcome
            );
            assert_eq!(evidence.observations[0].status, status);
            assert_eq!(evidence.observations[0].digest, None);
            assert_eq!(
                evidence.observations[0].provider,
                Some("mock-observer".into())
            );
        }
    }

    #[test]
    fn scenario_expectations_are_checked_before_claiming_equivalence() {
        let plan = plan();
        let mut scenario = scenario();
        scenario.expect.stdout = Some("expected".into());
        let mut evidence = Evidence::from_plan(&plan, "build.sh", b"emit").unwrap();
        let outcome = evaluate(
            &MockObserver(Ok(result(0, b"actual"))),
            &MockBackend(result(0, b"actual")),
            Policy::default(),
            &plan,
            &scenario,
            &mut evidence,
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(evidence.observations[0].status, ObservationStatus::Failed);
        assert!(
            evidence.observations[0]
                .reason
                .as_deref()
                .unwrap()
                .contains("expected stdout")
        );
    }
}
