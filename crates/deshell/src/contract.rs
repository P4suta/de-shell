#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde::Deserialize;
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
    }

    fn json(relative: &str) -> Value {
        let path = root().join(relative);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        crate::strict_json::parse(source.as_bytes())
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
    }

    #[test]
    fn effect_ir_v1_is_strict_and_uses_explicit_text_expressions() {
        let schema = json("contracts/schema/effect-ir-v1.schema.json");
        assert_eq!(schema["$id"], "https://deshell.dev/contracts/effect-ir/v1");
        assert_eq!(schema["properties"]["schema_version"]["const"], 1);
        assert_eq!(schema["additionalProperties"], false);

        let expression = &schema["$defs"]["textExpression"];
        assert_eq!(expression["type"], "object");
        assert!(
            expression["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "parts")
        );
        let variants = expression["properties"]["parts"]["items"]["oneOf"]
            .as_array()
            .unwrap();
        let kinds: Vec<_> = variants
            .iter()
            .map(|variant| variant["properties"]["kind"]["const"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["literal", "variable", "argument"]);

        let guarantee_source = serde_json::to_string(&schema["$defs"]["guarantee"]).unwrap();
        assert!(!guarantee_source.contains("differential"));
        assert!(guarantee_source.contains("formal"));
        assert!(guarantee_source.contains("exhaustive"));
        assert!(guarantee_source.contains("residual"));

        let value_types = serde_json::to_string(&schema["$defs"]["valueType"]).unwrap();
        for forbidden in ["bytes", "list", "stream", "record"] {
            assert!(
                !value_types.contains(forbidden),
                "unexpected public type {forbidden}"
            );
        }
    }

    #[test]
    fn evidence_v1_owns_differential_observations() {
        let schema = json("contracts/schema/evidence-v1.schema.json");
        let effect = json("contracts/schema/effect-ir-v1.schema.json");
        assert_eq!(schema["$id"], "https://deshell.dev/contracts/evidence/v1");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["$defs"]["guarantee"], effect["$defs"]["guarantee"]);
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field == "observations")
        );
        let observations = serde_json::to_string(&schema["$defs"]["observation"]).unwrap();
        assert!(observations.contains("different"));
        assert!(observations.contains("verified"));
    }

    #[test]
    fn language_neutral_contract_bundle_is_complete() {
        for relative in [
            "contracts/README.md",
            "contracts/effect-ir-v1.md",
            "contracts/canonical-json-v1.md",
            "contracts/diagnostics-v1.md",
            "contracts/json-rpc-v1.md",
            "contracts/project-v1.md",
            "contracts/cli/cases.json",
            "contracts/schema/diagnostic-v1.schema.json",
            "contracts/schema/protocol-v1.schema.json",
            "contracts/schema/project-v1.schema.json",
            "contracts/schema/scenario-v1.schema.json",
            "contracts/schema/lock-v1.schema.json",
            "contracts/schema/replay-v1.schema.json",
            "contracts/schema/corpus-audit-v1.schema.json",
            "contracts/golden/frontend-v1.json",
            "contracts/golden/transform-export-v1.json",
        ] {
            assert!(root().join(relative).is_file(), "missing {relative}");
        }
    }

    #[test]
    fn project_replay_and_audit_schemas_are_fresh_strict_v1_contracts() {
        for name in ["project", "scenario", "lock", "replay", "corpus-audit"] {
            let schema = json(&format!("contracts/schema/{name}-v1.schema.json"));
            assert_eq!(schema["additionalProperties"], false, "{name}");
            assert_eq!(
                schema["properties"]["schema_version"]
                    .get("const")
                    .or_else(|| schema["properties"]["version"].get("const")),
                Some(&serde_json::json!(1)),
                "{name}"
            );
        }
        let audit = fs::read_to_string(root().join("contracts/schema/corpus-audit-v1.schema.json"))
            .unwrap();
        assert!(!audit.contains("differential"));
        assert!(audit.contains("observations"));
        let replay = json("contracts/schema/replay-v1.schema.json");
        let effect = json("contracts/schema/effect-ir-v1.schema.json");
        assert_eq!(
            replay["$defs"]["sourceBytes"],
            effect["$defs"]["sourceBytes"]
        );
    }

    #[test]
    fn every_embedded_schema_is_self_contained_for_offline_validation() {
        fn external_references(value: &Value, output: &mut Vec<String>) {
            match value {
                Value::Object(fields) => {
                    if let Some(reference) = fields.get("$ref").and_then(Value::as_str)
                        && !reference.starts_with('#')
                    {
                        output.push(reference.into());
                    }
                    for value in fields.values() {
                        external_references(value, output);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        external_references(value, output);
                    }
                }
                _ => {}
            }
        }

        for name in [
            "effect-ir",
            "evidence",
            "diagnostic",
            "protocol",
            "project",
            "scenario",
            "lock",
            "replay",
            "corpus-audit",
        ] {
            let schema = json(&format!("contracts/schema/{name}-v1.schema.json"));
            let mut references = Vec::new();
            external_references(&schema, &mut references);
            assert!(
                references.is_empty(),
                "{name} schema requires unavailable external references: {references:?}"
            );
        }
    }

    #[test]
    fn corpus_audit_schema_matches_the_checked_in_auditor_report_shape() {
        let schema = json("contracts/schema/corpus-audit-v1.schema.json");
        for field in [
            "analysis_scope",
            "selection",
            "repositories",
            "summary",
            "fully_non_residual_files",
            "inventory_groups",
            "residual_reason_groups",
            "files",
            "failures",
        ] {
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item == field),
                "audit schema omitted {field}"
            );
        }
        assert_eq!(
            schema["properties"]["analysis_scope"]["const"],
            "shell_files"
        );
        assert_eq!(
            schema["properties"]["selection"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["properties"]["summary"]["properties"]["locations"]["type"],
            "object"
        );
        assert_eq!(
            schema["properties"]["summary"]["properties"]["nodes"]["$ref"],
            "#/$defs/nodeCounts"
        );
        assert_eq!(
            schema["$defs"]["file"]["properties"]["nodes"]["$ref"],
            "#/$defs/nodeCounts"
        );
        assert!(
            schema["$defs"]["file"]["properties"]
                .get("fully_non_residual")
                .is_some()
        );
        assert!(
            schema["$defs"]["file"]["properties"]
                .get("interpreter")
                .is_some()
        );
        assert!(schema["$defs"]["file"]["properties"].get("error").is_some());
    }

    #[test]
    fn corpus_auditor_persists_sorted_two_space_lf_json() {
        let script = fs::read_to_string(root().join("scripts/audit-corpus.ps1")).unwrap();
        assert!(script.contains("function ConvertTo-SortedJsonValue"));
        assert!(script.contains("ConvertTo-SortedJsonValue $report"));
        assert!(script.contains("$json + \"`n\""));
        assert!(!script.contains("$json + [Environment]::NewLine"));
    }

    #[test]
    fn every_persisted_project_path_uses_the_same_normalized_path_schema() {
        let expected =
            "^(?!/)(?!\\.{1,2}(?:/|$))(?!.*//)(?!.*\\/\\.{1,2}(?:/|$))(?!.*\\/$)[^\\\\:\\u0000]+$";
        let project = json("contracts/schema/project-v1.schema.json");
        let scenario = json("contracts/schema/scenario-v1.schema.json");
        let evidence = json("contracts/schema/evidence-v1.schema.json");
        let effect = json("contracts/schema/effect-ir-v1.schema.json");
        for (name, schema) in [
            ("project", &project),
            ("scenario", &scenario),
            ("evidence", &evidence),
            ("effect-ir", &effect),
        ] {
            assert_eq!(schema["$defs"]["path"]["pattern"], expected, "{name}");
        }
        assert_eq!(
            effect["$defs"]["span"]["properties"]["file"]["$ref"],
            "#/$defs/path"
        );
        assert_eq!(
            evidence["properties"]["source"]["properties"]["path"]["$ref"],
            "#/$defs/path"
        );
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldenCorpus {
        schema_version: u32,
        cases: Vec<GoldenCase>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GoldenCase {
        name: String,
        path: String,
        source_utf8: Option<String>,
        source_base64: Option<String>,
        root_operation: String,
        formal: usize,
        exhaustive: usize,
        residual: usize,
        plan_digest: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TransformCorpus {
        schema_version: u32,
        equivalent_rewrites: Vec<RewriteGolden>,
        modernizations: Vec<ModernizeGolden>,
        exports: Vec<ExportGolden>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RewriteGolden {
        name: String,
        path: String,
        source: String,
        output: String,
        edits: usize,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ModernizeGolden {
        name: String,
        path: String,
        source: String,
        profiles: Vec<String>,
        output: String,
        edits: usize,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExportGolden {
        name: String,
        path: String,
        source: String,
        target: String,
        filename: String,
        media_type: String,
        content_sha256: String,
    }

    #[test]
    fn shared_frontend_golden_corpus_has_zero_unexplained_differences() {
        let path = root().join("contracts/golden/frontend-v1.json");
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let corpus: GoldenCorpus = crate::strict_json::decode(&bytes).unwrap();
        assert_eq!(corpus.schema_version, 1);
        let required = [
            "posix",
            "zsh",
            "fish",
            "powershell",
            "cmd",
            "nushell",
            "unknown",
            "non_utf8",
            "int_boundary",
        ];
        for required in required {
            assert!(
                corpus.cases.iter().any(|case| case.name == required),
                "golden corpus omitted {required}"
            );
        }
        let mut differences = Vec::new();
        for case in corpus.cases {
            let source = match (case.source_utf8, case.source_base64) {
                (Some(text), None) => text.into_bytes(),
                (None, Some(value)) => base64::engine::general_purpose::STANDARD
                    .decode(value)
                    .unwrap(),
                _ => panic!("{} must declare exactly one source encoding", case.name),
            };
            let plan = crate::frontend::lower(
                &case.path,
                &source,
                crate::config::UnknownInterpreter::TraceOnly,
            )
            .unwrap();
            let report = crate::verify::audit(&plan, None).unwrap();
            let root_operation = plan.tasks[0].body.operation.name();
            let evidence =
                crate::evidence::Evidence::from_plan(&plan, &case.path, &source).unwrap();
            let actual = (
                root_operation,
                report.formal,
                report.exhaustive,
                report.residual,
                evidence.plan_digest.as_str(),
            );
            let expected = (
                case.root_operation.as_str(),
                case.formal,
                case.exhaustive,
                case.residual,
                case.plan_digest.as_str(),
            );
            if actual != expected {
                differences.push(format!(
                    "{}: expected {expected:?}, actual {actual:?}",
                    case.name
                ));
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }

    #[test]
    fn shared_transform_and_export_golden_corpus_has_zero_differences() {
        let path = root().join("contracts/golden/transform-export-v1.json");
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let corpus: TransformCorpus = crate::strict_json::decode(&bytes).unwrap();
        assert_eq!(corpus.schema_version, 1);
        assert!(!corpus.equivalent_rewrites.is_empty());
        assert!(!corpus.modernizations.is_empty());
        let mut targets = std::collections::BTreeSet::new();

        for case in corpus.equivalent_rewrites {
            let result = crate::rewrite::equivalent(&case.path, &case.source);
            assert_eq!(result.output, case.output, "{}", case.name);
            assert_eq!(result.edits.len(), case.edits, "{}", case.name);
        }
        for case in corpus.modernizations {
            let profiles = case
                .profiles
                .iter()
                .map(|name| match name.as_str() {
                    "portable" => crate::rewrite::Profile::Portable,
                    "secure" => crate::rewrite::Profile::Secure,
                    "reproducible" => crate::rewrite::Profile::Reproducible,
                    other => panic!("{} uses unknown profile {other}", case.name),
                })
                .collect::<Vec<_>>();
            let result = crate::rewrite::modernize(&case.path, &case.source, &profiles);
            assert_eq!(result.output, case.output, "{}", case.name);
            assert_eq!(result.edits.len(), case.edits, "{}", case.name);
        }
        for case in corpus.exports {
            let target = match case.target.as_str() {
                "cwl" => crate::exporter::Target::Cwl,
                "dagger" => crate::exporter::Target::Dagger,
                "nu" => crate::exporter::Target::Nushell,
                other => panic!("{} uses unknown target {other}", case.name),
            };
            targets.insert(case.target.clone());
            let plan = crate::frontend::lower(
                &case.path,
                case.source.as_bytes(),
                crate::config::UnknownInterpreter::TraceOnly,
            )
            .unwrap();
            let artifact = crate::exporter::export(&plan, target, false).unwrap();
            assert_eq!(artifact.filename, case.filename, "{}", case.name);
            assert_eq!(artifact.media_type, case.media_type, "{}", case.name);
            assert_eq!(
                crate::digest::sha256(&artifact.content),
                case.content_sha256,
                "{}",
                case.name
            );
        }
        assert_eq!(
            targets,
            ["cwl", "dagger", "nu"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}
