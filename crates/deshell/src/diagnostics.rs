use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum Mode {
    Human,
    Jsonl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
/// The severity vocabulary of `contracts/schema/diagnostic-v1.schema.json`.
///
/// `Note` is in the contract and nothing in de-shell emits one yet. That is the
/// whole reason it is unconstructed, and the `expect` used to give a different
/// one — "exercised only under specific platforms or feature gates", which was
/// not true of any of the three and so could have covered a variant that was
/// genuinely left over. An `expect` whose reason is false costs what an `allow`
/// costs.
///
/// The variant stays because the schema is the vocabulary a reader of the JSONL
/// stream decodes against, not because something might construct it later.
/// [`tests::severity_vocabulary_matches_the_diagnostic_schema`] is what holds
/// the two lists equal in both directions.
// `not(test)` because the schema comparison below does construct all three, so
// outside a test build is exactly where `Note` is unconstructed.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "`Note` is declared by contracts/schema/diagnostic-v1.schema.json and nothing emits one yet; the schema comparison in tests is what keeps the vocabularies equal"
    )
)]
pub(crate) enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    /// Every severity, in the order the schema lists them.
    ///
    /// Beside the enum so the two are read together, and `#[cfg(test)]` because
    /// the comparison with the schema is the only thing that needs it.
    #[cfg(test)]
    const ALL: &'static [Self] = &[Self::Error, Self::Warning, Self::Note];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Diagnostic {
    pub schema_version: u32,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub help: String,
    pub next_actions: Vec<crate::report::Action>,
    pub context: BTreeMap<String, String>,
}

impl Diagnostic {
    /// An error whose next step the caller supplies, or none if there is none
    /// to give.
    ///
    /// The default used to be `deshell --help`, which is the right next step
    /// for a misspelled flag and the wrong one for everything else. A reader
    /// following it for a project that has not been initialised spends three
    /// commands arriving at `deshell init` — a wrong next step costs more than
    /// a missing one, because it stops the search somewhere else.
    pub(crate) fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            help: "See the reported condition; no next step is known for it.".into(),
            next_actions: Vec::new(),
            context: BTreeMap::new(),
        }
    }

    /// A usage error, where reading the command's syntax is the next step.
    pub(crate) fn usage(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            help: "Run deshell --help for command syntax and examples.".into(),
            next_actions: vec![crate::report::Action::Command {
                argv: vec!["deshell".into(), "--help".into()],
            }],
            ..Self::error(code, message)
        }
    }

    pub(crate) fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            severity: Severity::Warning,
            code: code.into(),
            message: message.into(),
            help: "Review the reported condition before continuing.".into(),
            next_actions: Vec::new(),
            context: BTreeMap::new(),
        }
    }
}

pub(crate) fn emit(
    writer: &mut dyn Write,
    mode: Mode,
    diagnostic: &Diagnostic,
) -> std::io::Result<()> {
    match mode {
        Mode::Jsonl => {
            let value = serde_json::to_value(diagnostic)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let bytes =
                crate::canonical_json::canonical_bytes(&value).map_err(std::io::Error::other)?;
            writer.write_all(&bytes)?;
            writer.write_all(b"\n")
        }
        Mode::Human => {
            let severity = match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Note => "note",
            };
            writeln!(
                writer,
                "{severity}[{}]: {}",
                diagnostic.code, diagnostic.message
            )?;
            writeln!(writer, "  help: {}", diagnostic.help)?;
            for action in &diagnostic.next_actions {
                match action {
                    crate::report::Action::Command { argv } => writeln!(
                        writer,
                        "  next argv: {}",
                        serde_json::to_string(argv).unwrap_or_else(|_| "[]".into())
                    )?,
                    crate::report::Action::Review { paths } => {
                        writeln!(writer, "  review: {}", paths.join(", "))?
                    }
                }
            }
            for (name, value) in &diagnostic.context {
                writeln!(writer, "  {name}: {value}")?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The severity vocabulary and the diagnostic schema say the same thing.
    ///
    /// In both directions: a variant added here without a line in the schema
    /// writes a `severity` no reader can decode, and a value added to the schema
    /// without a variant here is a promise nothing can keep. `Severity::Note` is
    /// the second case standing still — it is in the contract and nothing emits
    /// one — which is why it is unconstructed and why that has to be checked
    /// rather than asserted in an `expect` reason.
    #[test]
    fn severity_vocabulary_matches_the_diagnostic_schema() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("contracts/schema/diagnostic-v1.schema.json");
        let schema: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&path).unwrap()).unwrap();
        let recorded = schema["properties"]["severity"]["enum"]
            .as_array()
            .expect("the diagnostic schema declares a severity enum")
            .iter()
            .map(|value| value.as_str().expect("a severity is a string").to_owned())
            .collect::<Vec<_>>();
        let declared = Severity::ALL
            .iter()
            .map(|severity| {
                serde_json::to_value(severity)
                    .unwrap()
                    .as_str()
                    .expect("a severity serialises as a string")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(declared, recorded);
    }

    #[test]
    fn jsonl_is_one_compact_strict_line_with_stable_keys() {
        let mut diagnostic = Diagnostic::usage("DESHELL_USAGE", "bad option");
        diagnostic.context.insert("argument".into(), "--bad".into());
        let mut output = Vec::new();
        emit(&mut output, Mode::Jsonl, &diagnostic).unwrap();
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(!output[..output.len() - 1].contains(&b'\n'));
        let value: serde_json::Value = crate::strict_json::parse(&output).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["severity"], "error");
        assert_eq!(value["code"], "DESHELL_USAGE");
        assert_eq!(output, b"{\"code\":\"DESHELL_USAGE\",\"context\":{\"argument\":\"--bad\"},\"help\":\"Run deshell --help for command syntax and examples.\",\"message\":\"bad option\",\"next_actions\":[{\"action\":\"command\",\"argv\":[\"deshell\",\"--help\"]}],\"schema_version\":1,\"severity\":\"error\"}\n");
    }

    /// An error offers a next step when there is one, and none when there is
    /// not.
    ///
    /// `Diagnostic::error` used to offer `deshell --help` for every error. It
    /// is the answer for a misspelled flag and for nothing else, and a reader
    /// following it spends commands arriving somewhere the answer is not.
    #[test]
    fn human_diagnostic_is_stderr_friendly_and_has_context() {
        let mut diagnostic = Diagnostic::error("DESHELL_INVALID_IR", "plan is invalid");
        diagnostic
            .context
            .insert("path".into(), ".deshell/plan.json".into());
        let mut output = Vec::new();
        emit(&mut output, Mode::Human, &diagnostic).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "error[DESHELL_INVALID_IR]: plan is invalid\n  help: See the reported condition; no next step is known for it.\n  path: .deshell/plan.json\n"
        );

        let mut diagnostic = Diagnostic::error("DESHELL_INVALID_IR", "plan is invalid");
        diagnostic.help = "Rebuild the plan.".into();
        diagnostic.next_actions = vec![crate::report::Action::Command {
            argv: vec!["deshell".into(), "migrate".into(), "plan".into()],
        }];
        let mut output = Vec::new();
        emit(&mut output, Mode::Human, &diagnostic).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "error[DESHELL_INVALID_IR]: plan is invalid\n  help: Rebuild the plan.\n  next argv: [\"deshell\",\"migrate\",\"plan\"]\n"
        );
    }
}
