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
    resolve_interpreter(path, source).unwrap_or_else(Interpreter::Unknown)
}

pub(crate) fn resolve_interpreter(path: &str, source: &[u8]) -> Result<Interpreter, String> {
    let extension = interpreter_from_extension(path);
    let shell_hint = path.to_ascii_lowercase().ends_with(".sh");
    let Some(program) = shebang_program(source) else {
        return Ok(extension.unwrap_or_else(|| Interpreter::Unknown("unknown".into())));
    };
    let shebang = interpreter_from_name(&program);
    if let Interpreter::Unknown(name) = &shebang {
        return Err(format!(
            "DESHELL_BLOCKER_UNKNOWN_INTERPRETER: unknown shebang interpreter {name} in {path}"
        ));
    }
    if let Some(extension) = extension
        && !shell_hint
        && extension != shebang
    {
        return Err(format!(
            "DESHELL_BLOCKER_INTERPRETER_CONFLICT: extension selects {} but shebang selects {} in {path}",
            extension.name(),
            shebang.name()
        ));
    }
    if shell_hint
        && !matches!(
            shebang,
            Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh
        )
    {
        return Err(format!(
            "DESHELL_BLOCKER_INTERPRETER_CONFLICT: .sh family hint conflicts with {} shebang in {path}",
            shebang.name()
        ));
    }
    Ok(shebang)
}

pub(crate) fn resolve_scanned_interpreter(
    path: &str,
    source: &[u8],
) -> Result<Option<Interpreter>, String> {
    if interpreter_from_extension(path).is_some() {
        return resolve_interpreter(path, source).map(Some);
    }
    let first_line = source
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    if first_line.starts_with(b"#![") {
        return Ok(None);
    }
    let Some(program) = shebang_program(source) else {
        return Ok(None);
    };
    if !matches!(interpreter_from_name(&program), Interpreter::Unknown(_)) {
        return resolve_interpreter(path, source).map(Some);
    }
    if is_recognized_non_shell_interpreter(&program) {
        return Ok(None);
    }
    resolve_interpreter(path, source).map(Some)
}

pub(crate) fn resolve_configured_interpreter(
    path: &str,
    source: &[u8],
    configured: &str,
) -> Result<Interpreter, String> {
    let configured = interpreter_from_name(configured);
    if let Interpreter::Unknown(name) = &configured {
        return Err(format!(
            "DESHELL_BLOCKER_UNKNOWN_INTERPRETER: unknown configured interpreter {name} in {path}"
        ));
    }
    let detected = resolve_interpreter(path, source)?;
    if matches!(detected, Interpreter::Unknown(_)) || detected == configured {
        return Ok(configured);
    }
    let has_shebang = source
        .split(|byte| *byte == b'\n')
        .next()
        .is_some_and(|line| line.starts_with(b"#!"));
    let sh_family_hint = path.to_ascii_lowercase().ends_with(".sh")
        && matches!(detected, Interpreter::Sh)
        && matches!(
            configured,
            Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh
        );
    if !has_shebang && sh_family_hint {
        return Ok(configured);
    }
    Err(format!(
        "DESHELL_BLOCKER_INTERPRETER_CONFLICT: project configuration selects {} but source selects {} in {path}",
        configured.name(),
        detected.name()
    ))
}

fn interpreter_from_extension(path: &str) -> Option<Interpreter> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".sh") {
        Some(Interpreter::Sh)
    } else if lower.ends_with(".bash") {
        Some(Interpreter::Bash)
    } else if lower.ends_with(".zsh") {
        Some(Interpreter::Zsh)
    } else if lower.ends_with(".fish") {
        Some(Interpreter::Fish)
    } else if lower.ends_with(".ps1") || lower.ends_with(".psm1") {
        Some(Interpreter::Powershell)
    } else if lower.ends_with(".cmd") || lower.ends_with(".bat") {
        Some(Interpreter::Cmd)
    } else if lower.ends_with(".nu") {
        Some(Interpreter::Nushell)
    } else {
        None
    }
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
    let interpreter = resolve_interpreter(&normalized, source)?;
    if let Interpreter::Unknown(name) = &interpreter
        && unknown_policy == UnknownInterpreter::Reject
    {
        return Err(format!("unknown interpreter is rejected by policy: {name}"));
    }

    let lowered = match std::str::from_utf8(source) {
        Err(_) => Err("source is not valid UTF-8 and cannot be statically lowered".into()),
        Ok(text) => match interpreter {
            Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh => validate_tree_sitter_cst(
                &normalized,
                text,
                tree_sitter_bash::LANGUAGE.into(),
                "tree-sitter-bash/0.25.1",
            )
            .and_then(|()| lower_posix(&normalized, text, &interpreter)),
            Interpreter::Fish => {
                validate_fish_cst(&normalized, text).and_then(|()| lower_fish(&normalized, text))
            }
            Interpreter::Cmd => {
                validate_cmd_cst(&normalized, text).and_then(|()| lower_cmd(&normalized, text))
            }
            Interpreter::Powershell => validate_powershell_syntax(&normalized, text)
                .and_then(|()| lower_powershell(&normalized, text)),
            Interpreter::Nushell => validate_nushell_syntax(&normalized, text)
                .and_then(|()| lower_nushell(&normalized, text, &interpreter)),
            Interpreter::Unknown(_) => Err(format!(
                "{} frontend is trace-only; unobserved behavior is not claimed as verified",
                interpreter.name()
            )),
        },
    };

    let (body, inputs, environment, invocation, platform_capabilities, nounset) = match lowered {
        Ok(lowered) => (
            lowered.body,
            lowered.inputs,
            lowered.environment,
            None,
            Vec::new(),
            lowered.nounset,
        ),
        Err(reason) => {
            let analysis = conservative_source_analysis(source, &interpreter, &reason);
            let body = if matches!(interpreter, Interpreter::Unknown(_)) {
                residual_node(&normalized, source, interpreter.name(), reason)
            } else {
                delegated_node(DelegatedNodeArgs {
                        path: &normalized,
                        source,
                        interpreter: interpreter.name(),
                        reason,
                        capabilities: analysis.capabilities.clone(),
                    })
            };
            (
                body,
                analysis.inputs,
                analysis.environment,
                None,
                Vec::new(),
                // A delegated body runs under its own interpreter, which reads
                // the option from the source it was handed.
                false,
            )
        }
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
            platform_capabilities,
            cacheable: false,
            nounset,
            invocation,
            body,
        }],
    };
    plan.assign_node_ids()?;
    plan.validate().map_err(|errors| errors.join("; "))?;
    Ok(plan)
}

pub(crate) fn lower_with_interpreter(
    path: &str,
    source: &[u8],
    unknown_policy: UnknownInterpreter,
    configured: &str,
) -> Result<Plan, String> {
    let interpreter = resolve_configured_interpreter(path, source, configured)?;
    let extension = match interpreter {
        Interpreter::Sh => "sh",
        Interpreter::Bash => "bash",
        Interpreter::Zsh => "zsh",
        Interpreter::Fish => "fish",
        Interpreter::Powershell => "ps1",
        Interpreter::Cmd => "cmd",
        Interpreter::Nushell => "nu",
        Interpreter::Unknown(name) => {
            return Err(format!("unknown configured interpreter: {name}"));
        }
    };
    let virtual_path = format!("{path}.deshell.{extension}");
    let mut plan = lower(&virtual_path, source, unknown_policy)?;
    rebind_source_path(&mut plan, &virtual_path, path)?;
    Ok(plan)
}

fn rebind_source_path(plan: &mut Plan, from: &str, to: &str) -> Result<(), String> {
    fn visit(node: &mut Node, from: &str, to: &str) {
        if let Some(span) = &mut node.source
            && span.file == from
        {
            span.file = to.into();
        }
        match &mut node.guarantee {
            Guarantee::Delegated { reason } | Guarantee::Residual { reason } => {
                *reason = reason.replace(from, to);
            }
            Guarantee::Native { .. } => {}
        }
        if let Operation::InterpreterCall {
            source_span,
            reason,
            ..
        } = &mut node.operation
        {
            if source_span.file == from {
                source_span.file = to.into();
            }
            *reason = reason.replace(from, to);
        }
        if let Operation::OpaqueCapsule {
            path: Some(path), ..
        } = &mut node.operation
            && path == from
        {
            *path = to.into();
        }
        match &mut node.operation {
            Operation::While { condition, body } => {
                visit(condition, from, to);
                visit(body, from, to);
            }
            Operation::Not { body } => visit(body, from, to),
            Operation::Pipeline { nodes, .. }
            | Operation::Sequence { nodes, .. }
            | Operation::Parallel { nodes } => {
                for child in nodes {
                    visit(child, from, to);
                }
            }
            Operation::Condition {
                predicate,
                if_true,
                if_false,
            } => {
                visit(predicate, from, to);
                visit(if_true, from, to);
                if let Some(child) = if_false {
                    visit(child, from, to);
                }
            }
            Operation::Match { cases, default, .. } => {
                for case in cases {
                    visit(&mut case.body, from, to);
                }
                if let Some(child) = default {
                    visit(child, from, to);
                }
            }
            Operation::Foreach { body, .. }
            | Operation::Scope { body, .. }
            | Operation::Redirect { body, .. }
            | Operation::CaptureStdout { body, .. }
            | Operation::Spawn { body, .. } => visit(body, from, to),
            Operation::TryFinally { body, finalizer } => {
                visit(body, from, to);
                visit(finalizer, from, to);
            }
            Operation::Exec { .. }
            | Operation::ExpandWords { .. }
            | Operation::TaskCall { .. }
            | Operation::SetVariable { .. }
            | Operation::SetEnvironment { .. }
            | Operation::SetWorkingDirectory { .. }
            | Operation::Wait { .. }
            | Operation::SendSignal { .. }
            | Operation::Test { .. }
        | Operation::FileRead { .. }
            | Operation::FileWrite { .. }
            | Operation::FileRemove { .. }
            | Operation::FileMetadata { .. }
            | Operation::FileSetMetadata { .. }
            | Operation::NetworkRequest { .. }
            | Operation::ClockRead { .. }
            | Operation::RandomBytes { .. }
            | Operation::InterpreterCall { .. }
            | Operation::OpaqueCapsule { .. } => {}
        }
    }
    for task in &mut plan.tasks {
        visit(&mut task.body, from, to);
    }
    plan.assign_node_ids()?;
    plan.validate().map_err(|errors| errors.join("; "))
}

#[derive(Default)]
struct Lowered {
    body: Node,
    inputs: BTreeSet<String>,
    environment: BTreeSet<String>,
    /// Whether `set -u` was in effect. Only the POSIX family reads it; the other
    /// frontends report `false` because they do not model the option.
    nounset: bool,
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
            guarantee: Guarantee::Native {
                semantic_model: "generated-v1".into(),
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

fn shebang_program(source: &[u8]) -> Option<String> {
    let first_line = source.split(|byte| *byte == b'\n').next()?;
    let command = first_line.strip_prefix(b"#!")?;
    let command = String::from_utf8_lossy(command);
    let words: Vec<&str> = command.split_ascii_whitespace().collect();
    let executable = words.first().map(|word| basename(word));
    if executable.as_deref() == Some("env") {
        words
            .iter()
            .skip(1)
            .find(|word| !word.starts_with('-'))
            .map(|word| basename(word))
    } else {
        executable
    }
}

fn is_recognized_non_shell_interpreter(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if name == "python"
        || name.strip_prefix("python").is_some_and(|version| {
            !version.is_empty()
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
        })
    {
        return true;
    }
    matches!(
        name.trim_end_matches(".exe"),
        "awk"
            | "bun"
            | "cargo"
            | "deno"
            | "elixir"
            | "escript"
            | "groovy"
            | "julia"
            | "lua"
            | "luajit"
            | "node"
            | "osascript"
            | "perl"
            | "php"
            | "racket"
            | "ruby"
            | "rust-script"
            | "swift"
            | "tclsh"
            | "wish"
    )
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

#[derive(Default)]
struct SourceAnalysis {
    inputs: BTreeSet<String>,
    environment: BTreeSet<String>,
    capabilities: Vec<String>,
}

fn conservative_source_analysis(
    source: &[u8],
    interpreter: &Interpreter,
    reason: &str,
) -> SourceAnalysis {
    let mut analysis = SourceAnalysis {
        capabilities: vec![
            "process".into(),
            "project_read".into(),
            "sandbox_write".into(),
        ],
        ..SourceAnalysis::default()
    };
    let Ok(text) = std::str::from_utf8(source) else {
        return analysis;
    };
    if matches!(
        interpreter,
        Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh | Interpreter::Fish
    ) {
        let mut index = 0;
        let mut quote = None;
        let mut escaped = false;
        let bytes = text.as_bytes();
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
            if quote == Some(b'\'') {
                if byte == b'\'' {
                    quote = None;
                }
                index += 1;
                continue;
            }
            if byte == b'\'' {
                quote = Some(b'\'');
                index += 1;
                continue;
            }
            if byte == b'"' {
                quote = if quote == Some(b'"') {
                    None
                } else {
                    Some(b'"')
                };
                index += 1;
                continue;
            }
            if byte == b'#' && quote.is_none() {
                index = text[index..]
                    .find('\n')
                    .map_or(bytes.len(), |newline| index + newline + 1);
                continue;
            }
            if byte == b'$' {
                let locals = BTreeSet::new();
                if let Ok((_, end)) = parse_expansion(ParseExpansionArgs {
                        source: text,
                        start: index,
                        inputs: &mut analysis.inputs,
                        environment: &mut analysis.environment,
                        locals: &locals,
                    }) {
                    index = end;
                    continue;
                }
            }
            index += 1;
        }
    } else if matches!(interpreter, Interpreter::Powershell | Interpreter::Nushell) {
        let (environment_prefix, argument_prefix, argument_suffix) = match interpreter {
            Interpreter::Powershell => ("$env:", "$args[", "]"),
            Interpreter::Nushell => ("$env.", "$args.", ""),
            _ => unreachable!(),
        };
        let lower = text.to_ascii_lowercase();
        let mut cursor = 0;
        while let Some(relative) = lower[cursor..].find(environment_prefix) {
            let start = cursor + relative + environment_prefix.len();
            let end = identifier_end(text.as_bytes(), start);
            if end > start {
                analysis.environment.insert(text[start..end].to_owned());
            }
            cursor = end.max(start + 1);
        }
        cursor = 0;
        while let Some(relative) = lower[cursor..].find(argument_prefix) {
            let start = cursor + relative + argument_prefix.len();
            let end = text.as_bytes()[start..]
                .iter()
                .position(|byte| !byte.is_ascii_digit())
                .map_or(text.len(), |relative| start + relative);
            let suffix_matches =
                argument_suffix.is_empty() || text[end..].starts_with(argument_suffix);
            if end > start
                && suffix_matches
                && let Ok(index) = text[start..end].parse::<u64>()
            {
                analysis.inputs.insert((index + 1).to_string());
            }
            cursor = end.max(start + 1);
        }
    } else if matches!(interpreter, Interpreter::Cmd) {
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != b'%' {
                index += 1;
                continue;
            }
            if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                analysis
                    .inputs
                    .insert((bytes[index + 1] - b'0').to_string());
                index += 2;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'%') {
                index += 2;
                continue;
            }
            let Some(relative_end) = bytes[index + 1..].iter().position(|byte| *byte == b'%')
            else {
                break;
            };
            let start = index + 1;
            let end = start + relative_end;
            if end > start
                && text[start..end]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                analysis.environment.insert(text[start..end].to_owned());
            }
            index = end + 1;
        }
    }
    if reason.contains("dynamic shell evaluation") {
        analysis.capabilities.push("dynamic_eval".into());
    }
    if analysis.environment.iter().any(|name| secret_name(name)) {
        analysis.capabilities.push("secret_read".into());
    }
    analysis.capabilities.sort();
    analysis.capabilities.dedup();
    analysis
}

fn identifier_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    end
}

/// The inputs of [`delegated_node`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`delegated_node`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct DelegatedNodeArgs<'a> {
    path: &'a str,
    source: &'a [u8],
    interpreter: &'a str,
    reason: String,
    capabilities: Vec<String>,
}

fn delegated_node(parts: DelegatedNodeArgs<'_>) -> Node {
    // Destructured without `..`: see `DelegatedNodeArgs`.
    let DelegatedNodeArgs {
        path,
        source,
        interpreter,
        reason,
        capabilities,
    } = parts;
    let span = source_span_for_bytes(path, source);
    Node {
        id: String::new(),
        operation: Operation::InterpreterCall {
            interpreter: interpreter.to_owned(),
            interpreter_pin: default_interpreter_pin(interpreter),
            source: SourceBytes::from_bytes(source),
            source_span: span.clone(),
            capabilities,
            reason: reason.clone(),
        },
        guarantee: Guarantee::Delegated { reason },
        source: Some(span),
    }
}

pub(crate) fn default_interpreter_pin(interpreter: &str) -> String {
    format!(
        "sha256:{}",
        crate::digest::sha256(format!("deshell-official-runtime-v1:{interpreter}").as_bytes())
    )
}

pub(crate) fn bind_interpreter_pins(
    plan: &mut Plan,
    pins: &crate::config::InterpreterPins,
) -> Result<(), String> {
    for task in &mut plan.tasks {
        bind_node_pin(&mut task.body, pins)?;
    }
    plan.validate().map_err(|errors| errors.join("; "))
}

fn bind_node_pin(node: &mut Node, pins: &crate::config::InterpreterPins) -> Result<(), String> {
    match &mut node.operation {
        Operation::While { condition, body } => {
            bind_node_pin(condition, pins)?;
            bind_node_pin(body, pins)?;
        }
        Operation::Not { body } => bind_node_pin(body, pins)?,
        Operation::InterpreterCall {
            interpreter,
            interpreter_pin,
            ..
        } => {
            *interpreter_pin = match interpreter.to_ascii_lowercase().as_str() {
                "sh" | "posix_sh" => &pins.posix_sh,
                "bash" => &pins.bash,
                "zsh" => &pins.zsh,
                "fish" => &pins.fish,
                "powershell" | "pwsh" => &pins.powershell,
                "cmd" => &pins.cmd,
                "nu" | "nushell" => &pins.nushell,
                other => return Err(format!("no lock pin for delegated interpreter: {other}")),
            }
            .clone();
        }
        Operation::Pipeline { nodes, .. }
        | Operation::Sequence { nodes, .. }
        | Operation::Parallel { nodes } => {
            for child in nodes {
                bind_node_pin(child, pins)?;
            }
        }
        Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            bind_node_pin(predicate, pins)?;
            bind_node_pin(if_true, pins)?;
            if let Some(child) = if_false {
                bind_node_pin(child, pins)?;
            }
        }
        Operation::Match { cases, default, .. } => {
            for case in cases {
                bind_node_pin(&mut case.body, pins)?;
            }
            if let Some(child) = default {
                bind_node_pin(child, pins)?;
            }
        }
        Operation::Foreach { body, .. }
        | Operation::Scope { body, .. }
        | Operation::Redirect { body, .. }
        | Operation::CaptureStdout { body, .. }
        | Operation::Spawn { body, .. } => {
            bind_node_pin(body, pins)?;
        }
        Operation::TryFinally { body, finalizer } => {
            bind_node_pin(body, pins)?;
            bind_node_pin(finalizer, pins)?;
        }
        Operation::Exec { .. }
        | Operation::ExpandWords { .. }
        | Operation::TaskCall { .. }
        | Operation::SetVariable { .. }
        | Operation::SetEnvironment { .. }
        | Operation::SetWorkingDirectory { .. }
        | Operation::Wait { .. }
        | Operation::SendSignal { .. }
        | Operation::Test { .. }
        | Operation::FileRead { .. }
        | Operation::FileWrite { .. }
        | Operation::FileRemove { .. }
        | Operation::FileMetadata { .. }
        | Operation::FileSetMetadata { .. }
        | Operation::NetworkRequest { .. }
        | Operation::ClockRead { .. }
        | Operation::RandomBytes { .. }
        | Operation::OpaqueCapsule { .. } => {}
    }
    Ok(())
}

fn source_span_for_bytes(path: &str, source: &[u8]) -> SourceSpan {
    if let Ok(text) = std::str::from_utf8(source)
        && let Ok(span) = span_for_range(path, text, 0, text.len())
    {
        return span;
    }
    SourceSpan {
        file: path.into(),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: source.len() as u64,
        start_byte: 0,
        end_byte: source.len() as u64,
    }
}

fn native_node(operation: Operation, basis: &str, span: SourceSpan) -> Node {
    Node {
        id: String::new(),
        operation,
        guarantee: Guarantee::Native {
            semantic_model: basis.to_owned(),
        },
        source: Some(span),
    }
}

fn validate_tree_sitter_cst(
    path: &str,
    source: &str,
    language: tree_sitter::Language,
    parser_name: &str,
) -> Result<(), String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| format!("parser unavailable for {path} ({parser_name}): {error}"))?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        format!("parser unavailable for {path} ({parser_name}): parser returned no CST")
    })?;
    let root = tree.root_node();
    if !root.has_error() {
        return Ok(());
    }
    let invalid = first_invalid_cst_node(root).unwrap_or(root);
    Err(format!(
        "parse error in {path} from {parser_name} at bytes {}..{} ({})",
        invalid.start_byte(),
        invalid.end_byte(),
        invalid.kind()
    ))
}

fn validate_fish_cst(path: &str, source: &str) -> Result<(), String> {
    const PARSER_NAME: &str = "tree-sitter-fish/3.6.0";
    if source.ends_with('\n') {
        return validate_tree_sitter_cst(path, source, tree_sitter_fish::language(), PARSER_NAME);
    }

    // fish accepts EOF as a command terminator, while the pinned Tree-sitter
    // grammar requires an explicit newline or semicolon. Normalize only the
    // parser view so lowering, source spans, and coverage retain exact bytes.
    let mut parser_source = String::with_capacity(source.len() + 1);
    parser_source.push_str(source);
    parser_source.push('\n');
    validate_tree_sitter_cst(
        path,
        &parser_source,
        tree_sitter_fish::language(),
        PARSER_NAME,
    )
}

fn validate_cmd_cst(path: &str, source: &str) -> Result<(), String> {
    const PARSER_NAME: &str = "tree-sitter-batch/0.11.1";
    if source.ends_with('\n') || source.ends_with('\r') {
        return validate_tree_sitter_cst(
            path,
            source,
            tree_sitter_batch::LANGUAGE.into(),
            PARSER_NAME,
        );
    }

    // cmd.exe accepts EOF as a command terminator, while the pinned grammar
    // requires the final command in a multi-line program to be newline-ended.
    // Normalize only the parser view so spans and archived bytes stay exact.
    let mut parser_source = String::with_capacity(source.len() + 1);
    parser_source.push_str(source);
    parser_source.push('\n');
    validate_tree_sitter_cst(
        path,
        &parser_source,
        tree_sitter_batch::LANGUAGE.into(),
        PARSER_NAME,
    )
}

fn first_invalid_cst_node(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.has_error()
            && let Some(invalid) = first_invalid_cst_node(child)
        {
            return Some(invalid);
        }
    }
    None
}

fn validate_nushell_syntax(path: &str, source: &str) -> Result<(), String> {
    let directory = tempfile::Builder::new()
        .prefix("deshell-nushell-parser-")
        .tempdir()
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let version = execute_parser_process(
        directory.path(),
        vec!["nu".into(), "--version".into()],
        1024 * 1024,
    )
    .map_err(|error| format!("runtime unavailable for {path} (nu-parser): {error}"))?;
    if version.stdout != b"0.115.1\n" && version.stdout != b"0.115.1\r\n" {
        return Err(format!(
            "runtime unavailable for {path}: expected Nushell 0.115.1, found {}",
            String::from_utf8_lossy(&version.stdout).trim()
        ));
    }
    let source_path = directory.path().join("source.nu");
    crate::patch::scratch::write(&source_path, source.as_bytes())
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let parsed = execute_parser_process(
        directory.path(),
        vec![
            "nu".into(),
            "--no-config-file".into(),
            "--no-std-lib".into(),
            "--no-history".into(),
            "--ide-check".into(),
            "100".into(),
            source_path.to_string_lossy().into_owned(),
        ],
        16 * 1024 * 1024,
    )
    .map_err(|error| format!("runtime unavailable for {path} (nu-parser): {error}"))?;
    for frame in parsed.stdout.split(|byte| *byte == b'\n') {
        let frame = frame.strip_suffix(b"\r").unwrap_or(frame);
        if frame.is_empty() {
            continue;
        }
        let diagnostic = crate::strict_json::parse(frame).map_err(|error| {
            format!("runtime unavailable for {path} (nu-parser output): {error}")
        })?;
        if diagnostic.get("type").and_then(serde_json::Value::as_str) != Some("diagnostic") {
            return Err(format!(
                "runtime unavailable for {path}: nu-parser returned an unknown frame"
            ));
        }
        if diagnostic
            .get("severity")
            .and_then(serde_json::Value::as_str)
            != Some("Error")
        {
            continue;
        }
        let span = diagnostic
            .get("span")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| format!("runtime unavailable for {path}: nu-parser omitted span"))?;
        let start = span
            .get("start")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| format!("runtime unavailable for {path}: invalid nu-parser start span"))?
            .min(source.len());
        let end = span
            .get("end")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| format!("runtime unavailable for {path}: invalid nu-parser end span"))?
            .min(source.len());
        let message = diagnostic
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Nushell syntax error");
        return Err(format!(
            "parse error in {path} from nu-parser/0.115.1 at bytes {start}..{end} ({message})"
        ));
    }
    Ok(())
}

fn execute_parser_process(
    root: &std::path::Path,
    argv: Vec<String>,
    stdout_bytes: u64,
) -> Result<crate::agent_process::Outcome, String> {
    let outcome = crate::agent_process::execute(
        root,
        crate::agent_process::Request {
            argv,
            environment: Vec::new(),
            working_directory: None,
            stdin: Vec::new(),
            limits: crate::agent_process::Limits {
                timeout_ms: 10_000,
                memory_bytes: 2 * 1024 * 1024 * 1024,
                processes: 1024,
                stdout_bytes,
                stderr_bytes: 1024 * 1024,
            },
        },
    )?;
    if outcome.exit_code != 0
        || outcome.signal.is_some()
        || outcome.timed_out
        || outcome.limit_exceeded.is_some()
        || !outcome.stderr.is_empty()
    {
        return Err(format!(
            "parser process failed: exit={} stderr={}",
            outcome.exit_code,
            String::from_utf8_lossy(&outcome.stderr)
        ));
    }
    Ok(outcome)
}

fn validate_powershell_syntax(path: &str, source: &str) -> Result<(), String> {
    let directory = tempfile::Builder::new()
        .prefix("deshell-powershell-parser-")
        .tempdir()
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let adapter = directory.path().join("adapter.ps1");
    crate::patch::scratch::write(
        &adapter,
        include_bytes!("../../../adapters/powershell/adapter.ps1"),
    )
    .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let request = serde_json::json!({
        "id": "parse",
        "jsonrpc": "2.0",
        "method": "frontend.parse",
        "params": {"source": source}
    });
    let mut input = crate::canonical_json::canonical_bytes(&request)
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    input.push(b'\n');
    let outcome = crate::agent_process::execute(
        directory.path(),
        crate::agent_process::Request {
            argv: vec![
                "pwsh".into(),
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                adapter.to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            working_directory: None,
            stdin: input,
            limits: crate::agent_process::Limits {
                timeout_ms: 10_000,
                memory_bytes: 8 * 1024 * 1024 * 1024,
                processes: 1024,
                stdout_bytes: 16 * 1024 * 1024,
                stderr_bytes: 1024 * 1024,
            },
        },
    )
    .map_err(|error| {
        format!("runtime unavailable for {path} (PowerShell Parser.ParseInput): {error}")
    })?;
    if outcome.exit_code != 0
        || outcome.signal.is_some()
        || outcome.timed_out
        || outcome.limit_exceeded.is_some()
        || !outcome.stderr.is_empty()
    {
        return Err(format!(
            "runtime unavailable for {path} (PowerShell Parser.ParseInput): exit={} stderr={}",
            outcome.exit_code,
            String::from_utf8_lossy(&outcome.stderr)
        ));
    }
    let frames = outcome
        .stdout
        .split(|byte| *byte == b'\n')
        .map(|frame| frame.strip_suffix(b"\r").unwrap_or(frame))
        .filter(|frame| !frame.is_empty())
        .collect::<Vec<_>>();
    if frames.len() != 1 {
        return Err(format!(
            "runtime unavailable for {path} (PowerShell Parser.ParseInput): expected one response frame"
        ));
    }
    let result = crate::protocol::decode_response(frames[0], &serde_json::json!("parse")).map_err(
        |error| format!("runtime unavailable for {path} (PowerShell Parser.ParseInput): {error}"),
    )?;
    if result.get("parser").and_then(serde_json::Value::as_str)
        != Some("System.Management.Automation.Language.Parser")
    {
        return Err(format!(
            "runtime unavailable for {path}: PowerShell adapter returned an unknown parser"
        ));
    }
    if result
        .get("runtime_version")
        .and_then(serde_json::Value::as_str)
        != Some("7.6.5")
    {
        return Err(format!(
            "runtime unavailable for {path}: expected PowerShell 7.6.5 parser runtime"
        ));
    }
    let valid = result
        .get("valid")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| format!("runtime unavailable for {path}: parser omitted validity"))?;
    let diagnostics = result
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("runtime unavailable for {path}: parser omitted diagnostics"))?;
    if valid && diagnostics.is_empty() {
        return Ok(());
    }
    let diagnostic = diagnostics.first().ok_or_else(|| {
        format!("runtime unavailable for {path}: parser validity contradicted diagnostics")
    })?;
    let start_utf16 = diagnostic
        .get("start_offset")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("runtime unavailable for {path}: invalid parser start offset"))?;
    let end_utf16 = diagnostic
        .get("end_offset")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| format!("runtime unavailable for {path}: invalid parser end offset"))?;
    let start = utf16_offset_to_byte(source, start_utf16)
        .ok_or_else(|| format!("runtime unavailable for {path}: parser start offset is invalid"))?;
    let end = utf16_offset_to_byte(source, end_utf16)
        .ok_or_else(|| format!("runtime unavailable for {path}: parser end offset is invalid"))?;
    let message = diagnostic
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("PowerShell syntax error");
    Err(format!(
        "parse error in {path} from PowerShell Parser.ParseInput at bytes {start}..{end} ({message})"
    ))
}

fn utf16_offset_to_byte(source: &str, target: usize) -> Option<usize> {
    let mut utf16 = 0;
    for (byte, character) in source.char_indices() {
        if utf16 == target {
            return Some(byte);
        }
        utf16 += character.len_utf16();
        if utf16 > target {
            return None;
        }
    }
    (utf16 == target).then_some(source.len())
}

#[derive(Clone, Copy)]
struct Range {
    start: usize,
    end: usize,
}

/// The shell options this frontend models.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ShellOptions {
    errexit: bool,
    nounset: bool,
    pipefail: bool,
}

/// Read a `set` statement, if every option in it is one this frontend models.
///
/// Returns `None` — leaving the statement to be delegated — when the statement is
/// not a `set`, or when it carries even one option that is not modelled. Taking
/// the modelled part of `set -euo pipefail` and dropping `-u` would change what
/// the script does, so an unmodelled option disqualifies the whole statement
/// rather than only itself.
///
/// `-e` is modelled because which commands it stops on is decided statically
/// rather than by the caller. `&&` and `||` lower into their own node, and the
/// other tested positions — `if`, `while`, `until`, `!` — are not in the native
/// subset and are delegated whole, so the only untested position that reaches a
/// sequence is a plain statement. The case where the option's meaning
/// *would* depend on the call site — a shell function, whose body runs to
/// completion when the call is tested — is delegated before reaching here,
/// because function definitions are not in the native subset.
///
/// `-u` is not modelled: its exceptions (`${x:-}`, `${x+}`, `$@` with no
/// arguments) are a table this frontend does not have. `-f` is `noglob` rather
/// than `nosplit` and changes expansion; `-x` is a side effect.
fn set_statement(statement: &str, current: ShellOptions) -> Option<ShellOptions> {
    let mut words = statement.split_whitespace();
    if words.next()? != "set" {
        return None;
    }
    let mut options = current;
    let mut saw_one = false;
    while let Some(word) = words.next() {
        let enable = match word.chars().next()? {
            '-' => true,
            '+' => false,
            _ => return None,
        };
        let rest = &word[1..];
        if rest.is_empty() {
            return None;
        }
        // Short options combine (`-eo pipefail` is `-e` plus `-o pipefail`), and
        // `o` takes the next word, so it is only valid as the last letter.
        let mut flags = rest.chars().peekable();
        while let Some(flag) = flags.next() {
            match flag {
                'e' => options.errexit = enable,
                'u' => options.nounset = enable,
                'o' if flags.peek().is_none() => match words.next()? {
                    "pipefail" => options.pipefail = enable,
                    _ => return None,
                },
                _ => return None,
            }
        }
        saw_one = true;
    }
    if saw_one { Some(options) } else { None }
}

/// Where a `while` ends, and where its `do` divides it.
///
/// Returns `None` for a nested loop or a missing `do`, so the statement is
/// delegated rather than lowered from a guess. `until` is not recognised here:
/// rewriting it as a negated `while` would be a rewrite, not a lowering.
fn while_arms(source: &str, statements: &[Range], start: usize) -> Option<(usize, usize)> {
    let word = |index: usize| source[statements[index].start..statements[index].end].trim();
    let mut do_at = None;
    for index in start + 1..statements.len() {
        let text = word(index);
        if text == "done" {
            return do_at.map(|at| (index, at));
        }
        if text.starts_with("while ") || text.starts_with("until ") || text.starts_with("for ") {
            return None;
        }
        if text.starts_with("do") && do_at.is_none() {
            do_at = Some(index);
        }
    }
    None
}

/// Where an `if` ends, given the statement that opens it.
///
/// Returns the index of the `fi` statement and the indices of the `then` and
/// optional `else` that divide it. A nested `if`, an `elif`, or a missing arm
/// yields `None` so the statement is delegated rather than lowered from a guess.
fn if_arms(source: &str, statements: &[Range], start: usize) -> Option<(usize, usize, Option<usize>)> {
    let word = |index: usize| source[statements[index].start..statements[index].end].trim();
    let mut then_at = None;
    let mut else_at = None;
    for index in start + 1..statements.len() {
        let text = word(index);
        if text == "fi" {
            return then_at.map(|then| (index, then, else_at));
        }
        if text.starts_with("if ") || text.starts_with("elif ") || text == "elif" {
            // A nested branch needs a stack this does not keep.
            return None;
        }
        if text.starts_with("then") && then_at.is_none() {
            then_at = Some(index);
            continue;
        }
        if text.starts_with("else") && else_at.is_none() {
            then_at?;
            else_at = Some(index);
        }
    }
    None
}

/// The statement index of the `esac` that closes a `case`, if this models it.
///
/// Returns `None` for a nested `case`, so the statement is delegated rather than
/// lowered from a guess.
fn case_end(source: &str, statements: &[Range], start: usize) -> Option<usize> {
    for index in start + 1..statements.len() {
        let text = source[statements[index].start..statements[index].end].trim();
        if text == "esac" {
            return Some(index);
        }
        if text.starts_with("case ") {
            return None;
        }
    }
    None
}

/// Split `PATTERN) BODY` into its two halves.
///
/// Only a single literal pattern is modelled. An alternation (`a|b`), a pattern
/// carrying an expansion, or a missing `)` yields `None`, because matching the
/// wrong arm runs the wrong command.
fn case_arm(text: &str) -> Option<(&str, &str)> {
    let (pattern, body) = text.split_once(')')?;
    let pattern = pattern.trim().trim_start_matches('(').trim();
    if pattern.is_empty()
        || pattern
            .bytes()
            .any(|byte| matches!(byte, b'|' | b'$' | b'`' | b'"' | b'\'' | b'[' | b'?'))
    {
        return None;
    }
    Some((pattern, body.trim().trim_end_matches(";;").trim()))
}

fn lower_posix(path: &str, source: &str, interpreter: &Interpreter) -> Result<Lowered, String> {
    let statements = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut locals = BTreeSet::new();
    let mut nodes = Vec::new();
    // Shell options apply from where they are set onwards, so this travels with
    // the statement cursor rather than being read once for the file.
    let mut options = ShellOptions::default();
    // `Operation::Sequence` carries one `on_failure` for the whole list, so a file
    // that changes `set -e` partway through has no honest lowering: the statements
    // before the change stop on failure and the ones after do not. Recording only
    // the final value would claim one region's behaviour for both.
    //
    // A bool records that an option was set; it cannot record where it applied.
    // Rather than widen the IR here, the file is delegated when the region
    // changes — a wrong `native` is worse than a `delegated`, because the first
    // is a claim of equivalence.
    let mut errexit_regions = 0_usize;
    let mut index = 0;
    while index < statements.len() {
        let range = statements[index];
        index += 1;
        let text = &source[range.start..range.end];
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // `while COND; do BODY; done`, rejoined the same way as `if`.
        if trimmed.starts_with("while ")
            && let Some((done_at, do_at)) = while_arms(source, &statements, index - 1)
        {
            let mut arm = |from: usize, to: usize, strip: &str| -> Result<Node, String> {
                let mut pieces = Vec::new();
                for statement in &statements[from..to] {
                    let raw = source[statement.start..statement.end].trim();
                    let text = raw.strip_prefix(strip).unwrap_or(raw).trim();
                    if text.is_empty() {
                        continue;
                    }
                    let offset = statement.start
                        + source[statement.start..statement.end]
                            .find(text)
                            .ok_or("loop statement is not inside its range")?;
                    pieces.push(lower_posix_control(LowerPosixControlArgs {
                        path,
                        source,
                        range: Range {
                            start: offset,
                            end: offset + text.len(),
                        },
                        interpreter,
                        inputs: &mut inputs,
                        environment: &mut environment,
                        locals: &mut locals,
                        pipefail: options.pipefail,
                    })?);
                }
                if pieces.len() == 1 {
                    return Ok(pieces.remove(0));
                }
                let first = pieces
                    .first()
                    .and_then(|node| node.source.clone())
                    .ok_or("loop arm is empty")?;
                let last = pieces
                    .last()
                    .and_then(|node| node.source.clone())
                    .ok_or("loop arm span is missing")?;
                Ok(native_node(
                    Operation::Sequence {
                        nodes: pieces,
                        on_failure: if options.errexit {
                            crate::ir::SequenceFailure::Stop
                        } else {
                            crate::ir::SequenceFailure::Continue
                        },
                    },
                    &format!("{}-static-sequence-v1", interpreter.name()),
                    cover_spans(first, last),
                ))
            };
            let condition = arm(index - 1, do_at, "while ");
            let loop_body = arm(do_at, done_at, "do");
            if let (Ok(condition), Ok(loop_body)) = (condition, loop_body) {
                let span =
                    span_for_range(path, source, statements[index - 1].start, statements[done_at].end)?;
                nodes.push(native_node(
                    Operation::While {
                        condition: Box::new(condition),
                        body: Box::new(loop_body),
                    },
                    &format!("{}-static-while-v1", interpreter.name()),
                    span,
                ));
                index = done_at + 1;
                continue;
            }
            return Err("shell compound syntax requires pinned interpreter delegation".into());
        }
        // `case WORD in PATTERN) BODY ;; esac`, rejoined the same way as `if`.
        // `*` becomes the default arm; a pattern this does not model leaves the
        // whole statement delegated.
        if let Some(word) = trimmed
            .strip_prefix("case ")
            .and_then(|rest| rest.strip_suffix(" in"))
            && let Some(esac_at) = case_end(source, &statements, index - 1)
        {
            let start_of_word = range.start
                + source[range.start..range.end]
                    .find(word)
                    .ok_or("case word is not inside its range")?;
            let words = tokenize_posix(
                &source[start_of_word..start_of_word + word.len()],
                &mut inputs,
                &mut environment,
                &locals,
            );
            let mut cases = Vec::new();
            let mut default = None;
            let mut modelled = words.as_ref().is_ok_and(|words| words.len() == 1);
            for statement in &statements[index..esac_at] {
                let text = source[statement.start..statement.end].trim();
                if text.is_empty() {
                    continue;
                }
                let Some((pattern, arm)) = case_arm(text) else {
                    modelled = false;
                    break;
                };
                let offset = statement.start
                    + source[statement.start..statement.end]
                        .find(arm)
                        .unwrap_or_default();
                let Ok(node) = lower_posix_control(LowerPosixControlArgs {
                    path,
                    source,
                    range: Range {
                        start: offset,
                        end: offset + arm.len(),
                    },
                    interpreter,
                    inputs: &mut inputs,
                    environment: &mut environment,
                    locals: &mut locals,
                    pipefail: options.pipefail,
                }) else {
                    modelled = false;
                    break;
                };
                if pattern == "*" {
                    default = Some(Box::new(node));
                } else {
                    cases.push(crate::ir::MatchCase {
                        pattern: crate::ir::TextExpression::literal(pattern),
                        body: node,
                    });
                }
            }
            if modelled
                && let Ok(mut words) = words
                && (!cases.is_empty() || default.is_some())
            {
                let span = span_for_range(path, source, range.start, statements[esac_at].end)?;
                nodes.push(native_node(
                    Operation::Match {
                        value: words.remove(0),
                        cases,
                        default,
                    },
                    &format!("{}-static-match-v1", interpreter.name()),
                    span,
                ));
                index = esac_at + 1;
                continue;
            }
            return Err("shell compound syntax requires pinned interpreter delegation".into());
        }
        // `if COND; then BODY; fi` arrives as several statements because the
        // splitter breaks on `;` and newlines. Rejoining them here keeps the
        // branch in the native subset; anything this cannot rejoin — a nested
        // `if`, an `elif`, a missing arm — falls through and is delegated.
        if trimmed.starts_with("if ")
            && let Some((fi_at, then_at, else_at)) = if_arms(source, &statements, index - 1)
        {
            let mut branch = |from: usize, to: usize, strip: &str| -> Result<Node, String> {
                let mut pieces = Vec::new();
                for statement in &statements[from..to] {
                    let raw = source[statement.start..statement.end].trim();
                    let body = raw.strip_prefix(strip).unwrap_or(raw).trim();
                    if body.is_empty() {
                        continue;
                    }
                    let offset = statement.start
                        + source[statement.start..statement.end]
                            .find(body)
                            .ok_or("branch statement is not inside its range")?;
                    pieces.push(lower_posix_control(LowerPosixControlArgs {
                        path,
                        source,
                        range: Range {
                            start: offset,
                            end: offset + body.len(),
                        },
                        interpreter,
                        inputs: &mut inputs,
                        environment: &mut environment,
                        locals: &mut locals,
                        pipefail: options.pipefail,
                    })?);
                }
                let first = pieces.first().ok_or("branch is empty")?;
                if pieces.len() == 1 {
                    return Ok(pieces.remove(0));
                }
                let span = cover_spans(
                    first.source.clone().ok_or("branch span is missing")?,
                    pieces
                        .last()
                        .and_then(|node| node.source.clone())
                        .ok_or("branch span is missing")?,
                );
                Ok(native_node(
                    Operation::Sequence {
                        nodes: pieces,
                        on_failure: if options.errexit {
                            crate::ir::SequenceFailure::Stop
                        } else {
                            crate::ir::SequenceFailure::Continue
                        },
                    },
                    &format!("{}-static-sequence-v1", interpreter.name()),
                    span,
                ))
            };
            let predicate = branch(index - 1, then_at, "if ");
            let true_end = else_at.unwrap_or(fi_at);
            let if_true = branch(then_at, true_end, "then");
            let if_false = match else_at {
                Some(at) => branch(at, fi_at, "else").map(Some),
                None => Ok(None),
            };
            if let (Ok(predicate), Ok(if_true), Ok(if_false)) = (predicate, if_true, if_false) {
                let span = span_for_range(
                    path,
                    source,
                    statements[index - 1].start,
                    statements[fi_at].end,
                )?;
                nodes.push(native_node(
                    Operation::Condition {
                        predicate: Box::new(predicate),
                        if_true: Box::new(if_true),
                        if_false: if_false.map(Box::new),
                    },
                    &format!("{}-static-condition-v1", interpreter.name()),
                    span,
                ));
                index = fi_at + 1;
                continue;
            }
            return Err("shell compound syntax requires pinned interpreter delegation".into());
        }
        if let Some(updated) = set_statement(trimmed, options) {
            // A `set` is a change of lowering state, not an operation: it emits no
            // node, and the statements after it carry its effect instead.
            if updated.errexit != options.errexit && !nodes.is_empty() {
                errexit_regions += 1;
            }
            options = updated;
            continue;
        }
        let node = lower_posix_control(LowerPosixControlArgs {
                path,
                source,
                range,
                interpreter,
                inputs: &mut inputs,
                environment: &mut environment,
                locals: &mut locals,
                pipefail: options.pipefail,
            })?;
        nodes.push(node);
    }
    if nodes.is_empty() {
        return Err("script contains no statically lowerable operation".into());
    }
    if errexit_regions > 0 {
        return Err(
            "errexit changes partway through the script and requires pinned interpreter delegation"
                .into(),
        );
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
        native_node(
            Operation::Sequence {
                nodes,
                on_failure: if options.errexit {
                    crate::ir::SequenceFailure::Stop
                } else {
                    crate::ir::SequenceFailure::Continue
                },
            },
            &format!("{}-static-sequence-v1", interpreter.name()),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
        nounset: options.nounset,
    })
}

fn shell_statements(source: &str) -> Result<Vec<Range>, String> {
    let mut output = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut token_started = false;
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if comment {
            if byte == b'\n' {
                comment = false;
                let range = trim_range(source, start, index);
                if range.start < range.end {
                    output.push(range);
                }
                start = index + 1;
                token_started = false;
            }
        } else if escaped {
            escaped = false;
        } else if byte == b'\\' && quote != Some(b'\'') {
            escaped = true;
        } else if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            token_started = true;
        } else if byte == b'#' && !token_started {
            comment = true;
        } else if byte == b'\n' || byte == b';' {
            let range = trim_range(source, start, index);
            if range.start < range.end {
                output.push(range);
            }
            start = index + 1;
            token_started = false;
        } else {
            token_started = !byte.is_ascii_whitespace();
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

/// The inputs of [`lower_posix_control`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_posix_control`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerPosixControlArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    interpreter: &'a Interpreter,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a mut BTreeSet<String>,
    /// Whether `set -o pipefail` is in effect at this statement.
    pipefail: bool,
}

fn lower_posix_control(parts: LowerPosixControlArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPosixControlArgs`.
    let LowerPosixControlArgs {
        path,
        source,
        range,
        interpreter,
        inputs,
        environment,
        locals,
        pipefail,
    } = parts;
    let controls = top_level_controls(source, range)?;
    if controls.is_empty() {
        return lower_posix_simple(LowerPosixSimpleArgs {
                path,
                source,
                range,
                interpreter,
                inputs,
                environment,
                locals,
            });
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
        nodes.push(lower_posix_simple(LowerPosixSimpleArgs {
                path,
                source,
                range: piece,
                interpreter,
                inputs,
                environment,
                locals,
            })?);
    }
    let span = span_for_range(path, source, range.start, range.end)?;
    match kind {
        "|" => Ok(native_node(
            Operation::Pipeline {
                nodes,
                status: if pipefail {
                    crate::ir::PipelineStatus::Pipefail
                } else {
                    crate::ir::PipelineStatus::Last
                },
            },
            "posix-static-pipeline-v1",
            span,
        )),
        "&&" => {
            let mut iterator = nodes.into_iter();
            let mut result = iterator.next().expect("pieces are non-empty");
            for next in iterator {
                result = native_node(
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
        "||" => Err(
            "POSIX || is outside the native model and requires pinned interpreter delegation"
                .into(),
        ),
        _ => Err("unsupported shell control operator".into()),
    }
}

fn top_level_controls(source: &str, range: Range) -> Result<Vec<(usize, &'static str)>, String> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    let mut output = Vec::new();
    let mut token_started = false;
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
            token_started = true;
            index += 1;
            continue;
        }
        if byte == b'#' && !token_started {
            break;
        }
        if byte.is_ascii_whitespace() {
            token_started = false;
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
        if byte == b'!' && !token_started && index > range.start {
            // A `!` that opens the statement is handled by the caller as a
            // prefix; one appearing mid-statement is history expansion or an
            // operator this does not model.
            return Err("POSIX negation remains delegated".into());
        }
        if byte == b'&' {
            // `>&` and `<&` duplicate a descriptor; the `&` there belongs to the
            // redirection, not to the control grammar.
            if index > range.start && matches!(bytes[index - 1], b'>' | b'<') {
                token_started = true;
                index += 1;
                continue;
            }
            // A lone `&` is background execution, which is not in the native
            // subset. `<` and `>` are redirections and are split off the simple
            // command later, so they are not control operators and pass through.
            return Err(
                "redirection or background execution requires pinned interpreter delegation".into(),
            );
        }
        token_started = true;
        index += 1;
    }
    Ok(output)
}

/// The inputs of [`lower_posix_simple`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_posix_simple`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerPosixSimpleArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    interpreter: &'a Interpreter,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a mut BTreeSet<String>,
}

fn lower_posix_simple(parts: LowerPosixSimpleArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPosixSimpleArgs`.
    let LowerPosixSimpleArgs {
        path,
        source,
        range,
        interpreter,
        inputs,
        environment,
        locals,
    } = parts;
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
            return Err("shell compound syntax requires pinned interpreter delegation".into());
        }
    }
    if raw.starts_with("eval ")
        || raw == "eval"
        || raw.starts_with("source ")
        || raw.starts_with(". ")
    {
        return Err("dynamic shell evaluation requires pinned interpreter delegation".into());
    }

    if let Some((name, rhs)) = standalone_assignment(raw) {
        if locals.contains(name) {
            return Err(format!(
                "mutable shell assignment requires pinned interpreter delegation: {name}"
            ));
        }
        let operation = if rhs.starts_with("$(") && rhs.ends_with(')') {
            let inner_start = range.start + raw.find("$(").unwrap() + 2;
            let inner_end = range.end - 1;
            let body = lower_posix_simple(LowerPosixSimpleArgs {
                    path,
                    source,
                    range: trim_range(source, inner_start, inner_end),
                    interpreter,
                    inputs,
                    environment,
                    locals,
                })?;
            Operation::CaptureStdout {
                name: name.to_owned(),
                value_type: PrimitiveType::Text,
                body: Box::new(body),
            }
        } else {
            let expression = parse_posix_word(ParsePosixWordArgs {
                    source: rhs,
                    allow_unquoted_expansion: true,
                    inputs,
                    environment,
                    locals,
                })?;
            Operation::SetVariable {
                name: name.to_owned(),
                value_type: infer_value_type(&expression),
                value: expression,
            }
        };
        locals.insert(name.to_owned());
        return Ok(native_node(
            operation,
            "posix-immutable-assignment-v1",
            span_for_range(path, source, range.start, range.end)?,
        ));
    }

    // `! COMMAND` inverts an exit status. Stripping the prefix here keeps the
    // body on the ordinary path, so `! [ -n "$X" ]` and `! command` both work
    // without the negation appearing in every branch below.
    if let Some(rest) = source[range.start..range.end].trim().strip_prefix("! ") {
        let offset = range.start
            + source[range.start..range.end]
                .find(rest)
                .ok_or("negated command is not inside its range")?;
        let inner = lower_posix_simple(LowerPosixSimpleArgs {
            path,
            source,
            range: Range {
                start: offset,
                end: offset + rest.len(),
            },
            interpreter,
            inputs,
            environment,
            locals,
        })?;
        return Ok(native_node(
            Operation::Not {
                body: Box::new(inner),
            },
            &format!("{}-static-negation-v1", interpreter.name()),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    // `[ ... ]` has to be recognised before tokenizing, because `[` and `]` are in
    // the glob character set the tokenizer refuses. The brackets are the builtin's
    // syntax here, not a pattern.
    let statement = source[range.start..range.end].trim();
    if let Some(inner) = statement
        .strip_prefix("[ ")
        .and_then(|rest| rest.strip_suffix(" ]"))
    {
        let offset = range.start
            + source[range.start..range.end]
                .find(inner)
                .ok_or("test operands are not inside their range")?;
        let operands = tokenize_posix(
            &source[offset..offset + inner.len()],
            inputs,
            environment,
            locals,
        )?;
        let predicate = test_predicate(&operands)
            .ok_or("unmodelled test operator requires pinned interpreter delegation")?;
        return Ok(native_node(
            Operation::Test { predicate },
            &format!("{}-static-test-v1", interpreter.name()),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    let (command, redirections) = split_redirections(&source[range.start..range.end])?;
    let words = tokenize_posix(&command, inputs, environment, locals)?;
    if words.is_empty() {
        return Err("empty shell command".into());
    }
    let executable = literal_expression(&words[0])
        .ok_or("dynamic executable requires pinned interpreter delegation")?;
    if executable == "test" {
        // `test` is a builtin, so it never reaches the shell's PATH lookup.
        // Modelling its operators is what keeps a conditional native; lowering it
        // to an `Exec` of `/bin/test` would substitute a different program.
        let predicate = test_predicate(&words[1..])
            .ok_or("unmodelled test operator requires pinned interpreter delegation")?;
        return Ok(native_node(
            Operation::Test { predicate },
            &format!("{}-static-test-v1", interpreter.name()),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    if shell_builtin(&executable) {
        return Err(format!(
            "shell builtin {executable} requires pinned interpreter delegation"
        ));
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
    let span = span_for_range(path, source, range.start, range.end)?;
    let exec = native_node(
        Operation::Exec {
            argv: words[argv_start..].to_vec(),
            environment: command_environment,
            working_directory: None,
        },
        &format!("{}-explicit-command-v1", interpreter.name()),
        span.clone(),
    );
    if redirections.is_empty() {
        return Ok(exec);
    }
    Ok(native_node(
        Operation::Redirect {
            redirections,
            body: Box::new(exec),
        },
        &format!("{}-explicit-redirection-v1", interpreter.name()),
        span,
    ))
}

/// Map a `test` operand list onto a modelled predicate.
///
/// Returns `None` for anything not in the table, including the file predicates,
/// negation, and the `-a`/`-o` connectives. An operator answered by a
/// neighbouring one would answer a different question, so the statement is
/// delegated instead.
fn test_predicate(operands: &[TextExpression]) -> Option<crate::ir::TestPredicate> {
    match operands {
        [flag, value] => match literal_expression(flag)?.as_str() {
            "-n" => Some(crate::ir::TestPredicate::NonEmpty {
                value: value.clone(),
            }),
            "-z" => Some(crate::ir::TestPredicate::Empty {
                value: value.clone(),
            }),
            _ => None,
        },
        [left, operator, right] => match literal_expression(operator)?.as_str() {
            "=" => Some(crate::ir::TestPredicate::StringEqual {
                left: left.clone(),
                right: right.clone(),
            }),
            "!=" => Some(crate::ir::TestPredicate::StringNotEqual {
                left: left.clone(),
                right: right.clone(),
            }),
            _ => None,
        },
        _ => None,
    }
}

fn shell_builtin(executable: &str) -> bool {
    [
        "!",
        ".",
        ":",
        "[",
        "alias",
        "bg",
        "bind",
        "break",
        "builtin",
        "caller",
        "cd",
        "command",
        "compgen",
        "complete",
        "continue",
        "declare",
        "dirs",
        "disown",
        "echo",
        "enable",
        "eval",
        "exec",
        "exit",
        "export",
        "false",
        "fc",
        "fg",
        "getopts",
        "hash",
        "help",
        "history",
        "jobs",
        "kill",
        "let",
        "local",
        "logout",
        "mapfile",
        "newgrp",
        "popd",
        "printf",
        "pushd",
        "pwd",
        "read",
        "readarray",
        "readonly",
        "return",
        "set",
        "shift",
        "shopt",
        "source",
        "suspend",
        "test",
        "times",
        "trap",
        "true",
        "type",
        "typeset",
        "ulimit",
        "umask",
        "unalias",
        "unset",
        "wait",
    ]
    .contains(&executable)
}

fn standalone_assignment(value: &str) -> Option<(&str, &str)> {
    let separator = value.find('=')?;
    let name = &value[..separator];
    if !valid_identifier(name) || name.contains(char::is_whitespace) {
        return None;
    }
    let rhs = &value[separator + 1..];
    if rhs.is_empty() {
        return None;
    }
    // Whitespace on the right normally means the statement is a command with a
    // leading assignment (`X=1 cmd`), which is a different operation. It does not
    // when the value is quoted, or when it is a single command substitution —
    // `x=$(cmd with args)` is one assignment, and its spaces belong to the
    // command being substituted.
    let quoted = rhs.starts_with('"') && rhs.ends_with('"');
    let whole_substitution = rhs.starts_with("$(")
        && rhs.ends_with(')')
        && matching_paren(rhs, 1) == Some(rhs.len() - 1);
    if rhs.bytes().any(|byte| byte.is_ascii_whitespace()) && !quoted && !whole_substitution {
        return None;
    }
    Some((name, rhs))
}

/// The index of the `)` closing the `(` at `open`, or `None` if it is unbalanced.
///
/// Counting is needed because `x=$(a) $(b)` is two substitutions and a space, not
/// one substitution containing a space — the first would otherwise look like it
/// spans to the end.
fn matching_paren(value: &str, open: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut depth = 0_usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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

fn lower_fish(path: &str, source: &str) -> Result<Lowered, String> {
    let ranges = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut nodes = Vec::new();
    for range in ranges {
        let raw = source[range.start..range.end].trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        nodes.push(lower_fish_control(LowerFishControlArgs {
                path,
                source,
                range,
                inputs: &mut inputs,
                environment: &mut environment,
            })?);
    }
    if nodes.is_empty() {
        return Err("fish script contains no static external invocation".into());
    }
    let body = if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        let first = nodes.first().unwrap().source.clone().unwrap();
        let last = nodes.last().unwrap().source.clone().unwrap();
        native_node(
            Operation::Sequence {
                nodes,
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            "fish-static-sequence-v1",
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
        nounset: false,
    })
}

/// The inputs of [`lower_fish_control`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_fish_control`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerFishControlArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_fish_control(parts: LowerFishControlArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerFishControlArgs`.
    let LowerFishControlArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let controls = top_level_controls(source, range)?;
    if controls.is_empty() {
        return lower_fish_simple(LowerFishSimpleArgs {
                path,
                source,
                range,
                inputs,
                environment,
            });
    }
    if controls.iter().any(|(_, operator)| *operator != "&&") {
        return Err("fish control syntax is outside the static && subset".into());
    }
    let mut pieces = Vec::new();
    let mut cursor = range.start;
    for (position, operator) in controls {
        let piece = trim_range(source, cursor, position);
        if piece.start == piece.end {
            return Err("fish && has an empty operand".into());
        }
        pieces.push(piece);
        cursor = position + operator.len();
    }
    let last = trim_range(source, cursor, range.end);
    if last.start == last.end {
        return Err("fish && has an empty operand".into());
    }
    pieces.push(last);
    let span = span_for_range(path, source, range.start, range.end)?;
    let mut nodes = pieces
        .into_iter()
        .map(|piece| lower_fish_simple(LowerFishSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
            }))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let mut result = nodes.next().expect("fish && pieces are non-empty");
    for next in nodes {
        result = native_node(
            Operation::Condition {
                predicate: Box::new(result),
                if_true: Box::new(next),
                if_false: None,
            },
            "fish-and-if-v1",
            span.clone(),
        );
    }
    Ok(result)
}

/// The inputs of [`lower_fish_simple`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_fish_simple`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerFishSimpleArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_fish_simple(parts: LowerFishSimpleArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerFishSimpleArgs`.
    let LowerFishSimpleArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let words = tokenize_fish(&source[range.start..range.end], inputs, environment)?;
    if words.len() < 2 || literal_expression(&words[0]).as_deref() != Some("command") {
        return Err("fish command is not an explicit external invocation".into());
    }
    let executable = literal_expression(&words[1])
        .ok_or("dynamic fish executable requires pinned interpreter delegation")?;
    if executable.is_empty() {
        return Err("fish external executable is empty".into());
    }
    Ok(native_node(
        Operation::Exec {
            argv: words[1..].to_vec(),
            environment: Vec::new(),
            working_directory: None,
        },
        "fish-static-external-command-v1",
        span_for_range(path, source, range.start, range.end)?,
    ))
}

fn tokenize_fish(
    source: &str,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
) -> Result<Vec<TextExpression>, String> {
    let mut words = Vec::new();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut quote = None;
    let mut started = false;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(b'\'') => {
                if byte == b'\'' {
                    quote = None;
                    index += 1;
                } else {
                    let character = source[index..].chars().next().unwrap();
                    literal.push(character);
                    index += character.len_utf8();
                }
                started = true;
            }
            Some(b'"') => {
                if byte == b'"' {
                    quote = None;
                    index += 1;
                    continue;
                }
                if byte == b'$' {
                    flush_literal(&mut parts, &mut literal);
                    let (part, next) = parse_fish_expansion(source, index, inputs, environment)?;
                    parts.push(part);
                    index = next;
                    started = true;
                    continue;
                }
                if byte == b'\\' || byte == b'`' {
                    return Err(
                        "fish quoted escape or substitution is outside the static subset".into(),
                    );
                }
                let character = source[index..].chars().next().unwrap();
                literal.push(character);
                index += character.len_utf8();
                started = true;
            }
            _ => {
                if byte.is_ascii_whitespace() {
                    finish_word(&mut words, &mut parts, &mut literal, started);
                    started = false;
                    index += 1;
                    continue;
                }
                if byte == b'\'' || byte == b'"' {
                    quote = Some(byte);
                    started = true;
                    index += 1;
                    continue;
                }
                if byte == b'$' {
                    return Err("unquoted fish expansion may produce multiple arguments".into());
                }
                if matches!(
                    byte,
                    b'\\'
                        | b'`'
                        | b'*'
                        | b'?'
                        | b'['
                        | b']'
                        | b'{'
                        | b'}'
                        | b'|'
                        | b'&'
                        | b';'
                        | b'<'
                        | b'>'
                ) {
                    return Err(
                        "fish dynamic or control syntax is outside the static subset".into(),
                    );
                }
                let character = source[index..].chars().next().unwrap();
                literal.push(character);
                index += character.len_utf8();
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated fish quote".into());
    }
    finish_word(&mut words, &mut parts, &mut literal, started);
    Ok(words)
}

fn parse_fish_expansion(
    source: &str,
    start: usize,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
) -> Result<(TextPart, usize), String> {
    let rest = &source[start + 1..];
    if let Some(rest) = rest.strip_prefix("argv[") {
        let close = rest.find(']').ok_or("unterminated fish argv index")?;
        let name = &rest[..close];
        if name
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
        {
            return Err("fish argv index must be a positive integer".into());
        }
        inputs.insert(name.into());
        return Ok((
            TextPart::Argument { name: name.into() },
            start + 1 + "argv[".len() + close + 1,
        ));
    }
    let mut end = start + 1;
    while source
        .as_bytes()
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    let name = &source[start + 1..end];
    if !valid_identifier(name) || name == "argv" {
        return Err("unsupported fish variable expansion".into());
    }
    environment.insert(name.into());
    Ok((TextPart::Variable { name: name.into() }, end))
}

fn lower_cmd(path: &str, source: &str) -> Result<Lowered, String> {
    let ranges = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut echo_off = false;
    let mut prologue = None;
    for range in ranges {
        let raw = source[range.start..range.end].trim();
        if raw.is_empty() || raw.to_ascii_lowercase().starts_with("rem ") {
            continue;
        }
        if matches!(raw.to_ascii_lowercase().as_str(), "@echo off" | "echo off") {
            if echo_off || !nodes.is_empty() {
                return Err("cmd echo suppression must be a single prologue".into());
            }
            echo_off = true;
            prologue = Some(span_for_range(path, source, range.start, range.end)?);
            continue;
        }
        if !echo_off && !raw.starts_with('@') {
            return Err("cmd command echo must be suppressed".into());
        }
        nodes.push(lower_cmd_control(LowerCmdControlArgs {
                path,
                source,
                range,
                inputs: &mut inputs,
                environment: &mut environment,
            })?);
    }
    if nodes.is_empty() {
        return Err("cmd script contains no static external invocation".into());
    }
    let body = if nodes.len() == 1 {
        let mut node = nodes.remove(0);
        if let Some(prologue) = prologue {
            let command = node.source.clone().unwrap();
            node.source = Some(cover_spans(prologue, command));
        }
        node
    } else {
        let first = prologue.unwrap_or_else(|| nodes.first().unwrap().source.clone().unwrap());
        let last = nodes.last().unwrap().source.clone().unwrap();
        native_node(
            Operation::Sequence {
                nodes,
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            "cmd-static-sequence-v1",
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
        nounset: false,
    })
}

/// The inputs of [`lower_cmd_control`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_cmd_control`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerCmdControlArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_cmd_control(parts: LowerCmdControlArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerCmdControlArgs`.
    let LowerCmdControlArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let controls = cmd_and_controls(source, range)?;
    if controls.is_empty() {
        return lower_cmd_simple(LowerCmdSimpleArgs {
                path,
                source,
                range,
                inputs,
                environment,
            });
    }
    let mut pieces = Vec::new();
    let mut cursor = range.start;
    for position in controls {
        let piece = trim_range(source, cursor, position);
        if piece.start == piece.end {
            return Err("cmd && has an empty operand".into());
        }
        pieces.push(piece);
        cursor = position + 2;
    }
    let last = trim_range(source, cursor, range.end);
    if last.start == last.end {
        return Err("cmd && has an empty operand".into());
    }
    pieces.push(last);
    let span = span_for_range(path, source, range.start, range.end)?;
    let mut nodes = pieces
        .into_iter()
        .map(|piece| lower_cmd_simple(LowerCmdSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
            }))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let mut result = nodes.next().expect("cmd && pieces are non-empty");
    for next in nodes {
        result = native_node(
            Operation::Condition {
                predicate: Box::new(result),
                if_true: Box::new(next),
                if_false: None,
            },
            "cmd-and-if-v1",
            span.clone(),
        );
    }
    Ok(result)
}

fn cmd_and_controls(source: &str, range: Range) -> Result<Vec<usize>, String> {
    let bytes = source.as_bytes();
    let mut quoted = false;
    let mut index = range.start;
    let mut output = Vec::new();
    while index < range.end {
        let byte = bytes[index];
        if byte == b'"' {
            quoted = !quoted;
            index += 1;
            continue;
        }
        if byte == b'^' {
            return Err("cmd caret escape syntax is outside the static subset".into());
        }
        if !quoted && byte == b'&' && bytes.get(index + 1) == Some(&b'&') {
            output.push(index);
            index += 2;
            continue;
        }
        if !quoted && matches!(byte, b'&' | b'|' | b';' | b'<' | b'>' | b'(' | b')') {
            return Err("cmd control syntax is outside the static && subset".into());
        }
        index += 1;
    }
    if quoted {
        return Err("unterminated cmd quote".into());
    }
    Ok(output)
}

/// The inputs of [`lower_cmd_simple`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_cmd_simple`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerCmdSimpleArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_cmd_simple(parts: LowerCmdSimpleArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerCmdSimpleArgs`.
    let LowerCmdSimpleArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let raw = source[range.start..range.end].trim();
    let command = raw.strip_prefix('@').unwrap_or(raw).trim_start();
    let argv = tokenize_cmd(command, inputs, environment)?;
    let executable = argv
        .first()
        .and_then(literal_expression)
        .ok_or("dynamic cmd executable requires pinned interpreter delegation")?;
    let executable = basename(&executable);
    if !executable.ends_with(".exe") && !executable.ends_with(".com") {
        return Err("cmd command requires an explicit .exe or .com executable".into());
    }
    Ok(native_node(
        Operation::Exec {
            argv,
            environment: Vec::new(),
            working_directory: None,
        },
        "cmd-static-external-command-v1",
        span_for_range(path, source, range.start, range.end)?,
    ))
}

fn tokenize_cmd(
    source: &str,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
) -> Result<Vec<TextExpression>, String> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut output = Vec::new();
    while index < bytes.len() {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let quoted = bytes[index] == b'"';
        let value = if quoted {
            index += 1;
            let start = index;
            while bytes.get(index).is_some_and(|byte| *byte != b'"') {
                if matches!(bytes[index], b'^' | b'!') {
                    return Err(
                        "cmd delayed expansion or escape syntax is outside the static subset"
                            .into(),
                    );
                }
                index += 1;
            }
            if bytes.get(index) != Some(&b'"') {
                return Err("unterminated cmd quoted argument".into());
            }
            let value = &source[start..index];
            index += 1;
            value
        } else {
            let start = index;
            while bytes
                .get(index)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                if matches!(
                    bytes[index],
                    b'^' | b'!' | b'%' | b'&' | b'|' | b';' | b'<' | b'>' | b'(' | b')'
                ) {
                    return Err("cmd dynamic syntax is outside the quoted static subset".into());
                }
                index += 1;
            }
            &source[start..index]
        };
        let token = if quoted {
            if let Some(name) = value.strip_prefix("%~") {
                if name
                    .parse::<usize>()
                    .ok()
                    .filter(|value| *value > 0)
                    .is_none()
                {
                    return Err("cmd argument index must be a positive integer".into());
                }
                inputs.insert(name.into());
                TextExpression {
                    parts: vec![TextPart::Argument { name: name.into() }],
                }
            } else if value.starts_with('%') && value.ends_with('%') && value.len() > 2 {
                let name = &value[1..value.len() - 1];
                if !valid_identifier(name) {
                    return Err("unsupported cmd environment expansion".into());
                }
                environment.insert(name.into());
                TextExpression {
                    parts: vec![TextPart::Variable { name: name.into() }],
                }
            } else if value.contains('%') {
                return Err("unsupported cmd percent expansion".into());
            } else {
                TextExpression::literal(value)
            }
        } else {
            TextExpression::literal(value)
        };
        if bytes
            .get(index)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            return Err("cmd argument concatenation is outside the static subset".into());
        }
        output.push(token);
    }
    if output.is_empty() {
        return Err("empty cmd command".into());
    }
    Ok(output)
}

fn lower_powershell(path: &str, source: &str) -> Result<Lowered, String> {
    let ranges = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut terminal_status_span = None;
    let range_count = ranges.len();
    for (index, range) in ranges.into_iter().enumerate() {
        let raw = source[range.start..range.end].trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        if raw == "exit $LASTEXITCODE" {
            if index + 1 != range_count {
                return Err("PowerShell LASTEXITCODE forwarding must be terminal".into());
            }
            terminal_status_span = Some(span_for_range(path, source, range.start, range.end)?);
            continue;
        }
        nodes.push(lower_powershell_control(LowerPowershellControlArgs {
                path,
                source,
                range,
                inputs: &mut inputs,
                environment: &mut environment,
            })?);
    }
    if nodes.is_empty() {
        return Err("PowerShell script contains no static external invocation".into());
    }
    let body = if nodes.len() == 1 && terminal_status_span.is_none() {
        nodes.remove(0)
    } else {
        let first = nodes.first().unwrap().source.clone().unwrap();
        let last =
            terminal_status_span.unwrap_or_else(|| nodes.last().unwrap().source.clone().unwrap());
        native_node(
            Operation::Sequence {
                nodes,
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            "powershell-static-sequence-with-status-v1",
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
        nounset: false,
    })
}

/// The inputs of [`lower_powershell_control`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_powershell_control`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerPowershellControlArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_powershell_control(parts: LowerPowershellControlArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPowershellControlArgs`.
    let LowerPowershellControlArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let controls = powershell_and_controls(source, range)?;
    if controls.is_empty() {
        return lower_powershell_simple(LowerPowershellSimpleArgs {
                path,
                source,
                range,
                inputs,
                environment,
            });
    }
    let mut pieces = Vec::new();
    let mut cursor = range.start;
    for position in controls {
        let piece = trim_range(source, cursor, position);
        if piece.start == piece.end {
            return Err("PowerShell && has an empty operand".into());
        }
        pieces.push(piece);
        cursor = position + 2;
    }
    let last = trim_range(source, cursor, range.end);
    if last.start == last.end {
        return Err("PowerShell && has an empty operand".into());
    }
    pieces.push(last);
    let span = span_for_range(path, source, range.start, range.end)?;
    let mut nodes = pieces
        .into_iter()
        .map(|piece| lower_powershell_simple(LowerPowershellSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
            }))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter();
    let mut result = nodes.next().expect("PowerShell && pieces are non-empty");
    for next in nodes {
        result = native_node(
            Operation::Condition {
                predicate: Box::new(result),
                if_true: Box::new(next),
                if_false: None,
            },
            "powershell-and-if-v1",
            span.clone(),
        );
    }
    Ok(result)
}

fn powershell_and_controls(source: &str, range: Range) -> Result<Vec<usize>, String> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut index = range.start;
    let mut output = Vec::new();
    while index < range.end {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            } else if byte == b'`' {
                return Err("PowerShell escape syntax is outside the static subset".into());
            }
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            index += 1;
            continue;
        }
        if byte == b'&' && bytes.get(index + 1) == Some(&b'&') {
            output.push(index);
            index += 2;
            continue;
        }
        if matches!(
            byte,
            b'`' | b'|' | b';' | b'<' | b'>' | b'{' | b'}' | b'(' | b')'
        ) {
            return Err("PowerShell control syntax is outside the static && subset".into());
        }
        index += 1;
    }
    if quote.is_some() {
        return Err("unterminated PowerShell quote".into());
    }
    Ok(output)
}

/// The inputs of [`lower_powershell_simple`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_powershell_simple`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerPowershellSimpleArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
}

fn lower_powershell_simple(parts: LowerPowershellSimpleArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPowershellSimpleArgs`.
    let LowerPowershellSimpleArgs {
        path,
        source,
        range,
        inputs,
        environment,
    } = parts;
    let words = tokenize_powershell(&source[range.start..range.end], inputs, environment)?;
    if words.len() < 2 || literal_expression(&words[0]).as_deref() != Some("&") {
        return Err("PowerShell command is not an explicit call-operator invocation".into());
    }
    literal_expression(&words[1])
        .filter(|value| !value.is_empty())
        .ok_or("dynamic PowerShell executable requires pinned interpreter delegation")?;
    Ok(native_node(
        Operation::Exec {
            argv: words[1..].to_vec(),
            environment: Vec::new(),
            working_directory: None,
        },
        "powershell-static-external-command-v1",
        span_for_range(path, source, range.start, range.end)?,
    ))
}

fn tokenize_powershell(
    source: &str,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
) -> Result<Vec<TextExpression>, String> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut output = Vec::new();
    while index < bytes.len() {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let token = if matches!(bytes[index], b'\'' | b'"') {
            let delimiter = bytes[index];
            index += 1;
            let start = index;
            while bytes.get(index).is_some_and(|byte| *byte != delimiter) {
                if bytes[index] == b'`' || (delimiter == b'"' && bytes[index] == b'$') {
                    return Err(
                        "PowerShell interpolated or escaped string is outside the static subset"
                            .into(),
                    );
                }
                index += 1;
            }
            if bytes.get(index) != Some(&delimiter) {
                return Err("unterminated PowerShell quoted argument".into());
            }
            let value = &source[start..index];
            index += 1;
            TextExpression::literal(value)
        } else {
            let start = index;
            while bytes
                .get(index)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                if matches!(
                    bytes[index],
                    b'`' | b'|' | b';' | b'<' | b'>' | b'{' | b'}' | b'(' | b')'
                ) {
                    return Err("PowerShell dynamic syntax is outside the static subset".into());
                }
                index += 1;
            }
            let value = &source[start..index];
            if let Some(index) = value
                .strip_prefix("$args[")
                .and_then(|value| value.strip_suffix(']'))
            {
                let index = index
                    .parse::<usize>()
                    .map_err(|_| "PowerShell args index must be a non-negative integer")?;
                let name = index
                    .checked_add(1)
                    .ok_or("PowerShell args index is too large")?
                    .to_string();
                inputs.insert(name.clone());
                TextExpression {
                    parts: vec![TextPart::Argument { name }],
                }
            } else if let Some(name) = value.strip_prefix("$env:") {
                if !valid_identifier(name) {
                    return Err("unsupported PowerShell environment variable".into());
                }
                environment.insert(name.into());
                TextExpression {
                    parts: vec![TextPart::Variable { name: name.into() }],
                }
            } else if value.starts_with('$') {
                return Err("unsupported PowerShell variable expression".into());
            } else {
                TextExpression::literal(value)
            }
        };
        if bytes
            .get(index)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            return Err("PowerShell argument concatenation is outside the static subset".into());
        }
        output.push(token);
    }
    if output.is_empty() {
        return Err("empty PowerShell command".into());
    }
    Ok(output)
}

fn lower_nushell(path: &str, source: &str, interpreter: &Interpreter) -> Result<Lowered, String> {
    if !source.trim_start().starts_with("def main ") {
        return lower_literal_family(path, source, interpreter);
    }
    let lines = nontrivia_line_ranges(source);
    if lines.len() != 9 {
        return Err("Nushell main must match the static argument/status-branch shape".into());
    }
    let header = lines[0].1;
    let signature = header
        .strip_prefix("def main [")
        .and_then(|value| value.strip_suffix("] {"))
        .ok_or("Nushell main signature is outside the static subset")?;
    let (parameter, parameter_type) = signature
        .split_once(':')
        .ok_or("Nushell main parameter requires an explicit string type")?;
    let parameter = parameter.trim();
    if !valid_identifier(parameter) || parameter_type.trim() != "string" {
        return Err("Nushell main requires one named string parameter".into());
    }
    if lines[3].1 != "if $env.LAST_EXIT_CODE == 0 {"
        || lines[5].1 != "} else {"
        || lines[7].1 != "}"
        || lines[8].1 != "}"
    {
        return Err("Nushell main condition is outside the static last-exit subset".into());
    }

    let mut environment = BTreeSet::new();
    let first = lower_nushell_external(LowerNushellExternalArgs {
            path,
            source,
            range: lines[1].0,
            parameter,
            environment: &mut environment,
        })?;
    let predicate = lower_nushell_external(LowerNushellExternalArgs {
            path,
            source,
            range: lines[2].0,
            parameter,
            environment: &mut environment,
        })?;
    let if_true = lower_nushell_external(LowerNushellExternalArgs {
            path,
            source,
            range: lines[4].0,
            parameter,
            environment: &mut environment,
        })?;
    let if_false = lower_nushell_external(LowerNushellExternalArgs {
            path,
            source,
            range: lines[6].0,
            parameter,
            environment: &mut environment,
        })?;
    let condition = native_node(
        Operation::Condition {
            predicate: Box::new(predicate),
            if_true: Box::new(if_true),
            if_false: Some(Box::new(if_false)),
        },
        "nushell-last-exit-condition-v1",
        span_for_range(path, source, lines[2].0.start, lines[7].0.end)?,
    );
    let span = cover_spans(
        first.source.clone().unwrap(),
        condition.source.clone().unwrap(),
    );
    Ok(Lowered {
        nounset: false,
        body: native_node(
            Operation::Sequence {
                nodes: vec![first, condition],
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            "nushell-static-main-sequence-v1",
            span,
        ),
        inputs: BTreeSet::from(["1".into()]),
        environment,
    })
}

fn nontrivia_line_ranges(source: &str) -> Vec<(Range, &str)> {
    let mut output = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let without_newline = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);
        let leading = without_newline.len() - without_newline.trim_start().len();
        let trimmed = without_newline.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            output.push((
                Range {
                    start: offset + leading,
                    end: offset + leading + trimmed.len(),
                },
                trimmed,
            ));
        }
        offset += line.len();
    }
    output
}

/// The inputs of [`lower_nushell_external`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`lower_nushell_external`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct LowerNushellExternalArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    parameter: &'a str,
    environment: &'a mut BTreeSet<String>,
}

fn lower_nushell_external(parts: LowerNushellExternalArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerNushellExternalArgs`.
    let LowerNushellExternalArgs {
        path,
        source,
        range,
        parameter,
        environment,
    } = parts;
    let mut argv =
        tokenize_nushell_external(&source[range.start..range.end], parameter, environment)?;
    let executable = argv
        .first_mut()
        .and_then(|value| match value.parts.as_mut_slice() {
            [TextPart::Literal { value }] => Some(value),
            _ => None,
        })
        .ok_or("Nushell external executable must be literal")?;
    let stripped = executable
        .strip_prefix('^')
        .ok_or("Nushell command is not an explicit external invocation")?;
    if stripped.is_empty() {
        return Err("Nushell external executable is empty".into());
    }
    *executable = stripped.into();
    Ok(native_node(
        Operation::Exec {
            argv,
            environment: Vec::new(),
            working_directory: None,
        },
        "nushell-static-external-command-v1",
        span_for_range(path, source, range.start, range.end)?,
    ))
}

fn tokenize_nushell_external(
    source: &str,
    parameter: &str,
    environment: &mut BTreeSet<String>,
) -> Result<Vec<TextExpression>, String> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut output = Vec::new();
    while index < bytes.len() {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let token = if matches!(bytes[index], b'\'' | b'"') {
            let delimiter = bytes[index];
            index += 1;
            let start = index;
            while bytes.get(index).is_some_and(|byte| *byte != delimiter) {
                if delimiter == b'"' && matches!(bytes[index], b'$' | b'`' | b'\\') {
                    return Err(
                        "Nushell interpolated or escaped string is outside the static subset"
                            .into(),
                    );
                }
                index += 1;
            }
            if bytes.get(index) != Some(&delimiter) {
                return Err("unterminated Nushell quoted argument".into());
            }
            let value = &source[start..index];
            index += 1;
            TextExpression::literal(value)
        } else {
            let start = index;
            while bytes
                .get(index)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                if matches!(bytes[index], b'`' | b'|' | b';' | b'<' | b'>' | b'(' | b')') {
                    return Err("Nushell control syntax is outside the static subset".into());
                }
                index += 1;
            }
            let value = &source[start..index];
            if value == format!("${parameter}") {
                TextExpression {
                    parts: vec![TextPart::Argument { name: "1".into() }],
                }
            } else if let Some(name) = value.strip_prefix("$env.") {
                if !valid_identifier(name) || name == "LAST_EXIT_CODE" {
                    return Err("unsupported Nushell environment cell path".into());
                }
                environment.insert(name.into());
                TextExpression {
                    parts: vec![TextPart::Variable { name: name.into() }],
                }
            } else if value.starts_with('$') {
                return Err("unsupported Nushell variable expression".into());
            } else {
                TextExpression::literal(value)
            }
        };
        if bytes
            .get(index)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            return Err("Nushell argument concatenation is outside the static subset".into());
        }
        output.push(token);
    }
    if output.is_empty() {
        return Err("empty Nushell external command".into());
    }
    Ok(output)
}

/// Split trailing redirections off a simple command.
///
/// Returns the command text with the redirections removed, and the redirections
/// in the order they were written. Only the unambiguous forms are recognised —
/// `>`, `>>`, `<`, an optional single-digit descriptor before any of them, and
/// `N>&M`. A heredoc (`<<`), `<>`, `>|` or a descriptor wider than one digit
/// leaves the whole statement to delegation, because getting those wrong changes
/// where a script's output goes.
fn split_redirections(source: &str) -> Result<(String, Vec<crate::ir::Redirection>), String> {
    let bytes = source.as_bytes();
    // Bytes rather than chars: `byte as char` would map each byte of a multi-byte
    // scalar to its own Latin-1 character and corrupt the text.
    let mut command = Vec::<u8>::new();
    let mut redirections = Vec::new();
    let mut index = 0;
    let mut quote: Option<u8> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            command.push(byte);
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
            command.push(byte);
            index += 1;
            continue;
        }
        if byte == b'\\' {
            command.push(byte);
            index += 1;
            if index < bytes.len() {
                command.push(bytes[index]);
                index += 1;
            }
            continue;
        }
        // A descriptor is only a descriptor when a redirection operator follows it
        // immediately; `echo 2 > file` redirects stdout and prints "2".
        let (fd, operator_at) = if byte.is_ascii_digit()
            && bytes.get(index + 1).is_some_and(|next| matches!(next, b'>' | b'<'))
            && !command.last().is_some_and(|last| !last.is_ascii_whitespace())
        {
            (u32::from(byte - b'0'), index + 1)
        } else if matches!(byte, b'>' | b'<') {
            (if byte == b'>' { 1 } else { 0 }, index)
        } else {
            command.push(byte);
            index += 1;
            continue;
        };
        let operator = bytes[operator_at];
        let mut cursor = operator_at + 1;
        let append = operator == b'>' && bytes.get(cursor) == Some(&b'>');
        if append {
            cursor += 1;
        }
        if operator == b'<' && bytes.get(cursor) == Some(&b'<') {
            return Err("heredoc requires pinned interpreter delegation".into());
        }
        if matches!(bytes.get(cursor), Some(b'|' | b'<' | b'>')) {
            return Err("redirection form requires pinned interpreter delegation".into());
        }
        if bytes.get(cursor) == Some(&b'&') {
            let target = bytes
                .get(cursor + 1)
                .filter(|byte| byte.is_ascii_digit())
                .ok_or("descriptor duplication requires pinned interpreter delegation")?;
            redirections.push(crate::ir::Redirection::Duplicate {
                fd,
                target_fd: u32::from(target - b'0'),
            });
            index = cursor + 2;
            continue;
        }
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let start = cursor;
        let mut target_quote: Option<u8> = None;
        while cursor < bytes.len() {
            let current = bytes[cursor];
            if let Some(open) = target_quote {
                if current == open {
                    target_quote = None;
                }
            } else if current == b'\'' || current == b'"' {
                target_quote = Some(current);
            } else if current.is_ascii_whitespace() {
                break;
            }
            cursor += 1;
        }
        if start == cursor {
            return Err("redirection target is missing".into());
        }
        let target = &source[start..cursor];
        let unquoted = strip_matching_quotes(target)
            .ok_or("redirection target requires pinned interpreter delegation")?;
        let path = crate::ir::TextExpression::literal(unquoted);
        redirections.push(if operator == b'<' {
            crate::ir::Redirection::Read { fd, path }
        } else {
            crate::ir::Redirection::Write { fd, path, append }
        });
        index = cursor;
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    let command = String::from_utf8(command).map_err(|_| "command is not valid UTF-8".to_owned())?;
    Ok((command, redirections))
}

/// A redirection target this frontend can represent: a bare or wholly quoted
/// word with no expansion in it. Anything else is delegated rather than guessed.
fn strip_matching_quotes(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let inner = match (bytes.first(), bytes.last()) {
        (Some(b'"'), Some(b'"')) | (Some(b'\''), Some(b'\'')) if bytes.len() >= 2 => {
            &value[1..value.len() - 1]
        }
        _ => value,
    };
    if inner.is_empty() || inner.bytes().any(|byte| matches!(byte, b'$' | b'`' | b'*' | b'?' | b'"' | b'\'')) {
        return None;
    }
    Some(inner.to_owned())
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
                    let (part, next) = parse_expansion(ParseExpansionArgs {
                            source,
                            start: index,
                            inputs,
                            environment,
                            locals,
                        })?;
                    parts.push(part);
                    index = next;
                    token_started = true;
                    continue;
                }
                if byte == b'`' {
                    return Err(
                        "command substitution requires pinned interpreter delegation".into(),
                    );
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
                    if next == b'\n' {
                        index += 2;
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
                    if escaped == '\n' {
                        index += 2;
                        continue;
                    }
                    literal.push(escaped);
                    token_started = true;
                    index += 1 + escaped.len_utf8();
                    continue;
                }
                if byte == b'$' {
                    return Err(
                        "unquoted expansion may split fields and requires pinned interpreter delegation"
                            .into(),
                    );
                }
                if byte == b'~' && !token_started {
                    return Err("tilde expansion remains delegated".into());
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
                    return Err(
                        "dynamic expansion or control syntax requires pinned interpreter delegation"
                            .into(),
                    );
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

/// The inputs of [`parse_posix_word`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`parse_posix_word`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ParsePosixWordArgs<'a> {
    source: &'a str,
    allow_unquoted_expansion: bool,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a BTreeSet<String>,
}

fn parse_posix_word(parts: ParsePosixWordArgs<'_>) -> Result<TextExpression, String> {
    // Destructured without `..`: see `ParsePosixWordArgs`.
    let ParsePosixWordArgs {
        source,
        allow_unquoted_expansion,
        inputs,
        environment,
        locals,
    } = parts;
    if allow_unquoted_expansion && source.starts_with('$') && !source.starts_with("$(") {
        let (part, end) = parse_expansion(ParseExpansionArgs {
                source,
                start: 0,
                inputs,
                environment,
                locals,
            })?;
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

/// The inputs of [`parse_expansion`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`parse_expansion`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ParseExpansionArgs<'a> {
    source: &'a str,
    start: usize,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a BTreeSet<String>,
}

fn parse_expansion(parts: ParseExpansionArgs<'_>) -> Result<(TextPart, usize), String> {
    // Destructured without `..`: see `ParseExpansionArgs`.
    let ParseExpansionArgs {
        source,
        start,
        inputs,
        environment,
        locals,
    } = parts;
    let bytes = source.as_bytes();
    if bytes.get(start + 1) == Some(&b'(') {
        return Err("command substitution requires pinned interpreter delegation".into());
    }
    let (name, end) = if bytes.get(start + 1) == Some(&b'{') {
        let relative = source[start + 2..]
            .find('}')
            .ok_or("unterminated braced expansion")?;
        let end = start + 2 + relative;
        let inner = &source[start + 2..end];
        // `${name:-fallback}` and `${name-fallback}`. The fallback is taken as a
        // literal: an expansion nested inside it is left to delegation rather
        // than half-represented.
        if let Some((name, fallback)) = inner
            .split_once(":-")
            .map(|(name, fallback)| ((name, true), fallback))
            .or_else(|| inner.split_once('-').map(|(n, f)| ((n, false), f)))
        {
            let ((name, empty_is_unset), fallback) = (name, fallback);
            if valid_identifier(name)
                && !fallback.contains('$')
                && !fallback.contains('`')
            {
                if !locals.contains(name) {
                    environment.insert(name.to_owned());
                }
                return Ok((
                    TextPart::DefaultValue {
                        name: name.to_owned(),
                        fallback: fallback.to_owned(),
                        empty_is_unset,
                    },
                    end + 1,
                ));
            }
        }
        (inner, end + 1)
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
            "parameter expansion syntax requires pinned interpreter delegation: {name}"
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
    validate_literal_family_source(source, interpreter)?;
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
        nodes.push(native_node(
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
        native_node(
            Operation::Sequence {
                nodes,
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            &format!("{}-static-sequence-v1", interpreter.name()),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs: BTreeSet::new(),
        environment: BTreeSet::new(),
        nounset: false,
    })
}

fn validate_literal_family_source(source: &str, interpreter: &Interpreter) -> Result<(), String> {
    match interpreter {
        Interpreter::Cmd
            if source.bytes().any(|byte| {
                matches!(byte, b';' | b'&' | b'|' | b'<' | b'>' | b'^' | b'(' | b')')
            }) =>
        {
            Err("cmd control or escape syntax requires pinned interpreter delegation".into())
        }
        Interpreter::Powershell if source.contains("''") || source.contains("\"\"") => {
            Err("PowerShell doubled-quote semantics require pinned interpreter delegation".into())
        }
        Interpreter::Fish | Interpreter::Nushell if source.contains('\\') => {
            Err("language-specific escape syntax requires pinned interpreter delegation".into())
        }
        _ => Ok(()),
    }
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
            detect("build.sh", b"#!/usr/bin/env bash\nprintf ok\n"),
            Interpreter::Bash,
            ".sh is only a family hint; an explicit Bash shebang wins"
        );
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
    fn concrete_extension_and_shebang_conflicts_are_blockers() {
        let error = lower(
            "build.ps1",
            b"#!/usr/bin/env bash\nprintf conflict\n",
            UnknownInterpreter::Reject,
        )
        .unwrap_err();
        assert!(
            error.contains("DESHELL_BLOCKER_INTERPRETER_CONFLICT"),
            "{error}"
        );
    }

    #[test]
    fn posix_quoted_expansions_become_explicit_parts() {
        let plan = lower(
            "scripts/build.sh",
            b"#!/bin/sh\n/usr/bin/printf '%s\\n' \"$NAME:$1\" '$NAME'\n",
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
        let node = body(
            "build.sh",
            b"/usr/bin/printf one | grep one\n/usr/bin/printf two\n",
        );
        let Operation::Sequence { nodes, .. } = node.operation else {
            panic!("expected sequence")
        };
        assert!(matches!(nodes[0].operation, Operation::Pipeline { .. }));
        assert!(matches!(nodes[1].operation, Operation::Exec { .. }));
    }

    #[test]
    fn set_o_pipefail_is_a_modelled_option_and_reaches_the_pipeline() {
        // `set -o pipefail` decides a pipeline's exit status, which is local and
        // static — unlike `set -e`, whose meaning depends on the call site. The IR
        // has carried `PipelineStatus::Pipefail` from the start; nothing read the
        // option that selects it, so every CI step that opens with
        // `set -euo pipefail` was delegated whole.
        let node = body(
            "build.sh",
            b"set -o pipefail\n/usr/bin/printf one | grep one\n",
        );
        // The toggle emits no node, so the pipeline is the whole body rather than
        // the first element of a sequence.
        let Operation::Pipeline { status, .. } = &node.operation else {
            panic!("expected the pipeline to be the body: {node:#?}")
        };
        assert_eq!(*status, crate::ir::PipelineStatus::Pipefail);
    }

    #[test]
    fn a_default_expansion_lowers_natively_with_its_fallback() {
        // `${VALUE:-fallback}` is the syntax `set -u` excepts, so an IR that
        // cannot say what it excepts cannot model the option. It is also the most
        // common expansion in CI scripts by itself.
        let node = body("build.sh", b"/bin/echo \"${VALUE:-fallback}\"\n");
        let Operation::Exec { argv, .. } = &node.operation else {
            panic!("expected exec: {node:#?}")
        };
        assert_eq!(
            argv[1].parts,
            [TextPart::DefaultValue {
                name: "VALUE".into(),
                fallback: "fallback".into(),
                empty_is_unset: true,
            }]
        );

        // `-` differs from `:-`: it substitutes only when the name is unset, not
        // when it is set to the empty string.
        let node = body("build.sh", b"/bin/echo \"${VALUE-fallback}\"\n");
        let Operation::Exec { argv, .. } = &node.operation else {
            panic!("expected exec")
        };
        assert_eq!(
            argv[1].parts,
            [TextPart::DefaultValue {
                name: "VALUE".into(),
                fallback: "fallback".into(),
                empty_is_unset: false,
            }]
        );

        // An empty fallback is the form that appears in `${GITHUB_STEP_SUMMARY:-}`.
        let node = body("build.sh", b"/bin/echo \"${VALUE:-}\"\n");
        let Operation::Exec { argv, .. } = &node.operation else {
            panic!("expected exec")
        };
        assert_eq!(
            argv[1].parts,
            [TextPart::DefaultValue {
                name: "VALUE".into(),
                fallback: String::new(),
                empty_is_unset: true,
            }]
        );
    }

    #[test]
    fn an_assigning_command_substitution_lowers_to_a_capture() {
        // `NAME=$(COMMAND)` is the form `Operation::CaptureStdout` represents: a
        // name, and the command whose stdout becomes its value.
        let node = body("build.sh", b"value=$(/bin/echo hello)\n");
        let Operation::CaptureStdout { name, body: inner, .. } = &node.operation else {
            panic!("expected capture: {node:#?}")
        };
        assert_eq!(name, "value");
        assert!(matches!(inner.operation, Operation::Exec { .. }));

        // A substitution used as an argument has nowhere to go: `TextPart` has no
        // variant for "the output of a command", so it stays delegated rather than
        // being approximated by a capture into a name nobody wrote.
        let node = body("build.sh", b"/bin/echo \"$(/bin/echo hello)\"\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "an argument substitution must delegate: {node:#?}"
        );
    }

    #[test]
    fn a_while_loop_lowers_to_a_loop() {
        // `while COND; do BODY; done`. The statement splitter breaks on `;` and
        // newlines, so this arrives as several statements and is rejoined the same
        // way `if` and `case` are.
        let node = body(
            "build.sh",
            b"while [ -n \"$VALUE\" ]; do /bin/echo tick; done\n",
        );
        let Operation::While { condition, body: inner } = &node.operation else {
            panic!("expected while: {node:#?}")
        };
        assert!(matches!(condition.operation, Operation::Test { .. }));
        assert!(matches!(inner.operation, Operation::Exec { .. }));

        // `until` inverts the condition, and is not modelled: rewriting it as a
        // negated `while` would be a rewrite rather than a lowering.
        let node = body(
            "build.sh",
            b"until [ -n \"$VALUE\" ]; do /bin/echo tick; done\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "until is not modelled: {node:#?}"
        );
    }

    #[test]
    fn a_negated_command_lowers_to_a_not() {
        // `! cmd` inverts an exit status, which is what makes `if ! command -v x`
        // work — the single most common way a CI script asks whether a tool is
        // missing.
        let node = body("build.sh", b"! /bin/echo hello\n");
        let Operation::Not { body: inner } = &node.operation else {
            panic!("expected not: {node:#?}")
        };
        assert!(matches!(inner.operation, Operation::Exec { .. }));

        // It composes with the predicates: `! [ -n "$X" ]`.
        let node = body("build.sh", b"! [ -n \"$VALUE\" ]\n");
        let Operation::Not { body: inner } = &node.operation else {
            panic!("expected not: {node:#?}")
        };
        assert!(matches!(inner.operation, Operation::Test { .. }));
    }

    #[test]
    fn a_test_builtin_lowers_to_a_modelled_predicate() {
        // `[` is a shell builtin and was refused by name, which stopped nearly
        // every shell conditional: the branch shape is implemented, but its
        // condition is written with `[` almost every time.
        //
        // Lowering it to `/bin/test` instead would be a rewrite, not a lowering —
        // the builtin and the external utility are not the same program — so the
        // operators are modelled directly.
        let node = body("build.sh", b"[ -n \"$VALUE\" ]\n");
        let Operation::Test { predicate } = &node.operation else {
            panic!("expected test: {node:#?}")
        };
        assert_eq!(
            predicate,
            &crate::ir::TestPredicate::NonEmpty {
                value: crate::ir::TextExpression {
                    parts: vec![TextPart::Variable {
                        name: "VALUE".into()
                    }]
                }
            }
        );

        let node = body("build.sh", b"[ \"$A\" = \"b\" ]\n");
        let Operation::Test { predicate } = &node.operation else {
            panic!("expected test")
        };
        assert!(matches!(
            predicate,
            crate::ir::TestPredicate::StringEqual { .. }
        ));

        // `-f` is not modelled: the runner cannot ask about a path, so answering
        // would mean answering about the wrong filesystem.
        let node = body("build.sh", b"[ -f \"$PATHNAME\" ]\n");
        assert!(matches!(node.operation, Operation::InterpreterCall { .. }));

        // An operator this does not model keeps the statement delegated rather
        // than being approximated by a neighbouring one.
        let node = body("build.sh", b"[ \"$A\" -nt \"$B\" ]\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "an unmodelled operator must delegate: {node:#?}"
        );
    }

    #[test]
    fn a_case_statement_lowers_to_a_match() {
        // `Operation::Match` and `MatchCase` have been in the IR from the start.
        // `case` was refused as compound syntax, so a target-triple dispatch — the
        // shape every cross-platform CI step uses — was delegated whole.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  a) /bin/echo first ;;\n  *) /bin/echo other ;;\nesac\n",
        );
        let Operation::Match {
            value,
            cases,
            default,
        } = &node.operation
        else {
            panic!("expected match: {node:#?}")
        };
        assert_eq!(value.parts, [TextPart::Argument { name: "1".into() }]);
        assert_eq!(cases.len(), 1, "{cases:#?}");
        assert_eq!(cases[0].pattern, crate::ir::TextExpression::literal("a"));
        assert!(matches!(cases[0].body.operation, Operation::Exec { .. }));
        assert!(default.is_some(), "`*` is the default arm");

        // A pattern this does not model keeps the whole statement delegated.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  a|b) /bin/echo alt ;;\nesac\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "an alternation pattern must delegate: {node:#?}"
        );
    }

    #[test]
    fn an_if_statement_lowers_to_a_condition() {
        // `Operation::Condition` has been in the IR from the start. `if` was
        // refused as compound syntax, so every CI step that branches — which is
        // most of them — was delegated whole.
        let node = body(
            "build.sh",
            b"if /bin/test x = x; then /bin/echo yes; fi\n",
        );
        let Operation::Condition {
            predicate,
            if_true,
            if_false,
        } = &node.operation
        else {
            panic!("expected condition: {node:#?}")
        };
        assert!(matches!(predicate.operation, Operation::Exec { .. }));
        assert!(matches!(if_true.operation, Operation::Exec { .. }));
        assert!(if_false.is_none());

        // With an `else` arm.
        let node = body(
            "build.sh",
            b"if /bin/test x = x; then /bin/echo yes; else /bin/echo no; fi\n",
        );
        let Operation::Condition { if_false, .. } = &node.operation else {
            panic!("expected condition")
        };
        assert!(
            if_false.is_some(),
            "the else arm must survive: {node:#?}"
        );

        // A form this does not model stays delegated rather than being guessed at.
        let node = body(
            "build.sh",
            b"if /bin/test x = x; then /bin/echo a; elif /bin/test y = y; then /bin/echo b; fi\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "elif is not modelled and must delegate: {node:#?}"
        );
    }

    fn body_of(path: &str, source: &[u8]) -> crate::ir::Node {
        body(path, source)
    }

    #[test]
    fn a_simple_redirection_lowers_natively() {
        // `Operation::Redirect` and every `Redirection` form have been in the IR
        // from the start; the tokenizer refused the operators that select them, so
        // `cmd >file` was delegated whole. Redirections are the most common reason
        // a CI step leaves the native subset after `set`.
        let node = body("build.sh", b"/bin/echo hello >\"out.txt\"\n");
        let Operation::Redirect {
            redirections,
            body,
        } = &node.operation
        else {
            panic!("expected redirect: {node:#?}")
        };
        assert_eq!(
            redirections,
            &[crate::ir::Redirection::Write {
                fd: 1,
                path: crate::ir::TextExpression::literal("out.txt"),
                append: false,
            }]
        );
        assert!(matches!(body.operation, Operation::Exec { .. }));

        // `2>` names a descriptor, and `>>` appends.
        let node = body_of("build.sh", b"/bin/echo hello 2>>\"log.txt\"\n");
        let Operation::Redirect { redirections, .. } = &node.operation else {
            panic!("expected redirect: {node:#?}")
        };
        assert_eq!(
            redirections,
            &[crate::ir::Redirection::Write {
                fd: 2,
                path: crate::ir::TextExpression::literal("log.txt"),
                append: true,
            }]
        );

        // `2>&1` duplicates rather than opening a path.
        let node = body_of("build.sh", b"/bin/echo hello 2>&1\n");
        let Operation::Redirect { redirections, .. } = &node.operation else {
            panic!("expected redirect: {node:#?}")
        };
        assert_eq!(
            redirections,
            &[crate::ir::Redirection::Duplicate {
                fd: 2,
                target_fd: 1,
            }]
        );
    }

    #[test]
    fn set_e_stops_a_sequence_and_an_unmodelled_option_does_not() {
        // `set -e` stops on a command that is *not tested*, and which commands
        // those are is already the shape of the tree, so the option is a property
        // of the sequence.
        let node = body("build.sh", b"set -e\n/bin/echo one\n/bin/echo two\n");
        let Operation::Sequence { on_failure, .. } = &node.operation else {
            panic!("expected sequence: {node:#?}")
        };
        assert_eq!(*on_failure, crate::ir::SequenceFailure::Stop);

        let node = body("build.sh", b"/bin/echo one\n/bin/echo two\n");
        let Operation::Sequence { on_failure, .. } = &node.operation else {
            panic!("expected sequence: {node:#?}")
        };
        assert_eq!(*on_failure, crate::ir::SequenceFailure::Continue);

        // `-u` is not modelled, so the whole statement is refused rather than
        // having its `-e` and `pipefail` taken and its `-u` dropped.
        // `-e`, `-u` and `-o pipefail` are all modelled now, so the combined form
        // that opens nearly every CI step is accepted whole.
        assert_eq!(
            set_statement("set -euo pipefail", ShellOptions::default()),
            Some(ShellOptions {
                errexit: true,
                nounset: true,
                pipefail: true
            })
        );
        assert_eq!(
            set_statement("set -eo pipefail", ShellOptions::default()),
            Some(ShellOptions {
                errexit: true,
                nounset: false,
                pipefail: true
            })
        );
        assert_eq!(
            set_statement("set +e", ShellOptions {
                errexit: true,
                nounset: false,
                pipefail: false
            }),
            Some(ShellOptions::default())
        );
        assert_eq!(set_statement("set -f", ShellOptions::default()), None);
        assert_eq!(set_statement("/bin/echo set -e", ShellOptions::default()), None);
    }

    #[test]
    fn a_sequence_whose_errexit_changes_partway_is_not_claimed_as_native() {
        // `Operation::Sequence` carries one `on_failure` for the whole list, so a
        // file that turns `set -e` on and back off has no honest lowering: the
        // statements before `set +e` stop on failure and the ones after do not.
        //
        // Taking the last value seen would have claimed `continue` for the whole
        // sequence, which is wrong for every statement above the `set +e` — a
        // failure there ends the script in the shell and would not here. A bool
        // records that the option was set; it cannot record where it applied.
        let node = body(
            "build.sh",
            b"set -e\n/bin/echo one\nset +e\n/bin/echo two\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "a sequence with two errexit regions must delegate: {node:#?}"
        );

        // One region throughout is still native, in either state.
        let node = body("build.sh", b"set -e\n/bin/echo one\n/bin/echo two\n");
        let Operation::Sequence { on_failure, .. } = &node.operation else {
            panic!("expected sequence: {node:#?}")
        };
        assert_eq!(*on_failure, crate::ir::SequenceFailure::Stop);

        // Setting the same value again does not split anything.
        let node = body(
            "build.sh",
            b"set -e\n/bin/echo one\nset -e\n/bin/echo two\n",
        );
        assert!(matches!(node.operation, Operation::Sequence { .. }));
    }

    #[test]
    fn a_pipeline_before_set_o_pipefail_keeps_last_status() {
        // The option applies from where it is set, not to the whole file.
        let node = body(
            "build.sh",
            b"/usr/bin/printf one | grep one\nset -o pipefail\n/usr/bin/printf two | grep two\n",
        );
        let Operation::Sequence { nodes, .. } = node.operation else {
            panic!("expected sequence")
        };
        let Operation::Pipeline { status, .. } = &nodes[0].operation else {
            panic!("expected a pipeline first: {nodes:#?}")
        };
        assert_eq!(*status, crate::ir::PipelineStatus::Last);
        let Operation::Pipeline { status, .. } = &nodes[1].operation else {
            panic!("expected a pipeline second: {nodes:#?}")
        };
        assert_eq!(*status, crate::ir::PipelineStatus::Pipefail);
    }

    #[test]
    fn fish_quoted_inputs_environment_and_and_branch_lower_natively() {
        let plan = lower(
            "corpus.fish",
            concat!(
                "#!/usr/bin/env fish\n",
                "command /usr/bin/printf '%s:%s\\n' \"$argv[1]\" \"$CORPUS_ENV\"\n",
                "command /bin/test \"$argv[1]\" = pass && command /usr/bin/printf '%s\\n' branch\n",
            )
            .as_bytes(),
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(task.inputs[0].name, "1");
        assert_eq!(task.environment, ["CORPUS_ENV"]);
        let Operation::Sequence { nodes, .. } = &task.body.operation else {
            panic!("expected native fish sequence: {:#?}", task.body)
        };
        let Operation::Exec { argv, .. } = &nodes[0].operation else {
            panic!("expected fish exec")
        };
        assert_eq!(argv[2].parts, [TextPart::Argument { name: "1".into() }]);
        assert_eq!(
            argv[3].parts,
            [TextPart::Variable {
                name: "CORPUS_ENV".into()
            }]
        );
        assert!(matches!(nodes[1].operation, Operation::Condition { .. }));
        plan.validate().unwrap();
    }

    #[test]
    fn fish_embedded_snippet_without_trailing_newline_lowers_natively() {
        let plan = lower(
            "embedded.fish",
            b"command ./target/deshell-corpus-helper branch",
            UnknownInterpreter::Reject,
        )
        .unwrap();

        let Operation::Exec { argv, .. } = &plan.tasks[0].body.operation else {
            panic!("expected native fish exec: {:#?}", plan.tasks[0].body)
        };
        assert_eq!(
            argv[0].parts,
            [TextPart::Literal {
                value: "./target/deshell-corpus-helper".into()
            }]
        );
        assert_eq!(
            argv[1].parts,
            [TextPart::Literal {
                value: "branch".into()
            }]
        );
        plan.validate().unwrap();
    }

    #[test]
    fn nushell_main_input_environment_and_last_exit_branch_lower_natively() {
        let plan = lower(
            "corpus.nu",
            concat!(
                "def main [value: string] {\n",
                "  ^/usr/bin/printf '%s:%s\\n' $value $env.CORPUS_ENV\n",
                "  ^/bin/test $value '=' pass\n",
                "  if $env.LAST_EXIT_CODE == 0 {\n",
                "    ^/usr/bin/printf '%s\\n' branch\n",
                "  } else {\n",
                "    ^/usr/bin/false\n",
                "  }\n",
                "}\n",
            )
            .as_bytes(),
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(task.inputs[0].name, "1");
        assert_eq!(task.environment, ["CORPUS_ENV"]);
        let Operation::Sequence { nodes, .. } = &task.body.operation else {
            panic!("expected native Nushell sequence: {:#?}", task.body)
        };
        assert!(matches!(nodes[0].operation, Operation::Exec { .. }));
        let Operation::Condition {
            predicate,
            if_true,
            if_false,
        } = &nodes[1].operation
        else {
            panic!("expected last-exit condition")
        };
        assert!(matches!(predicate.operation, Operation::Exec { .. }));
        assert!(matches!(if_true.operation, Operation::Exec { .. }));
        assert!(matches!(
            if_false.as_deref().map(|node| &node.operation),
            Some(Operation::Exec { .. })
        ));
        plan.validate().unwrap();
    }

    #[test]
    fn powershell_args_environment_and_and_branch_lower_natively() {
        let plan = lower(
            "corpus.ps1",
            concat!(
                "& './corpus-helper' 'emit' $args[0] $env:CORPUS_ENV\n",
                "& './corpus-helper' 'test' $args[0] && & './corpus-helper' 'branch'\n",
                "exit $LASTEXITCODE\n",
            )
            .as_bytes(),
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(task.inputs[0].name, "1");
        assert_eq!(task.environment, ["CORPUS_ENV"]);
        let Operation::Sequence { nodes, .. } = &task.body.operation else {
            panic!("expected native PowerShell sequence: {:#?}", task.body)
        };
        let Operation::Exec { argv, .. } = &nodes[0].operation else {
            panic!("expected PowerShell exec")
        };
        assert_eq!(argv[2].parts, [TextPart::Argument { name: "1".into() }]);
        assert_eq!(
            argv[3].parts,
            [TextPart::Variable {
                name: "CORPUS_ENV".into()
            }]
        );
        assert!(matches!(nodes[1].operation, Operation::Condition { .. }));
        plan.validate().unwrap();
    }

    #[test]
    fn cmd_quoted_argument_environment_and_and_branch_lower_natively() {
        let plan = lower(
            "corpus.cmd",
            concat!(
                "@echo off\r\n",
                "target\\deshell-corpus-helper.exe emit \"%~1\" \"%CORPUS_ENV%\"\r\n",
                "target\\deshell-corpus-helper.exe test \"%~1\" && target\\deshell-corpus-helper.exe branch\r\n",
            )
            .as_bytes(),
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let task = &plan.tasks[0];
        assert_eq!(task.inputs[0].name, "1");
        assert_eq!(task.environment, ["CORPUS_ENV"]);
        let Operation::Sequence { nodes, .. } = &task.body.operation else {
            panic!("expected native cmd sequence: {:#?}", task.body)
        };
        let Operation::Exec { argv, .. } = &nodes[0].operation else {
            panic!("expected cmd exec")
        };
        assert_eq!(argv[2].parts, [TextPart::Argument { name: "1".into() }]);
        assert_eq!(
            argv[3].parts,
            [TextPart::Variable {
                name: "CORPUS_ENV".into()
            }]
        );
        assert!(matches!(nodes[1].operation, Operation::Condition { .. }));
        plan.validate().unwrap();
    }

    #[test]
    fn cmd_embedded_lf_script_with_echo_prologue_lowers_natively() {
        let plan = lower(
            "embedded.cmd",
            concat!("@echo off\n", "target\\deshell-corpus-helper.exe branch",).as_bytes(),
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let Operation::Exec { argv, .. } = &plan.tasks[0].body.operation else {
            panic!("expected native cmd exec: {:#?}", plan.tasks[0].body)
        };
        assert_eq!(
            argv[0].parts,
            [TextPart::Literal {
                value: "target\\deshell-corpus-helper.exe".into()
            }]
        );
        plan.validate().unwrap();
    }

    #[test]
    fn unsupported_known_sources_are_lossless_pinned_delegations() {
        let source = b"eval \"$DYNAMIC_SECRET\" \"$1\"\n";
        let plan = lower("build.sh", source, UnknownInterpreter::Reject).unwrap();
        let task = &plan.tasks[0];
        assert_eq!(
            task.inputs
                .iter()
                .map(|input| input.name.as_str())
                .collect::<Vec<_>>(),
            ["1"]
        );
        assert_eq!(task.environment, ["DYNAMIC_SECRET"]);
        assert_eq!(task.secrets, ["DYNAMIC_SECRET"]);
        let node = task.body.clone();
        assert!(matches!(node.guarantee, Guarantee::Delegated { .. }));
        let Operation::InterpreterCall {
            source: capsule,
            interpreter_pin,
            source_span,
            capabilities,
            ..
        } = node.operation
        else {
            panic!("expected interpreter call")
        };
        assert_eq!(capsule.to_bytes().unwrap(), source);
        assert!(interpreter_pin.starts_with("sha256:"));
        assert_eq!(source_span.end_byte, source.len() as u64);
        assert!(capabilities.contains(&"dynamic_eval".to_owned()));

        let bytes = b"printf '\xff'\n";
        let node = body("bad.sh", bytes);
        let Operation::InterpreterCall { source, .. } = node.operation else {
            panic!("expected interpreter call")
        };
        assert!(matches!(source, SourceBytes::Base64 { .. }));
        assert_eq!(source.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn failed_frontends_share_one_conservative_typed_interface_analysis() {
        for (interpreter, source, argument, environment) in [
            (Interpreter::Sh, "eval \"$TOKEN\" \"$1\"", "1", "TOKEN"),
            (
                Interpreter::Bash,
                "eval \"${API_SECRET}\" \"$2\"",
                "2",
                "API_SECRET",
            ),
            (
                Interpreter::Powershell,
                "Invoke-Expression $env:ACCESS_TOKEN $args[0]",
                "1",
                "ACCESS_TOKEN",
            ),
            (
                Interpreter::Cmd,
                "call %PRIVATE_KEY% %3",
                "3",
                "PRIVATE_KEY",
            ),
            (
                Interpreter::Nushell,
                "nu -c $env.PASSWORD $args.0",
                "1",
                "PASSWORD",
            ),
        ] {
            let analysis = conservative_source_analysis(
                source.as_bytes(),
                &interpreter,
                "dynamic shell evaluation requires pinned interpreter delegation",
            );
            assert!(analysis.inputs.contains(argument), "{interpreter:?}");
            assert!(
                analysis.environment.contains(environment),
                "{interpreter:?}"
            );
            assert!(analysis.capabilities.contains(&"secret_read".to_owned()));
            assert!(analysis.capabilities.contains(&"dynamic_eval".to_owned()));
        }
    }

    #[test]
    fn posix_continuations_are_removed_and_expansion_boundaries_delegate() {
        let plan = lower(
            "build.sh",
            b"/usr/bin/printf '%s' foo\\\n  bar\n",
            UnknownInterpreter::Reject,
        )
        .unwrap();
        let Operation::Exec { argv, .. } = &plan.tasks[0].body.operation else {
            panic!("line continuation should remain a static exec");
        };
        assert_eq!(literal_expression(&argv[3]).as_deref(), Some("bar"));
        for source in [
            b"/usr/bin/printf '%s' ~/value\n".as_slice(),
            b"! false\n".as_slice(),
            b"command printf ok\n".as_slice(),
        ] {
            let plan = lower("build.sh", source, UnknownInterpreter::Reject).unwrap();
            assert!(matches!(
                plan.tasks[0].body.operation,
                Operation::InterpreterCall { .. }
            ));
        }
        let comment = lower(
            "build.sh",
            b"/usr/bin/printf ok # ; printf must-not-run\n",
            UnknownInterpreter::Reject,
        )
        .unwrap();
        assert!(matches!(
            comment.tasks[0].body.operation,
            Operation::Exec { .. }
        ));
    }

    #[test]
    fn foreign_control_and_escape_syntax_is_never_misclassified_as_native() {
        for (path, source) in [
            (
                "build.cmd",
                b"@echo off\r\n@one.exe & two.exe\r\n".as_slice(),
            ),
            ("build.ps1", b"& 'tool.exe' 'it''s'\n".as_slice()),
            ("build.fish", b"command printf foo\\ bar\n".as_slice()),
        ] {
            let plan = lower(path, source, UnknownInterpreter::Reject).unwrap();
            assert!(matches!(
                plan.tasks[0].body.operation,
                Operation::InterpreterCall { .. }
            ));
        }
    }

    #[test]
    fn literal_subsets_cover_all_declared_interpreters() {
        let fixtures: &[(&str, &[u8], &str)] = &[
            ("build.zsh", b"/usr/bin/printf zsh\n", "/usr/bin/printf"),
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
    fn shell_builtins_are_delegated_instead_of_masquerading_as_external_execs() {
        for source in [
            b"printf value\n".as_slice(),
            b"echo value\n".as_slice(),
            b"true\n".as_slice(),
            b"test -f input\n".as_slice(),
        ] {
            let node = body("build.sh", source);
            assert!(matches!(node.guarantee, Guarantee::Delegated { .. }));
            assert!(matches!(node.operation, Operation::InterpreterCall { .. }));
        }
    }

    #[test]
    fn source_columns_count_unicode_scalars_while_bytes_remain_half_open() {
        let node = body("unicode.sh", "/usr/bin/printf 'é'\n".as_bytes());
        let span = node.source.unwrap();
        assert_eq!(span.start_line, 1);
        assert_eq!(span.start_column, 0);
        assert_eq!(span.end_line, 1);
        assert_eq!(span.end_column, 19);
        assert_eq!(span.start_byte, 0);
        assert_eq!(span.end_byte, 20);
    }

    #[test]
    fn posix_single_quoted_unicode_is_preserved_as_utf8_text() {
        let plan = lower(
            "unicode.sh",
            "/usr/bin/printf '%s' '日本語'\n".as_bytes(),
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
            "/usr/bin/printf \\日本語\n".as_bytes(),
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

    #[test]
    fn interpreter_pins_propagate_through_every_recursive_operation() {
        let pins = crate::config::InterpreterPins {
            posix_sh: "pin-sh".into(),
            bash: "pin-bash".into(),
            zsh: "pin-zsh".into(),
            fish: "pin-fish".into(),
            powershell: "pin-powershell".into(),
            cmd: "pin-cmd".into(),
            nushell: "pin-nushell".into(),
        };
        let call = |interpreter: &str| {
            delegated_node(DelegatedNodeArgs {
                    path: "script",
                    source: b"source",
                    interpreter,
                    reason: "delegated".into(),
                    capabilities: vec![],
                })
        };
        let native = |operation| Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Native {
                semantic_model: "test".into(),
            },
            source: None,
        };
        let mut root = native(Operation::TryFinally {
            body: Box::new(native(Operation::Sequence {
                nodes: vec![
                    native(Operation::Pipeline {
                        nodes: vec![call("sh"), call("posix_sh"), call("bash")],
                        status: crate::ir::PipelineStatus::Last,
                    }),
                    native(Operation::Parallel {
                        nodes: vec![call("zsh"), call("fish")],
                    }),
                    native(Operation::Condition {
                        predicate: Box::new(call("powershell")),
                        if_true: Box::new(call("pwsh")),
                        if_false: Some(Box::new(call("cmd"))),
                    }),
                    native(Operation::Match {
                        value: TextExpression::literal("value"),
                        cases: vec![crate::ir::MatchCase {
                            pattern: TextExpression::literal("case"),
                            body: call("nu"),
                        }],
                        default: Some(Box::new(call("nushell"))),
                    }),
                    native(Operation::Foreach {
                        variable: "item".into(),
                        items: vec![TextExpression::literal("value")],
                        body: Box::new(call("sh")),
                    }),
                    native(Operation::Scope {
                        variables: vec![],
                        environment: vec![],
                        working_directory: None,
                        body: Box::new(call("bash")),
                    }),
                    native(Operation::Redirect {
                        redirections: vec![],
                        body: Box::new(call("zsh")),
                    }),
                    native(Operation::CaptureStdout {
                        name: "captured".into(),
                        value_type: PrimitiveType::Text,
                        body: Box::new(call("fish")),
                    }),
                    native(Operation::Spawn {
                        handle: "child".into(),
                        body: Box::new(call("cmd")),
                    }),
                ],
                on_failure: crate::ir::SequenceFailure::Continue,
            })),
            finalizer: Box::new(Node::default()),
        });
        bind_node_pin(&mut root, &pins).unwrap();
        let encoded = serde_json::to_string(&root).unwrap();
        for pin in [
            "pin-sh",
            "pin-bash",
            "pin-zsh",
            "pin-fish",
            "pin-powershell",
            "pin-cmd",
            "pin-nushell",
        ] {
            assert!(encoded.contains(pin), "missing {pin}");
        }
        let mut unknown = call("future-shell");
        assert!(
            bind_node_pin(&mut unknown, &pins)
                .unwrap_err()
                .contains("no lock pin")
        );
    }

    #[test]
    fn posix_assignments_and_command_environment_preserve_typed_boundaries() {
        let capture = body(
            "capture.sh",
            b"VALUE=$(/usr/bin/true)\n/usr/bin/printf '%s' \"$VALUE\"\n",
        );
        assert!(
            matches!(capture.operation, Operation::Sequence { .. }),
            "{capture:#?}"
        );
        let Operation::Sequence { nodes, .. } = capture.operation else {
            panic!("expected assignment sequence")
        };
        assert!(matches!(
            nodes[0].operation,
            Operation::CaptureStdout { .. }
        ));

        let command = body("environment.sh", b"MODE=test /usr/bin/env\n");
        let Operation::Exec { environment, .. } = command.operation else {
            panic!("expected command-local environment")
        };
        assert_eq!(environment[0].name, "MODE");

        for source in [
            b"MODE=test OTHER=value\n".as_slice(),
            b"VALUE=one\nVALUE=two\n".as_slice(),
            b"$PROGRAM argument\n".as_slice(),
            b"if true\nthen\nfi\n".as_slice(),
        ] {
            assert!(matches!(
                body("delegated.sh", source).operation,
                Operation::InterpreterCall { .. }
            ));
        }
    }

    #[test]
    fn posix_tokenizer_covers_escape_and_expansion_error_boundaries() {
        let mut inputs = BTreeSet::new();
        let mut environment = BTreeSet::new();
        let locals = BTreeSet::new();
        let words = tokenize_posix(
            "\"a\\$b\\`c\\\"d\\\\e\\q\\\nend\"",
            &mut inputs,
            &mut environment,
            &locals,
        )
        .unwrap();
        assert_eq!(
            literal_expression(&words[0]).as_deref(),
            Some("a$b`c\"d\\e\\qend")
        );
        assert!(tokenize_posix("\"trailing\\", &mut inputs, &mut environment, &locals).is_err());
        assert!(
            parse_posix_word(ParsePosixWordArgs {
                    source: "one two",
                    allow_unquoted_expansion: false,
                    inputs: &mut inputs,
                    environment: &mut environment,
                    locals: &locals,
                }).is_err()
        );
        let argument =
            parse_posix_word(ParsePosixWordArgs {
                    source: "$1",
                    allow_unquoted_expansion: true,
                    inputs: &mut inputs,
                    environment: &mut environment,
                    locals: &locals,
                }).unwrap();
        assert!(matches!(argument.parts[0], TextPart::Argument { .. }));
        for expansion in ["$(date)", "${MISSING", "${}", "$"] {
            assert!(parse_expansion(ParseExpansionArgs {
                    source: expansion,
                    start: 0,
                    inputs: &mut inputs,
                    environment: &mut environment,
                    locals: &locals,
                }).is_err());
        }
    }

    #[test]
    fn literal_frontends_reject_every_ambiguous_command_shape() {
        for (source, family) in [
            ("'unterminated", LiteralFamily::Fish),
            ("'$VALUE'", LiteralFamily::Fish),
            ("'bad`value'", LiteralFamily::Powershell),
            ("bad*value", LiteralFamily::Nushell),
            ("bad^value", LiteralFamily::Cmd),
        ] {
            assert!(literal_words(source, family).is_err(), "{source}");
        }
        assert_eq!(
            literal_words("& 'tool.exe' 'value'", LiteralFamily::Powershell).unwrap()[0],
            "&"
        );
        assert_eq!(
            literal_words("^tool value", LiteralFamily::Nushell).unwrap()[0],
            "^tool"
        );

        for (source, interpreter) in [
            ("printf value", Interpreter::Fish),
            ("echo value", Interpreter::Powershell),
            ("tool.exe", Interpreter::Cmd),
            ("@tool value", Interpreter::Cmd),
            ("git status", Interpreter::Nushell),
            ("^ value", Interpreter::Nushell),
            ("^one\n^two", Interpreter::Nushell),
            ("/usr/bin/true", Interpreter::Sh),
        ] {
            assert!(
                lower_literal_family("source", source, &interpreter).is_err(),
                "{interpreter:?}: {source}"
            );
        }
    }
}
