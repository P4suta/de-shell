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
#[allow(dead_code)]
pub(crate) enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Diagnostic {
    pub schema_version: u32,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub context: BTreeMap<String, String>,
}

impl Diagnostic {
    pub(crate) fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
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

    #[test]
    fn jsonl_is_one_compact_strict_line_with_stable_keys() {
        let mut diagnostic = Diagnostic::error("DESHELL_USAGE", "bad option");
        diagnostic.context.insert("argument".into(), "--bad".into());
        let mut output = Vec::new();
        emit(&mut output, Mode::Jsonl, &diagnostic).unwrap();
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        assert!(!output[..output.len() - 1].contains(&b'\n'));
        let value: serde_json::Value = crate::strict_json::parse(&output).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["severity"], "error");
        assert_eq!(value["code"], "DESHELL_USAGE");
        assert_eq!(output, b"{\"code\":\"DESHELL_USAGE\",\"context\":{\"argument\":\"--bad\"},\"message\":\"bad option\",\"schema_version\":1,\"severity\":\"error\"}\n");
    }

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
            "error[DESHELL_INVALID_IR]: plan is invalid\n  path: .deshell/plan.json\n"
        );
    }
}
