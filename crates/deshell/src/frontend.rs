use crate::config::UnknownInterpreter;
use crate::ir::{
    Binding, Guarantee, NamedExpression, Node, Operation, Plan, PrimitiveType, SourceBytes,
    SourceSpan, Task, TextExpression, TextPart, ValueType,
};
use std::collections::{BTreeMap, BTreeSet};

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

/// What the host said about the shell this source runs under.
///
/// A GitHub workflow step with no `shell:` key runs as `bash -e {0}`, and one
/// that says `shell: bash` runs as `bash --noprofile --norc -eo pipefail {0}`.
/// Both are bash, so the interpreter name cannot tell them apart, and a pipeline
/// reports a different status under each.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct HostShell {
    /// Whether the host named it, as opposed to leaving the runner's default.
    pub named: bool,
}

pub(crate) fn lower(
    path: &str,
    source: &[u8],
    unknown_policy: UnknownInterpreter,
) -> Result<Plan, String> {
    lower_under_host(path, source, unknown_policy, HostShell::default())
}

/// Lower under what the host said. A shell file has no host, which is what
/// [`HostShell::default`] means.
fn lower_under_host(
    path: &str,
    source: &[u8],
    unknown_policy: UnknownInterpreter,
    host: HostShell,
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

    // Asked before any parser runs: whether these bytes are a program at all is
    // a question about the host, not about the shell grammar, and the grammar
    // answered it by accident for three shapes and wrongly for the rest.
    let substituted =
        std::str::from_utf8(source).is_ok_and(|text| holds_a_host_substitution(&normalized, text));
    let lowered = if substituted {
        Err("the step holds a GitHub expression, which the runner substitutes before any shell sees it; these bytes are a template rather than a program".into())
    } else {
        match std::str::from_utf8(source) {
            Err(_) => Err("source is not valid UTF-8 and cannot be statically lowered".into()),
            Ok(text) => match interpreter {
                Interpreter::Sh | Interpreter::Bash | Interpreter::Zsh => validate_tree_sitter_cst(
                    &normalized,
                    text,
                    tree_sitter_bash::LANGUAGE.into(),
                    "tree-sitter-bash/0.25.1",
                )
                .and_then(|()| lower_posix(&normalized, text, &interpreter, host)),
                Interpreter::Fish => validate_fish_cst(&normalized, text)
                    .and_then(|()| lower_fish(&normalized, text)),
                Interpreter::Cmd => {
                    validate_cmd_cst(&normalized, text).and_then(|()| lower_cmd(&normalized, text))
                }
                // These two start a process, so they are the only lowerings that
                // can end without an answer. `Unmeasured` leaves this function
                // rather than joining the delegation path below: a guarantee that
                // depends on whether a parser finished in time is not a guarantee.
                Interpreter::Powershell => match validate_powershell_syntax(&normalized, text) {
                    Ok(()) => lower_powershell(&normalized, text),
                    Err(LoweringFailure::Delegate(reason)) => Err(reason),
                    Err(failure @ LoweringFailure::Unmeasured(_)) => return Err(failure.message()),
                },
                Interpreter::Nushell => match validate_nushell_syntax(&normalized, text) {
                    Ok(()) => lower_nushell(&normalized, text, &interpreter),
                    Err(LoweringFailure::Delegate(reason)) => Err(reason),
                    Err(failure @ LoweringFailure::Unmeasured(_)) => return Err(failure.message()),
                },
                Interpreter::Unknown(_) => Err(format!(
                    "{} frontend is trace-only; unobserved behavior is not claimed as verified",
                    interpreter.name()
                )),
            },
        }
    };

    let (body, inputs, environment, invocation, platform_capabilities, nounset, mut tasks) =
        match lowered {
            Ok(lowered) => (
                lowered.body,
                lowered.inputs,
                lowered.environment,
                None,
                Vec::new(),
                lowered.nounset,
                lowered.tasks,
            ),
            Err(reason) => {
                let analysis = conservative_source_analysis(source, &interpreter, &reason);
                // A template is a residual for the same reason an unknown
                // interpreter is: nothing here can say what it does, so nothing
                // claims to.
                let body = if substituted || matches!(interpreter, Interpreter::Unknown(_)) {
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
                    // and defines its own functions inside that source.
                    Vec::new(),
                )
            }
        };
    let environment: Vec<String> = environment.into_iter().collect();
    let secrets = environment
        .iter()
        .filter(|name| secret_name(name))
        .cloned()
        .collect();
    // The entry task first: `Plan::validate` looks a call's target up in a
    // table built from all of them, so the order is for a reader.
    let mut all_tasks = vec![Task {
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
    }];
    all_tasks.append(&mut tasks);
    let mut plan = Plan {
        schema_version: 1,
        generator: "deshell/0.1.0".into(),
        entrypoint: "main".into(),
        tasks: all_tasks,
    };
    plan.assign_node_ids()?;
    plan.validate().map_err(|errors| errors.join("; "))?;
    Ok(plan)
}

/// The inputs of [`lower_with_interpreter`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to
/// a many-argument function stays invisible to every call site that already
/// compiles. [`lower_with_interpreter`] takes this apart without `..`, so a
/// field added here fails to compile until somebody gives it a destination —
/// which is how `host` got to every caller that had one to give.
pub(crate) struct LowerWithInterpreterArgs<'a> {
    pub path: &'a str,
    pub source: &'a [u8],
    pub unknown_policy: UnknownInterpreter,
    pub configured: &'a str,
    pub host: HostShell,
}

pub(crate) fn lower_with_interpreter(parts: LowerWithInterpreterArgs<'_>) -> Result<Plan, String> {
    // Destructured without `..`: see `LowerWithInterpreterArgs`.
    let LowerWithInterpreterArgs {
        path,
        source,
        unknown_policy,
        configured,
        host,
    } = parts;
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
    let mut plan = lower_under_host(&virtual_path, source, unknown_policy, host)?;
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
            Operation::NoOp | Operation::WriteStdout { .. } | Operation::Exit { .. } => {}
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
    /// Tasks the entry body calls, in the order they were defined.
    ///
    /// A shell function is a task: keeping the definition and its call sites as
    /// one definition and several calls carries "these are the same check" into
    /// the generated program, which inlining them would leave only in a
    /// reader's head.
    tasks: Vec<crate::ir::Task>,
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

/// The host a lowering path belongs to.
///
/// [`lower_with_interpreter`] appends `.deshell.<extension>` so that the right
/// parser is chosen for an embedded block; the host is what is left when that
/// comes off. Written once so the question below is asked the same way whether a
/// caller came through the embedded path or the direct one.
fn host_path_of(path: &str) -> &str {
    match path.rfind(".deshell.") {
        Some(index) => &path[..index],
        None => path,
    }
}

/// Whether these bytes are a GitHub workflow step holding an expression the
/// runner substitutes before any shell sees them.
///
/// `${{ ... }}` is not shell. GitHub evaluates it while writing the script file,
/// so the bytes the scanner read are a template and the bytes bash receives are
/// something else.
///
/// This looked handled because `tree-sitter-bash` rejects some of the shapes, so
/// they were delegated with a parse error. `bash -n` accepts all of them, and
/// the shapes tree-sitter accepts went through: measured here,
/// `run: /bin/echo '${{ matrix.os }}'` lowers to
/// `argv = ["/bin/echo", "${{ matrix.os }}"]` and is claimed `native`. Once the
/// step becomes `uses: ./.github/actions/...` the expression sits inside a
/// generated file, where GitHub does not substitute, so the program prints the
/// template where the step printed the value.
///
/// Verification does not catch it. It runs the original from the same template,
/// so the baseline is a program the runner never runs and the two agree.
///
/// Neither guarantee is available: `native` claims a program that is not these
/// bytes, and `delegated` would run the template under bash, which is also not
/// what happens. So nothing is claimed and the plan blocks — the shell is
/// visible, and what it does is not.
fn holds_a_host_substitution(path: &str, source: &str) -> bool {
    crate::migration::is_github_workflow_path(host_path_of(path)) && source.contains("${{")
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
        Operation::NoOp | Operation::WriteStdout { .. } | Operation::Exit { .. } => {}
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

/// Declare the vocabulary of native claims once.
///
/// The enum, the list of every variant and the two tables are generated from
/// the same rows, so they cannot drift: a variant added without a suffix does
/// not compile, and one added without being in the list is impossible because
/// there is no separate list to forget. A hand-written `ALL` had already lost
/// two variants by the time it was first read.
macro_rules! semantic_models {
    ($($variant:ident => $suffix:literal, $evidence:expr;)*) => {
        /// The claim a native node makes about how it behaves.
        ///
        /// A name rather than a string. `Guarantee::Native` carries a
        /// `semantic_model`, and the validator asked only that it not be empty
        /// — so a claim could name anything, including a model that does not
        /// exist and a measurement nobody took. This enum is the whole
        /// vocabulary, so a node cannot be built with a claim outside it.
        ///
        /// `contracts/semantic-models-v1.json` records the same list along with
        /// the recording each claim rests on, and a test compares the two. A
        /// model with evidence is one whose behaviour was measured across the
        /// shells; a model without is one whose meaning is the IR operation's
        /// own definition — a sequence runs its statements in order — and there
        /// is nothing to measure.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        enum SemanticModel {
            $($variant,)*
        }

        impl SemanticModel {
            /// Every model, so a gate can walk the vocabulary itself.
            ///
            /// The gate is the test that compares this with the contract, so
            /// this is built for the test alone rather than carried into the
            /// binary unused.
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$variant,)*];

            fn suffix(self) -> &'static str {
                match self {
                    $(Self::$variant => $suffix,)*
                }
            }

            /// The recording this claim rests on, if it rests on a measurement.
            #[cfg(test)]
            fn evidence(self) -> Option<&'static str> {
                match self {
                    $(Self::$variant => $evidence,)*
                }
            }
        }
    };
}

semantic_models! {
    AndIf => "and-if-v1", None;
    ExplicitCommand => "explicit-command-v1", None;
    StaticFunctionCall => "static-function-call-v1", None;
    ExplicitRedirection => "explicit-redirection-v1", None;
    ImmutableAssignment => "immutable-assignment-v1", None;
    LastExitCondition => "last-exit-condition-v1", None;
    StaticCondition => "static-condition-v1", None;
    StaticDoubleBracket => "static-double-bracket-v1", None;
    StaticEcho => "static-echo-v1", Some("contracts/golden/echo-builtin-semantics-v1.json");
    StaticEmptyArm => "static-empty-arm-v1", None;
    StaticExit => "static-exit-v1", Some("contracts/golden/exit-builtin-semantics-v1.json");
    StaticExitChecked => "static-exit-checked-v1", Some("contracts/golden/exit-builtin-semantics-v1.json");
    StaticExternalCommand => "static-external-command-v1", None;
    StaticMainSequence => "static-main-sequence-v1", None;
    StaticMatch => "static-match-v1", None;
    StaticNegation => "static-negation-v1", None;
    StaticPipeline => "static-pipeline-v1", None;
    StaticPrintf => "static-printf-v1", Some("contracts/golden/printf-builtin-semantics-v1.json");
    StaticSequence => "static-sequence-v1", None;
    StaticSequenceWithStatus => "static-sequence-with-status-v1", None;
    StaticTest => "static-test-v1", Some("contracts/golden/test-builtin-semantics-v1.json");
    StaticWhile => "static-while-v1", None;
}

/// A word a claim is written under that is not the interpreter's own name.
///
/// Two of these predate the vocabulary: `posix` where [`Interpreter::Sh`] is
/// called `sh`, and `nushell` where [`Interpreter::Nushell`] is called `nu`.
/// The spellings are quoted by recorded evidence and by approvals keyed on its
/// digest, so correcting them would invalidate what has been approved rather
/// than fix anything. Naming them here keeps them from being typed by hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ModelPrefix {
    Posix,
    Nushell,
}

impl ModelPrefix {
    fn word(self) -> &'static str {
        match self {
            Self::Posix => "posix",
            Self::Nushell => "nushell",
        }
    }
}

/// A `semantic_model`, which can only be built from the vocabulary.
///
/// A newtype rather than a `String` parameter: `native_node` took a `&str`, so
/// any caller could invent a claim, and the validator only asked that it not be
/// empty.
struct NativeBasis(String);

impl SemanticModel {
    /// The `semantic_model` an interpreter writes for this claim.
    fn named(self, interpreter: &Interpreter) -> NativeBasis {
        NativeBasis(format!("{}-{}", interpreter.name(), self.suffix()))
    }

    /// The same, under a word that is not the interpreter's own name.
    fn under(self, prefix: ModelPrefix) -> NativeBasis {
        NativeBasis(format!("{}-{}", prefix.word(), self.suffix()))
    }
}

/// A node that claims to behave the way the shell did, and says on what basis.
///
/// The basis is a [`SemanticModel`] rather than a string: a `&str` parameter
/// let a caller name a model that does not exist, and the validator only asked
/// that the name not be empty.
fn native_node(operation: Operation, basis: NativeBasis, span: SourceSpan) -> Node {
    // Destructured rather than read through a field: the newtype exists so that
    // the only way to reach this string is through the vocabulary.
    let NativeBasis(semantic_model) = basis;
    Node {
        id: String::new(),
        operation,
        guarantee: Guarantee::Native { semantic_model },
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

fn validate_nushell_syntax(path: &str, source: &str) -> Result<(), LoweringFailure> {
    let directory = tempfile::Builder::new()
        .prefix("deshell-nushell-parser-")
        .tempdir()
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let version = execute_parser_process(
        &parser_working_directory(directory.path()),
        vec!["nu".into(), "--version".into()],
        1024 * 1024,
    )
    .map_err(|failure| parser_failure(path, "nu-parser", failure))?;
    if version.stdout != b"0.115.1\n" && version.stdout != b"0.115.1\r\n" {
        return Err(LoweringFailure::Delegate(format!(
            "runtime unavailable for {path}: expected Nushell 0.115.1, found {}",
            String::from_utf8_lossy(&version.stdout).trim()
        )));
    }
    let source_path = directory.path().join("source.nu");
    crate::patch::scratch::write(&source_path, source.as_bytes())
        .map_err(|error| format!("runtime unavailable for {path}: {error}"))?;
    let parsed = execute_parser_process(
        &parser_working_directory(directory.path()),
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
    .map_err(|failure| parser_failure(path, "nu-parser", failure))?;
    for frame in parsed.stdout.split(|byte| *byte == b'\n') {
        let frame = frame.strip_suffix(b"\r").unwrap_or(frame);
        if frame.is_empty() {
            continue;
        }
        let diagnostic = crate::strict_json::parse(frame).map_err(|error| {
            format!("runtime unavailable for {path} (nu-parser output): {error}")
        })?;
        if diagnostic.get("type").and_then(serde_json::Value::as_str) != Some("diagnostic") {
            return Err(LoweringFailure::Delegate(format!(
                "runtime unavailable for {path}: nu-parser returned an unknown frame"
            )));
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
        return Err(LoweringFailure::Delegate(format!(
            "parse error in {path} from nu-parser/0.115.1 at bytes {start}..{end} ({message})"
        )));
    }
    Ok(())
}

/// Why a frontend could not lower a source.
///
/// Two different facts that used to be one `String` with one consequence. A
/// runtime that is not installed, or a source the interpreter rejects, is
/// knowledge: the block is delegated to the pinned interpreter and the reason
/// says so. A parser that ran out of its time or memory budget, or died, is not
/// knowledge — nobody looked — and delegating on it makes the guarantee a
/// function of how busy the machine was.
///
/// Measured here after the working-directory fix below let the PowerShell
/// parser actually run: on an idle machine the whole suite takes 37 seconds and
/// passes, and under load it takes 140 and the same sources come back
/// `delegated`. Same bytes, same de-shell, different answer about what was
/// proven. That is the thing the guarantee vocabulary exists to prevent, so a
/// budget that ran out is an error the caller sees rather than a quiet
/// downgrade.
enum LoweringFailure {
    /// The interpreter answered, or is not there to answer. Delegate, and say
    /// this as the reason.
    Delegate(String),
    /// Nobody measured. Fail instead of claiming anything about the source.
    Unmeasured(String),
}

impl From<String> for LoweringFailure {
    /// A plain message is a delegation: every `?` inside a parser path that is
    /// not the budget check below is a fact about the runtime or the source.
    fn from(message: String) -> Self {
        Self::Delegate(message)
    }
}

/// Name the file and the parser in a failure, without losing which of the two
/// kinds it is.
///
/// Wrapping with `format!` would have flattened both into one string, which is
/// how they came to share a consequence in the first place.
fn parser_failure(path: &str, parser: &str, failure: LoweringFailure) -> LoweringFailure {
    match failure {
        LoweringFailure::Delegate(message) => LoweringFailure::Delegate(format!(
            "runtime unavailable for {path} ({parser}): {message}"
        )),
        LoweringFailure::Unmeasured(message) => {
            LoweringFailure::Unmeasured(format!("{path} was not examined ({parser}): {message}"))
        }
    }
}

impl LoweringFailure {
    fn message(self) -> String {
        match self {
            Self::Delegate(message) | Self::Unmeasured(message) => message,
        }
    }
}

/// Whether a parser process ended without giving an answer.
///
/// A budget that ran out, or a process that died, used to share a branch with a
/// non-zero exit status, and the three together became one delegation. A
/// non-zero exit is the interpreter answering; these are it not answering.
fn unmeasured_outcome(outcome: &crate::agent_process::Outcome) -> Option<LoweringFailure> {
    if outcome.timed_out {
        return Some(LoweringFailure::Unmeasured(
            "parser process exceeded its time budget, so the source was not examined".into(),
        ));
    }
    if let Some(limit) = &outcome.limit_exceeded {
        return Some(LoweringFailure::Unmeasured(format!(
            "parser process exceeded its {limit} budget, so the source was not examined"
        )));
    }
    if let Some(signal) = outcome.signal {
        return Some(LoweringFailure::Unmeasured(format!(
            "parser process was killed by signal {signal}, so the source was not examined"
        )));
    }
    None
}

/// Where a parser process runs.
///
/// Not the scratch directory, which is what it was — not as a decision, but
/// because the scratch directory was the only directory in scope. It decided
/// which interpreter answered, and that made de-shell unable to use any
/// interpreter installed through a version manager: `mise`, `asdf` and `volta`
/// put a shim on `PATH` that resolves the version from the configuration file
/// nearest the working directory, and under the system temporary root there is
/// none. Measured on this machine with one `PATH`: `pwsh` answers `7.6.5` from
/// a project that declares it and `No version is set for shim: pwsh` from
/// `/tmp`.
///
/// de-shell then reported `runtime unavailable` and delegated the block. That
/// is the honest fallback for a runtime it cannot reach; the runtime was there,
/// so the report was true about what de-shell had measured and false about the
/// world. Four tests in this crate failed for it, and the failure was read as
/// the machine's fault twice before the directory was measured.
///
/// The rule now is the one already applied to `PATH`: de-shell does not choose
/// the environment that resolves an interpreter, it keeps the one it was
/// invoked with. Taking `PATH` from the invocation and the working directory
/// from `--root` would resolve the program from one place and its version from
/// another, which is neither.
///
/// Both parsers pass absolute paths for their scratch files and start the
/// interpreter with its configuration disabled (`-NoProfile`,
/// `--no-config-file --no-std-lib`), so this decides which interpreter runs and
/// not what it then reads.
fn parser_working_directory(scratch: &std::path::Path) -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| scratch.to_path_buf())
}

fn execute_parser_process(
    root: &std::path::Path,
    argv: Vec<String>,
    stdout_bytes: u64,
) -> Result<crate::agent_process::Outcome, LoweringFailure> {
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
    )
    .map_err(LoweringFailure::Delegate)?;
    if let Some(unmeasured) = unmeasured_outcome(&outcome) {
        return Err(unmeasured);
    }
    if outcome.exit_code != 0 || !outcome.stderr.is_empty() {
        return Err(LoweringFailure::Delegate(format!(
            "parser process failed: exit={} stderr={}",
            outcome.exit_code,
            String::from_utf8_lossy(&outcome.stderr)
        )));
    }
    Ok(outcome)
}

/// The PowerShell parser, started once and kept.
///
/// `adapters/powershell/adapter.ps1` has always been a loop over framed requests
/// on stdin. de-shell started one, sent one request and let it die, so every
/// parse paid a process start — 0.26 s alone, 4.6 s when sixteen ran at once,
/// because `pwsh` here resolves through a version-manager shim that serialises.
/// Under the test suite's parallelism that reached the ten-second budget, and
/// once a budget failure stopped being quietly delegated it became a visible
/// intermittent failure. It had been reaching the budget all along; what changed
/// was that somebody could see it.
///
/// A `Mutex` rather than a pool: the cost was starting, not parsing, so one
/// agent answering in turn is both faster than many and simpler to reason about.
/// A failed request drops the agent, so the next parse starts a fresh one.
static POWERSHELL_PARSER: std::sync::Mutex<Option<PowershellParser>> = std::sync::Mutex::new(None);

struct PowershellParser {
    agent: crate::agent_process::Agent,
    /// Holds `adapter.ps1` for as long as the agent runs.
    _directory: tempfile::TempDir,
}

/// How long one parse may take.
///
/// Unchanged. What changed is what it now measures: a parse, rather than a
/// parse plus a process start plus whatever a version manager was doing.
const POWERSHELL_PARSE_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

fn start_powershell_parser() -> Result<PowershellParser, String> {
    let directory = tempfile::Builder::new()
        .prefix("deshell-powershell-parser-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let adapter = directory.path().join("adapter.ps1");
    crate::patch::scratch::write(
        &adapter,
        include_bytes!("../../../adapters/powershell/adapter.ps1"),
    )
    .map_err(|error| error.to_string())?;
    let agent = crate::agent_process::Agent::start(
        &parser_working_directory(directory.path()),
        &[
            "pwsh".into(),
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-File".into(),
            adapter.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(PowershellParser {
        agent,
        _directory: directory,
    })
}

/// Ask the parser one question, starting it if it is not running.
///
/// Every failure drops the agent. A parser that stopped answering is not one to
/// ask again, and a fresh one costs a single process start.
fn ask_powershell_parser(request: &[u8]) -> Result<Vec<u8>, LoweringFailure> {
    let mut held = POWERSHELL_PARSER
        .lock()
        .map_err(|_| LoweringFailure::Delegate("the PowerShell parser lock is poisoned".into()))?;
    if held.is_none() {
        *held = Some(start_powershell_parser().map_err(LoweringFailure::Delegate)?);
    }
    let parser = held
        .as_mut()
        .ok_or_else(|| LoweringFailure::Delegate("the PowerShell parser is not running".into()))?;
    match parser.agent.request(request, POWERSHELL_PARSE_BUDGET) {
        Ok(line) => Ok(line),
        Err(error) => {
            *held = None;
            Err(agent_failure(error))
        }
    }
}

/// What an agent's failure means for a lowering.
///
/// The same split `unmeasured_outcome` makes for a one-shot process, stated for
/// a long-lived one: a budget that ran out is not an answer about the source,
/// and an agent that ended is the runtime being unavailable. A named function
/// rather than an inline `match`, so a test can ask it directly instead of a
/// source guard asking whether the right words appear in the right function.
fn agent_failure(error: crate::agent_process::AgentError) -> LoweringFailure {
    match error {
        crate::agent_process::AgentError::TimedOut => LoweringFailure::Unmeasured(
            "parser process exceeded its time budget, so the source was not examined".into(),
        ),
        crate::agent_process::AgentError::Ended(message) => LoweringFailure::Delegate(message),
    }
}

fn validate_powershell_syntax(path: &str, source: &str) -> Result<(), LoweringFailure> {
    let request = serde_json::json!({
        "id": "parse",
        "jsonrpc": "2.0",
        "method": "frontend.parse",
        "params": {"source": source}
    });
    let input = crate::canonical_json::canonical_bytes(&request).map_err(|error| {
        LoweringFailure::Delegate(format!("runtime unavailable for {path}: {error}"))
    })?;
    let frame = ask_powershell_parser(&input)
        .map_err(|failure| parser_failure(path, "PowerShell Parser.ParseInput", failure))?;
    if frame.is_empty() {
        return Err(LoweringFailure::Delegate(format!(
            "runtime unavailable for {path} (PowerShell Parser.ParseInput): the parser answered with an empty frame"
        )));
    }
    let frames = [frame.as_slice()];
    let result = crate::protocol::decode_response(frames[0], &serde_json::json!("parse")).map_err(
        |error| format!("runtime unavailable for {path} (PowerShell Parser.ParseInput): {error}"),
    )?;
    if result.get("parser").and_then(serde_json::Value::as_str)
        != Some("System.Management.Automation.Language.Parser")
    {
        return Err(LoweringFailure::Delegate(format!(
            "runtime unavailable for {path}: PowerShell adapter returned an unknown parser"
        )));
    }
    if result
        .get("runtime_version")
        .and_then(serde_json::Value::as_str)
        != Some("7.6.5")
    {
        return Err(LoweringFailure::Delegate(format!(
            "runtime unavailable for {path}: expected PowerShell 7.6.5 parser runtime"
        )));
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
    Err(LoweringFailure::Delegate(format!(
        "parse error in {path} from PowerShell Parser.ParseInput at bytes {start}..{end} ({message})"
    )))
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
fn if_arms(
    source: &str,
    statements: &[Range],
    start: usize,
) -> Option<(usize, usize, Option<usize>)> {
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

/// Names a shell answers from itself rather than from the environment.
///
/// A generated program resolves a name through the process environment, which
/// is right for `PATH` and wrong for `RANDOM`: every measured shell supplies a
/// value no environment carries, so the generated program reads nothing and
/// writes an empty string where the script wrote a number. Lowering one of
/// these as an ordinary expansion is a silent substitution, and it was being
/// claimed as native.
///
/// The union across bash, `/bin/sh`, zsh and dash, for the same reason the
/// builtin table is a union: delegating a name this shell does not supply costs
/// a delegation, while lowering one it does supply reads the wrong thing.
/// `contracts/golden/shell-variable-inventory-v1.json` holds the measurement
/// and `cargo xtask shell-variables` re-runs it.
const SHELL_SUPPLIED_VARIABLES: &[&str] = &[
    "BASH",
    "BASH_COMMAND",
    "BASH_SUBSHELL",
    "BASH_VERSION",
    "COLUMNS",
    "DIRSTACK",
    "EUID",
    "GROUPS",
    "HOSTNAME",
    "HOSTTYPE",
    "IFS",
    "LINENO",
    "LINES",
    "MACHTYPE",
    "OPTARG",
    "OPTIND",
    "OSTYPE",
    "PIPESTATUS",
    "PPID",
    "PS1",
    "PS2",
    "PS4",
    "RANDOM",
    "SECONDS",
    "UID",
];

/// The names PowerShell answers itself.
///
/// A script that writes `$ErrorActionPreference = 'Stop'` is not assigning a
/// value; it is changing how the script handles errors, and `$LASTEXITCODE`
/// decides what a later `exit` reports. Modelling either as a plain assignment
/// would carry the text and drop the meaning.
///
/// Measured with `Get-Variable` rather than listed from the documentation:
/// `contracts/golden/powershell-variable-inventory-v1.json` holds it and
/// `cargo xtask powershell-variables` re-runs it. A name absent here is the
/// script's own.
const POWERSHELL_SUPPLIED_VARIABLES: &[&str] = &[
    "$",
    "?",
    "^",
    "args",
    "ConfirmPreference",
    "DebugPreference",
    "EnabledExperimentalFeatures",
    "Error",
    "ErrorActionPreference",
    "ErrorView",
    "ExecutionContext",
    "false",
    "FormatEnumerationLimit",
    "HOME",
    "Host",
    "InformationPreference",
    "input",
    "IsCoreCLR",
    "IsLinux",
    "IsMacOS",
    "IsWindows",
    "LASTEXITCODE",
    "MaximumHistoryCount",
    "MyInvocation",
    "NestedPromptLevel",
    "null",
    "OutputEncoding",
    "PID",
    "PROFILE",
    "ProgressPreference",
    "PSBoundParameters",
    "PSCommandPath",
    "PSCulture",
    "PSDefaultParameterValues",
    "PSEdition",
    "PSEmailServer",
    "PSHOME",
    "PSNativeCommandArgumentPassing",
    "PSNativeCommandUseErrorActionPreference",
    "PSScriptRoot",
    "PSSessionApplicationName",
    "PSSessionConfigurationName",
    "PSSessionOption",
    "PSStyle",
    "PSUICulture",
    "PSVersionTable",
    "PWD",
    "ShellId",
    "StackTrace",
    "true",
    "VerbosePreference",
    "WarningPreference",
    "WhatIfPreference",
];

/// Whether PowerShell answers this name itself. Case-insensitive, because
/// PowerShell variable names are.
fn powershell_supplied_variable(name: &str) -> bool {
    POWERSHELL_SUPPLIED_VARIABLES
        .iter()
        .any(|supplied| supplied.eq_ignore_ascii_case(name))
}

/// Whether the shell answers this name itself.
fn shell_supplied_variable(name: &str) -> bool {
    SHELL_SUPPLIED_VARIABLES.contains(&name)
}

/// The name of the function a statement defines, if it defines one.
///
/// Only `name() {`, which is the form POSIX defines. `function name {` is a
/// bash and zsh extension, and the two differ over whether the body's
/// variables are local, so it is left alone.
fn function_name(text: &str) -> Option<&str> {
    let (name, rest) = text.split_once('(')?;
    let name = name.trim();
    let rest = rest.trim_start().strip_prefix(')')?.trim();
    if rest != "{" || !valid_identifier(name) {
        return None;
    }
    Some(name)
}

/// The statement index of the `}` that closes a function opened at `start`.
///
/// `None` for a definition inside another, so the file is delegated rather than
/// lowered from a guess about which brace closes which.
fn function_end(source: &str, statements: &[Range], start: usize) -> Option<usize> {
    for index in start + 1..statements.len() {
        let text = source[statements[index].start..statements[index].end].trim();
        if text == "}" {
            return Some(index);
        }
        if function_name(text).is_some() {
            return None;
        }
    }
    None
}

/// Split `PATTERN) BODY` into its patterns and its body.
///
/// An alternation (`a|b`) becomes several patterns sharing one body, which is
/// what the shell does with it. A pattern carrying an expansion or a glob, or a
/// missing `)`, yields `None`: matching the wrong arm runs the wrong command.
fn case_arm(text: &str) -> Option<(Vec<&str>, &str)> {
    let (patterns, body) = text.split_once(')')?;
    let patterns = patterns.trim().trim_start_matches('(').trim();
    if patterns.is_empty() {
        return None;
    }
    let patterns: Vec<&str> = patterns.split('|').map(str::trim).collect();
    if patterns.iter().any(|pattern| pattern.is_empty()) {
        return None;
    }
    Some((patterns, body.trim()))
}

/// Read a `case` pattern into pieces, resolving quoting as the shell does.
///
/// Quoting is settled during word expansion, before anything is matched, so a
/// `*` is a metacharacter exactly when it reaches the matcher unquoted.
/// Measured: `'a*c'` matches only `a*c`, `a\*c` matches only `a*c`, and
/// `"a"*"c"` matches both `abc` and `a*c` — so a flag saying whether the whole
/// pattern is a glob gets the third one wrong whichever way it guesses.
/// `contracts/golden/case-pattern-semantics-v1.json` records all three.
///
/// `None` for anything outside the model:
///
/// - `[...]`, because `[^a]` negates the set in bash and is the two-member set
///   `{^, a}` in dash. The two agree for `^bc` by opposite rules and part for
///   `bac`, so there is no single meaning to lower.
/// - An expansion, because what a `$x` holds at run time decides whether the
///   pattern has a metacharacter in it, and that is not knowable here.
/// - A backslash at the end, which is a line continuation rather than a quote.
/// - `(`, which opens an extglob list that three of the four shells refuse.
fn case_pattern(pattern: &str, interpreter: &Interpreter) -> Option<crate::ir::PatternExpression> {
    let mut pieces: Vec<crate::ir::PatternPiece> = Vec::new();
    let mut literal = String::new();
    let flush = |literal: &mut String, pieces: &mut Vec<crate::ir::PatternPiece>| {
        if !literal.is_empty() {
            pieces.push(crate::ir::PatternPiece::Literal {
                value: crate::ir::TextExpression::literal(std::mem::take(literal).as_str()),
            });
        }
    };
    let mut rest = pattern.chars().peekable();
    while let Some(character) = rest.next() {
        match character {
            // Quoted: every character up to the close is itself, including a
            // `*`. A single quote also protects a backslash.
            '\'' => loop {
                match rest.next()? {
                    '\'' => break,
                    quoted => literal.push(quoted),
                }
            },
            '"' => {
                loop {
                    match rest.next()? {
                        '"' => break,
                        // Inside double quotes a backslash quotes only a few
                        // characters; the rest keep the backslash. Refusing is
                        // narrower than modelling which is which.
                        '\\' => return None,
                        '$' | '`' => return None,
                        quoted => literal.push(quoted),
                    }
                }
            }
            '\\' => literal.push(rest.next()?),
            '*' => {
                flush(&mut literal, &mut pieces);
                pieces.push(crate::ir::PatternPiece::AnyRun);
            }
            '?' => {
                flush(&mut literal, &mut pieces);
                pieces.push(crate::ir::PatternPiece::AnyCharacter);
            }
            // `(` opens an extglob list — `@(a|b)`, which bash, `/bin/sh` and
            // dash refuse outright and zsh reads as a literal that matches
            // nothing. It is also where the arm splitter would cut: an arm
            // written `@(a|b))` has its first `)` inside the pattern.
            // `$'\n'` is a spelling, not a pattern feature: the word expander
            // turns it into a newline and what reaches the matcher is an
            // ordinary character. Measured, and bash-only — zsh drops the
            // backslash of an unknown escape where bash keeps it, and dash has
            // no such form at all and reads the whole thing literally, which is
            // how a line-break guard written this way accepts every input under
            // `sh` on Ubuntu.
            '$' if matches!(interpreter, Interpreter::Bash) && rest.peek() == Some(&'\'') => {
                rest.next();
                loop {
                    match rest.next()? {
                        '\'' => break,
                        '\\' => match rest.next()? {
                            'n' => literal.push('\n'),
                            't' => literal.push('\t'),
                            'r' => literal.push('\r'),
                            '\\' => literal.push('\\'),
                            '\'' => literal.push('\''),
                            // bash keeps the backslash and zsh drops it, so the
                            // sequence has no single meaning to lower.
                            _ => return None,
                        },
                        quoted => literal.push(quoted),
                    }
                }
            }
            '[' | '$' | '`' | '(' | ')' => return None,
            other => literal.push(other),
        }
    }
    flush(&mut literal, &mut pieces);
    // An empty pattern matches only the empty string, which `PatternExpression`
    // says with one empty literal rather than with no pieces at all.
    if pieces.is_empty() {
        pieces.push(crate::ir::PatternPiece::Literal {
            value: crate::ir::TextExpression::literal(""),
        });
    }
    Some(crate::ir::PatternExpression { pieces })
}

/// Decode the base64 a corpus stores a word in.
///
/// Shared with the generator tests, which check the compiled expressions
/// against the same recording this module's tests check the matcher against.
#[cfg(test)]
pub(crate) fn decode_base64(encoded: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0_u32;
    let mut count = 0_u32;
    let mut output = Vec::new();
    for byte in encoded.bytes() {
        if byte == b'=' {
            break;
        }
        let value = ALPHABET
            .iter()
            .position(|candidate| *candidate == byte)
            .expect("base64 alphabet");
        bits = (bits << 6) | u32::try_from(value).expect("six bits");
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push(u8::try_from((bits >> count) & 0xff).expect("one byte"));
        }
    }
    output
}

/// [`case_pattern`], for the generator tests that check the compiled
/// expressions against the same corpus this reads.
#[cfg(test)]
pub(crate) fn case_pattern_for_tests(pattern: &str) -> Option<crate::ir::PatternExpression> {
    case_pattern(pattern, &Interpreter::Bash)
}

/// Group the statements between `in` and `esac` into arms.
///
/// An arm is `PATTERN) BODY` ended by `;;`, and `BODY` can span any number of
/// statements. The returned range covers the whole body; `None` is an arm that
/// runs nothing. The last arm may omit its `;;`, which POSIX permits.
///
/// Returns `None` when a statement cannot belong to an arm — a body before any
/// pattern, a `;&` or `;;&` fallthrough this does not model — so the `case` is
/// delegated rather than lowered from a guess.
fn case_arms<'a>(source: &'a str, statements: &[Range]) -> Option<Vec<(Vec<&'a str>, Vec<Range>)>> {
    let mut arms: Vec<(Vec<&'a str>, Vec<Range>)> = Vec::new();
    let mut open: Option<(Vec<&'a str>, Vec<Range>)> = None;
    for statement in statements {
        let text = source[statement.start..statement.end].trim();
        if text.is_empty() {
            continue;
        }
        if text == ";;" {
            arms.push(open.take()?);
            continue;
        }
        match &mut open {
            // A `;&` or `;;&` arrives here as its own statement and has no `)`,
            // so `case_arm` rejects it and the fallthrough is delegated.
            None => {
                let (patterns, first) = case_arm(text)?;
                // The body is a list of statements, so the first one — which
                // shares a line with the pattern — is the first element rather
                // than the start of a range covering all of them. Covering them
                // with one range joined them into a single command.
                let mut body = Vec::new();
                if !first.is_empty() {
                    let start = statement.start
                        + source[statement.start..statement.end]
                            .find(first)
                            .unwrap_or_default();
                    body.push(Range {
                        start,
                        end: start + first.len(),
                    });
                }
                open = Some((patterns, body));
            }
            Some((_, body)) => body.push(Range {
                start: statement.start,
                end: statement.end,
            }),
        }
    }
    // POSIX lets the final arm omit its `;;`; `esac` ends it.
    if let Some(arm) = open.take() {
        arms.push(arm);
    }
    Some(arms)
}

/// One literal or conversion of a `printf` format string.
enum FormatPiece {
    /// Bytes written as they are, with the escapes already resolved.
    Literal(String),
    /// A `%s`, which writes one argument.
    String,
}

/// Split a `printf` format string into its pieces, or `None` if it holds a
/// conversion or an escape this does not model.
///
/// Narrow on purpose. `%d` rejects an argument that is not a number, `%b`
/// rescans its argument for escapes, and `%q` quotes for re-input — each is a
/// different function of the argument, so admitting them without modelling them
/// would write different bytes. An unknown escape is left as written by every
/// measured shell, but only because they all chose the same thing to do with a
/// sequence the standard leaves undefined, so it is refused rather than relied
/// on.
fn printf_format(format: &str) -> Option<Vec<FormatPiece>> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut rest = format.chars();
    while let Some(character) = rest.next() {
        match character {
            '\\' => match rest.next()? {
                'n' => literal.push('\n'),
                't' => literal.push('\t'),
                'r' => literal.push('\r'),
                '\\' => literal.push('\\'),
                _ => return None,
            },
            '%' => match rest.next()? {
                '%' => literal.push('%'),
                's' => {
                    if !literal.is_empty() {
                        pieces.push(FormatPiece::Literal(std::mem::take(&mut literal)));
                    }
                    pieces.push(FormatPiece::String);
                }
                _ => return None,
            },
            other => literal.push(other),
        }
    }
    if !literal.is_empty() {
        pieces.push(FormatPiece::Literal(literal));
    }
    Some(pieces)
}

/// What a shell ends with when `exit` is given something that is not a number.
///
/// Measured, per interpreter: bash and `/bin/sh` end with 255, zsh with 0. A
/// plan names the interpreter its source runs under, so there is no choosing
/// between them — the answer is that one's, and a caller reading `$?` sees what
/// it would have seen. `contracts/golden/exit-builtin-semantics-v1.json`
/// records it.
///
/// An interpreter this has not measured is refused rather than given a borrowed
/// answer.
fn non_numeric_exit_status(interpreter: &Interpreter) -> Result<u8, String> {
    match interpreter {
        Interpreter::Bash | Interpreter::Sh => Ok(255),
        Interpreter::Zsh => Ok(0),
        Interpreter::Fish
        | Interpreter::Powershell
        | Interpreter::Cmd
        | Interpreter::Nushell
        | Interpreter::Unknown(_) => Err(format!(
            "what {} does with a non-numeric exit status is not measured; the status requires pinned interpreter delegation",
            interpreter.name()
        )),
    }
}

/// The bytes `printf` writes for these arguments, if that is provable.
///
/// The format is reused until the arguments run out, and the last pass fills
/// the conversions it has no argument for with nothing. Both counts are known
/// here, so the passes are written out rather than looped: the result is one
/// expression whose bytes are the ones the shell would have written.
fn printf_contents(arguments: &[crate::ir::TextExpression]) -> Option<Vec<TextPart>> {
    let (format, values) = arguments.split_first()?;
    let format = literal_expression(format)?;
    let pieces = printf_format(&format)?;
    let conversions = pieces
        .iter()
        .filter(|piece| matches!(piece, FormatPiece::String))
        .count();
    // With no conversion the format is written once and the arguments are
    // ignored, which every measured shell does.
    let passes = match conversions {
        0 => 1,
        conversions => values.len().div_ceil(conversions).max(1),
    };

    let mut parts: Vec<TextPart> = Vec::new();
    // The canonical form has no two adjacent literals, and writing the passes
    // out creates them wherever a pass ends in one and the next begins in one.
    let mut push = |part: TextPart| match (parts.last_mut(), &part) {
        (Some(TextPart::Literal { value }), TextPart::Literal { value: next }) => {
            value.push_str(next);
        }
        _ => parts.push(part),
    };
    let mut next = 0_usize;
    for _ in 0..passes {
        for piece in &pieces {
            match piece {
                FormatPiece::Literal(value) => push(TextPart::Literal {
                    value: value.clone(),
                }),
                FormatPiece::String => {
                    if let Some(value) = values.get(next) {
                        for part in &value.parts {
                            push(part.clone());
                        }
                    }
                    next += 1;
                }
            }
        }
    }
    Some(parts)
}

/// The bytes bash's `echo` writes for these arguments, if that is provable.
///
/// `None` means the first argument could begin with `-`, so bash might read it
/// as `-n`, `-e` or `-E` instead of printing it. Everything after the first
/// argument is printed verbatim whatever it holds, because bash stops looking
/// for options at the first one that is not one.
fn echo_contents(arguments: &[crate::ir::TextExpression]) -> Option<Vec<TextPart>> {
    if let Some(first) = arguments.first() {
        // Bash reads the first argument as an option only when the whole of it
        // is `-` followed by nothing but `n`, `e` and `E`. Measured: `echo "$1
        // is bad"` with `$1` set to `-n` prints `-n is bad`, because `-n is
        // bad` is not an option.
        //
        // So a first argument is safe when it cannot spell one: either it
        // begins with a literal character that is not `-`, or one of its
        // literal parts carries a character an option cannot contain. Only an
        // argument that could still expand into an option — `echo "$1"` — is
        // refused.
        let begins_safely = matches!(
            first.parts.first(),
            Some(TextPart::Literal { value }) if !value.starts_with('-')
        );
        let carries_a_disqualifier = first.parts.iter().any(|part| match part {
            TextPart::Literal { value } => value
                .bytes()
                .any(|byte| !matches!(byte, b'-' | b'n' | b'e' | b'E')),
            TextPart::Variable { .. }
            | TextPart::Argument { .. }
            | TextPart::DefaultValue { .. } => false,
        });
        if !begins_safely && !carries_a_disqualifier {
            return None;
        }
    }
    let mut parts: Vec<TextPart> = Vec::new();
    // The canonical form has no two adjacent literals, and joining arguments
    // creates them: `echo a b` is `"a"`, `" "`, `"b"`, `"\n"` before merging.
    let mut push = |part: TextPart| match (parts.last_mut(), &part) {
        (Some(TextPart::Literal { value }), TextPart::Literal { value: next }) => {
            value.push_str(next);
        }
        _ => parts.push(part),
    };
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            push(TextPart::Literal { value: " ".into() });
        }
        for part in &argument.parts {
            push(part.clone());
        }
    }
    push(TextPart::Literal { value: "\n".into() });
    Some(parts)
}

/// The inputs of [`lower_statements`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to
/// this struct is a compile error at the call site rather than a default.
struct LowerStatementsArgs<'a> {
    path: &'a str,
    source: &'a str,
    /// The statements to walk, in source order.
    statements: &'a [Range],
    /// A keyword the first statement carries — `then`, `else`, `do` — or the
    /// empty string when it carries none.
    strip: &'a str,
    interpreter: &'a Interpreter,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a mut BTreeSet<String>,
    functions: &'a mut BTreeMap<String, usize>,
    tasks: &'a mut Vec<crate::ir::Task>,
    /// Shell options travel with the cursor, because a `set` applies from where
    /// it is written onwards.
    options: &'a mut ShellOptions,
    /// How many times `set -e` changed. A file that changes it partway through
    /// has no honest lowering, and the count is the file's rather than a list's.
    errexit_regions: &'a mut usize,
}

/// Lower a run of statements into the nodes they become.
///
/// One walk for the file, a branch, a loop body and a `case` arm. It was two:
/// the file's handled `if`, `while` and `case`, and the one used for bodies
/// handled a single command — so a `case` inside a function, or an `if` inside
/// a `case` arm, delegated for no reason but which walk reached it.
fn lower_statements(parts: LowerStatementsArgs<'_>) -> Result<Vec<Node>, String> {
    // Destructured without `..`: see `LowerStatementsArgs`.
    let LowerStatementsArgs {
        path,
        source,
        statements,
        strip,
        interpreter,
        inputs,
        environment,
        locals,
        functions,
        tasks,
        options,
        errexit_regions,
    } = parts;
    let statements = statements.to_vec();
    let mut nodes: Vec<Node> = Vec::new();
    let mut index = 0;
    while index < statements.len() {
        let range = statements[index];
        index += 1;
        let text = &source[range.start..range.end];
        // A branch's first statement carries the keyword that opened it —
        // `then`, `else`, `do`. Stripping it here is what lets every construct
        // below read a statement without knowing which arm it is in.
        let trimmed = text
            .trim()
            .strip_prefix(strip)
            .unwrap_or(text.trim())
            .trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let range = Range {
            start: range.start + text.find(trimmed).unwrap_or_default(),
            end: range.start + text.find(trimmed).unwrap_or_default() + trimmed.len(),
        };
        // `name() { BODY }` becomes a task of its own.
        if let Some(name) = function_name(trimmed) {
            let Some(close_at) = function_end(source, &statements, index - 1) else {
                return Err(
                    "shell function definition requires pinned interpreter delegation".into(),
                );
            };
            if functions.contains_key(name) {
                return Err(
                    "shell function is defined twice; delegation keeps the last one".into(),
                );
            }
            let mut body_inputs = BTreeSet::new();
            let mut body_environment = BTreeSet::new();
            let mut body_locals = BTreeSet::new();
            let body = lower_statement_list(LowerStatementListArgs {
                path,
                functions: &*functions,
                source,
                statements: &statements[index..close_at],
                strip: "",
                interpreter,
                inputs: &mut body_inputs,
                environment: &mut body_environment,
                locals: &mut body_locals,
                errexit: options.errexit,
                pipefail: options.pipefail,
            })?
            .ok_or("shell function body is empty")?;
            let arity = body_inputs
                .iter()
                .filter_map(|name| name.parse::<usize>().ok())
                .max()
                .unwrap_or(0);
            // The caller's environment is the function's, so what the body reads
            // is read by the file.
            environment.extend(body_environment.iter().cloned());
            functions.insert(name.to_owned(), arity);
            tasks.push(crate::ir::Task {
                name: name.to_owned(),
                inputs: (1..=arity)
                    .map(|position| Binding {
                        name: position.to_string(),
                        value_type: ValueType::Primitive(PrimitiveType::Text),
                    })
                    .collect(),
                outputs: vec![],
                environment: body_environment.into_iter().collect(),
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                nounset: false,
                invocation: None,
                body,
            });
            index = close_at + 1;
            continue;
        }
        // `while COND; do BODY; done`, rejoined the same way as `if`.
        if trimmed.starts_with("while ")
            && let Some((done_at, do_at)) = while_arms(source, &statements, index - 1)
        {
            let mut arm = |from: usize, to: usize, strip: &str| -> Result<Node, String> {
                lower_statement_list(LowerStatementListArgs {
                    path,
                    functions: &*functions,
                    source,
                    statements: &statements[from..to],
                    strip,
                    interpreter,
                    inputs: &mut *inputs,
                    environment: &mut *environment,
                    locals: &mut *locals,
                    errexit: options.errexit,
                    pipefail: options.pipefail,
                })?
                .ok_or_else(|| "loop arm is empty".to_owned())
            };
            let condition = arm(index - 1, do_at, "while ");
            let loop_body = arm(do_at, done_at, "do");
            if let (Ok(condition), Ok(loop_body)) = (condition, loop_body) {
                let span = span_for_range(
                    path,
                    source,
                    statements[index - 1].start,
                    statements[done_at].end,
                )?;
                nodes.push(native_node(
                    Operation::While {
                        condition: Box::new(condition),
                        body: Box::new(loop_body),
                    },
                    SemanticModel::StaticWhile.named(interpreter),
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
                &mut *inputs,
                &mut *environment,
                locals,
            );
            let mut cases = Vec::new();
            let mut default = None;
            let mut modelled = words.as_ref().is_ok_and(|words| words.len() == 1);
            // Grouping the arms is separate from lowering them: an arm's body
            // can be any number of statements, and the boundary is the `;;`
            // rather than the end of a line. Doing it in one pass made every
            // arm exactly one statement long, so an arm written across lines
            // delegated the whole `case`.
            match case_arms(source, &statements[index..esac_at]) {
                Some(arms) => {
                    for (patterns, body) in arms {
                        let lowered = lower_statement_list(LowerStatementListArgs {
                            path,
                            functions: &*functions,
                            source,
                            statements: &body,
                            strip: "",
                            interpreter,
                            inputs: &mut *inputs,
                            environment: &mut *environment,
                            locals: &mut *locals,
                            errexit: options.errexit,
                            pipefail: options.pipefail,
                        });
                        // `a|b) ;;` is a real arm that does nothing, and it is
                        // how a script says "these values are fine". Running
                        // nothing is the behaviour, not a gap in the model.
                        let node = match lowered {
                            Ok(Some(node)) => node,
                            Ok(None) => native_node(
                                Operation::NoOp,
                                SemanticModel::StaticEmptyArm.named(interpreter),
                                span_for_range(path, source, range.start, range.end)?,
                            ),
                            Err(_) => {
                                modelled = false;
                                break;
                            }
                        };
                        for pattern in patterns {
                            // A bare `*` is the default arm rather than a case
                            // that matches everything: the IR runs the default
                            // when no case matched, which is the same thing and
                            // is what `esac` with no `*` leaves undone.
                            if pattern == "*" {
                                default = Some(Box::new(node.clone()));
                                continue;
                            }
                            let Some(pattern) = case_pattern(pattern, interpreter) else {
                                modelled = false;
                                break;
                            };
                            cases.push(crate::ir::MatchCase {
                                pattern,
                                body: node.clone(),
                            });
                        }
                        if !modelled {
                            break;
                        }
                    }
                }
                None => modelled = false,
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
                    SemanticModel::StaticMatch.named(interpreter),
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
                lower_statement_list(LowerStatementListArgs {
                    path,
                    functions: &*functions,
                    source,
                    statements: &statements[from..to],
                    strip,
                    interpreter,
                    inputs: &mut *inputs,
                    environment: &mut *environment,
                    locals: &mut *locals,
                    errexit: options.errexit,
                    pipefail: options.pipefail,
                })?
                .ok_or_else(|| "branch is empty".to_owned())
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
                    SemanticModel::StaticCondition.named(interpreter),
                    span,
                ));
                index = fi_at + 1;
                continue;
            }
            return Err("shell compound syntax requires pinned interpreter delegation".into());
        }
        if let Some(updated) = set_statement(trimmed, *options) {
            // A `set` is a change of lowering state, not an operation: it emits no
            // node, and the statements after it carry its effect instead.
            if updated.errexit != options.errexit && !nodes.is_empty() {
                *errexit_regions += 1;
            }
            *options = updated;
            continue;
        }
        let node = lower_posix_control(LowerPosixControlArgs {
            path,
            functions: &*functions,
            source,
            range,
            interpreter,
            inputs: &mut *inputs,
            environment: &mut *environment,
            locals: &mut *locals,
            pipefail: options.pipefail,
        })?;
        nodes.push(node);
    }
    Ok(nodes)
}

/// The shell options the host has already set before the script's first line.
///
/// A GitHub workflow step is not run as a bare script. The runner writes the
/// `run:` text to a file and executes `bash -e {0}` — `set -e` is in effect
/// whether or not the step says so. de-shell read the text alone and lowered a
/// two-command step to `Sequence { on_failure: Continue }`, then called it
/// `native`: the step stops at the first failure and the replacement would not.
///
/// `pipefail` is the other half and is not knowable from here. The runner's
/// default is `bash -e {0}` without it, and an explicit `shell: bash` is
/// `bash --noprofile --norc -eo pipefail {0}` with it, and the scanner reports
/// both as `bash`. So `errexit` is stated and `pipefail` is not — see
/// `lower_posix`, which delegates a pipeline it cannot place.
fn host_shell_options(path: &str, host: HostShell) -> ShellOptions {
    if crate::migration::is_github_workflow_path(host_path_of(path)) {
        ShellOptions {
            errexit: true,
            nounset: false,
            // The runner's default is `bash -e {0}` and a named `bash` is
            // `bash --noprofile --norc -eo pipefail {0}`. Both are known once
            // the host says which it is; neither is a guess.
            pipefail: host.named,
        }
    } else {
        ShellOptions::default()
    }
}

fn lower_posix(
    path: &str,
    source: &str,
    interpreter: &Interpreter,
    host: HostShell,
) -> Result<Lowered, String> {
    let statements = shell_statements(source)?;
    let mut inputs = BTreeSet::new();
    let mut environment = BTreeSet::new();
    let mut locals = BTreeSet::new();
    // Shell options apply from where they are set onwards, so this travels with
    // the statement cursor rather than being read once for the file. It starts
    // from what the host has already set: see `host_shell_options`.
    let mut options = host_shell_options(path, host);
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
    // A function is a task, and its call sites are `TaskCall`s. The arity is
    // the highest `$N` its body reads, because a shell function's definition
    // does not state one — so a call that passes a different number is refused
    // rather than lowered with a `$2` that would be empty here and an error
    // under `set -u` there.
    let mut functions: BTreeMap<String, usize> = BTreeMap::new();
    let mut tasks: Vec<crate::ir::Task> = Vec::new();
    let mut nodes: Vec<Node> = lower_statements(LowerStatementsArgs {
        path,
        source,
        statements: &statements,
        strip: "",
        interpreter,
        inputs: &mut inputs,
        environment: &mut environment,
        locals: &mut locals,
        functions: &mut functions,
        tasks: &mut tasks,
        options: &mut options,
        errexit_regions: &mut errexit_regions,
    })?;
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
            SemanticModel::StaticSequence.named(interpreter),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        body,
        inputs,
        environment,
        tasks,
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
            // `;;`, `;;&` and `;&` end a `case` arm. Splitting on `;` alone drops
            // them, and with them the only record of where an arm's body ends —
            // an arm written across several lines became several statements with
            // no boundary between them. Each is emitted as a statement of its own
            // so the `case` lowering can read it; no other shell construct
            // contains them.
            let terminator = if byte != b';' {
                0
            } else if bytes[index..].starts_with(b";;&") {
                3
            } else if bytes[index..].starts_with(b";;") || bytes[index..].starts_with(b";&") {
                2
            } else {
                0
            };
            if terminator > 0 {
                output.push(Range {
                    start: index,
                    end: index + terminator,
                });
                index += terminator - 1;
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
/// The inputs of [`lower_statement_list`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to
/// this struct is a compile error at the call site rather than a default.
struct LowerStatementListArgs<'a> {
    path: &'a str,
    /// The functions defined so far, and how many positional arguments each
    /// body reads. A call to one becomes a task call rather than an exec of a
    /// program with that name.
    functions: &'a BTreeMap<String, usize>,
    source: &'a str,
    /// The statements that make up the list, in source order.
    statements: &'a [Range],
    /// A keyword the first statement of the list carries — `if `, `then`,
    /// `else` — or the empty string when the list has none.
    strip: &'a str,
    interpreter: &'a Interpreter,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a mut BTreeSet<String>,
    /// Whether `set -e` is in effect, which is what a sequence records.
    errexit: bool,
    /// Whether `set -o pipefail` is in effect at these statements.
    pipefail: bool,
}

/// Lower a run of statements into one node, or `None` if the run is empty.
///
/// A branch and a `case` arm are lists, not commands. Handing the whole byte
/// range to [`lower_posix_control`] instead joined the statements into a single
/// `Exec` — `echo a` followed by `echo b` became `echo a echo b` — and claimed
/// it as native, which is the shape of defect this tool exists to report.
fn lower_statement_list(parts: LowerStatementListArgs<'_>) -> Result<Option<Node>, String> {
    // Destructured without `..`: see `LowerStatementListArgs`.
    let LowerStatementListArgs {
        path,
        functions,
        source,
        statements,
        strip,
        interpreter,
        inputs,
        environment,
        locals,
        errexit,
        pipefail,
    } = parts;
    let mut options = ShellOptions {
        errexit,
        pipefail,
        nounset: false,
    };
    let mut errexit_regions = 0;
    // A definition inside a branch would be visible after the branch in the
    // shell and not here, so the walk is given a map it may read and a list it
    // may not add to without this noticing.
    let mut nested = functions.clone();
    let mut nested_tasks = Vec::new();
    let mut pieces = lower_statements(LowerStatementsArgs {
        path,
        source,
        statements,
        strip,
        interpreter,
        inputs,
        environment,
        locals,
        functions: &mut nested,
        tasks: &mut nested_tasks,
        options: &mut options,
        errexit_regions: &mut errexit_regions,
    })?;
    if !nested_tasks.is_empty() {
        return Err(
            "a function defined inside a branch outlives it and requires pinned interpreter delegation"
                .into(),
        );
    }
    if errexit_regions > 0 {
        return Err(
            "errexit changes partway through a branch and requires pinned interpreter delegation"
                .into(),
        );
    }
    let Some(first) = pieces.first() else {
        return Ok(None);
    };
    if pieces.len() == 1 {
        return Ok(Some(pieces.remove(0)));
    }
    let span = cover_spans(
        first.source.clone().ok_or("list span is missing")?,
        pieces
            .last()
            .and_then(|node| node.source.clone())
            .ok_or("list span is missing")?,
    );
    Ok(Some(native_node(
        Operation::Sequence {
            nodes: pieces,
            on_failure: if errexit {
                crate::ir::SequenceFailure::Stop
            } else {
                crate::ir::SequenceFailure::Continue
            },
        },
        SemanticModel::StaticSequence.named(interpreter),
        span,
    )))
}

struct LowerPosixControlArgs<'a> {
    path: &'a str,
    /// The functions defined so far, and how many positional arguments each
    /// body reads. A call to one becomes a task call rather than an exec of a
    /// program with that name.
    functions: &'a BTreeMap<String, usize>,
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
        functions,
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
            functions,
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
            functions,
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
            SemanticModel::StaticPipeline.under(ModelPrefix::Posix),
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
                    SemanticModel::AndIf.under(ModelPrefix::Posix),
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
        if byte == b'!'
            && !token_started
            && index > range.start
            && bytes
                .get(index + 1)
                .is_none_or(|next| next.is_ascii_whitespace())
        {
            // A `!` that opens the statement is handled by the caller as a
            // prefix; one appearing mid-statement is history expansion or an
            // operator this does not model.
            //
            // The reserved word is a complete token, so a `!` with a character
            // after it is part of a word instead. That is what `!=` is, and
            // reading it as a negation refused every `if [ "$a" != "b" ]` in
            // this repository's own workflows — the `!=` case has been in
            // `contracts/golden/test-builtin-semantics-v1.json` and modelled as
            // `TestPredicate::StringNotEqual` the whole time, and nothing
            // reached it.
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
    /// The functions defined so far, and how many positional arguments each
    /// body reads. A call to one becomes a task call rather than an exec of a
    /// program with that name.
    functions: &'a BTreeMap<String, usize>,
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
        functions,
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
                functions,
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
            SemanticModel::ImmutableAssignment.under(ModelPrefix::Posix),
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
            functions,
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
            SemanticModel::StaticNegation.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    // `[ ... ]` and `[[ ... ]]` have to be recognised before tokenizing, because
    // `[` and `]` are in the glob character set the tokenizer refuses. The
    // brackets are syntax here, not a pattern.
    let statement = source[range.start..range.end].trim();
    if let Some(inner) = statement
        .strip_prefix("[[ ")
        .and_then(|rest| rest.strip_suffix(" ]]"))
    {
        // `[[` is a keyword rather than a builtin: it does not split words or
        // expand globs in its operands, and `==` matches a pattern. The operand
        // text is read before tokenizing for the same reason as `[`.
        let offset = range.start
            + source[range.start..range.end]
                .find(inner)
                .ok_or("test operands are not inside their range")?;
        let predicate = double_bracket_predicate(DoubleBracketArgs {
            path,
            source,
            offset,
            text: inner,
            inputs,
            environment,
            locals,
        })?;
        return Ok(native_node(
            Operation::Test { predicate },
            SemanticModel::StaticDoubleBracket.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
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
            SemanticModel::StaticTest.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    let (command, raw_redirections) = split_redirections(&source[range.start..range.end])?;
    let mut redirections = Vec::new();
    for raw in raw_redirections {
        // The target is expanded the same way an argument is. A word that
        // expands to more than one field is a different redirection in the
        // shell — an ambiguous one, which bash refuses — so it is delegated.
        let path = |target: &str,
                    inputs: &mut BTreeSet<String>,
                    environment: &mut BTreeSet<String>|
         -> Result<crate::ir::TextExpression, String> {
            let mut words = tokenize_posix(target, inputs, environment, locals)?;
            if words.len() != 1 {
                return Err(
                    "redirection target is not one word and requires pinned interpreter delegation"
                        .into(),
                );
            }
            Ok(words.remove(0))
        };
        redirections.push(match raw {
            RawRedirection::Duplicate { fd, target_fd } => {
                crate::ir::Redirection::Duplicate { fd, target_fd }
            }
            RawRedirection::Read { fd, target } => crate::ir::Redirection::Read {
                fd,
                path: path(&target, inputs, environment)?,
            },
            RawRedirection::Write { fd, target, append } => crate::ir::Redirection::Write {
                fd,
                path: path(&target, inputs, environment)?,
                append,
            },
        });
    }
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
            SemanticModel::StaticTest.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    if executable == "echo" && matches!(interpreter, Interpreter::Bash) {
        // `echo` is a builtin, and the three builtins do not agree. Measured on
        // macOS: `/bin/sh` and `zsh` interpret backslash escapes in every
        // argument, `/bin/bash` interprets none; `/bin/sh` prints `-n` rather
        // than consuming it, and `zsh` prints nothing for a lone `-`.
        // `contracts/golden/echo-builtin-semantics-v1.json` records this.
        //
        // Bash's own rule is narrow enough to prove: options are read only from
        // the first argument, and without `-e` no argument is rescanned. So a
        // first argument that cannot begin with `-` makes the whole call
        // equivalent to writing the arguments joined by a space and a newline.
        // A first argument whose leading byte is not known statically — `echo
        // "$1"` — is refused, because `$1` may be `-n` at run time.
        if let Some(parts) = echo_contents(&words[1..]) {
            return Ok(native_node(
                Operation::WriteStdout {
                    contents: crate::ir::TextExpression { parts },
                },
                SemanticModel::StaticEcho.named(interpreter),
                span_for_range(path, source, range.start, range.end)?,
            ));
        }
        return Err("echo argument requires pinned interpreter delegation".into());
    }
    if executable == "exit" {
        // Measured on macOS: every shell reduces the status modulo 256, negatives
        // and values above 255 alike. They agree on nothing else — bash and
        // `/bin/sh` exit 255 and write a message naming the interpreter's own
        // path and a line number, while zsh exits 0 in silence — so a status this
        // cannot read statically has no lowering that is not one shell
        // impersonating another. `contracts/golden/exit-builtin-semantics-v1.json`
        // records the measurement.
        //
        // A bare `exit` ends with the last command's status, which the IR has no
        // term for, so it is delegated rather than reported as 0.
        if let [status] = &words[1..] {
            // A status the lowering can read is the whole claim; one that
            // arrives at run time makes the claim over the domain the shells
            // agree on, and the plan stops outside it rather than choosing a
            // shell to imitate.
            let (status, non_numeric, model) =
                match literal_expression(status) {
                    Some(literal) if literal.trim().parse::<i64>().is_ok() => (
                        crate::ir::TextExpression::literal(literal.trim()),
                        crate::ir::NonNumericStatus::Unreachable,
                        SemanticModel::StaticExit,
                    ),
                    // A literal that is not a number is where the shells part, and
                    // it is knowable here, so it is refused rather than deferred to
                    // a run that would stop anyway.
                    Some(_) => return Err(
                        "exit status is not a number and requires pinned interpreter delegation"
                            .into(),
                    ),
                    None => (
                        status.clone(),
                        crate::ir::NonNumericStatus::Ends {
                            status: non_numeric_exit_status(interpreter)?,
                        },
                        SemanticModel::StaticExitChecked,
                    ),
                };
            return Ok(native_node(
                Operation::Exit {
                    status,
                    non_numeric,
                },
                model.named(interpreter),
                span_for_range(path, source, range.start, range.end)?,
            ));
        }
        return Err("exit takes one status here and requires pinned interpreter delegation".into());
    }
    // A call to a function defined in this file is a task call, not an exec of
    // a program that happens to share its name. The arity is checked here
    // because a shell function has none: `$2` in the body is empty when the
    // call passed one argument, and an error when `set -u` is on, so a call
    // that does not line up has two behaviours rather than one.
    if let Some(arity) = functions.get(&executable) {
        if words.len() - 1 != *arity {
            return Err(format!(
                "call to {executable} passes {} argument(s) where its body reads {arity}; delegation keeps both meanings",
                words.len() - 1
            ));
        }
        return Ok(native_node(
            Operation::TaskCall {
                task: executable.clone(),
                arguments: Vec::new(),
                positional: words[1..].to_vec(),
            },
            SemanticModel::StaticFunctionCall.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    if executable == "printf"
        && let Some(parts) = printf_contents(&words[1..])
    {
        // Unlike `echo`, all four measured shells agree on `printf` — which is
        // why this is modelled for every interpreter rather than for bash alone.
        // `contracts/golden/printf-builtin-semantics-v1.json` records it.
        return Ok(native_node(
            Operation::WriteStdout {
                contents: crate::ir::TextExpression { parts },
            },
            SemanticModel::StaticPrintf.named(interpreter),
            span_for_range(path, source, range.start, range.end)?,
        ));
    }
    if let Some(treatment) = builtin_treatment(&executable) {
        // Reaching here means no branch above lowered the name, so whatever the
        // table says, this call is delegated. The reason says which of the two
        // reasons it is, because "this shell's builtin differs from the one that
        // is modelled" and "nobody has looked at this name" send a reader to
        // different places.
        return Err(match treatment {
            BuiltinTreatment::Modelled => format!(
                "shell builtin {executable} is modelled for another interpreter; {} requires pinned interpreter delegation",
                interpreter.name()
            ),
            BuiltinTreatment::Delegated => {
                format!("shell builtin {executable} requires pinned interpreter delegation")
            }
            BuiltinTreatment::Unexamined => format!(
                "shell builtin {executable} has no model yet and requires pinned interpreter delegation"
            ),
        });
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
        SemanticModel::ExplicitCommand.named(interpreter),
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
        SemanticModel::ExplicitRedirection.named(interpreter),
        span,
    ))
}

/// The inputs of [`double_bracket_predicate`].
struct DoubleBracketArgs<'a> {
    path: &'a str,
    source: &'a str,
    offset: usize,
    text: &'a str,
    inputs: &'a mut BTreeSet<String>,
    environment: &'a mut BTreeSet<String>,
    locals: &'a BTreeSet<String>,
}

/// Read the operands of `[[ ... ]]`.
///
/// `==` and `!=` take a pattern on the right. Only the three anchored shapes are
/// modelled — `p*`, `*s`, `*i*` — and a pattern with `?`, a bracket class or an
/// interior `*` leaves the statement delegated. The remaining operators are the
/// ones `[` has, read the same way.
fn double_bracket_predicate(
    parts: DoubleBracketArgs<'_>,
) -> Result<crate::ir::TestPredicate, String> {
    // Destructured without `..`: see `DoubleBracketArgs`.
    let DoubleBracketArgs {
        path,
        source,
        offset,
        text,
        inputs,
        environment,
        locals,
    } = parts;
    let _ = path;
    // The pattern operand is taken from the raw text rather than the tokenizer,
    // because `*` is exactly what the tokenizer refuses.
    let comparison = text
        .split_once(" == ")
        .map(|halves| (halves, true))
        .or_else(|| text.split_once(" != ").map(|halves| (halves, false)));
    if let Some(((left_text, right_text), equal)) = comparison {
        let left_words = tokenize_posix(left_text.trim(), inputs, environment, locals)?;
        let [value] = left_words.as_slice() else {
            return Err("double-bracket comparison needs one word on the left".into());
        };
        let pattern = right_text.trim().trim_matches('"');
        if let Some(predicate) = anchored_pattern(value, pattern) {
            if !equal {
                return Err("a negated pattern match is not modelled".into());
            }
            return Ok(predicate);
        }
        if pattern
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
        {
            return Err("unmodelled shell pattern requires pinned interpreter delegation".into());
        }
        let right_expression = crate::ir::TextExpression::literal(pattern);
        return Ok(if equal {
            crate::ir::TestPredicate::StringEqual {
                left: value.clone(),
                right: right_expression,
            }
        } else {
            crate::ir::TestPredicate::StringNotEqual {
                left: value.clone(),
                right: right_expression,
            }
        });
    }
    let operands = tokenize_posix(text, inputs, environment, locals).map_err(|_| {
        let _ = (source, offset);
        "double-bracket operands are outside the static subset".to_owned()
    })?;
    test_predicate(&operands)
        .ok_or_else(|| "unmodelled test operator requires pinned interpreter delegation".into())
}

/// A glob with exactly one anchor, as one of the three modelled predicates.
fn anchored_pattern(value: &TextExpression, pattern: &str) -> Option<crate::ir::TestPredicate> {
    let body = pattern.strip_prefix('*');
    let leading = body.is_some();
    let body = body.unwrap_or(pattern);
    let trailing = body.ends_with('*');
    let body = body.strip_suffix('*').unwrap_or(body);
    if body.is_empty()
        || body
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
    {
        return None;
    }
    match (leading, trailing) {
        (false, true) => Some(crate::ir::TestPredicate::StartsWith {
            value: value.clone(),
            prefix: body.to_owned(),
        }),
        (true, false) => Some(crate::ir::TestPredicate::EndsWith {
            value: value.clone(),
            suffix: body.to_owned(),
        }),
        (true, true) => Some(crate::ir::TestPredicate::Contains {
            value: value.clone(),
            infix: body.to_owned(),
        }),
        (false, false) => None,
    }
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

/// What the frontend does with a shell builtin.
///
/// Three values, not two. A builtin never reaches the shell's `PATH` lookup, so
/// lowering one to an `Exec` substitutes a different program — `which` is a zsh
/// builtin *and* a program in `/usr/bin`, and they do not answer the same way.
/// A name the table does not mention takes exactly that wrong path, silently.
///
/// So "read it and chose to delegate" is kept apart from "nobody has decided".
/// Both delegate, which is the safe side; separating them is what makes the
/// remaining work a value a gate can read rather than an absence nothing can
/// see. `cargo xtask builtin-table` asks the shells on the runner for their own
/// list and fails on a name this table does not answer for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuiltinTreatment {
    /// Lowered to an operation, for at least one interpreter, against a
    /// measurement in `contracts/golden/`.
    Modelled,
    /// Read, and handed to the pinned interpreter on purpose.
    Delegated,
    /// Nobody has decided. Delegated too, and named differently so that saying
    /// so is not the same as never having looked.
    Unexamined,
}

/// Every name a measured shell resolves as a builtin, and this frontend's
/// answer for it.
///
/// The union across `bash`, `/bin/sh` and `zsh` rather than one table per
/// interpreter: over-delegating a zsh builtin in a bash script costs a
/// delegation, while under-delegating runs a different program.
/// `contracts/golden/shell-builtin-inventory-v1.json` holds the same answers
/// and the measurement they came from, and a test compares the two.
const SHELL_BUILTINS: &[(&str, BuiltinTreatment)] = &[
    ("-", BuiltinTreatment::Unexamined),
    (".", BuiltinTreatment::Unexamined),
    (":", BuiltinTreatment::Unexamined),
    ("[", BuiltinTreatment::Modelled),
    ("alias", BuiltinTreatment::Unexamined),
    ("autoload", BuiltinTreatment::Unexamined),
    ("bg", BuiltinTreatment::Unexamined),
    ("bind", BuiltinTreatment::Unexamined),
    ("bindkey", BuiltinTreatment::Unexamined),
    ("break", BuiltinTreatment::Unexamined),
    ("builtin", BuiltinTreatment::Unexamined),
    ("bye", BuiltinTreatment::Unexamined),
    ("caller", BuiltinTreatment::Unexamined),
    ("cd", BuiltinTreatment::Unexamined),
    ("chdir", BuiltinTreatment::Unexamined),
    ("command", BuiltinTreatment::Unexamined),
    ("compadd", BuiltinTreatment::Unexamined),
    ("comparguments", BuiltinTreatment::Unexamined),
    ("compcall", BuiltinTreatment::Unexamined),
    ("compctl", BuiltinTreatment::Unexamined),
    ("compdescribe", BuiltinTreatment::Unexamined),
    ("compfiles", BuiltinTreatment::Unexamined),
    ("compgen", BuiltinTreatment::Unexamined),
    ("compgroups", BuiltinTreatment::Unexamined),
    ("complete", BuiltinTreatment::Unexamined),
    ("compquote", BuiltinTreatment::Unexamined),
    ("compset", BuiltinTreatment::Unexamined),
    ("comptags", BuiltinTreatment::Unexamined),
    ("comptry", BuiltinTreatment::Unexamined),
    ("compvalues", BuiltinTreatment::Unexamined),
    ("continue", BuiltinTreatment::Unexamined),
    ("declare", BuiltinTreatment::Unexamined),
    ("dirs", BuiltinTreatment::Unexamined),
    ("disable", BuiltinTreatment::Unexamined),
    ("disown", BuiltinTreatment::Unexamined),
    ("echo", BuiltinTreatment::Modelled),
    ("echotc", BuiltinTreatment::Unexamined),
    ("echoti", BuiltinTreatment::Unexamined),
    ("emulate", BuiltinTreatment::Unexamined),
    ("enable", BuiltinTreatment::Unexamined),
    ("eval", BuiltinTreatment::Unexamined),
    ("exec", BuiltinTreatment::Unexamined),
    ("exit", BuiltinTreatment::Modelled),
    ("export", BuiltinTreatment::Unexamined),
    ("false", BuiltinTreatment::Unexamined),
    ("fc", BuiltinTreatment::Unexamined),
    ("fg", BuiltinTreatment::Unexamined),
    ("float", BuiltinTreatment::Unexamined),
    ("functions", BuiltinTreatment::Unexamined),
    ("getln", BuiltinTreatment::Unexamined),
    ("getopts", BuiltinTreatment::Unexamined),
    ("hash", BuiltinTreatment::Unexamined),
    ("help", BuiltinTreatment::Unexamined),
    ("history", BuiltinTreatment::Unexamined),
    ("integer", BuiltinTreatment::Unexamined),
    ("jobs", BuiltinTreatment::Unexamined),
    ("kill", BuiltinTreatment::Unexamined),
    ("let", BuiltinTreatment::Unexamined),
    ("limit", BuiltinTreatment::Unexamined),
    ("local", BuiltinTreatment::Unexamined),
    ("log", BuiltinTreatment::Unexamined),
    ("logout", BuiltinTreatment::Unexamined),
    ("noglob", BuiltinTreatment::Unexamined),
    ("popd", BuiltinTreatment::Unexamined),
    ("print", BuiltinTreatment::Unexamined),
    ("printf", BuiltinTreatment::Modelled),
    ("private", BuiltinTreatment::Unexamined),
    ("pushd", BuiltinTreatment::Unexamined),
    ("pushln", BuiltinTreatment::Unexamined),
    ("pwd", BuiltinTreatment::Unexamined),
    ("r", BuiltinTreatment::Unexamined),
    ("read", BuiltinTreatment::Unexamined),
    ("readonly", BuiltinTreatment::Unexamined),
    ("rehash", BuiltinTreatment::Unexamined),
    ("return", BuiltinTreatment::Unexamined),
    ("sched", BuiltinTreatment::Unexamined),
    ("set", BuiltinTreatment::Delegated),
    ("setopt", BuiltinTreatment::Unexamined),
    ("shift", BuiltinTreatment::Unexamined),
    ("shopt", BuiltinTreatment::Unexamined),
    ("source", BuiltinTreatment::Unexamined),
    ("suspend", BuiltinTreatment::Unexamined),
    ("test", BuiltinTreatment::Modelled),
    ("times", BuiltinTreatment::Unexamined),
    ("trap", BuiltinTreatment::Unexamined),
    ("true", BuiltinTreatment::Unexamined),
    ("ttyctl", BuiltinTreatment::Unexamined),
    ("type", BuiltinTreatment::Unexamined),
    ("typeset", BuiltinTreatment::Unexamined),
    ("ulimit", BuiltinTreatment::Unexamined),
    ("umask", BuiltinTreatment::Unexamined),
    ("unalias", BuiltinTreatment::Unexamined),
    ("unfunction", BuiltinTreatment::Unexamined),
    ("unhash", BuiltinTreatment::Unexamined),
    ("unlimit", BuiltinTreatment::Unexamined),
    ("unset", BuiltinTreatment::Unexamined),
    ("unsetopt", BuiltinTreatment::Unexamined),
    ("vared", BuiltinTreatment::Unexamined),
    ("wait", BuiltinTreatment::Unexamined),
    ("whence", BuiltinTreatment::Unexamined),
    ("where", BuiltinTreatment::Unexamined),
    ("which", BuiltinTreatment::Unexamined),
    ("zcompile", BuiltinTreatment::Unexamined),
    ("zformat", BuiltinTreatment::Unexamined),
    ("zle", BuiltinTreatment::Unexamined),
    ("zmodload", BuiltinTreatment::Unexamined),
    ("zparseopts", BuiltinTreatment::Unexamined),
    ("zregexparse", BuiltinTreatment::Unexamined),
    ("zstyle", BuiltinTreatment::Unexamined),
];

/// This frontend's answer for a name, or `None` if no measured shell resolves
/// it as a builtin.
fn builtin_treatment(executable: &str) -> Option<BuiltinTreatment> {
    SHELL_BUILTINS
        .iter()
        .find(|(name, _)| *name == executable)
        .map(|(_, treatment)| *treatment)
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
            SemanticModel::StaticSequence.named(&Interpreter::Fish),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        // Only the POSIX frontend reads a function definition.
        tasks: Vec::new(),
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
        .map(|piece| {
            lower_fish_simple(LowerFishSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
            })
        })
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
            SemanticModel::AndIf.named(&Interpreter::Fish),
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
        SemanticModel::StaticExternalCommand.named(&Interpreter::Fish),
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
            SemanticModel::StaticSequence.named(&Interpreter::Cmd),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        // Only the POSIX frontend reads a function definition.
        tasks: Vec::new(),
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
        .map(|piece| {
            lower_cmd_simple(LowerCmdSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
            })
        })
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
            SemanticModel::AndIf.named(&Interpreter::Cmd),
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
        SemanticModel::StaticExternalCommand.named(&Interpreter::Cmd),
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
    let mut locals = BTreeSet::new();
    let mut nodes = Vec::new();
    let mut terminal_status_span = None;
    let range_count = ranges.len();
    for (index, range) in ranges.into_iter().enumerate() {
        let raw = source[range.start..range.end].trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        if powershell_declaration(raw) {
            // A declaration, not a statement: it runs nothing.
            //
            // Measured with pwsh 7.6.5: a script carrying `[CmdletBinding()]`
            // and an empty `param()` behaves as one without them when it is
            // called with no arguments, and rejects a positional argument that
            // the same script without them would leave in `$args`. Nothing here
            // claims anything about that second case — the IR has no term for
            // "an unexpected argument is an error", so what the declarations buy
            // is only that they stop the file at the first line they appear on.
            //
            // A `param(...)` that declares parameters is a different thing and
            // is still refused: those are the task's inputs, with types.
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
            locals: &mut locals,
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
            SemanticModel::StaticSequenceWithStatus.named(&Interpreter::Powershell),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        // Only the POSIX frontend reads a function definition.
        tasks: Vec::new(),
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
    locals: &'a mut BTreeSet<String>,
}

/// Whether this statement declares something rather than running it.
///
/// `[CmdletBinding()]` and an empty `param()` open five of the six PowerShell
/// files in this repository and were the first thing every one of them was
/// refused for, because both carry parentheses and the `&&` splitter refuses
/// those. Neither runs anything.
///
/// Only the empty `param()`. A `param($Name)` declares the task's inputs and
/// their types, which is a separate piece of work and is still refused rather
/// than skipped — skipping it would drop the arity and leave a script that
/// reads `$Name` reading nothing.
fn powershell_declaration(statement: &str) -> bool {
    let statement = statement.trim();
    if statement.eq_ignore_ascii_case("param()") {
        return true;
    }
    // `$ErrorActionPreference = <literal>` reaches nothing this frontend
    // lowers. Measured: a failing external command is unaffected by it and a
    // failing cmdlet is not, so skipping it is sound exactly while every cmdlet
    // is refused —
    // `a_powershell_cmdlet_is_refused_which_is_what_skipping_the_preference_rests_on`
    // is that invariant and
    // `contracts/golden/powershell-preference-semantics-v1.json` is the
    // measurement.
    if let Some((name, value)) = statement.split_once('=')
        && name.trim().eq_ignore_ascii_case("$ErrorActionPreference")
        && powershell_literal_string(value.trim()).is_some()
    {
        return true;
    }
    let Some(inner) = statement
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return false;
    };
    // An attribute with no arguments of its own, such as `[CmdletBinding()]`.
    inner
        .strip_suffix("()")
        .is_some_and(|name| !name.is_empty() && name.chars().all(char::is_alphanumeric))
}

fn lower_powershell_control(parts: LowerPowershellControlArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPowershellControlArgs`.
    let LowerPowershellControlArgs {
        path,
        source,
        range,
        inputs,
        environment,
        locals,
    } = parts;
    let controls = powershell_and_controls(source, range)?;
    if controls.is_empty() {
        return lower_powershell_simple(LowerPowershellSimpleArgs {
            path,
            source,
            range,
            inputs,
            environment,
            locals,
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
        .map(|piece| {
            lower_powershell_simple(LowerPowershellSimpleArgs {
                path,
                source,
                range: piece,
                inputs,
                environment,
                locals,
            })
        })
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
            SemanticModel::AndIf.named(&Interpreter::Powershell),
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
    /// The names this script has assigned. A `$name` that is not here is one
    /// nothing in the script set, so reading it would be reading whatever the
    /// session happened to hold.
    locals: &'a mut BTreeSet<String>,
}

/// The inputs of [`lower_powershell_assignment`].
struct LowerPowershellAssignmentArgs<'a> {
    path: &'a str,
    source: &'a str,
    range: Range,
    locals: &'a mut BTreeSet<String>,
}

/// `$name = <literal>`, if that is what this statement is.
///
/// `Ok(None)` means the statement is not an assignment and the caller carries
/// on; an assignment this cannot model is an `Err`, because taking the part it
/// understands and dropping the rest is how a substitution becomes silent.
///
/// A name PowerShell answers itself is refused: `$ErrorActionPreference = 'Stop'`
/// changes how the script handles errors and `$LASTEXITCODE` decides what a
/// later `exit` reports, and neither is a value the IR can carry as text. See
/// `POWERSHELL_SUPPLIED_VARIABLES`.
fn lower_powershell_assignment(
    parts: LowerPowershellAssignmentArgs<'_>,
) -> Result<Option<Node>, String> {
    let LowerPowershellAssignmentArgs {
        path,
        source,
        range,
        locals,
    } = parts;
    let statement = source[range.start..range.end].trim();
    let Some(rest) = statement.strip_prefix('$') else {
        return Ok(None);
    };
    let Some((name, value)) = rest.split_once('=') else {
        return Ok(None);
    };
    let name = name.trim();
    let value = value.trim();
    if !valid_identifier(name) {
        return Ok(None);
    }
    if powershell_supplied_variable(name) {
        // `$ErrorActionPreference` never reaches here: it is a declaration, and
        // `powershell_declaration` takes it before a statement is lowered.
        // Every other supplied name is refused, because carrying it as text
        // would drop what it means.
        return Err(format!(
            "PowerShell answers ${name} itself, so assigning it is not a value this can carry"
        ));
    }
    let literal = powershell_literal_string(value)
        .ok_or_else(|| format!("PowerShell assignment to ${name} is not a literal string"))?;
    locals.insert(name.to_owned());
    Ok(Some(native_node(
        Operation::SetVariable {
            name: name.to_owned(),
            value_type: crate::ir::ValueType::Primitive(crate::ir::PrimitiveType::Text),
            value: TextExpression::literal(literal),
        },
        SemanticModel::ImmutableAssignment.named(&Interpreter::Powershell),
        span_for_range(path, source, range.start, range.end)?,
    )))
}

/// The text a quoted PowerShell string holds, if it holds only text.
///
/// A single-quoted string is literal. A double-quoted one interpolates, so one
/// holding `$` or a backtick is not text and is refused rather than read as if
/// it were.
fn powershell_literal_string(value: &str) -> Option<&str> {
    let inner = value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .filter(|inner| !inner.contains(['$', '`']))
        })?;
    (!inner.contains(['\'', '"'])).then_some(inner)
}

fn lower_powershell_simple(parts: LowerPowershellSimpleArgs<'_>) -> Result<Node, String> {
    // Destructured without `..`: see `LowerPowershellSimpleArgs`.
    let LowerPowershellSimpleArgs {
        path,
        source,
        range,
        inputs,
        environment,
        locals,
    } = parts;
    // `$name = <literal>` is an assignment, not an invocation. Measured with
    // pwsh 7.6.5: a value holding a space stays one argument when the variable
    // is passed on, so the text travels whole and nothing splits it.
    if let Some(node) = lower_powershell_assignment(LowerPowershellAssignmentArgs {
        path,
        source,
        range,
        locals,
    })? {
        return Ok(node);
    }
    let words = tokenize_powershell(&source[range.start..range.end], inputs, environment, locals)?;
    let first = literal_expression(words.first().ok_or("PowerShell command is empty")?);
    // `& 'path' args` and `./path args` are the same invocation. Measured with
    // pwsh 7.6.5: `./echo.ps1 a b`, `.\echo.ps1 a b` and `& './echo.ps1' a b`
    // all print `args=a,b` and leave the same status, and a bare `echo.ps1 a b`
    // is not recognised at all — which is why a path prefix is required here
    // rather than any first word.
    //
    // Nine of this repository's own blockers were
    // `run: ./scripts/install-nushell.ps1`, refused for being the second form.
    let argv_start = match first.as_deref() {
        Some("&") => 1,
        Some(word) if powershell_path_command(word) => 0,
        _ => {
            return Err(
                "PowerShell command is neither a call-operator invocation nor a path".into(),
            );
        }
    };
    if words.len() <= argv_start {
        return Err("PowerShell call operator names no command".into());
    }
    literal_expression(&words[argv_start])
        .filter(|value| !value.is_empty())
        .ok_or("dynamic PowerShell executable requires pinned interpreter delegation")?;
    Ok(native_node(
        Operation::Exec {
            argv: words[argv_start..].to_vec(),
            environment: Vec::new(),
            working_directory: None,
        },
        SemanticModel::StaticExternalCommand.named(&Interpreter::Powershell),
        span_for_range(path, source, range.start, range.end)?,
    ))
}

/// Whether this word invokes a command by naming its path.
///
/// PowerShell resolves a command name that carries a path separator as a path
/// and anything else through its command table, so `./build.ps1` runs the file
/// and `build.ps1` does not — measured, not assumed. A drive-qualified or
/// absolute path is the same case.
fn powershell_path_command(word: &str) -> bool {
    word.starts_with("./")
        || word.starts_with(".\\")
        || word.starts_with("../")
        || word.starts_with("..\\")
        || word.starts_with('/')
        || word.starts_with('\\')
        || word
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && word.as_bytes()[0].is_ascii_alphabetic())
}

fn tokenize_powershell(
    source: &str,
    inputs: &mut BTreeSet<String>,
    environment: &mut BTreeSet<String>,
    locals: &BTreeSet<String>,
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
            } else if let Some(name) = value.strip_prefix('$')
                && valid_identifier(name)
                && locals.contains(name)
            {
                // A whole argument that is a variable the script assigned.
                // Measured with pwsh 7.6.5: a value holding a space arrives as
                // one argument, so nothing splits and the text travels whole.
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
        SemanticModel::LastExitCondition.under(ModelPrefix::Nushell),
        span_for_range(path, source, lines[2].0.start, lines[7].0.end)?,
    );
    let span = cover_spans(
        first.source.clone().unwrap(),
        condition.source.clone().unwrap(),
    );
    Ok(Lowered {
        // Only the POSIX frontend reads a function definition.
        tasks: Vec::new(),
        nounset: false,
        body: native_node(
            Operation::Sequence {
                nodes: vec![first, condition],
                on_failure: crate::ir::SequenceFailure::Continue,
            },
            SemanticModel::StaticMainSequence.under(ModelPrefix::Nushell),
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
        SemanticModel::StaticExternalCommand.under(ModelPrefix::Nushell),
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
/// A redirection whose target has not been expanded yet.
///
/// Splitting runs before the tokenizer, because finding the operators means
/// knowing where the quotes are. The target is a word like any other and is
/// expanded where the rest of the command is — which is what lets `>>"${OUT}"`
/// lower, the form a workflow uses to append to `$GITHUB_OUTPUT`.
enum RawRedirection {
    Read {
        fd: u32,
        target: String,
    },
    Write {
        fd: u32,
        target: String,
        append: bool,
    },
    Duplicate {
        fd: u32,
        target_fd: u32,
    },
}

fn split_redirections(source: &str) -> Result<(String, Vec<RawRedirection>), String> {
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
            && bytes
                .get(index + 1)
                .is_some_and(|next| matches!(next, b'>' | b'<'))
            && !command
                .last()
                .is_some_and(|last| !last.is_ascii_whitespace())
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
            redirections.push(RawRedirection::Duplicate {
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
        let target = source[start..cursor].to_owned();
        redirections.push(if operator == b'<' {
            RawRedirection::Read { fd, target }
        } else {
            RawRedirection::Write { fd, target, append }
        });
        index = cursor;
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    let command =
        String::from_utf8(command).map_err(|_| "command is not valid UTF-8".to_owned())?;
    Ok((command, redirections))
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
                && !shell_supplied_variable(name)
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
        // A name the shell answers from itself is not an environment variable,
        // and lowering it as one reads nothing: `${RANDOM}` becomes an empty
        // string where the script had a number. A local of the same name shadows
        // the shell's, which is why the check comes first.
        if !locals.contains(name) && shell_supplied_variable(name) {
            return Err(format!(
                "{name} is supplied by the shell rather than the environment and requires pinned interpreter delegation"
            ));
        }
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
            SemanticModel::StaticExternalCommand.named(interpreter),
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
            SemanticModel::StaticSequence.named(interpreter),
            cover_spans(first, last),
        )
    };
    Ok(Lowered {
        // Only the POSIX frontend reads a function definition.
        tasks: Vec::new(),
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

    /// A budget that ran out is not a delegation.
    ///
    /// The two were one branch: a non-zero exit, a signal, a timeout and a
    /// memory limit all became `runtime unavailable` and the block was
    /// delegated. A non-zero exit is the interpreter answering; the other three
    /// are it not answering, and delegating on them makes the guarantee a
    /// function of how busy the machine was.
    ///
    /// This is not hypothetical. Once the working-directory fix let the
    /// PowerShell parser actually run, the suite took 37 seconds idle and 140
    /// under load, and under load the same sources came back delegated. Same
    /// bytes, same de-shell, different claim about what was proven.
    #[test]
    fn budget_failures_are_not_delegations() {
        let answered = crate::agent_process::Outcome {
            exit_code: 1,
            stdout: Vec::new(),
            stderr: b"syntax error".to_vec(),
            timed_out: false,
            limit_exceeded: None,
            signal: None,
        };
        assert!(
            unmeasured_outcome(&answered).is_none(),
            "a non-zero exit is the interpreter answering"
        );
        assert!(
            unmeasured_outcome(&crate::agent_process::Outcome {
                exit_code: 0,
                timed_out: true,
                ..answered.clone()
            })
            .is_some()
        );
        assert!(
            unmeasured_outcome(&crate::agent_process::Outcome {
                exit_code: 0,
                limit_exceeded: Some("memory".into()),
                ..answered.clone()
            })
            .is_some()
        );
        assert!(
            unmeasured_outcome(&crate::agent_process::Outcome {
                exit_code: 0,
                signal: Some(9),
                ..answered
            })
            .is_some()
        );
    }

    /// Both parser paths classify a budget failure the same way.
    ///
    /// There are two, because one parser is a one-shot process and the other is
    /// a long-lived agent, and each states the rule for its own shape. Asked
    /// behaviourally rather than by reading the source for the right words: this
    /// test used to check that both functions mentioned `unmeasured_outcome`,
    /// and when the PowerShell path moved to an agent the guard failed for the
    /// right reason and said nothing useful about whether the rule still held.
    #[test]
    fn both_parser_paths_route_a_budget_failure_away_from_delegation() {
        assert!(matches!(
            agent_failure(crate::agent_process::AgentError::TimedOut),
            LoweringFailure::Unmeasured(_)
        ));
        assert!(matches!(
            agent_failure(crate::agent_process::AgentError::Ended("gone".into())),
            LoweringFailure::Delegate(_)
        ));
        assert!(matches!(
            unmeasured_outcome(&crate::agent_process::Outcome {
                exit_code: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: true,
                limit_exceeded: None,
                signal: None,
            }),
            Some(LoweringFailure::Unmeasured(_))
        ));
    }

    /// An agent that does not answer within its budget times out rather than
    /// waiting forever.
    #[test]
    fn an_agent_that_does_not_answer_runs_out_of_its_budget() {
        let directory = tempfile::tempdir().unwrap();
        let mut agent = crate::agent_process::Agent::start(
            directory.path(),
            &["sh".into(), "-c".into(), "sleep 30".into()],
        )
        .unwrap();
        let started = std::time::Instant::now();
        let answer = agent.request(b"{}", std::time::Duration::from_millis(250));
        assert!(matches!(
            answer,
            Err(crate::agent_process::AgentError::TimedOut)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// `!=` is an operator, not the negation reserved word.
    ///
    /// The statement splitter read any `!` starting a token as the reserved
    /// word, so `[ "$a" != "b" ]` was refused as "POSIX negation remains
    /// delegated" — and with it every `if [ "$a" != "b" ]` in this repository's
    /// own workflows. `TestPredicate::StringNotEqual` has been in the IR and
    /// `string-not-equal` in
    /// `contracts/golden/test-builtin-semantics-v1.json` the whole time, and
    /// nothing reached them.
    ///
    /// The reserved word is a complete token, so a `!` with a character after
    /// it is part of a word. `[ ! -f x ]` is still refused, because there the
    /// `!` is a complete token and an argument to `[` rather than a pipeline
    /// negation.
    #[test]
    fn a_bang_followed_by_a_character_is_part_of_a_word() {
        let predicate = |source: &str| {
            let lowered = lower_posix(
                ".github/workflows/ci.yml.deshell.sh",
                source,
                &Interpreter::Bash,
                HostShell::default(),
            )?;
            let Operation::Condition { predicate, .. } = lowered.body.operation else {
                panic!("an if is a condition");
            };
            let Operation::Test { predicate } = predicate.operation else {
                panic!("a bracket test is a test");
            };
            Ok::<crate::ir::TestPredicate, String>(predicate)
        };
        assert!(matches!(
            predicate("if [ \"$A\" != \"b\" ]; then\n/bin/echo no\nfi\n").unwrap(),
            crate::ir::TestPredicate::StringNotEqual { .. }
        ));
        assert!(matches!(
            predicate("if [ \"$A\" = \"b\" ]; then\n/bin/echo no\nfi\n").unwrap(),
            crate::ir::TestPredicate::StringEqual { .. }
        ));
        // A `!` that is a complete token is the reserved word, and a mid-statement
        // one is still refused.
        assert!(
            lower_posix(
                "build.sh",
                "/bin/echo one ! /bin/echo two\n",
                &Interpreter::Bash,
                HostShell::default(),
            )
            .is_err()
        );
        // One that opens the statement is the prefix the caller handles.
        assert!(
            lower_posix(
                "build.sh",
                "! /usr/bin/false\n",
                &Interpreter::Bash,
                HostShell::default(),
            )
            .is_ok()
        );
    }

    /// A declaration runs nothing, and `$ErrorActionPreference` reaches nothing
    /// this lowers.
    ///
    /// `[CmdletBinding()]` and an empty `param()` open five of the six
    /// PowerShell files in this repository, and both carry parentheses, so the
    /// `&&` splitter refused every one of them on its first line.
    ///
    /// Skipping `$ErrorActionPreference` is the narrower claim and rests on
    /// something: measured, a failing external command is unaffected by it and a
    /// failing cmdlet is not, so it holds exactly while every cmdlet is refused.
    /// `a_powershell_cmdlet_is_refused_which_is_what_skipping_the_preference_rests_on`
    /// is that invariant and
    /// `contracts/golden/powershell-preference-semantics-v1.json` is the
    /// measurement.
    #[test]
    fn a_powershell_declaration_runs_nothing() {
        let lowered = lower_powershell(
            "build.ps1",
            "[CmdletBinding()]\nparam()\n$ErrorActionPreference = 'Stop'\n& '/bin/echo' one\n",
        )
        .unwrap();
        let Operation::Exec { argv, .. } = &lowered.body.operation else {
            panic!("only the invocation is left: {:?}", lowered.body.operation);
        };
        assert_eq!(literal_expression(&argv[0]).as_deref(), Some("/bin/echo"));

        // A `param` that declares parameters is the task's inputs and is not a
        // thing to skip.
        assert!(
            lower_powershell("build.ps1", "param($Name)\n& '/bin/echo' one\n").is_err(),
            "a declared parameter was dropped"
        );
        // Every other supplied name still refuses.
        assert!(lower_powershell("build.ps1", "$LASTEXITCODE = '0'\n& '/bin/echo' one\n").is_err());
        // And a preference whose value is not literal text is not read.
        assert!(
            lower_powershell(
                "build.ps1",
                "$ErrorActionPreference = $wanted\n& '/bin/echo' one\n"
            )
            .is_err()
        );
    }

    /// A cmdlet is refused, which is what skipping `$ErrorActionPreference`
    /// rests on.
    ///
    /// The preference reaches a cmdlet and nothing else this frontend lowers —
    /// measured. So the day a cmdlet becomes lowerable, skipping the preference
    /// stops being sound, and this is the test that says so rather than a
    /// sentence in a comment.
    #[test]
    fn a_powershell_cmdlet_is_refused_which_is_what_skipping_the_preference_rests_on() {
        for cmdlet in [
            "Get-Item /deshell/nope",
            "Write-Output one",
            "Join-Path a b",
            "Test-Path /deshell/nope",
        ] {
            let source = format!("$ErrorActionPreference = 'Stop'\n{cmdlet}\n");
            assert!(
                lower_powershell("build.ps1", &source).is_err(),
                "{cmdlet} lowered while the preference that governs it was skipped"
            );
        }
    }

    /// A PowerShell script's own variable is a value; one PowerShell answers is
    /// not.
    ///
    /// `$word = 'hello world'` then `& '/bin/echo' $word` passes one argument
    /// holding a space — measured with pwsh 7.6.5, where nothing splits it. So
    /// the text travels whole.
    ///
    /// `$ErrorActionPreference = 'Stop'` has the same shape and is not an
    /// assignment: it changes how the script handles errors, and
    /// `$LASTEXITCODE` decides what a later `exit` reports.
    /// `contracts/golden/powershell-variable-inventory-v1.json` is the measured
    /// list of names PowerShell answers and `cargo xtask powershell-variables`
    /// re-runs it.
    #[test]
    fn a_powershell_script_variable_is_a_value_and_a_supplied_one_is_not() {
        let lowered =
            lower_powershell("build.ps1", "$word = 'hello world'\n& '/bin/echo' $word\n").unwrap();
        let Operation::Sequence { nodes, .. } = &lowered.body.operation else {
            panic!(
                "two statements are a sequence: {:?}",
                lowered.body.operation
            );
        };
        let Operation::SetVariable { name, value, .. } = &nodes[0].operation else {
            panic!("the first is an assignment");
        };
        assert_eq!(name, "word");
        assert_eq!(literal_expression(value).as_deref(), Some("hello world"));
        let Operation::Exec { argv, .. } = &nodes[1].operation else {
            panic!("the second is an exec");
        };
        assert_eq!(
            argv[1].parts,
            vec![TextPart::Variable {
                name: "word".into()
            }]
        );

        // A name PowerShell answers itself is refused rather than carried as
        // text. `$ErrorActionPreference` is the exception and is skipped
        // instead: see `a_powershell_declaration_runs_nothing` and the invariant
        // it rests on.
        for supplied in ["$LASTEXITCODE = '0'", "$PID = '1'", "$PSHOME = '/x'"] {
            let source = format!("{supplied}\n& '/bin/echo' one\n");
            assert!(
                lower_powershell("build.ps1", &source).is_err(),
                "{supplied} was carried as a value"
            );
        }

        // A variable nothing assigned is not read.
        assert!(lower_powershell("build.ps1", "& '/bin/echo' $nothing\n").is_err());
        // An assignment that is not a literal string is refused whole rather
        // than having the part it understands taken.
        assert!(lower_powershell("build.ps1", "$x = 1 + 1\n& '/bin/echo' $x\n").is_err());
        assert!(lower_powershell("build.ps1", "$x = \"pre-$y\"\n& '/bin/echo' $x\n").is_err());
    }

    /// A PowerShell command that names a path is an invocation.
    ///
    /// `& 'path' args` and `./path args` are the same thing, and the frontend
    /// took only the first — nine of this repository's own blockers were
    /// `run: ./scripts/install-nushell.ps1`. A bare name is not a command at
    /// all, which is why a path prefix is required rather than any first word.
    ///
    /// `contracts/golden/powershell-invocation-semantics-v1.json` is the
    /// measurement and `cargo xtask powershell-invocation` re-measures it.
    #[test]
    fn a_powershell_command_that_names_a_path_is_an_invocation() {
        let argv = |source: &str| {
            let lowered = lower_powershell(".github/workflows/ci.yml.deshell.ps1", source)?;
            let Operation::Exec {
                argv,
                environment: _,
                working_directory: _,
            } = lowered.body.operation
            else {
                panic!("an invocation is an exec");
            };
            Ok::<Vec<String>, String>(
                argv.iter()
                    .map(|word| literal_expression(word).unwrap_or_default())
                    .collect(),
            )
        };
        let expected = vec!["./scripts/install-nushell.ps1".to_owned(), "a".to_owned()];
        assert_eq!(
            argv("& './scripts/install-nushell.ps1' a\n").unwrap(),
            expected
        );
        assert_eq!(argv("./scripts/install-nushell.ps1 a\n").unwrap(), expected);
        assert_eq!(
            argv(".\\scripts\\install-nushell.ps1 a\n").unwrap(),
            vec![".\\scripts\\install-nushell.ps1".to_owned(), "a".to_owned()]
        );
        // A bare name reaches no file, so nothing is claimed about it.
        assert!(argv("install-nushell.ps1 a\n").is_err());
    }

    /// A workflow step is lowered under the options the runner sets, not only
    /// the ones the step's text sets.
    ///
    /// GitHub writes the `run:` text to a file and executes `bash -e {0}`, so
    /// `set -e` is in effect whether or not the step says so. de-shell read the
    /// text alone: a two-command step became
    /// `Sequence { on_failure: Continue }` and was claimed `native`, so the step
    /// stopped at the first failure and the replacement would not have.
    ///
    /// `pipefail` is the other half and depends on whether the host named the
    /// shell: the runner's default is `bash -e {0}` without it, and an explicit
    /// `shell: bash` is `bash --noprofile --norc -eo pipefail {0}` with it. Both
    /// are bash, so the interpreter name cannot say which — the scanner carries
    /// the answer instead.
    #[test]
    fn a_workflow_step_carries_the_options_the_runner_sets() {
        let workflow = ".github/workflows/ci.yml.deshell.sh";
        let plain = "build.sh";
        let default_shell = HostShell { named: false };
        let named_shell = HostShell { named: true };

        assert!(host_shell_options(workflow, default_shell).errexit);
        assert!(!host_shell_options(workflow, default_shell).pipefail);
        assert!(host_shell_options(workflow, named_shell).pipefail);
        assert!(!host_shell_options(plain, named_shell).errexit);

        let two_commands = "/usr/bin/false\n/bin/echo after\n";
        let stopping =
            lower_posix(workflow, two_commands, &Interpreter::Bash, default_shell).unwrap();
        let Operation::Sequence {
            nodes: _,
            on_failure,
        } = &stopping.body.operation
        else {
            panic!("a two-command step is a sequence");
        };
        assert_eq!(*on_failure, crate::ir::SequenceFailure::Stop);

        let carrying_on =
            lower_posix(plain, two_commands, &Interpreter::Bash, default_shell).unwrap();
        let Operation::Sequence {
            nodes: _,
            on_failure,
        } = &carrying_on.body.operation
        else {
            panic!("a two-command script is a sequence");
        };
        assert_eq!(
            *on_failure,
            crate::ir::SequenceFailure::Continue,
            "a plain script has no `set -e` unless it says so"
        );

        // The same pipeline, two statuses, decided by what the host said.
        let pipeline = "/bin/echo one | /usr/bin/tr a b\n";
        let status = |host| {
            let lowered = lower_posix(workflow, pipeline, &Interpreter::Bash, host).unwrap();
            let Operation::Pipeline { nodes: _, status } = lowered.body.operation else {
                panic!("a pipeline is a pipeline");
            };
            status
        };
        assert_eq!(status(default_shell), crate::ir::PipelineStatus::Last);
        assert_eq!(status(named_shell), crate::ir::PipelineStatus::Pipefail);
    }

    /// A parser runs where de-shell runs, not in its own scratch directory.
    ///
    /// The scratch directory holds the source and the adapter. It became the
    /// working directory by accident, and that decided which interpreter
    /// answered: a version-manager shim on `PATH` resolves its version from the
    /// configuration nearest the working directory, so from the system
    /// temporary root it resolves nothing. Four tests in this crate failed on
    /// this machine for that reason, and it was read as the machine's fault
    /// twice before the directory was measured.
    #[test]
    fn a_parser_runs_where_deshell_runs_and_not_in_its_scratch_directory() {
        let scratch = tempfile::tempdir().unwrap();
        let chosen = parser_working_directory(scratch.path());
        assert_eq!(chosen, std::env::current_dir().unwrap());
        assert_ne!(
            chosen,
            scratch.path(),
            "the scratch directory decides which interpreter answers"
        );
    }

    use super::*;
    use crate::ir::{Guarantee, Operation, SourceBytes, TextPart};

    fn body(path: &str, source: &[u8]) -> crate::ir::Node {
        lower(path, source, UnknownInterpreter::TraceOnly)
            .unwrap()
            .tasks
            .remove(0)
            .body
    }

    /// An arm's body is a list of statements, and every one of them runs.
    ///
    /// The first version of this lowering handed the arm's whole byte range to
    /// the single-command path, which tokenised `echo a` followed by `echo b`
    /// into one `Exec` of `echo a echo b` — and called it native. The earlier
    /// test asserted only that an arm existed and that its body was an `Exec`,
    /// which both held.
    #[test]
    fn every_statement_of_a_case_arm_is_its_own_command() {
        let node = body(
            "build.sh",
            b"#!/bin/bash\ncase \"$1\" in\n  0)\n    /bin/echo a\n    /bin/echo b\n    ;;\nesac\n",
        );
        let Operation::Match { cases, .. } = &node.operation else {
            panic!("expected match: {node:#?}")
        };
        let [case] = cases.as_slice() else {
            panic!("expected one arm: {cases:#?}")
        };
        let Operation::Sequence { nodes, .. } = &case.body.operation else {
            panic!("an arm of two statements is a sequence: {case:#?}")
        };
        assert_eq!(nodes.len(), 2, "{nodes:#?}");
        for (node, expected) in nodes.iter().zip(["a", "b"]) {
            let Operation::Exec { argv, .. } = &node.operation else {
                panic!("expected exec: {node:#?}")
            };
            assert_eq!(
                argv,
                &[
                    crate::ir::TextExpression::literal("/bin/echo"),
                    crate::ir::TextExpression::literal(expected),
                ],
                "a statement of the arm ran with another statement's words"
            );
        }
    }

    /// The builtin table answers for every name a measured shell reports.
    ///
    /// Reads `contracts/golden/shell-builtin-inventory-v1.json`, which
    /// `cargo xtask builtin-table` re-measures on each runner. Before this, the
    /// table was a hand-written list of 62 names against 109 that the shells on
    /// this machine report — so `which`, `print`, `whence` and forty others fell
    /// through to the external-command path and became an `Exec` of whatever
    /// program `PATH` happened to hold.
    #[test]
    fn the_builtin_table_answers_for_every_measured_builtin() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/shell-builtin-inventory-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let treatments = corpus["treatments"]
            .as_object()
            .expect("corpus records treatments");

        for (name, treatment) in treatments {
            let expected = match treatment.as_str().expect("treatment is a string") {
                "modelled" => BuiltinTreatment::Modelled,
                "delegated" => BuiltinTreatment::Delegated,
                "unexamined" => BuiltinTreatment::Unexamined,
                other => panic!("{name} has an unknown treatment {other:?}"),
            };
            assert_eq!(
                builtin_treatment(name),
                Some(expected),
                "the table and the recording disagree about {name}"
            );
        }
        for (name, _) in SHELL_BUILTINS {
            assert!(
                treatments.contains_key(*name),
                "{name} is in the table but not in the recording"
            );
        }

        // Every name a shell reports has to be answered for, whatever the
        // answer is. An absent name is the one state the table cannot express.
        for shell in ["bash", "sh", "zsh"] {
            for name in corpus["shells"][shell]
                .as_array()
                .expect("corpus records this shell")
            {
                let name = name.as_str().expect("builtin name is a string");
                assert!(
                    builtin_treatment(name).is_some(),
                    "{shell} resolves {name} as a builtin and the table does not mention it"
                );
            }
        }

        // The table is sorted, so a name added out of order is a diff that reads
        // as one line rather than as a move.
        let names: Vec<&str> = SHELL_BUILTINS.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "the builtin table is not sorted");
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            names.len(),
            "the builtin table repeats a name"
        );
    }

    /// The vocabulary of native claims, and the evidence behind each one.
    ///
    /// `Guarantee::Native` carries a string and the validator asked only that
    /// it not be empty, so a node could claim a model that does not exist. The
    /// frontend builds the string from [`SemanticModel`] now; this checks that
    /// the enum and `contracts/semantic-models-v1.json` say the same thing, and
    /// that every recording a model cites is a file that is actually there.
    #[test]
    fn every_native_claim_names_a_model_that_exists_and_evidence_that_does() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let raw = std::fs::read_to_string(root.join("contracts/semantic-models-v1.json"))
            .expect("contract is readable");
        let contract: serde_json::Value = serde_json::from_str(&raw).expect("contract is JSON");
        let models = contract["models"]
            .as_object()
            .expect("contract lists models");

        for model in SemanticModel::ALL {
            let recorded = models
                .get(model.suffix())
                .unwrap_or_else(|| panic!("{} is not in the contract", model.suffix()));
            assert_eq!(
                recorded["evidence"].as_str(),
                model.evidence(),
                "{} cites different evidence than the contract",
                model.suffix()
            );
            // A model that cites a recording has to cite one that is there. A
            // path that has moved is how a claim keeps its wording and loses
            // what made it true.
            if let Some(evidence) = model.evidence() {
                assert!(
                    root.join(evidence).is_file(),
                    "{} cites {evidence}, which is not a file",
                    model.suffix()
                );
            }
        }
        for name in models.keys() {
            assert!(
                SemanticModel::ALL
                    .iter()
                    .any(|model| model.suffix() == name),
                "{name} is in the contract and not in the vocabulary"
            );
        }

        // The vocabulary is what the lowering actually writes, not a list beside
        // it: a model no node ever builds would pass everything above.
        let node = body("build.sh", b"#!/bin/bash\nprintf '%s' a\n");
        let Guarantee::Native { semantic_model } = &node.guarantee else {
            panic!("expected a native guarantee: {node:#?}")
        };
        let NativeBasis(expected) = SemanticModel::StaticPrintf.named(&Interpreter::Bash);
        assert_eq!(semantic_model, &expected);
    }

    /// The pattern model answers what the shells answer, or refuses.
    ///
    /// Reads `contracts/golden/case-pattern-semantics-v1.json`, which
    /// `cargo xtask case-patterns` re-measures on every runner. A pattern this
    /// lowers has to reach the same verdict as every shell that agreed on one;
    /// a pattern the shells disagree about has to be refused, because there is
    /// no single answer to lower.
    ///
    /// The corpus marks two cases where all four shells agree and the agreement
    /// is not evidence — a guard built from a command substitution that lost
    /// its newline, and a bracket set that answers the same by opposite rules.
    /// Both carry a `script` this cannot reproduce from a pattern alone, so they
    /// are skipped by the same rule that skips the rest: they are not patterns
    /// this reads.
    #[test]
    fn the_case_pattern_model_answers_what_the_shells_answer() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/case-pattern-semantics-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let cases = corpus["cases"].as_array().expect("corpus has cases");
        assert!(!cases.is_empty());

        let mut lowered = 0_usize;
        let mut refused = 0_usize;
        for case in cases {
            let id = case["id"].as_str().expect("case has an id");
            // The pattern is read out of the script rather than out of the
            // `pattern` field, which is written for a reader: a pattern holding
            // a real newline is shown there as `<LF>`.
            //
            // A case whose script does more than match a pattern is measuring
            // something else — a variable built by a command substitution, for
            // one — and there is no pattern here to read.
            let script = case["script"].as_str().expect("case has a script");
            let Some(pattern) = script
                .strip_prefix("case \"$1\" in\n  ")
                .and_then(|rest| rest.split(") printf MATCH").next())
            else {
                continue;
            };
            let word = crate::frontend::decode_base64(
                case["word_base64"].as_str().expect("case has a word"),
            );
            let word = String::from_utf8(word).expect("word is UTF-8");
            // Checked against bash, because that is the interpreter the
            // patterns are read for: `$'\n'` is a bash form, and the shells
            // part over it. A pattern this refuses needs no agreement to hold.
            let expected = case["bash"].as_str().expect("case records bash");

            // An alternation is several patterns in the source and several cases
            // in the IR, so each side is read on its own.
            let alternatives: Vec<&str> = pattern.split('|').collect();
            let models: Option<Vec<crate::ir::PatternExpression>> = alternatives
                .iter()
                .map(|one| case_pattern(one, &Interpreter::Bash))
                .collect();
            let Some(models) = models else {
                refused += 1;
                continue;
            };
            lowered += 1;

            assert_ne!(expected, "ERROR", "{id} lowers but bash refuses it");
            let matched = models.iter().any(|model| {
                let pieces: Vec<crate::ir::MatchPiece<'_>> = model
                    .pieces
                    .iter()
                    .map(|piece| match piece {
                        crate::ir::PatternPiece::Literal { value } => {
                            let text: String = value
                                .parts
                                .iter()
                                .map(|part| match part {
                                    TextPart::Literal { value } => value.clone(),
                                    other => panic!("{id} has a dynamic piece: {other:#?}"),
                                })
                                .collect();
                            crate::ir::MatchPiece::Literal(text.into())
                        }
                        crate::ir::PatternPiece::AnyRun => crate::ir::MatchPiece::AnyRun,
                        crate::ir::PatternPiece::AnyCharacter => {
                            crate::ir::MatchPiece::AnyCharacter
                        }
                    })
                    .collect();
                crate::ir::PatternExpression::matches(&pieces, &word)
            });
            assert_eq!(
                if matched { "MATCH" } else { "NOMATCH" },
                expected,
                "{id}: pattern {pattern:?} against {word:?}"
            );
        }
        assert!(
            lowered > 0 && refused > 0,
            "{lowered} lowered, {refused} refused"
        );
    }

    /// A function is a task, and its call sites are calls.
    ///
    /// Inlining the body at each call site would produce a program that runs
    /// the same and says less: the definition is where "these are the same
    /// check" is written down, and the project being migrated is the one that
    /// has to keep it true afterwards.
    #[test]
    fn a_shell_function_becomes_a_task_its_callers_call() {
        let plan = lower(
            "build.sh",
            b"#!/bin/bash\nreject() {\n  case \"$2\" in\n    *x*)\n      echo \"$1 is bad\"\n      exit 2\n      ;;\n  esac\n}\nreject \"name\" \"$1\"\nreject \"other\" \"$2\"\n",
            UnknownInterpreter::TraceOnly,
        )
        .unwrap();
        assert_eq!(plan.tasks.len(), 2, "{plan:#?}");
        let [main, reject] = plan.tasks.as_slice() else {
            panic!("expected two tasks: {plan:#?}")
        };
        assert_eq!(reject.name, "reject");
        // The arity is what the body reads, because the definition states none.
        assert_eq!(
            reject
                .inputs
                .iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>(),
            ["1", "2"]
        );

        let Operation::Sequence { nodes, .. } = &main.body.operation else {
            panic!("expected a sequence: {main:#?}")
        };
        assert_eq!(nodes.len(), 2, "{nodes:#?}");
        for (node, expected) in nodes.iter().zip(["name", "other"]) {
            let Operation::TaskCall {
                task,
                arguments,
                positional,
            } = &node.operation
            else {
                panic!("expected a task call: {node:#?}")
            };
            assert_eq!(task, "reject");
            assert!(arguments.is_empty());
            assert_eq!(positional.len(), 2);
            assert_eq!(positional[0], crate::ir::TextExpression::literal(expected));
        }
    }

    /// A call whose argument count does not match what the body reads is
    /// delegated.
    ///
    /// A shell function has no arity, so `$2` in a body called with one
    /// argument is empty here and an error under `set -u` — two behaviours
    /// where a task call has one.
    #[test]
    fn a_call_that_does_not_match_the_body_is_delegated() {
        let node = body(
            "build.sh",
            b"#!/bin/bash\nf() {\n  echo \"$1 $2\"\n}\nf one\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "{node:#?}"
        );
    }

    /// A name the shell answers itself is not lowered as an environment read.
    ///
    /// `d="x${RANDOM}"` lowered to a variable expansion and was claimed native.
    /// A generated program resolves a name through the process environment, and
    /// no environment carries `RANDOM` — so the program wrote `x` where the
    /// script wrote `x15226`, silently, with a guarantee saying the two agreed.
    #[test]
    fn a_name_the_shell_supplies_is_not_lowered_as_an_environment_read() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/shell-variable-inventory-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let shells: Vec<&str> = corpus["shells"]
            .as_array()
            .expect("corpus lists shells")
            .iter()
            .map(|value| value.as_str().expect("shell is a string"))
            .collect();
        let names = corpus["names"].as_object().expect("corpus records names");
        assert!(!names.is_empty());

        let mut supplied = 0_usize;
        for (name, states) in names {
            let shell_supplies = shells
                .iter()
                .any(|shell| states[*shell].as_str() == Some("SHELL"));
            assert_eq!(
                shell_supplied_variable(name),
                shell_supplies,
                "the table and the recording disagree about {name}"
            );
            if !shell_supplies {
                continue;
            }
            supplied += 1;
            // Both spellings: a plain expansion and one with a default.
            for source in [
                format!("#!/bin/bash\n/bin/echo \"${{{name}}}\"\n"),
                format!("#!/bin/bash\n/bin/echo \"${{{name}:-x}}\"\n"),
            ] {
                let node = body("build.sh", source.as_bytes());
                assert!(
                    matches!(node.operation, Operation::InterpreterCall { .. }),
                    "{name} lowered rather than delegating: {node:#?}"
                );
            }
        }
        assert!(supplied > 10, "{supplied} names");

        // A local of the same name is the script's own, and shadows the shell's.
        let node = body(
            "build.sh",
            b"#!/bin/bash\nRANDOM=fixed\n/bin/echo \"${RANDOM}\"\n",
        );
        assert!(
            !matches!(node.operation, Operation::InterpreterCall { .. }),
            "an assigned name is the script's own: {node:#?}"
        );

        // An ordinary environment name still lowers.
        let node = body("build.sh", b"#!/bin/bash\n/bin/echo \"${GITHUB_OUTPUT}\"\n");
        assert!(
            matches!(
                node.operation,
                Operation::WriteStdout { .. } | Operation::Exec { .. }
            ),
            "{node:#?}"
        );
    }

    /// `$'\n'` in a pattern is a spelling, and it is bash's.
    ///
    /// The word expander turns it into a newline, after which the pattern is an
    /// ordinary `*<LF>*` — so there is no ANSI-C pattern support to write. The
    /// form is bash's: zsh drops the backslash of an unknown escape where bash
    /// keeps it, and dash has no such form at all and reads the whole thing
    /// literally, which is how a line-break guard written this way accepts every
    /// input under `sh` on Ubuntu.
    #[test]
    fn ansi_c_quoting_in_a_pattern_is_expanded_for_bash_and_delegated_elsewhere() {
        let node = body(
            "build.sh",
            b"#!/bin/bash\ncase \"$1\" in\n  *$'\\n'*) /bin/echo bad ;;\nesac\n",
        );
        let Operation::Match { cases, .. } = &node.operation else {
            panic!("expected a match: {node:#?}")
        };
        assert_eq!(
            cases[0].pattern.pieces,
            [
                crate::ir::PatternPiece::AnyRun,
                crate::ir::PatternPiece::Literal {
                    value: crate::ir::TextExpression::literal("\n")
                },
                crate::ir::PatternPiece::AnyRun,
            ]
        );

        // `sh` is not one program — measured, this machine's is neither the
        // bash on `PATH` nor dash — so the form has no single meaning there.
        for shebang in ["#!/bin/sh", "#!/bin/zsh"] {
            let node = body(
                "build.sh",
                format!("{shebang}\ncase \"$1\" in\n  *$'\\n'*) /bin/echo bad ;;\nesac\n")
                    .as_bytes(),
            );
            assert!(
                matches!(node.operation, Operation::InterpreterCall { .. }),
                "{shebang} must delegate: {node:#?}"
            );
        }

        // An escape bash and zsh read differently is refused for both.
        let node = body(
            "build.sh",
            b"#!/bin/bash\ncase \"$1\" in\n  *$'\\q'*) /bin/echo bad ;;\nesac\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "an unknown escape must delegate: {node:#?}"
        );
    }

    /// The `echo` lowering writes the bytes bash writes, checked against a
    /// measurement of bash rather than against a reading of its manual.
    ///
    /// `contracts/golden/echo-builtin-semantics-v1.json` is measured by
    /// `cargo xtask echo-semantics`; this reads the same file, so widening the
    /// lowering without widening the recording fails here, and widening the
    /// recording without re-measuring fails there.
    #[test]
    fn the_echo_lowering_writes_what_bash_writes() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/echo-builtin-semantics-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let cases = corpus["cases"].as_array().expect("corpus has cases");
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().expect("case has a name");
            let arguments: Vec<String> = case["arguments"]
                .as_array()
                .expect("case has arguments")
                .iter()
                .map(|value| value.as_str().expect("argument is a string").to_owned())
                .collect();
            let modelled = case["modelled"].as_bool().expect("case says modelled");
            let quoted = arguments
                .iter()
                .map(|argument| format!("'{}'", argument.replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join(" ");
            let node = body(
                "build.sh",
                format!("#!/bin/bash\necho {quoted}\n").as_bytes(),
            );
            let Operation::WriteStdout { contents } = &node.operation else {
                assert!(!modelled, "{name} is modelled but delegated: {node:#?}");
                continue;
            };
            assert!(modelled, "{name} is not modelled but lowered: {node:#?}");
            let written: String = contents
                .parts
                .iter()
                .map(|part| match part {
                    TextPart::Literal { value } => value.clone(),
                    other => panic!("{name} has a non-literal part: {other:#?}"),
                })
                .collect();
            assert_eq!(
                written,
                case["bash"].as_str().expect("case records bash"),
                "{name} writes different bytes than bash"
            );
        }
    }

    /// The `exit` lowering ends with the status the shells end with.
    ///
    /// Reads the same file `cargo xtask exit-semantics` measures, so widening
    /// the lowering without widening the recording fails here.
    #[test]
    fn the_exit_lowering_ends_with_what_the_shells_end_with() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/exit-builtin-semantics-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let cases = corpus["cases"].as_array().expect("corpus has cases");
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().expect("case has a name");
            let status = case["status"].as_str().expect("case has a status");
            let modelled = case["modelled"].as_bool().expect("case says modelled");
            let node = body(
                "build.sh",
                format!("#!/bin/bash\nexit '{}'\n", status.replace('\'', "'\\''")).as_bytes(),
            );
            let Operation::Exit {
                status: lowered, ..
            } = &node.operation
            else {
                assert!(!modelled, "{name} is modelled but delegated: {node:#?}");
                continue;
            };
            assert!(modelled, "{name} is not modelled but lowered: {node:#?}");
            let [TextPart::Literal { value }] = lowered.parts.as_slice() else {
                panic!("{name} lowered to a status that is not a literal: {lowered:#?}")
            };
            let reduced = value.parse::<i64>().expect("status is an integer") % 256;
            assert_eq!(
                reduced.rem_euclid(256),
                case["bash"].as_i64().expect("case records bash"),
                "{name} ends with a different status than bash"
            );
        }
    }

    /// A bare `exit` is delegated; a status read at run time is not.
    ///
    /// A bare `exit` ends with the last command's status, which the IR has no
    /// term for. A status that arrives at run time does have one: the claim is
    /// made over the domain every shell agrees on and `NonNumericStatus` says
    /// what happens outside it, which is a value rather than a silent choice.
    #[test]
    fn a_bare_exit_is_delegated_and_a_run_time_status_states_its_domain() {
        let node = body("build.sh", b"#!/bin/bash\nexit\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "a bare exit must delegate: {node:#?}"
        );

        // A literal that is not a number is knowable here, and the shells part
        // over it, so it is refused at lowering rather than at a run.
        let node = body("build.sh", b"#!/bin/bash\nexit abc\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "a non-numeric literal must delegate: {node:#?}"
        );

        for source in [
            "#!/bin/bash\nexit \"$1\"\n".as_bytes(),
            "#!/bin/bash\nexit \"${CODE:-2}\"\n".as_bytes(),
        ] {
            let node = body("build.sh", source);
            let Operation::Exit { non_numeric, .. } = &node.operation else {
                panic!("expected an exit: {node:#?}")
            };
            // bash's answer, which is what this script pins.
            assert_eq!(
                *non_numeric,
                crate::ir::NonNumericStatus::Ends { status: 255 }
            );
        }

        // A zsh script carries zsh's answer. There is no choosing between the
        // shells: a plan names the interpreter its source runs under.
        for source in [
            "#!/bin/zsh\nexit \"$1\"\n".as_bytes(),
            "#!/bin/sh\nexit \"$1\"\n".as_bytes(),
        ] {
            let node = body("build.sh", source);
            let Operation::Exit { non_numeric, .. } = &node.operation else {
                panic!("expected an exit: {node:#?}")
            };
            let expected = if source.starts_with(b"#!/bin/zsh") {
                0
            } else {
                255
            };
            assert_eq!(
                *non_numeric,
                crate::ir::NonNumericStatus::Ends { status: expected }
            );
        }

        // A literal integer is the whole claim, under the model that says so.
        let node = body("build.sh", b"#!/bin/bash\nexit 2\n");
        let Operation::Exit { non_numeric, .. } = &node.operation else {
            panic!("expected an exit: {node:#?}")
        };
        assert_eq!(*non_numeric, crate::ir::NonNumericStatus::Unreachable);
    }

    /// The `printf` lowering writes the bytes the shells write.
    ///
    /// Reads the same file `cargo xtask printf-semantics` measures. Unlike the
    /// `echo` corpus, every column has to agree — `printf` is modelled for every
    /// interpreter, so a disagreement is the reason that would stop being true.
    #[test]
    fn the_printf_lowering_writes_what_the_shells_write() {
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/contracts/golden/printf-builtin-semantics-v1.json"
        ))
        .expect("corpus is readable");
        let corpus: serde_json::Value = serde_json::from_str(&raw).expect("corpus is JSON");
        let shells: Vec<&str> = corpus["shells"]
            .as_array()
            .expect("corpus lists shells")
            .iter()
            .map(|value| value.as_str().expect("shell is a string"))
            .collect();
        let cases = corpus["cases"].as_array().expect("corpus has cases");
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().expect("case has a name");
            let arguments: Vec<String> = case["arguments"]
                .as_array()
                .expect("case has arguments")
                .iter()
                .map(|value| value.as_str().expect("argument is a string").to_owned())
                .collect();
            let modelled = case["modelled"].as_bool().expect("case says modelled");
            let quoted = arguments
                .iter()
                .map(|argument| format!("'{}'", argument.replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join(" ");
            let node = body(
                "build.sh",
                format!("#!/bin/bash\nprintf {quoted}\n").as_bytes(),
            );
            let Operation::WriteStdout { contents } = &node.operation else {
                assert!(!modelled, "{name} is modelled but delegated: {node:#?}");
                continue;
            };
            assert!(modelled, "{name} is not modelled but lowered: {node:#?}");
            let written: String = contents
                .parts
                .iter()
                .map(|part| match part {
                    TextPart::Literal { value } => value.clone(),
                    other => panic!("{name} has a non-literal part: {other:#?}"),
                })
                .collect();
            for shell in &shells {
                assert_eq!(
                    written,
                    case[*shell].as_str().expect("case records this shell"),
                    "{name} writes different bytes than {shell}"
                );
            }
        }
    }

    /// `echo` is modelled for bash only, because the builtins disagree.
    ///
    /// The same corpus records `/bin/sh` interpreting `\t` and printing `-n`
    /// where bash does neither, so lowering a `sh` script's `echo` with bash's
    /// rule would substitute different bytes.
    #[test]
    fn echo_is_delegated_for_the_shells_whose_builtin_differs() {
        for shebang in ["#!/bin/sh", "#!/bin/zsh"] {
            let node = body("build.sh", format!("{shebang}\necho hello\n").as_bytes());
            assert!(
                matches!(node.operation, Operation::InterpreterCall { .. }),
                "{shebang} must delegate echo: {node:#?}"
            );
        }
        // A first argument that is not known until the expansion happens may be
        // `-n`, which bash would consume instead of printing.
        let node = body("build.sh", b"#!/bin/bash\necho \"$1\"\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "a dynamic first argument must delegate: {node:#?}"
        );
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
        let Operation::CaptureStdout {
            name, body: inner, ..
        } = &node.operation
        else {
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
        let Operation::While {
            condition,
            body: inner,
        } = &node.operation
        else {
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
    fn a_double_bracket_test_lowers_with_its_pattern() {
        // `[[` is a bash keyword, not a builtin: it does not split words or expand
        // globs in its operands, and `==` matches a pattern rather than comparing
        // strings. `[[ "$REF" == v* ]]` is how a release workflow asks whether a
        // ref is a tag.
        let node = body("build.sh", b"[[ \"$REF\" == v* ]]\n");
        let Operation::Test { predicate } = &node.operation else {
            panic!("expected test: {node:#?}")
        };
        assert_eq!(
            predicate,
            &crate::ir::TestPredicate::StartsWith {
                value: crate::ir::TextExpression {
                    parts: vec![TextPart::Variable { name: "REF".into() }]
                },
                prefix: "v".into(),
            }
        );

        // Without a pattern, `==` is a string comparison.
        let node = body("build.sh", b"[[ \"$A\" == \"b\" ]]\n");
        let Operation::Test { predicate } = &node.operation else {
            panic!("expected test")
        };
        assert!(matches!(
            predicate,
            crate::ir::TestPredicate::StringEqual { .. }
        ));

        // The operators `[` has work here too.
        let node = body("build.sh", b"[[ -n \"$VALUE\" ]]\n");
        let Operation::Test { predicate } = &node.operation else {
            panic!("expected test")
        };
        assert!(matches!(
            predicate,
            crate::ir::TestPredicate::NonEmpty { .. }
        ));

        // A pattern this does not model keeps the statement delegated rather than
        // matching something else.
        let node = body("build.sh", b"[[ \"$REF\" == v?.[0-9] ]]\n");
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "an unmodelled pattern must delegate: {node:#?}"
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
        assert_eq!(cases[0].pattern, crate::ir::PatternExpression::literal("a"));
        assert!(matches!(cases[0].body.operation, Operation::Exec { .. }));
        assert!(default.is_some(), "`*` is the default arm");

        // `a|b)` is one arm in the source and two cases in the IR: the shell
        // runs the same body for either value, and `Operation::Match` has no
        // alternation of its own to carry the `|` across.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  a|b) /bin/echo alt ;;\nesac\n",
        );
        let Operation::Match { cases, .. } = &node.operation else {
            panic!("expected match: {node:#?}")
        };
        assert_eq!(cases.len(), 2, "{cases:#?}");
        assert_eq!(cases[0].pattern, crate::ir::PatternExpression::literal("a"));
        assert_eq!(cases[1].pattern, crate::ir::PatternExpression::literal("b"));
        assert_eq!(cases[0].body.operation, cases[1].body.operation);

        // `0|1) ;;` — an arm whose body is empty. It is how a script says "these
        // exit codes are fine"; running nothing is the behaviour, not a gap in
        // the model.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  0|1) ;;\n  *) /bin/echo bad ;;\nesac\n",
        );
        let Operation::Match { cases, default, .. } = &node.operation else {
            panic!("expected match: {node:#?}")
        };
        assert_eq!(cases.len(), 2, "{cases:#?}");
        for case in cases {
            assert!(
                matches!(case.body.operation, Operation::NoOp),
                "an empty arm runs nothing: {case:#?}"
            );
        }
        assert!(default.is_some());

        // A glob is a pattern of pieces rather than a string to compare, and
        // whether a `*` is a metacharacter was decided by the quoting around it.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  a*) /bin/echo glob ;;\nesac\n",
        );
        let Operation::Match { cases, .. } = &node.operation else {
            panic!("expected match: {node:#?}")
        };
        assert_eq!(
            cases[0].pattern.pieces,
            [
                crate::ir::PatternPiece::Literal {
                    value: crate::ir::TextExpression::literal("a")
                },
                crate::ir::PatternPiece::AnyRun,
            ]
        );
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  'a*') /bin/echo lit ;;\nesac\n",
        );
        let Operation::Match { cases, .. } = &node.operation else {
            panic!("expected match: {node:#?}")
        };
        assert_eq!(
            cases[0].pattern.exact().as_deref(),
            Some("a*"),
            "a quoted star is a character, not a metacharacter"
        );

        // A bracket set delegates: `[^a]` negates in bash and is the set
        // `{^, a}` in dash, so there is no single meaning to lower.
        let node = body(
            "build.sh",
            b"case \"$1\" in\n  [ab]) /bin/echo set ;;\nesac\n",
        );
        assert!(
            matches!(node.operation, Operation::InterpreterCall { .. }),
            "a bracket set must delegate: {node:#?}"
        );
    }

    #[test]
    fn an_if_statement_lowers_to_a_condition() {
        // `Operation::Condition` has been in the IR from the start. `if` was
        // refused as compound syntax, so every CI step that branches — which is
        // most of them — was delegated whole.
        let node = body("build.sh", b"if /bin/test x = x; then /bin/echo yes; fi\n");
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
        assert!(if_false.is_some(), "the else arm must survive: {node:#?}");

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
        let Operation::Redirect { redirections, body } = &node.operation else {
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
                pipefail: true,
            })
        );
        assert_eq!(
            set_statement("set -eo pipefail", ShellOptions::default()),
            Some(ShellOptions {
                errexit: true,
                nounset: false,
                pipefail: true,
            })
        );
        assert_eq!(
            set_statement(
                "set +e",
                ShellOptions {
                    errexit: true,
                    nounset: false,
                    pipefail: false,
                }
            ),
            Some(ShellOptions::default())
        );
        assert_eq!(set_statement("set -f", ShellOptions::default()), None);
        assert_eq!(
            set_statement("/bin/echo set -e", ShellOptions::default()),
            None
        );
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
            // A `printf` whose format holds a conversion this does not model.
            b"printf '%d' 1\n".as_slice(),
            b"true\n".as_slice(),
            b"test -f input\n".as_slice(),
            // A zsh builtin that is also a program in `/usr/bin`. It was absent
            // from the table, so it became an `Exec` of the program — a
            // different one, with a different output format.
            b"which cargo\n".as_slice(),
            b"print value\n".as_slice(),
            b"whence -p cargo\n".as_slice(),
        ] {
            let node = body("build.sh", source);
            assert!(
                matches!(node.guarantee, Guarantee::Delegated { .. }),
                "{node:#?}"
            );
            assert!(
                matches!(node.operation, Operation::InterpreterCall { .. }),
                "{node:#?}"
            );
        }

        // `echo` and `printf` are modelled, and the reason each of them is is a
        // measurement rather than a name: see the corpora those two tests read.
        for source in [
            b"#!/bin/bash\necho value\n".as_slice(),
            b"#!/bin/bash\nprintf '%s' value\n".as_slice(),
        ] {
            let node = body("build.sh", source);
            assert!(
                matches!(node.operation, Operation::WriteStdout { .. }),
                "{node:#?}"
            );
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
                            pattern: crate::ir::PatternExpression::literal("case"),
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
            })
            .is_err()
        );
        let argument = parse_posix_word(ParsePosixWordArgs {
            source: "$1",
            allow_unquoted_expansion: true,
            inputs: &mut inputs,
            environment: &mut environment,
            locals: &locals,
        })
        .unwrap();
        assert!(matches!(argument.parts[0], TextPart::Argument { .. }));
        for expansion in ["$(date)", "${MISSING", "${}", "$"] {
            assert!(
                parse_expansion(ParseExpansionArgs {
                    source: expansion,
                    start: 0,
                    inputs: &mut inputs,
                    environment: &mut environment,
                    locals: &locals,
                })
                .is_err()
            );
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
