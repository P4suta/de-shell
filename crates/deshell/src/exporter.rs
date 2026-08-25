use crate::ir::{Node, Operation, Plan, TextExpression, TextPart};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Internal,
    Dagger,
    Nushell,
    Cwl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Artifact {
    pub filename: String,
    pub media_type: String,
    pub content: Vec<u8>,
}

pub(crate) fn export(plan: &Plan, target: Target, bridge: bool) -> Result<Artifact, String> {
    plan.validate().map_err(|errors| errors.join("; "))?;
    if target == Target::Internal {
        return Ok(Artifact {
            filename: "plan.json".into(),
            media_type: "application/vnd.deshell.effect-ir+json".into(),
            content: plan.encode_pretty()?,
        });
    }
    let task = plan
        .tasks
        .iter()
        .find(|task| task.name == plan.entrypoint)
        .ok_or_else(|| format!("entrypoint task not found: {}", plan.entrypoint))?;
    let commands = if task.inputs.is_empty()
        && task.outputs.is_empty()
        && task.environment.is_empty()
        && task.secrets.is_empty()
        && task.invocation.is_none()
    {
        flatten_exec(&task.body)
    } else {
        Err(format!(
            "strict exporter cannot represent task {} interface",
            task.name
        ))
    };
    let commands = match commands {
        Ok(commands) => commands,
        Err(_) if bridge => vec![bridge_argv()],
        Err(error) => return Err(format!("{error}; use --bridge explicitly")),
    };
    match target {
        Target::Internal => unreachable!(),
        Target::Dagger => Ok(dagger(&commands)),
        Target::Nushell => Ok(nushell(&commands)),
        Target::Cwl if commands.len() == 1 => cwl(&commands[0]),
        Target::Cwl => {
            Err("strict CWL CommandLineTool export requires exactly one Exec node".into())
        }
    }
}

fn flatten_exec(node: &Node) -> Result<Vec<Vec<String>>, String> {
    match &node.operation {
        Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            if !environment.is_empty() {
                return Err(format!(
                    "strict exporter cannot represent node {} (exec.environment)",
                    node.id
                ));
            }
            if working_directory.is_some() {
                return Err(format!(
                    "strict exporter cannot represent node {} (exec.working_directory)",
                    node.id
                ));
            }
            let argv = argv.iter().map(literal).collect::<Result<Vec<_>, _>>()?;
            Ok(vec![argv])
        }
        Operation::Sequence { nodes } => {
            let mut output = Vec::new();
            for child in nodes {
                output.extend(flatten_exec(child)?);
            }
            Ok(output)
        }
        other => Err(format!(
            "strict exporter cannot represent node {} ({})",
            node.id,
            other.name()
        )),
    }
}

fn literal(expression: &TextExpression) -> Result<String, String> {
    match expression.parts.as_slice() {
        [TextPart::Literal { value }] => Ok(value.clone()),
        _ => Err("strict exporter cannot represent a dynamic text expression".into()),
    }
}

fn bridge_argv() -> Vec<String> {
    [
        "deshell",
        "run",
        "--allow-residual",
        "--allow-file-read",
        "--allow-file-write",
        "--allow-network",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn dagger(commands: &[Vec<String>]) -> Artifact {
    let steps = commands.iter().map(|argv| {
        let argv = serde_json::to_string(argv).expect("argv JSON serialization");
        format!("    container = container.withExec({argv});\n    output += await container.stdout();")
    }).collect::<Vec<_>>().join("\n");
    let content = format!(
        concat!(
            "import {{ dag, Container, object, func }} from \"@dagger.io/dagger\";\n\n",
            "@object()\nexport class Deshell {{\n",
            "  @func()\n  async main(): Promise<string> {{\n",
            "    let container: Container = dag.container().from(\"alpine@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce\");\n",
            "    let output = \"\";\n",
            "{}\n",
            "    return output;\n  }}\n}}\n"
        ),
        steps,
    );
    Artifact {
        filename: "deshell.dagger.ts".into(),
        media_type: "text/typescript".into(),
        content: content.into_bytes(),
    }
}

fn nushell(commands: &[Vec<String>]) -> Artifact {
    let lines = commands
        .iter()
        .map(|argv| {
            let values = argv
                .iter()
                .map(|value| serde_json::to_string(value).expect("argv string JSON"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("  run-external {values}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    Artifact {
        filename: "deshell.nu".into(),
        media_type: "text/x-nushell".into(),
        content: format!("export def main [] {{\n{lines}\n}}\n").into_bytes(),
    }
}

fn cwl(argv: &[String]) -> Result<Artifact, String> {
    let (executable, arguments) = argv
        .split_first()
        .ok_or("CWL command argv must not be empty")?;
    let value = serde_json::json!({
        "arguments": arguments,
        "baseCommand": [executable],
        "class": "CommandLineTool",
        "cwlVersion": "v1.2",
        "inputs": {},
        "outputs": {"stdout": {"type": "stdout"}},
        "stdout": "deshell.stdout"
    });
    Ok(Artifact {
        filename: "deshell.cwl".into(),
        media_type: "application/cwl+json".into(),
        content: crate::canonical_json::pretty_bytes(&value)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Guarantee, Node, Operation, Task, TextExpression, TextPart};

    fn node(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Formal {
                basis: "export-test-v1".into(),
            },
            source: None,
        }
    }
    fn exec(values: &[&str]) -> Node {
        node(Operation::Exec {
            argv: values
                .iter()
                .map(|value| TextExpression::literal(*value))
                .collect(),
            environment: vec![],
            working_directory: None,
        })
    }
    fn plan(body: Node) -> Plan {
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
                body,
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    #[test]
    fn internal_export_is_the_canonical_plan_bytes() {
        let plan = plan(exec(&["printf", "hello"]));
        let artifact = export(&plan, Target::Internal, false).unwrap();
        assert_eq!(artifact.filename, "plan.json");
        assert_eq!(artifact.content, plan.encode_pretty().unwrap());
    }

    #[test]
    fn strict_literal_exports_are_well_formed() {
        let plan = plan(exec(&["printf", "%s", "hello"]));
        let cwl = export(&plan, Target::Cwl, false).unwrap();
        let document: serde_json::Value = crate::strict_json::parse(&cwl.content).unwrap();
        assert_eq!(document["cwlVersion"], "v1.2");
        assert_eq!(document["baseCommand"], serde_json::json!(["printf"]));
        assert_eq!(document["arguments"], serde_json::json!(["%s", "hello"]));
        let nu = String::from_utf8(export(&plan, Target::Nushell, false).unwrap().content).unwrap();
        assert!(nu.starts_with("export def main [] {"));
        let dagger =
            String::from_utf8(export(&plan, Target::Dagger, false).unwrap().content).unwrap();
        assert!(dagger.contains("alpine@sha256:"));
        assert!(dagger.contains("withExec"));
    }

    #[test]
    fn strict_export_rejects_dynamic_or_unrepresentable_nodes() {
        let dynamic = plan(node(Operation::Exec {
            argv: vec![
                TextExpression::literal("emit"),
                TextExpression {
                    parts: vec![TextPart::Variable {
                        name: "VALUE".into(),
                    }],
                },
            ],
            environment: vec![],
            working_directory: None,
        }));
        let error = export(&dynamic, Target::Nushell, false).unwrap_err();
        assert!(error.contains("dynamic text expression"), "{error}");
        let file = plan(node(Operation::FileRead {
            path: TextExpression::literal("input"),
        }));
        assert!(
            export(&file, Target::Dagger, false)
                .unwrap_err()
                .contains("file_read")
        );
    }

    #[test]
    fn bridge_is_explicit_and_never_silently_drops_effects() {
        let file = plan(node(Operation::FileRead {
            path: TextExpression::literal("input"),
        }));
        let artifact = export(&file, Target::Cwl, true).unwrap();
        let text = String::from_utf8(artifact.content).unwrap();
        assert!(text.contains("deshell"));
        assert!(text.contains("run"));
        assert!(text.contains("--allow-file-read"));
    }

    #[test]
    fn cwl_requires_exactly_one_exec_while_sequence_exports_elsewhere() {
        let sequence = plan(node(Operation::Sequence {
            nodes: vec![exec(&["one"]), exec(&["two"])],
        }));
        assert!(export(&sequence, Target::Cwl, false).is_err());
        assert!(export(&sequence, Target::Nushell, false).is_ok());
        assert!(export(&sequence, Target::Dagger, false).is_ok());
    }
}
