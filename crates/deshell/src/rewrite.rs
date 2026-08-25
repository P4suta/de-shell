use crate::ir::SourceSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Edit {
    pub rule: String,
    pub original: SourceSpan,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RewriteResult {
    pub output: String,
    pub edits: Vec<Edit>,
}

pub(crate) fn equivalent(path: &str, source: &str) -> RewriteResult {
    let mut output = String::with_capacity(source.len() + 16);
    let mut edits = Vec::new();
    let mut state = QuoteState::Normal;
    let mut index = 0;
    while index < source.len() {
        let character = source[index..]
            .chars()
            .next()
            .expect("valid UTF-8 boundary");
        let width = character.len_utf8();
        match (state, character) {
            (QuoteState::Single, '\'') => {
                output.push(character);
                state = QuoteState::Normal;
                index += width;
            }
            (QuoteState::Single, _) => {
                output.push(character);
                index += width;
            }
            (QuoteState::Normal | QuoteState::Double, '\\') => {
                output.push(character);
                index += width;
                if index < source.len() {
                    let escaped = source[index..].chars().next().unwrap();
                    output.push(escaped);
                    index += escaped.len_utf8();
                }
            }
            (QuoteState::Normal, '\'') => {
                output.push(character);
                state = QuoteState::Single;
                index += width;
            }
            (QuoteState::Normal, '"') => {
                output.push(character);
                state = QuoteState::Double;
                index += width;
            }
            (QuoteState::Double, '"') => {
                output.push(character);
                state = QuoteState::Normal;
                index += width;
            }
            (QuoteState::Normal | QuoteState::Double, '`') => {
                if let Some(closing) = find_closing(source, index + 1) {
                    let body = &source[index + 1..closing];
                    if safe_substitution(body) {
                        let replacement = format!("$({body})");
                        output.push_str(&replacement);
                        edits.push(Edit {
                            rule: "posix.backticks.simple".into(),
                            original: span(path, source, index, closing + 1),
                            replacement,
                        });
                        index = closing + 1;
                    } else {
                        output.push_str(&source[index..=closing]);
                        index = closing + 1;
                    }
                } else {
                    output.push_str(&source[index..]);
                    index = source.len();
                }
            }
            _ => {
                output.push(character);
                index += width;
            }
        }
    }
    RewriteResult { output, edits }
}

#[derive(Clone, Copy)]
enum QuoteState {
    Normal,
    Single,
    Double,
}

fn find_closing(source: &str, mut index: usize) -> Option<usize> {
    let mut escaped = false;
    while index < source.len() {
        let character = source[index..].chars().next()?;
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '`' {
            return Some(index);
        }
        index += character.len_utf8();
    }
    None
}

fn safe_substitution(body: &str) -> bool {
    !body.trim().is_empty()
        && !body
            .chars()
            .any(|character| matches!(character, '\\' | '`' | '$' | '\n' | '\r' | '(' | ')'))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Profile {
    Portable,
    Secure,
    Reproducible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Severity {
    Warning,
    High,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Finding {
    pub rule: String,
    pub profile: Profile,
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
    pub auto_applicable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ModernizeResult {
    pub output: String,
    pub edits: Vec<Edit>,
    pub findings: Vec<Finding>,
}

pub(crate) fn modernize(path: &str, source: &str, profiles: &[Profile]) -> ModernizeResult {
    let mut output = source.to_owned();
    let mut edits = Vec::new();
    let mut findings = Vec::new();
    if profiles.contains(&Profile::Secure) && !has_strict_mode(source) {
        let interpreter = crate::frontend::detect(path, source.as_bytes());
        if matches!(
            interpreter,
            crate::frontend::Interpreter::Sh
                | crate::frontend::Interpreter::Bash
                | crate::frontend::Interpreter::Zsh
        ) {
            let offset = insertion_after_shebang(source);
            let replacement = "set -eu\n".to_owned();
            let source_span = span(path, source, offset, offset);
            output.insert_str(offset, &replacement);
            edits.push(Edit {
                rule: "secure.strict-mode".into(),
                original: source_span.clone(),
                replacement,
            });
            findings.push(Finding {
                rule: "secure.strict-mode".into(),
                profile: Profile::Secure,
                severity: Severity::Warning,
                message: "Enable errexit and nounset; this intentionally changes failure behavior and requires --apply.".into(),
                span: source_span,
                auto_applicable: true,
            });
        }
    }
    let whole = || span(path, source, 0, source.len());
    if profiles.contains(&Profile::Secure)
        && source.contains("curl ")
        && (source.contains("| sh") || source.contains("| bash"))
    {
        findings.push(Finding {
            rule: "secure.remote-code-pipe".into(), profile: Profile::Secure, severity: Severity::High,
            message: "Download-then-execute pipeline needs a pinned digest and a reviewed two-step replacement.".into(),
            span: whole(), auto_applicable: false,
        });
    }
    if profiles.contains(&Profile::Secure) && source.contains("chmod 777") {
        findings.push(Finding {
            rule: "secure.world-writable".into(),
            profile: Profile::Secure,
            severity: Severity::High,
            message: "World-writable permissions should be replaced with the least required mode."
                .into(),
            span: whole(),
            auto_applicable: false,
        });
    }
    if profiles.contains(&Profile::Portable) && source.contains("[[") {
        findings.push(Finding {
            rule: "portable.double-bracket".into(),
            profile: Profile::Portable,
            severity: Severity::Warning,
            message: "[[ ... ]] is not POSIX; review a test/case replacement.".into(),
            span: whole(),
            auto_applicable: false,
        });
    }
    if profiles.contains(&Profile::Reproducible) && source.contains(":latest") {
        findings.push(Finding {
            rule: "reproducible.latest-tag".into(),
            profile: Profile::Reproducible,
            severity: Severity::Warning,
            message: "Replace floating :latest references with an immutable digest.".into(),
            span: whole(),
            auto_applicable: false,
        });
    }
    ModernizeResult {
        output,
        edits,
        findings,
    }
}

fn has_strict_mode(source: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim();
        line.starts_with("set -e") || line.starts_with("set -o errexit")
    })
}

fn insertion_after_shebang(source: &str) -> usize {
    if source.starts_with("#!") {
        source.find('\n').map_or(source.len(), |index| index + 1)
    } else {
        0
    }
}

fn span(path: &str, source: &str, start_byte: usize, end_byte: usize) -> SourceSpan {
    let (start_line, start_column) = position(source, start_byte);
    let (end_line, end_column) = position(source, end_byte);
    SourceSpan {
        file: path.into(),
        start_line,
        start_column,
        end_line,
        end_column,
        start_byte: start_byte as u64,
        end_byte: end_byte as u64,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_safe_backticks_and_is_idempotent() {
        let first = equivalent("build.sh", "echo `printf hi`\nprintf '`date`'\n");
        assert_eq!(first.output, "echo $(printf hi)\nprintf '`date`'\n");
        assert_eq!(first.edits.len(), 1);
        assert_eq!(first.edits[0].rule, "posix.backticks.simple");
        assert_eq!(first.edits[0].original.start_byte, 5);
        assert_eq!(first.edits[0].original.end_byte, 16);
        let second = equivalent("build.sh", &first.output);
        assert_eq!(second.output, first.output);
        assert!(second.edits.is_empty());
        assert_eq!(
            equivalent("build.sh", "echo `printf \\`nested\\``\n")
                .edits
                .len(),
            0
        );
    }

    #[test]
    fn rewrite_spans_count_unicode_scalars() {
        let result = equivalent("unicode.sh", "printf é; echo `date`\n");
        let span = &result.edits[0].original;
        assert_eq!(span.start_byte, 16);
        assert_eq!(span.start_column, 15);
    }

    #[test]
    fn secure_modernization_is_explicit_profile_scoped_and_idempotent() {
        let source = "#!/bin/sh\necho hello\n";
        assert_eq!(
            modernize("build.sh", source, &[Profile::Portable]).output,
            source
        );
        let first = modernize("build.sh", source, &[Profile::Secure]);
        assert_eq!(first.output, "#!/bin/sh\nset -eu\necho hello\n");
        assert_eq!(first.edits.len(), 1);
        assert!(first.findings[0].auto_applicable);
        let second = modernize("build.sh", &first.output, &[Profile::Secure]);
        assert_eq!(second.output, first.output);
        assert!(second.edits.is_empty());
    }

    #[test]
    fn dangerous_changes_remain_review_only_findings() {
        let source = "curl https://example.invalid/install | sh\nchmod 777 out\nimage:latest\n";
        let result = modernize(
            "install.sh",
            source,
            &[Profile::Secure, Profile::Reproducible],
        );
        assert!(
            result.findings.iter().any(
                |finding| finding.rule == "secure.remote-code-pipe" && !finding.auto_applicable
            )
        );
        assert!(
            result
                .findings
                .iter()
                .any(|finding| finding.rule == "secure.world-writable" && !finding.auto_applicable)
        );
        assert!(
            result.findings.iter().any(
                |finding| finding.rule == "reproducible.latest-tag" && !finding.auto_applicable
            )
        );
        assert!(result.output.contains("curl https://"));
    }
}
