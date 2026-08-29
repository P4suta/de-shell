use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Ok,
    NotReady,
    Blocked,
    Different,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Command { argv: Vec<String> },
    Review { paths: Vec<String> },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Item {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Details {
    pub counts: BTreeMap<String, u64>,
    pub values: BTreeMap<String, String>,
    pub paths: Vec<String>,
    pub items: Vec<Item>,
    pub output: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    pub schema_version: u32,
    pub command: String,
    pub status: Status,
    pub summary: String,
    pub next_actions: Vec<Action>,
    pub details: Details,
}

impl Report {
    pub(crate) fn new(
        command: impl Into<String>,
        status: Status,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            command: command.into(),
            status,
            summary: summary.into(),
            next_actions: Vec::new(),
            details: Details::default(),
        }
    }

    pub(crate) fn emit_json(&self, writer: &mut dyn Write) -> Result<(), String> {
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        writer
            .write_all(&crate::canonical_json::pretty_bytes(&value)?)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn emit_human(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writeln!(writer, "{}", self.summary)?;
        for line in &self.details.output {
            writeln!(writer, "{line}")?;
        }
        for action in &self.next_actions {
            match action {
                Action::Command { argv } => writeln!(writer, "next argv: {}", argv_json(argv))?,
                Action::Review { paths } => writeln!(writer, "review: {}", paths.join(", "))?,
            }
        }
        Ok(())
    }
}

fn argv_json(argv: &[String]) -> String {
    serde_json::to_string(argv).unwrap_or_else(|_| "[]".into())
}
