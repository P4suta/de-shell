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
    let protected = protected_ranges(source);
    let mut protected_index = 0;
    let mut state = QuoteState::Normal;
    let mut index = 0;
    while index < source.len() {
        while protected_index < protected.len() && protected[protected_index].1 <= index {
            protected_index += 1;
        }
        if let Some(&(start, end)) = protected.get(protected_index)
            && index >= start
            && index < end
        {
            output.push_str(&source[index..end]);
            index = end;
            state = QuoteState::Normal;
            continue;
        }
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

/// Returns byte ranges whose text is data rather than shell code. Conservatively
/// protecting every heredoc body also protects quoted heredocs without trying to
/// reinterpret expansion rules.
fn protected_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = comment_ranges(source);
    let lines = source_lines(source);
    let mut pending: std::collections::VecDeque<(String, bool)> = std::collections::VecDeque::new();
    for (start, end) in lines {
        let line = &source[start..end];
        if let Some((delimiter, strip_tabs)) = pending.front() {
            ranges.push((start, end));
            let candidate = line
                .trim_end_matches(['\n', '\r'])
                .strip_prefix(if *strip_tabs { "\t" } else { "" })
                .unwrap_or_else(|| line.trim_end_matches(['\n', '\r']));
            let candidate = if *strip_tabs {
                candidate.trim_start_matches('\t')
            } else {
                candidate
            };
            if candidate == delimiter {
                pending.pop_front();
            }
            continue;
        }
        for delimiter in heredoc_delimiters(line) {
            pending.push_back(delimiter);
        }
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn source_lines(source: &str) -> Vec<(usize, usize)> {
    let mut output = Vec::new();
    let mut start = 0;
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            output.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < source.len() {
        output.push((start, source.len()));
    }
    output
}

fn comment_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut output = Vec::new();
    let mut state = QuoteState::Normal;
    let mut escaped = false;
    let mut previous = None;
    for (index, character) in source.char_indices() {
        if escaped {
            escaped = false;
            previous = Some(character);
            continue;
        }
        match (state, character) {
            (QuoteState::Normal | QuoteState::Double, '\\') => escaped = true,
            (QuoteState::Normal, '\'') => state = QuoteState::Single,
            (QuoteState::Single, '\'') => state = QuoteState::Normal,
            (QuoteState::Normal, '"') => state = QuoteState::Double,
            (QuoteState::Double, '"') => state = QuoteState::Normal,
            (QuoteState::Normal, '#')
                if previous.is_none_or(|value: char| {
                    value.is_whitespace() || matches!(value, ';' | '&' | '|' | '(' | ')')
                }) =>
            {
                let end = source[index..]
                    .find('\n')
                    .map_or(source.len(), |offset| index + offset + 1);
                output.push((index, end));
            }
            _ => {}
        }
        if character == '\n' {
            state = QuoteState::Normal;
            previous = None;
        } else {
            previous = Some(character);
        }
    }
    output
}

fn heredoc_delimiters(line: &str) -> Vec<(String, bool)> {
    let bytes = line.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    let mut quote = None;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if byte == b'\\' && quote != Some(b'\'') {
            escaped = true;
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            index += 1;
            continue;
        }
        if quote.is_none() && byte == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace())
        {
            break;
        }
        if quote.is_none() && bytes[index..].starts_with(b"<<") {
            index += 2;
            let strip_tabs = bytes.get(index) == Some(&b'-');
            index += usize::from(strip_tabs);
            while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                index += 1;
            }
            let mut delimiter = String::new();
            let mut delimiter_quote = None;
            while let Some(&value) = bytes.get(index) {
                if delimiter_quote.is_none()
                    && (value.is_ascii_whitespace() || matches!(value, b';' | b'&' | b'|'))
                {
                    break;
                }
                if matches!(value, b'\'' | b'"') {
                    if delimiter_quote == Some(value) {
                        delimiter_quote = None;
                    } else if delimiter_quote.is_none() {
                        delimiter_quote = Some(value);
                    } else {
                        delimiter.push(value as char);
                    }
                } else if value == b'\\' && delimiter_quote != Some(b'\'') {
                    index += 1;
                    if let Some(&escaped) = bytes.get(index) {
                        delimiter.push(escaped as char);
                    }
                } else if value.is_ascii() {
                    delimiter.push(value as char);
                } else {
                    let character = line[index..].chars().next().expect("UTF-8 line");
                    delimiter.push(character);
                    index += character.len_utf8() - 1;
                }
                index += 1;
            }
            if !delimiter.is_empty() {
                output.push((delimiter, strip_tabs));
            }
            continue;
        }
        index += 1;
    }
    output
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

    #[test]
    fn comments_and_heredoc_data_are_never_rewritten() {
        let source = concat!(
            "echo `date` # leave `comment` alone\n",
            "cat <<'DATA'\n",
            "literal `not code`\n",
            "DATA\n",
            "cat <<-EOF\n",
            "\talso `not code`\n",
            "\tEOF\n"
        );
        let result = equivalent("build.sh", source);
        assert_eq!(result.output, source.replacen("`date`", "$(date)", 1));
        assert_eq!(result.edits.len(), 1);
    }
}
