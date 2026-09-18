use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

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

/// The source a message like `action.yml@19750..20912` points at.
///
/// `None` when the message names no range, when the file is not under the root,
/// or when the range is not inside it — a wrong excerpt would be worse than
/// none, because it would be read as the thing the message is about.
fn anchored_source(root: &Path, message: &str) -> Option<serde_json::Value> {
    let (anchor, _) = message.split_once(": ")?;
    let (path, range) = anchor.rsplit_once('@')?;
    let (start, end) = range.split_once("..")?;
    let (start, end) = (start.parse::<usize>().ok()?, end.parse::<usize>().ok()?);
    if start > end || crate::ir::normalize_path(path).ok()? != path {
        return None;
    }
    let bytes = std::fs::read(root.join(path)).ok()?;
    if end > bytes.len() {
        return None;
    }
    let text = std::str::from_utf8(bytes.get(start..end)?).ok()?;
    // Lines are one-based and count from the start of the file, which is what a
    // reader opening the file will see.
    let first = bytes
        .get(..start)?
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let last = first + text.lines().count().saturating_sub(1);
    Some(serde_json::json!({
        "path": path,
        "start_byte": start,
        "end_byte": end,
        "start_line": first,
        "end_line": last,
        "text": text,
    }))
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

    /// Emit the report for a reader that cannot ask a follow-up question.
    ///
    /// The same values as `emit_json`, plus two things a consumer would
    /// otherwise spend a round trip on:
    ///
    /// - the source each anchored message refers to, read and inlined. A
    ///   message says `action.yml@19750..20912`; resolving that means opening
    ///   the file and slicing it, which is a step between being told about a
    ///   problem and being able to look at it.
    /// - a `schema` block saying what each field means, so the shape does not
    ///   have to be learned from an example first.
    ///
    /// Emitted beside the canonical values rather than instead of them: a
    /// consumer that already knows the shape reads the same fields it always
    /// did.
    pub(crate) fn emit_agent(&self, root: &Path, writer: &mut dyn Write) -> Result<(), String> {
        let mut value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let object = value
            .as_object_mut()
            .ok_or("a report serializes to an object")?;
        object.insert(
            "schema".into(),
            serde_json::json!({
                "status": "ok, blocked, or failed. `blocked` means the command ran and the work cannot proceed.",
                "summary": "one line, the same line the human format prints first.",
                "next_actions": "each is an argv to run as it stands; no substitution is needed.",
                "details.counts": "numbers the summary is derived from, so a claim can be checked against them.",
                "details.items": "one per finding. `name` is a stable code; `message` is prose.",
                "details.items[].source": "present when the message names a byte range: the file, the range, the lines it covers, and the text itself.",
            }),
        );
        let items = object
            .get_mut("details")
            .and_then(|details| details.get_mut("items"))
            .and_then(|items| items.as_array_mut())
            .ok_or("a report's details carry an items array")?;
        for (item, source) in items.iter_mut().zip(self.details.items.iter()) {
            let Some(excerpt) = source
                .message
                .as_deref()
                .and_then(|message| anchored_source(root, message))
            else {
                continue;
            };
            let Some(item) = item.as_object_mut() else {
                continue;
            };
            item.insert("source".into(), excerpt);
        }
        writer
            .write_all(&crate::canonical_json::pretty_bytes(&value)?)
            .map_err(|error| error.to_string())
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

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "a test lays out the tree the agent format reads, which the production ban routes through the patch layer"
)]
mod tests {
    use super::*;

    /// The agent format resolves a byte anchor into the source it names.
    ///
    /// A message says `build.sh@10..20`. Turning that into something a reader
    /// can look at means opening the file and slicing it — a step between being
    /// told about a problem and being able to see it, and one this session
    /// wrote a throwaway script for more than once.
    #[test]
    fn the_agent_format_inlines_the_source_an_anchor_points_at() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("build.sh"), "one\ntwo\nthree\nfour\n").unwrap();
        let mut report = Report::new("migrate", Status::Blocked, "one blocker");
        report.details.items.push(Item {
            name: Some("DESHELL_BLOCKER_UNIMPLEMENTED_SEMANTIC".into()),
            message: Some("build.sh@4..13: a reason".into()),
            ..Item::default()
        });
        // No anchor, and an anchor outside the file: neither may invent an
        // excerpt, because a wrong one reads as the thing the message is about.
        report.details.items.push(Item {
            message: Some("a message with no anchor".into()),
            ..Item::default()
        });
        report.details.items.push(Item {
            message: Some("build.sh@4..900: past the end".into()),
            ..Item::default()
        });
        report.details.items.push(Item {
            message: Some("../outside.sh@0..1: not under the root".into()),
            ..Item::default()
        });

        let mut output = Vec::new();
        report.emit_agent(directory.path(), &mut output).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

        // The shape is described in the output, so a reader needs no example
        // first.
        assert!(value["schema"]["details.items[].source"].is_string());

        let items = value["details"]["items"].as_array().unwrap();
        assert_eq!(items[0]["source"]["text"], "two\nthree");
        assert_eq!(items[0]["source"]["start_line"], 2);
        assert_eq!(items[0]["source"]["end_line"], 3);
        assert_eq!(items[0]["source"]["path"], "build.sh");
        for item in &items[1..] {
            assert!(item.get("source").is_none(), "{item}");
        }

        // The canonical values are still the canonical values: a consumer that
        // knows the shape reads the fields it always did.
        let mut plain = Vec::new();
        report.emit_json(&mut plain).unwrap();
        let plain: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(value["status"], plain["status"]);
        assert_eq!(
            value["details"]["items"][0]["message"],
            plain["details"]["items"][0]["message"]
        );
    }
}
