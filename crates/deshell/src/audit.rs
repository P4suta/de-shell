use crate::config::{AuditAcknowledgement, AuditSeverity};
use crate::scanner::{FindingKind, Inventory};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Category {
    Injection,
    SupplyChain,
    Filesystem,
    Secret,
    Race,
    Status,
    Nondeterminism,
    Interpreter,
    Portability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Finding {
    pub schema_version: u32,
    pub rule_id: String,
    pub category: Category,
    pub severity: AuditSeverity,
    pub confidence: Confidence,
    pub message: String,
    pub url: String,
    pub path: String,
    pub span: Span,
    pub location_digest: String,
    pub acknowledged: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
    pub start_byte: u64,
    pub end_byte: u64,
}

struct Rule {
    id: &'static str,
    category: Category,
    severity: AuditSeverity,
    confidence: Confidence,
    message: &'static str,
    expression: regex::Regex,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            "shell.dynamic-eval",
            Category::Injection,
            AuditSeverity::High,
            r"\beval\b",
            "Dynamic evaluation can reinterpret untrusted text as commands.",
        ),
        rule(
            "supply-chain.download-execute",
            Category::SupplyChain,
            AuditSeverity::Critical,
            r"(?m)\b(?:curl|wget)\b[^\r\n|]*\|[ \t]*(?:sh|bash|zsh|pwsh|powershell)\b",
            "Downloaded bytes are executed without an independently verified artifact digest.",
        ),
        rule(
            "supply-chain.unpinned-reference",
            Category::SupplyChain,
            AuditSeverity::Medium,
            r#"(?i):latest\b|https?://[^[:space:]'"]+\.(?:sh|ps1)\b"#,
            "Artifact or image reference is mutable or lacks an immutable digest.",
        ),
        rule(
            "filesystem.dangerous-delete",
            Category::Filesystem,
            AuditSeverity::High,
            r"(?m)\brm[ \t]+-[A-Za-z]*r[A-Za-z]*[ \t]+[^\r\n;]+",
            "Recursive deletion depends on a path whose boundary must be proven.",
        ),
        rule(
            "filesystem.unquoted-expansion",
            Category::Filesystem,
            AuditSeverity::High,
            r"(?m)\b(?:rm|cp|mv|chmod|chown|install|tar)\b[^\r\n#]*?[ \t](?P<risk>\$\{?[A-Za-z_][A-Za-z0-9_]*\}?)(?:/|[ \t;&|]|$)",
            "An unquoted path expansion can split into multiple arguments.",
        ),
        rule(
            "filesystem.unbounded-glob",
            Category::Filesystem,
            AuditSeverity::High,
            r"(?m)\b(?:rm|cp|mv|chmod|chown|install|tar)\b[^\r\n#]*?[ \t](?P<risk>[^ \t\r\n;|&]*[*?][^ \t\r\n;|&]*)",
            "A filesystem mutation depends on ambient glob expansion and no-match policy.",
        ),
        rule(
            "filesystem.symlink-race",
            Category::Race,
            AuditSeverity::High,
            r"(?m)(?P<risk>\bln[ \t]+-[A-Za-z]*s[A-Za-z]*\b)",
            "Symlink creation requires a reviewed boundary against path substitution races.",
        ),
        rule(
            "filesystem.toctou-check",
            Category::Race,
            AuditSeverity::High,
            r"(?m)(?P<risk>\btest[ \t]+-[efL]\b|\[[ \t]+-[efL]\b)",
            "A path existence or type check can race with a later filesystem operation.",
        ),
        rule(
            "filesystem.world-writable",
            Category::Filesystem,
            AuditSeverity::High,
            r"(?m)\bchmod[ \t]+(?:0?777|a\+w)\b",
            "World-writable permissions exceed least privilege.",
        ),
        rule(
            "privilege.escalation",
            Category::Filesystem,
            AuditSeverity::High,
            r"(?m)(?:^|[;&|][ \t]*)sudo\b",
            "Privileged execution requires an explicit reviewed capability boundary.",
        ),
        rule(
            "secret.argv-exposure",
            Category::Secret,
            AuditSeverity::High,
            r"(?i)\$(?:\{)?[A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|PRIVATE_KEY)[A-Z0-9_]*(?:\})?",
            "A secret-like environment value is exposed through process arguments or output.",
        ),
        rule(
            "filesystem.temp-race",
            Category::Race,
            AuditSeverity::High,
            r#"(?m)\bmktemp[ \t]+-u\b|(?:^|[[:space:]'"])/tmp/[^[:space:]'"]+"#,
            "Predictable or non-atomically reserved temporary paths permit TOCTOU or symlink races.",
        ),
        rule(
            "status.unchecked-cwd",
            Category::Status,
            AuditSeverity::Medium,
            r"(?m)^[ \t]*cd[ \t]+[^\r\n;&|]+$",
            "Working-directory changes must have explicit failure behavior.",
        ),
        rule(
            "status.pipeline",
            Category::Status,
            AuditSeverity::Medium,
            r"(?m)^[^#\r\n]*[^|]\|[^|][^\r\n]*$",
            "Pipeline status semantics must be explicit and checked.",
        ),
        rule(
            "nondeterminism.clock",
            Category::Nondeterminism,
            AuditSeverity::Low,
            r"\bdate\b|\bGet-Date\b",
            "Wall-clock input makes behavior dependent on execution time.",
        ),
        rule(
            "nondeterminism.random",
            Category::Nondeterminism,
            AuditSeverity::Medium,
            r"\$RANDOM\b|\b(?:openssl[ \t]+rand|uuidgen)\b",
            "Unseeded randomness makes repeated verification unstable.",
        ),
        rule(
            "nondeterminism.ambient-environment",
            Category::Nondeterminism,
            AuditSeverity::Low,
            r"\$(?:\{)?(?:PATH|LANG|LC_ALL|TZ|HOME)(?:\})?\b",
            "Ambient environment state is not declared as an input.",
        ),
        rule(
            "portability.bashism",
            Category::Portability,
            AuditSeverity::Medium,
            r"\[\[|\]\]",
            "Interpreter-specific syntax conflicts with a portable shell contract.",
        ),
    ]
});

static POWERSHELL_BLOCK_COMMENT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?s)<#.*?#>").expect("static PowerShell block-comment regex")
});
static POWERSHELL_SINGLE_HERE_STRING: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?ms)^[ \t]*@'\r?\n.*?^[ \t]*'@[ \t]*\r?$")
        .expect("static PowerShell single here-string regex")
});
static POWERSHELL_DOUBLE_HERE_STRING: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new("(?ms)^[ \\t]*@\"\\r?\\n.*?^[ \\t]*\"@[ \\t]*\\r?$")
        .expect("static PowerShell double here-string regex")
});

fn rule(
    id: &'static str,
    category: Category,
    severity: AuditSeverity,
    expression: &'static str,
    message: &'static str,
) -> Rule {
    Rule {
        id,
        category,
        severity,
        confidence: Confidence::High,
        message,
        expression: regex::Regex::new(expression).expect("static audit regex"),
    }
}

pub(crate) fn analyze(
    root: &Path,
    inventory: &Inventory,
    acknowledgements: &[AuditAcknowledgement],
    acknowledgement_max_days: u32,
) -> Result<Vec<Finding>, String> {
    let mut output = Vec::new();
    for location in &inventory.findings {
        if location.kind == FindingKind::Candidate {
            let source = read_host_source(root, &location.path)?;
            output.push(make_finding(
                "shell.dynamic-command",
                Category::Injection,
                AuditSeverity::High,
                Confidence::High,
                "A dynamic process call cannot be proven to avoid shell interpretation.",
                &location.path,
                &source,
                location.span.start_byte as usize,
                location.span.end_byte as usize,
                &location.content_digest,
                acknowledgements,
                acknowledgement_max_days,
            )?);
            continue;
        }
        let snippet = match std::str::from_utf8(&location.source) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let protected = audit_protected_ranges(location.interpreter.as_deref(), snippet);
        let host_source = if location.kind == FindingKind::ShellFile {
            snippet.to_owned()
        } else {
            read_host_source(root, &location.path)?
        };
        for rule in RULES.iter() {
            if rule.id == "portability.bashism" && location.interpreter.as_deref() != Some("sh") {
                continue;
            }
            for captures in rule.expression.captures_iter(snippet) {
                let matched = captures
                    .name("risk")
                    .or_else(|| captures.get(0))
                    .expect("every regex capture has a whole match");
                if protected
                    .iter()
                    .any(|(start, end)| matched.start() < *end && matched.end() > *start)
                {
                    continue;
                }
                let (start, end) = if location.kind == FindingKind::ShellFile {
                    (matched.start(), matched.end())
                } else {
                    embedded_host_match_span(&host_source, snippet, location, &matched)
                };
                output.push(make_finding(
                    rule.id,
                    rule.category,
                    rule.severity,
                    rule.confidence,
                    rule.message,
                    &location.path,
                    &host_source,
                    start,
                    end,
                    &location.content_digest,
                    acknowledgements,
                    acknowledgement_max_days,
                )?);
            }
        }
    }
    for error in &inventory.errors {
        if error.stage != "interpreter" {
            return Err(format!(
                "audit requires a complete scan: {}:{}:{}",
                error.path.as_deref().unwrap_or("<root>"),
                error.stage,
                error.message
            ));
        }
        let Some(path) = &error.path else {
            continue;
        };
        let source = read_host_source(root, path)?;
        output.push(make_finding(
            if error.message.contains("CONFLICT") {
                "interpreter.conflict"
            } else {
                "interpreter.unknown"
            },
            Category::Interpreter,
            AuditSeverity::Critical,
            Confidence::High,
            &error.message,
            path,
            &source,
            0,
            source.len(),
            &crate::digest::sha256(source.as_bytes()),
            acknowledgements,
            acknowledgement_max_days,
        )?);
    }
    if let Some(skipped) = inventory.skipped.first() {
        return Err(format!(
            "audit requires a complete scan: {}:skipped:{}",
            skipped.path, skipped.reason
        ));
    }
    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.span.start_byte.cmp(&right.span.start_byte))
            .then_with(|| left.span.end_byte.cmp(&right.span.end_byte))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
    output.dedup_by(|left, right| {
        left.rule_id == right.rule_id
            && left.path == right.path
            && left.span.start_byte == right.span.start_byte
            && left.span.end_byte == right.span.end_byte
    });
    Ok(output)
}

fn embedded_host_match_span(
    host: &str,
    snippet: &str,
    location: &crate::scanner::Finding,
    matched: &regex::Match<'_>,
) -> (usize, usize) {
    let fallback = (
        location.span.start_byte as usize,
        location.span.end_byte as usize,
    );
    let (start, end) = fallback;
    if start > end
        || end > host.len()
        || !host.is_char_boundary(start)
        || !host.is_char_boundary(end)
    {
        return fallback;
    }
    let needle = matched.as_str();
    if needle.is_empty() {
        return fallback;
    }
    let ordinal = snippet[..matched.start()].match_indices(needle).count();
    host[start..end]
        .match_indices(needle)
        .nth(ordinal)
        .map_or(fallback, |(offset, value)| {
            (start + offset, start + offset + value.len())
        })
}

fn audit_protected_ranges(interpreter: Option<&str>, source: &str) -> Vec<(usize, usize)> {
    let mut ranges = match interpreter {
        Some("sh" | "bash" | "zsh" | "fish" | "nu" | "powershell") => {
            crate::rewrite::protected_ranges(source)
        }
        Some("cmd") => cmd_comment_ranges(source),
        _ => Vec::new(),
    };
    if interpreter == Some("powershell") {
        for expression in [
            &*POWERSHELL_BLOCK_COMMENT,
            &*POWERSHELL_SINGLE_HERE_STRING,
            &*POWERSHELL_DOUBLE_HERE_STRING,
        ] {
            ranges.extend(
                expression
                    .find_iter(source)
                    .map(|matched| (matched.start(), matched.end())),
            );
        }
    }
    ranges.sort_unstable();
    ranges
}

fn cmd_comment_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive('\n') {
        let command = line
            .trim_start_matches([' ', '\t'])
            .strip_prefix('@')
            .unwrap_or_else(|| line.trim_start_matches([' ', '\t']))
            .trim_start_matches([' ', '\t']);
        let lower = command.to_ascii_lowercase();
        let is_rem = lower
            .strip_prefix("rem")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']));
        if is_rem || command.starts_with("::") {
            ranges.push((start, start + line.len()));
        }
        start += line.len();
    }
    ranges
}

#[allow(clippy::too_many_arguments)]
fn make_finding(
    rule_id: &str,
    category: Category,
    severity: AuditSeverity,
    confidence: Confidence,
    message: &str,
    path: &str,
    source: &str,
    start: usize,
    end: usize,
    source_digest: &str,
    acknowledgements: &[AuditAcknowledgement],
    acknowledgement_max_days: u32,
) -> Result<Finding, String> {
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return Err(format!(
            "audit span is outside UTF-8 source for {path}: {start}..{end}"
        ));
    }
    let (start_line, start_column) = position(source, start);
    let (end_line, end_column) = position(source, end);
    let location_digest = crate::digest::sha256(
        format!("deshell.audit-location.v1\0{rule_id}\0{path}\0{start}\0{end}\0{source_digest}")
            .as_bytes(),
    );
    let acknowledged = !matches!(
        rule_id.split_once('.').map(|value| value.0),
        Some("shell" | "interpreter")
    ) && acknowledgements.iter().any(|acknowledgement| {
        acknowledgement.rule == rule_id
            && acknowledgement.location_digest == location_digest
            && acknowledgement_is_active(
                &acknowledgement.expires,
                acknowledgement_max_days,
                current_unix_days(),
            )
    });
    Ok(Finding {
        schema_version: 1,
        rule_id: rule_id.into(),
        category,
        severity,
        confidence,
        message: message.into(),
        url: format!("https://deshell.dev/rules/{rule_id}"),
        path: path.into(),
        span: Span {
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte: start as u64,
            end_byte: end as u64,
        },
        location_digest,
        acknowledged,
    })
}

fn read_host_source(root: &Path, relative: &str) -> Result<String, String> {
    let (_, path) = crate::project::resolve_entry(root, relative)?;
    String::from_utf8(
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    )
    .map_err(|_| format!("audit host source is not UTF-8: {relative}"))
}

fn position(source: &str, byte: usize) -> (u64, u64) {
    let mut line = 1;
    let mut column = 0;
    for character in source[..byte].chars() {
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn acknowledgement_is_active(expires: &str, max_days: u32, today: i64) -> bool {
    let Some(expiry) = parse_date_days(expires) else {
        return false;
    };
    expiry >= today && expiry - today <= i64::from(max_days)
}

fn current_unix_days() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() / 86_400) as i64
}

fn parse_date_days(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return None;
    }
    let year = value[..4].parse::<i64>().ok()?;
    let month = value[5..7].parse::<i64>().ok()?;
    let day = value[8..].parse::<i64>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    (civil_from_days(days) == (year, month as u32, day as u32)).then_some(days)
}

// Howard Hinnant's civil calendar conversion, with Unix epoch day zero.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{
        ByteSpan, Finding as InventoryFinding, InterpreterConfidence, ScanError, Skipped,
    };

    fn inventory_finding(path: &str, kind: FindingKind, source: &[u8]) -> InventoryFinding {
        InventoryFinding {
            path: path.into(),
            kind,
            interpreter: Some("sh".into()),
            interpreter_confidence: InterpreterConfidence::High,
            locator: None,
            span: ByteSpan {
                start_byte: 0,
                end_byte: source.len() as u64,
            },
            content_digest: crate::digest::sha256(source),
            source: source.to_vec(),
        }
    }

    fn inventory(findings: Vec<InventoryFinding>) -> Inventory {
        Inventory {
            schema_version: crate::scanner::INVENTORY_SCHEMA_VERSION,
            findings,
            skipped: vec![],
            errors: vec![],
        }
    }

    #[test]
    fn unix_epoch_calendar_conversion_is_stable() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_693), (2026, 8, 28));
    }

    #[test]
    fn acknowledgement_window_rejects_expired_and_overlong_exceptions() {
        let today = 20_693;
        assert!(acknowledgement_is_active("2026-08-28", 30, today));
        assert!(acknowledgement_is_active("2026-09-27", 30, today));
        assert!(!acknowledgement_is_active("2026-09-28", 30, today));
        assert!(!acknowledgement_is_active("2026-08-27", 30, today));
    }

    #[test]
    fn migration_blockers_are_not_suppressible_by_hardening_acknowledgements() {
        for rule_id in ["shell.dynamic-command", "interpreter.conflict"] {
            let initial = make_finding(
                rule_id,
                Category::Injection,
                AuditSeverity::High,
                Confidence::High,
                "blocker",
                "build.sh",
                "eval value\n",
                0,
                4,
                &crate::digest::sha256(b"eval value\n"),
                &[],
                u32::MAX,
            )
            .unwrap();
            let acknowledgement = AuditAcknowledgement {
                rule: rule_id.into(),
                location_digest: initial.location_digest,
                reason: "reviewed".into(),
                owner: "security".into(),
                expires: "2099-01-01".into(),
            };
            let repeated = make_finding(
                rule_id,
                Category::Injection,
                AuditSeverity::High,
                Confidence::High,
                "blocker",
                "build.sh",
                "eval value\n",
                0,
                4,
                &crate::digest::sha256(b"eval value\n"),
                &[acknowledgement],
                u32::MAX,
            )
            .unwrap();
            assert!(!repeated.acknowledged, "{rule_id} was suppressed");
        }
    }

    #[test]
    fn audit_directly_classifies_rules_sorts_findings_and_skips_non_utf8_shell_bytes() {
        let source = b"eval value\ncurl https://example.test/install.sh | sh\nrm -rf $TARGET/*.tmp\ncd work\ndate\necho $PATH\n[[ -f file ]]\n";
        let duplicate = inventory_finding("z.sh", FindingKind::ShellFile, source);
        let findings = analyze(
            Path::new("."),
            &inventory(vec![
                duplicate.clone(),
                inventory_finding("a.sh", FindingKind::ShellFile, source),
                duplicate,
                inventory_finding("binary.sh", FindingKind::ShellFile, b"\xff"),
            ]),
            &[],
            30,
        )
        .unwrap();
        let rules = findings
            .iter()
            .map(|finding| finding.rule_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in [
            "shell.dynamic-eval",
            "supply-chain.download-execute",
            "supply-chain.unpinned-reference",
            "filesystem.dangerous-delete",
            "filesystem.unquoted-expansion",
            "filesystem.unbounded-glob",
            "status.unchecked-cwd",
            "nondeterminism.clock",
            "nondeterminism.ambient-environment",
            "portability.bashism",
        ] {
            assert!(rules.contains(expected), "missing {expected:?}: {rules:?}");
        }
        assert!(findings.windows(2).all(|pair| {
            (
                &pair[0].path,
                pair[0].span.start_byte,
                pair[0].span.end_byte,
                &pair[0].rule_id,
            ) <= (
                &pair[1].path,
                pair[1].span.start_byte,
                pair[1].span.end_byte,
                &pair[1].rule_id,
            )
        }));
        let z_eval = findings
            .iter()
            .filter(|finding| finding.path == "z.sh" && finding.rule_id == "shell.dynamic-eval")
            .count();
        assert_eq!(z_eval, 1, "duplicate locations must be collapsed");
    }

    #[test]
    fn candidate_and_interpreter_errors_become_precise_unsuppressible_findings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("host.py"), "os.system(command)\n").unwrap();
        std::fs::write(root.path().join("script.unknown"), "echo value\n").unwrap();
        let candidate_source = b"os.system(command)";
        let mut candidate = inventory_finding("host.py", FindingKind::Candidate, candidate_source);
        candidate.interpreter = None;
        candidate.span = ByteSpan {
            start_byte: 0,
            end_byte: candidate_source.len() as u64,
        };
        let mut input = inventory(vec![candidate]);
        input.errors = vec![
            ScanError {
                path: None,
                stage: "interpreter".into(),
                message: "UNKNOWN_INTERPRETER".into(),
            },
            ScanError {
                path: Some("script.unknown".into()),
                stage: "interpreter".into(),
                message: "INTERPRETER_CONFLICT".into(),
            },
            ScanError {
                path: Some("script.unknown".into()),
                stage: "interpreter".into(),
                message: "UNKNOWN_INTERPRETER".into(),
            },
        ];
        let findings = analyze(root.path(), &input, &[], 30).unwrap();
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.rule_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "shell.dynamic-command",
                "interpreter.conflict",
                "interpreter.unknown"
            ]
        );
        assert!(findings.iter().all(|finding| !finding.acknowledged));
    }

    #[test]
    fn incomplete_inventory_and_unreadable_hosts_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let mut failed = inventory(vec![]);
        failed.errors.push(ScanError {
            path: Some("bad.sh".into()),
            stage: "parse".into(),
            message: "syntax error".into(),
        });
        assert!(
            analyze(root.path(), &failed, &[], 30)
                .unwrap_err()
                .contains("complete scan")
        );

        let mut skipped = inventory(vec![]);
        skipped.skipped.push(Skipped {
            path: "large.sh".into(),
            reason: "too large".into(),
        });
        assert!(
            analyze(root.path(), &skipped, &[], 30)
                .unwrap_err()
                .contains("skipped")
        );

        std::fs::write(root.path().join("host.py"), b"\xff").unwrap();
        let candidate = inventory_finding("host.py", FindingKind::Candidate, b"x");
        assert!(
            analyze(root.path(), &inventory(vec![candidate]), &[], 30)
                .unwrap_err()
                .contains("not UTF-8")
        );
    }

    #[test]
    fn hardening_acknowledgement_matches_exact_location_and_active_window() {
        let source = "rm -rf target\n";
        let initial = make_finding(
            "filesystem.dangerous-delete",
            Category::Filesystem,
            AuditSeverity::High,
            Confidence::High,
            "dangerous",
            "build.sh",
            source,
            0,
            13,
            &crate::digest::sha256(source.as_bytes()),
            &[],
            u32::MAX,
        )
        .unwrap();
        let acknowledgement = AuditAcknowledgement {
            rule: initial.rule_id.clone(),
            location_digest: initial.location_digest.clone(),
            reason: "reviewed".into(),
            owner: "security".into(),
            expires: "2099-01-01".into(),
        };
        let acknowledged = make_finding(
            &initial.rule_id,
            initial.category,
            initial.severity,
            initial.confidence,
            &initial.message,
            &initial.path,
            source,
            0,
            13,
            &crate::digest::sha256(source.as_bytes()),
            &[acknowledgement],
            u32::MAX,
        )
        .unwrap();
        assert!(acknowledged.acknowledged);
    }

    #[test]
    fn span_comment_and_calendar_helpers_reject_ambiguous_boundaries() {
        let source = "éval\nnext";
        for (start, end) in [(2, 1), (0, source.len() + 1), (1, 2)] {
            assert!(
                make_finding(
                    "test.rule",
                    Category::Portability,
                    AuditSeverity::Low,
                    Confidence::Low,
                    "test",
                    "build.sh",
                    source,
                    start,
                    end,
                    &crate::digest::sha256(source.as_bytes()),
                    &[],
                    30,
                )
                .is_err()
            );
        }
        assert_eq!(position(source, source.len()), (2, 4));

        let mut location = inventory_finding("host.yml", FindingKind::EmbeddedShell, b"");
        location.span = ByteSpan {
            start_byte: 1,
            end_byte: 2,
        };
        let empty = regex::Regex::new("^").unwrap();
        let matched = empty.find("").unwrap();
        assert_eq!(
            embedded_host_match_span("é", "", &location, &matched),
            (1, 2)
        );
        location.span = ByteSpan {
            start_byte: 0,
            end_byte: 2,
        };
        assert_eq!(
            embedded_host_match_span("é", "", &location, &matched),
            (0, 2)
        );

        assert!(audit_protected_ranges(None, "eval value").is_empty());
        assert_eq!(
            cmd_comment_ranges("@REM eval value\r\n:: curl x | sh\n").len(),
            2
        );
        let powershell = "<# eval value #>\n@'\ncurl x | sh\n'@\n@\"\neval x\n\"@\n";
        assert!(audit_protected_ranges(Some("powershell"), powershell).len() >= 3);

        for invalid in [
            "bad",
            "2026/01/01",
            "2026-00-01",
            "2026-01-00",
            "2026-02-30",
        ] {
            assert_eq!(parse_date_days(invalid), None, "accepted {invalid}");
        }
        let ancient = parse_date_days("0000-01-01").unwrap();
        assert_eq!(civil_from_days(ancient), (0, 1, 1));
    }
}
