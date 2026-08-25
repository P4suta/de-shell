use crate::config::UnknownInterpreter;
use crate::ir::{
    Binding, Guarantee, NamedExpression, Node, Operation, Plan, PrimitiveType, SourceBytes,
    SourceSpan, Task, TextExpression, TextPart, ValueType,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Interpreter {
    Sh,
    Bash,
    Zsh,
    Fish,
    Powershell,
    Cmd,
    Nushell,
    Unknown(String),
}

impl Interpreter {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
            Self::Cmd => "cmd",
            Self::Nushell => "nu",
            Self::Unknown(name) => name,
        }
    }
}

pub(crate) fn detect(path: &str, source: &[u8]) -> Interpreter {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".sh") {
        return Interpreter::Sh;
    }
    if lower.ends_with(".bash") {
        return Interpreter::Bash;
    }
    if lower.ends_with(".zsh") {
        return Interpreter::Zsh;
    }
    if lower.ends_with(".fish") {
        return Interpreter::Fish;
    }
    if lower.ends_with(".ps1") || lower.ends_with(".psm1") {
        return Interpreter::Powershell;
    }
    if lower.ends_with(".cmd") || lower.ends_with(".bat") {
        return Interpreter::Cmd;
    }
    if lower.ends_with(".nu") {
        return Interpreter::Nushell;
    }

    let first_line = source
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let Some(command) = first_line.strip_prefix(b"#!") else {
        return Interpreter::Unknown("unknown".into());
    };
    let command = String::from_utf8_lossy(command);
    let words: Vec<&str> = command.split_ascii_whitespace().collect();
    let executable = words.first().map(|word| basename(word));
    let program = if executable.as_deref() == Some("env") {
        words
            .iter()
            .skip(1)
            .find(|word| !word.starts_with('-'))
            .map(|word| basename(word))
    } else {
        executable
    };
    interpreter_from_name(program.as_deref().unwrap_or("unknown"))
}

pub(crate) fn lower(
    path: &str,
    source: &[u8],
    unknown_policy: UnknownInterpreter,
) -> Result<Plan, String> {
    let normalized = crate::ir::normalize_path(path)?;
    if normalized != path.replace('\\', "/") {
        return Err(format!("entry path is not normalized: {path}"));
    }
    let interpreter = detect(&normalized, source);
    if let Interpreter::Unknown(name) = &interpreter
        && unknown_policy == UnknownInterpreter::Reject
    {
        return Err(format!("unknown interpreter is rejected by policy: {name}"));
    }

    let lowered = match std::str::from_utf8(source) {
        Err(_) => Err("source is not valid UTF-8 and cannot be statically lowered".into()),
        Ok(text) => match interpreter {
            Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh => {
                lower_posix(&normalized, text, &interpreter)
            }
            Interpreter::Fish
            | Interpreter::Powershell
            | Interpreter::Cmd
            | Interpreter::Nushell => lower_literal_family(&normalized, text, &interpreter),
            Interpreter::Unknown(_) => Err(format!(
                "{} frontend is trace-only; unobserved behavior is not claimed as verified",
                interpreter.name()
            )),
        },
    };

    let (body, inputs, environment, invocation) = match lowered {
        Ok(lowered) => (lowered.body, lowered.inputs, lowered.environment, None),
        Err(reason) => (
            residual_node(&normalized, source, interpreter.name(), reason),
            BTreeSet::new(),
            BTreeSet::new(),
            None,
        ),
    };
    let environment: Vec<String> = environment.into_iter().collect();
    let secrets = environment
        .iter()
        .filter(|name| secret_name(name))
        .cloned()
        .collect();
    let mut plan = Plan {
        schema_version: 1,
        generator: "deshell/0.1.0".into(),
        entrypoint: "main".into(),
        tasks: vec![Task {
            name: "main".into(),
            inputs: inputs
                .into_iter()
                .map(|name| Binding {
                    name,
                    value_type: ValueType::Primitive(PrimitiveType::Text),
                })
                .collect(),
            outputs: vec![],
            environment,
            secrets,
            platform_capabilities: vec![],
            cacheable: false,
            invocation,
            body,
        }],
    };
    plan.assign_node_ids()?;
    plan.validate().map_err(|errors| errors.join("; "))?;
    Ok(plan)
}

#[derive(Default)]
struct Lowered {
    body: Node,
    inputs: BTreeSet<String>,
    environment: BTreeSet<String>,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: String::new(),
            operation: Operation::Exec {
                argv: vec![TextExpression::literal("true")],
                environment: vec![],
                working_directory: None,
            },
            guarantee: Guarantee::Formal {
                basis: "generated-v1".into(),
            },
            source: None,
        }
    }
}

fn interpreter_from_name(name: &str) -> Interpreter {
    match name.to_ascii_lowercase().trim_end_matches(".exe") {
        "sh" | "dash" | "ash" | "ksh" | "mksh" => Interpreter::Sh,
        "bash" => Interpreter::Bash,
        "zsh" => Interpreter::Zsh,
        "fish" => Interpreter::Fish,
        "powershell" | "pwsh" => Interpreter::Powershell,
        "cmd" => Interpreter::Cmd,
        "nu" | "nushell" => Interpreter::Nushell,
        other => Interpreter::Unknown(other.to_owned()),
    }
}

fn basename(value: &str) -> String {
    value
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

fn residual_node(path: &str, source: &[u8], interpreter: &str, reason: String) -> Node {
    let span = std::str::from_utf8(source)
        .ok()
        .and_then(|text| span_for_range(path, text, 0, text.len()).ok());
    Node {
        id: String::new(),
        operation: Operation::OpaqueCapsule {
            interpreter: interpreter.to_owned(),
            source: SourceBytes::from_bytes(source),
            path: Some(path.to_owned()),
        },
        guarantee: Guarantee::Residual { reason },
        source: span,
    }
}

fn formal_node(operation: Operation, basis: &str, span: SourceSpan) -> Node {
    Node {
        id: String::new(),
        operation,
        guarantee: Guarantee::Formal {
            basis: basis.to_owned(),
        },
        source: Some(span),
    }
}

#[derive(Clone, Copy)]
struct Range {
    start: usize,
    end: usize,
}

fn lower_posix(path: &str, source: &str, interpreter: &Interpreter) -> Result<Lowered, String> {
    let statements = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut locals = BTreeSet::new();
    let mut nodes = Vec::new();
    for range in statements {
        let text = &source[range.start..range.end];
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "set -e"
            || trimmed == "set -u"
            || trimmed == "set -eu"
            || trimmed == "set -ue"
        {
            continue;
        }
        let node = lower_posix_control(
            path,
            source,
            range,
            interpreter,
            &mut inputs,
            &mut environment,
            &mut locals,
        )?;
        nodes.push(node);
    }
    if nodes.is_empty() {
        return Err("script contains no statically lowerable operation".into());
    }
    let body = if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        let first = nodes
            .first()
            .and_then(|node| node.source.clone())
            .ok_or("sequence source span is missing")?;
        let last = nodes
            .last()
            .and_then(|node| node.source.clone())
            .ok_or("sequence source span is missing")?;
        formal_node(
            Operation::Sequence { nodes },
            &format!("{}-static-sequence-v1", interpreter.name()),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
    })
}

fn shell_statements(source: &str) -> Result<Vec<Range>, String> {
    let mut output = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if byte == b'\\' && quote != Some(b'\'') {
            escaped = true;
        } else if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'\n' || byte == b';' {
            let range = trim_range(source, start, index);
            if range.start < range.end {
                output.push(range);
            }
            start = index + 1;
        }
        index += 1;
    }
    if quote.is_some() {
        return Err("unterminated shell quote".into());
    }
    if escaped {
        return Err("trailing shell escape".into());
    }
    let range = trim_range(source, start, source.len());
    if range.start < range.end {
        output.push(range);
    }
    Ok(output)
}

fn trim_range(source: &str, mut start: usize, mut end: usize) -> Range {
    while start < end && source.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && source.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Range { start, end }
}

fn lower_posix_control(
    path: &str,
    source: &str,
    range: Range,
    interpreter: &Interpreter,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &mut BTreeSet<String>,
) -> Result<Node, String> {
    let controls = top_level_controls(source, range)?;
    if controls.is_empty() {
        return lower_posix_simple(
            path,
            source,
            range,
            interpreter,
            inputs,
            environment,
            locals,
        );
    }
    let kind = controls[0].1;
    if controls.iter().any(|(_, current)| *current != kind) {
        return Err("mixed shell control operators are outside the static subset".into());
    }
    let mut pieces = Vec::new();
    let mut cursor = range.start;
    for (position, operator) in &controls {
        let piece = trim_range(source, cursor, *position);
        if piece.start == piece.end {
            return Err("shell control operator has an empty operand".into());
        }
        pieces.push(piece);
        cursor = position + operator.len();
    }
    let piece = trim_range(source, cursor, range.end);
    if piece.start == piece.end {
        return Err("shell control operator has an empty operand".into());
    }
    pieces.push(piece);

    let mut nodes = Vec::new();
    for piece in pieces {
        nodes.push(lower_posix_simple(
            path,
            source,
            piece,
            interpreter,
            inputs,
            environment,
            locals,
        )?);
    }
    let span = span_for_range(path, source, range.start, range.end)?;
    match kind {
        "|" => Ok(formal_node(
            Operation::Pipeline { nodes },
            "posix-static-pipeline-v1",
            span,
        )),
        "&&" => {
            let mut iterator = nodes.into_iter();
            let mut result = iterator.next().expect("pieces are non-empty");
            for next in iterator {
                result = formal_node(
                    Operation::Condition {
                        predicate: Box::new(result),
                        if_true: Box::new(next),
                        if_false: None,
                    },
                    "posix-and-if-v1",
                    span.clone(),
                );
            }
            Ok(result)
        }
        "||" => Err("POSIX || requires an explicit no-op branch and remains residual".into()),
        _ => Err("unsupported shell control operator".into()),
    }
}

fn top_level_controls(source: &str, range: Range) -> Result<Vec<(usize, &'static str)>, String> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    let mut output = Vec::new();
    let mut index = range.start;
    while index < range.end {
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
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'|' {
            if index + 1 < range.end && bytes[index + 1] == b'|' {
                output.push((index, "||"));
                index += 2;
            } else {
                output.push((index, "|"));
                index += 1;
            }
            continue;
        }
        if byte == b'&' && index + 1 < range.end && bytes[index + 1] == b'&' {
            output.push((index, "&&"));
            index += 2;
            continue;
        }
        if matches!(byte, b'<' | b'>' | b'&') {
            return Err("redirection or background execution remains residual".into());
        }
        index += 1;
    }
    Ok(output)
}

fn lower_posix_simple(
    path: &str,
    source: &str,
    range: Range,
    interpreter: &Interpreter,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &mut BTreeSet<String>,
) -> Result<Node, String> {
    let raw = source[range.start..range.end].trim();
    for reserved in [
        "if ",
        "then",
        "fi",
        "for ",
        "while ",
        "until ",
        "case ",
        "function ",
        "{",
        "}",
    ] {
        if raw == reserved.trim() || raw.starts_with(reserved) {
            return Err("shell compound syntax remains residual".into());
        }
    }
    if raw.starts_with("eval ")
        || raw == "eval"
        || raw.starts_with("source ")
        || raw.starts_with(". ")
    {
        return Err("dynamic shell evaluation remains residual".into());
    }

    if let Some((name, rhs)) = standalone_assignment(raw) {
        if locals.contains(name) {
            return Err(format!("mutable shell assignment remains residual: {name}"));
        }
        let operation = if rhs.starts_with("$(") && rhs.ends_with(')') {
            let inner_start = range.start + raw.find("$(").unwrap() + 2;
            let inner_end = range.end - 1;
            let body = lower_posix_simple(
                path,
                source,
                trim_range(source, inner_start, inner_end),
                interpreter,
                inputs,
                environment,
                locals,
            )?;
            Operation::CaptureStdout {
                name: name.to_owned(),
                value_type: PrimitiveType::Text,
                body: Box::new(body),
            }
        } else {
            let expression = parse_posix_word(rhs, true, inputs, environment, locals)?;
            Operation::SetVariable {
                name: name.to_owned(),
                value_type: infer_value_type(&expression),
                value: expression,
            }
        };
        locals.insert(name.to_owned());
        return Ok(formal_node(
            operation,
            "posix-immutable-assignment-v1",
            span_for_range(path, source, range.start, range.end)?,
        ));
    }

    let words = tokenize_posix(&source[range.start..range.end], inputs, environment, locals)?;
    if words.is_empty() {
        return Err("empty shell command".into());
    }
    let executable = literal_expression(&words[0]).ok_or("dynamic executable remains residual")?;
    if [
        "cd", "export", "unset", "read", "shift", "trap", "return", "exit", "exec", "set",
    ]
    .contains(&executable.as_str())
    {
        return Err(format!("shell builtin {executable} remains residual"));
    }
    let mut command_environment = Vec::new();
    let mut argv_start = 0;
    while argv_start < words.len() {
        let Some(literal) = literal_expression(&words[argv_start]) else {
            break;
        };
        let Some((name, value)) = literal.split_once('=') else {
            break;
        };
        if !valid_identifier(name) {
            break;
        }
        command_environment.push(NamedExpression {
            name: name.into(),
            value: TextExpression::literal(value),
        });
        argv_start += 1;
    }
    if argv_start == words.len() {
        return Err("command-local environment is missing an executable".into());
    }
    Ok(formal_node(
        Operation::Exec {
            argv: words[argv_start..].to_vec(),
            environment: command_environment,
            working_directory: None,
        },
        &format!("{}-explicit-command-v1", interpreter.name()),
        span_for_range(path, source, range.start, range.end)?,
    ))
}

fn standalone_assignment(value: &str) -> Option<(&str, &str)> {
    let separator = value.find('=')?;
    let name = &value[..separator];
    if !valid_identifier(name) || name.contains(char::is_whitespace) {
        return None;
    }
    let rhs = &value[separator + 1..];
    if rhs.is_empty()
        || rhs.bytes().any(|byte| byte.is_ascii_whitespace())
            && !(rhs.starts_with('"') && rhs.ends_with('"'))
    {
        return None;
    }
    Some((name, rhs))
}

fn infer_value_type(expression: &TextExpression) -> ValueType {
    let Some(value) = literal_expression(expression) else {
        return ValueType::Primitive(PrimitiveType::Text);
    };
    if value == "true" || value == "false" {
        ValueType::Primitive(PrimitiveType::Bool)
    } else if value.parse::<i64>().is_ok() && value.parse::<i64>().unwrap().to_string() == value {
        ValueType::Primitive(PrimitiveType::Int)
    } else {
        ValueType::Primitive(PrimitiveType::Text)
    }
}

fn tokenize_posix(
    source: &str,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &BTreeSet<String>,
) -> Result<Vec<TextExpression>, String> {
    let mut words = Vec::new();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut quote = None;
    let mut token_started = false;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(b'\'') => {
                if byte == b'\'' {
                    quote = None;
                } else {
                    let character = source[index..]
                        .chars()
                        .next()
                        .expect("valid UTF-8 boundary");
                    literal.push(character);
                    index += character.len_utf8();
                    token_started = true;
                    continue;
                }
                token_started = true;
                index += 1;
            }
            Some(b'"') => {
                if byte == b'"' {
                    quote = None;
                    index += 1;
                    continue;
                }
                if byte == b'$' {
                    flush_literal(&mut parts, &mut literal);
                    let (part, next) = parse_expansion(source, index, inputs, environment, locals)?;
                    parts.push(part);
                    index = next;
                    token_started = true;
                    continue;
                }
                if byte == b'`' {
                    return Err("command substitution remains residual".into());
                }
                if byte == b'\\' {
                    let next = *bytes
                        .get(index + 1)
                        .ok_or("trailing escape in double quote")?;
                    if matches!(next, b'$' | b'`' | b'"' | b'\\') {
                        literal.push(next as char);
                        index += 2;
                        token_started = true;
                        continue;
                    }
                    literal.push('\\');
                    index += 1;
                    token_started = true;
                    continue;
                }
                let character = source[index..].chars().next().unwrap();
                literal.push(character);
                index += character.len_utf8();
                token_started = true;
            }
            _ => {
                if byte.is_ascii_whitespace() {
                    finish_word(&mut words, &mut parts, &mut literal, token_started);
                    token_started = false;
                    index += 1;
                    continue;
                }
                if byte == b'#' && !token_started {
                    break;
                }
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                    token_started = true;
                    index += 1;
                    continue;
                }
                if byte == b'\\' {
                    let escaped = source
                        .get(index + 1..)
                        .ok_or("trailing shell escape")?
                        .chars()
                        .next()
                        .ok_or("trailing shell escape")?;
                    literal.push(escaped);
                    token_started = true;
                    index += 1 + escaped.len_utf8();
                    continue;
                }
                if byte == b'$' {
                    return Err("unquoted expansion may split fields and remains residual".into());
                }
                if matches!(
                    byte,
                    b'`' | b'*'
                        | b'?'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'<'
                        | b'>'
                        | b'|'
                        | b'&'
                        | b';'
                ) {
                    return Err("dynamic expansion or control syntax remains residual".into());
                }
                let character = source[index..].chars().next().unwrap();
                literal.push(character);
                index += character.len_utf8();
                token_started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated shell quote".into());
    }
    finish_word(&mut words, &mut parts, &mut literal, token_started);
    Ok(words)
}

fn parse_posix_word(
    source: &str,
    allow_unquoted_expansion: bool,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &BTreeSet<String>,
) -> Result<TextExpression, String> {
    if allow_unquoted_expansion && source.starts_with('$') && !source.starts_with("$(") {
        let (part, end) = parse_expansion(source, 0, inputs, environment, locals)?;
        if end == source.len() {
            return Ok(TextExpression { parts: vec![part] });
        }
    }
    let words = tokenize_posix(source, inputs, environment, locals)?;
    if words.len() != 1 {
        return Err("assignment value is not one static word".into());
    }
    Ok(words.into_iter().next().unwrap())
}

fn parse_expansion(
    source: &str,
    start: usize,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &BTreeSet<String>,
) -> Result<(TextPart, usize), String> {
    let bytes = source.as_bytes();
    if bytes.get(start + 1) == Some(&b'(') {
        return Err("command substitution remains residual".into());
    }
    let (name, end) = if bytes.get(start + 1) == Some(&b'{') {
        let relative = source[start + 2..]
            .find('}')
            .ok_or("unterminated braced expansion")?;
        let end = start + 2 + relative;
        (&source[start + 2..end], end + 1)
    } else {
        let mut end = start + 1;
        if bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        } else {
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                end += 1;
            }
        }
        (&source[start + 1..end], end)
    };
    if name.is_empty() {
        return Err("unsupported shell special parameter".into());
    }
    if name.bytes().all(|byte| byte.is_ascii_digit()) {
        inputs.insert(name.to_owned());
        Ok((
            TextPart::Argument {
                name: name.to_owned(),
            },
            end,
        ))
    } else if valid_identifier(name) {
        if !locals.contains(name) {
            environment.insert(name.to_owned());
        }
        Ok((
            TextPart::Variable {
                name: name.to_owned(),
            },
            end,
        ))
    } else {
        Err(format!(
            "parameter expansion syntax remains residual: {name}"
        ))
    }
}

fn flush_literal(parts: &mut Vec<TextPart>, literal: &mut String) {
    if !literal.is_empty() {
        parts.push(TextPart::Literal {
            value: std::mem::take(literal),
        });
    }
}

fn finish_word(
    words: &mut Vec<TextExpression>,
    parts: &mut Vec<TextPart>,
    literal: &mut String,
    started: bool,
) {
    if !started {
        return;
    }
    flush_literal(parts, literal);
    if parts.is_empty() {
        parts.push(TextPart::Literal {
            value: String::new(),
        });
    }
    words.push(TextExpression {
        parts: std::mem::take(parts),
    });
}

fn literal_expression(expression: &TextExpression) -> Option<String> {
    match expression.parts.as_slice() {
        [TextPart::Literal { value }] => Some(value.clone()),
        _ => None,
    }
}

fn valid_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn lower_literal_family(
    path: &str,
    source: &str,
    interpreter: &Interpreter,
) -> Result<Lowered, String> {
    let ranges = shell_statements(source)?;
    let mut nodes = Vec::new();
    let mut cmd_echo_off = false;
    for range in ranges {
        let raw = source[range.start..range.end].trim();
        if raw.is_empty() || (raw.starts_with('#') && !matches!(interpreter, Interpreter::Cmd)) {
            continue;
        }
        let command = match interpreter {
            Interpreter::Fish => {
                let words = literal_words(raw, LiteralFamily::Fish)?;
                if words.first().map(String::as_str) != Some("command") || words.len() < 2 {
                    return Err("fish command is not an explicit external invocation".into());
                }
                words[1..].to_vec()
            }
            Interpreter::Powershell => {
                let words = literal_words(raw, LiteralFamily::Powershell)?;
                if words.first().map(String::as_str) != Some("&") || words.len() < 2 {
                    return Err(
                        "PowerShell command is not an explicit call-operator invocation".into(),
                    );
                }
                words[1..].to_vec()
            }
            Interpreter::Cmd => {
                let lower = raw.to_ascii_lowercase();
                if lower == "@echo off" || lower == "echo off" {
                    cmd_echo_off = true;
                    continue;
                }
                let locally_suppressed = raw.starts_with('@');
                if !cmd_echo_off && !locally_suppressed {
                    return Err("cmd command echo must be suppressed".into());
                }
                let command = raw.strip_prefix('@').unwrap_or(raw);
                let words = literal_words(command, LiteralFamily::Cmd)?;
                let executable = words.first().ok_or("empty cmd command")?;
                let executable_lower = basename(executable);
                if !executable_lower.ends_with(".exe") && !executable_lower.ends_with(".com") {
                    return Err("cmd command requires an explicit .exe or .com executable".into());
                }
                words
            }
            Interpreter::Nushell => {
                let words = literal_words(raw, LiteralFamily::Nushell)?;
                let first = words.first().ok_or("empty Nushell command")?;
                let Some(executable) = first.strip_prefix('^') else {
                    return Err("Nushell command is not an explicit external invocation".into());
                };
                if executable.is_empty() {
                    return Err("Nushell external executable is empty".into());
                }
                let executable = executable.to_owned();
                let mut command = words;
                command[0] = executable;
                command
            }
            _ => return Err("not a literal frontend family".into()),
        };
        let argv = command.into_iter().map(TextExpression::literal).collect();
        nodes.push(formal_node(
            Operation::Exec {
                argv,
                environment: vec![],
                working_directory: None,
            },
            &format!("{}-static-external-command-v1", interpreter.name()),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    if nodes.is_empty() {
        return Err("script contains no static external invocation".into());
    }
    if matches!(interpreter, Interpreter::Nushell) && nodes.len() > 1 {
        return Err("multiple Nushell statements require a pinned runtime status contract".into());
    }
    let body = if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        let first = nodes.first().unwrap().source.clone().unwrap();
        let last = nodes.last().unwrap().source.clone().unwrap();
        formal_node(
            Operation::Sequence { nodes },
            &format!("{}-static-sequence-v1", interpreter.name()),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs: BTreeSet::new(),
        environment: BTreeSet::new(),
    })
}

#[derive(Clone, Copy)]
enum LiteralFamily {
    Fish,
    Powershell,
    Cmd,
    Nushell,
}

fn literal_words(source: &str, family: LiteralFamily) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
                index += 1;
                continue;
            }
            if byte == b'$' && !(matches!(family, LiteralFamily::Powershell) && delimiter == b'\'')
            {
                return Err("dynamic expansion is outside the literal subset".into());
            }
            if byte == b'`' {
                return Err("escape or substitution syntax is outside the literal subset".into());
            }
            let character = source[index..].chars().next().unwrap();
            word.push(character);
            index += character.len_utf8();
            started = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
            index += 1;
            continue;
        }
        let supports_single = !matches!(family, LiteralFamily::Cmd);
        if byte == b'"' || (byte == b'\'' && supports_single) {
            quote = Some(byte);
            started = true;
            index += 1;
            continue;
        }
        let allowed_prefix = (matches!(family, LiteralFamily::Powershell)
            && byte == b'&'
            && words.is_empty()
            && !started)
            || (matches!(family, LiteralFamily::Nushell)
                && byte == b'^'
                && words.is_empty()
                && !started);
        if !allowed_prefix
            && matches!(
                byte,
                b'$' | b'`'
                    | b'*'
                    | b'?'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'%'
                    | b'!'
                    | b'|'
                    | b';'
                    | b'<'
                    | b'>'
            )
        {
            return Err("dynamic or control syntax is outside the literal subset".into());
        }
        if matches!(family, LiteralFamily::Cmd) && byte == b'^' {
            return Err("cmd escape syntax is outside the literal subset".into());
        }
        let character = source[index..].chars().next().unwrap();
        word.push(character);
        index += character.len_utf8();
        started = true;
    }
    if quote.is_some() {
        return Err("unterminated quoted argument".into());
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

fn span_for_range(
    path: &str,
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> Result<SourceSpan, String> {
    if start_byte > end_byte
        || end_byte > source.len()
        || !source.is_char_boundary(start_byte)
        || !source.is_char_boundary(end_byte)
    {
        return Err("source span byte boundary is invalid".into());
    }
    let (start_line, start_column) = line_column(source, start_byte);
    let (end_line, end_column) = line_column(source, end_byte);
    Ok(SourceSpan {
        file: path.to_owned(),
        start_line,
        start_column,
        end_line,
        end_column,
        start_byte: start_byte as u64,
        end_byte: end_byte as u64,
    })
}

fn line_column(source: &str, byte: usize) -> (u64, u64) {
    let mut line = 1_u64;
    let mut column = 0_u64;
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

fn cover_spans(first: SourceSpan, last: SourceSpan) -> SourceSpan {
    SourceSpan {
        file: first.file,
        start_line: first.start_line,
        start_column: first.start_column,
        start_byte: first.start_byte,
        end_line: last.end_line,
        end_column: last.end_column,
        end_byte: last.end_byte,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Guarantee, Operation, SourceBytes, TextPart};

    fn body(path: &str, source: &[u8]) -> crate::ir::Node {
        lower(path, source, UnknownInterpreter::TraceOnly)
            .unwrap()
            .tasks
            .remove(0)
            .body
    }

    #[test]
    fn detects_extension_and_portable_shebangs() {
        assert_eq!(detect("build.ps1", b""), Interpreter::Powershell);
        assert_eq!(
            detect("build", b"#!/usr/bin/env -S bash -eu\necho ok\n"),
            Interpreter::Bash
        );
        assert_eq!(detect("build", b"#!/bin/zsh\necho ok\n"), Interpreter::Zsh);
        assert!(matches!(
            detect("build.custom", b"data"),
            Interpreter::Unknown(_)
        ));
    }

    #[test]
    fn posix_quoted_expansions_become_explicit_parts() {
        let plan = lower(
            "scripts/build.sh",
            b"#!/bin/sh\nprintf '%s\\n' \"$NAME:$1\" '$NAME'\n",
            UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(task.environment, ["NAME"]);
        assert_eq!(task.inputs[0].name, "1");
        let Operation::Exec { argv, .. } = &task.body.operation else {
            panic!("expected exec")
        };
        assert_eq!(
            argv[2].parts,
            [
                TextPart::Variable {
                    name: "NAME".into()
                },
                TextPart::Literal { value: ":".into() },
                TextPart::Argument { name: "1".into() },
            ]
        );
        assert_eq!(
            argv[3].parts,
            [TextPart::Literal {
                value: "$NAME".into()
            }]
        );
        plan.validate().unwrap();
    }

    #[test]
    fn posix_pipeline_and_sequence_keep_control_flow() {
        let node = body("build.sh", b"printf one | grep one\nprintf two\n");
        let Operation::Sequence { nodes } = node.operation else {
            panic!("expected sequence")
        };
        assert!(matches!(nodes[0].operation, Operation::Pipeline { .. }));
        assert!(matches!(nodes[1].operation, Operation::Exec { .. }));
    }

    #[test]
    fn unsupported_and_non_utf8_sources_are_lossless_residuals() {
        let source = b"eval \"$DYNAMIC\"\n";
        let node = body("build.sh", source);
        assert!(matches!(node.guarantee, Guarantee::Residual { .. }));
        let Operation::OpaqueCapsule {
            source: capsule, ..
        } = node.operation
        else {
            panic!("expected capsule")
        };
        assert_eq!(capsule.to_bytes().unwrap(), source);

        let bytes = b"printf '\xff'\n";
        let node = body("bad.sh", bytes);
        let Operation::OpaqueCapsule { source, .. } = node.operation else {
            panic!("expected capsule")
        };
        assert!(matches!(source, SourceBytes::Base64 { .. }));
        assert_eq!(source.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn literal_subsets_cover_all_declared_interpreters() {
        let fixtures: &[(&str, &[u8], &str)] = &[
            ("build.zsh", b"printf zsh\n", "printf"),
            ("build.fish", b"command printf fish\n", "printf"),
            ("build.ps1", b"& '/bin/echo' 'powershell'\n", "/bin/echo"),
            (
                "build.cmd",
                b"@echo off\n@cmd.exe /d /s /c echo cmd\n",
                "cmd.exe",
            ),
            ("build.nu", b"^git status\n", "git"),
        ];
        for (path, source, expected) in fixtures {
            let node = body(path, source);
            let Operation::Exec { argv, .. } = node.operation else {
                panic!("{path} did not lower: {node:?}")
            };
            assert_eq!(
                argv[0].parts,
                [TextPart::Literal {
                    value: (*expected).into()
                }],
                "{path}"
            );
        }
    }

    #[test]
    fn unknown_interpreter_policy_is_enforced() {
        let traced = lower(
            "build.custom",
            b"do something\n",
            UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        assert!(matches!(
            traced.tasks[0].body.operation,
            Operation::OpaqueCapsule { .. }
        ));
        let error = lower(
            "build.custom",
            b"do something\n",
            UnknownInterpreter::Reject,
        )
        .unwrap_err();
        assert!(error.contains("unknown interpreter"), "{error}");
    }

    #[test]
    fn source_columns_count_unicode_scalars_while_bytes_remain_half_open() {
        let node = body("unicode.sh", "printf 'é'\n".as_bytes());
        let span = node.source.unwrap();
        assert_eq!(span.start_line, 1);
        assert_eq!(span.start_column, 0);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.end_column, 10);
        assert_eq!(span.start_byte, 0);
        assert_eq!(span.end_byte, 11);
    }

    #[test]
    fn posix_single_quoted_unicode_is_preserved_as_utf8_text() {
        let plan = lower(
            "unicode.sh",
            "printf '%s' '日本語'\n".as_bytes(),
            UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        let Operation::Exec { argv, .. } = &plan.tasks[0].body.operation else {
            panic!("expected exec")
        };
        assert_eq!(
            argv[2].parts,
            [TextPart::Literal {
                value: "日本語".into()
            }]
        );
    }

    #[test]
    fn posix_escaped_unicode_starts_on_a_scalar_boundary() {
        let plan = lower(
            "unicode.sh",
            "printf \\日本語\n".as_bytes(),
            UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        let Operation::Exec { argv, .. } = &plan.tasks[0].body.operation else {
            panic!("expected exec")
        };
        assert_eq!(
            argv[1].parts,
            [TextPart::Literal {
                value: "日本語".into()
            }]
        );
    }
}
