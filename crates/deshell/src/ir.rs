use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TextExpression {
    pub parts: Vec<TextPart>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub(crate) enum TextPart {
    Literal { value: String },
    Variable { name: String },
    Argument { name: String },
}

impl TextExpression {
    pub(crate) fn literal(value: impl Into<String>) -> Self {
        Self {
            parts: vec![TextPart::Literal {
                value: value.into(),
            }],
        }
    }

    pub(crate) fn evaluate(
        &self,
        variables: &BTreeMap<String, String>,
        arguments: &BTreeMap<String, String>,
    ) -> Result<String, String> {
        validate_expression(self, None)?;
        let mut output = String::new();
        for part in &self.parts {
            match part {
                TextPart::Literal { value } => output.push_str(value),
                TextPart::Variable { name } => output.push_str(
                    variables
                        .get(name)
                        .ok_or_else(|| format!("runtime variable is not defined: {name}"))?,
                ),
                TextPart::Argument { name } => output.push_str(
                    arguments
                        .get(name)
                        .ok_or_else(|| format!("task argument is not defined: {name}"))?,
                ),
            }
        }
        Ok(output)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum ValueType {
    Primitive(PrimitiveType),
    List { list: Box<ValueType> },
    Record { record: Vec<RecordField> },
    Secret { secret: Box<ValueType> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordField {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: ValueType,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrimitiveType {
    Text,
    Bool,
    Int,
    Path,
    Bytes,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PipelineStatus {
    Last,
    Pipefail,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedExpression {
    pub name: String,
    pub value: TextExpression,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceSpan {
    pub file: String,
    pub start_line: u64,
    pub start_column: u64,
    pub end_line: u64,
    pub end_column: u64,
    pub start_byte: u64,
    pub end_byte: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "level")]
pub(crate) enum Guarantee {
    Native { semantic_model: String },
    Delegated { reason: String },
    Residual { reason: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "encoding")]
pub(crate) enum SourceBytes {
    Utf8 { text: String },
    Base64 { base64: String },
}

impl SourceBytes {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(text) => Self::Utf8 {
                text: text.to_owned(),
            },
            Err(_) => Self::Base64 {
                base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        }
    }

    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Utf8 { text } => Ok(text.as_bytes().to_vec()),
            Self::Base64 { base64 } => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(base64)
                    .map_err(|error| format!("invalid capsule base64: {error}"))?;
                if base64::engine::general_purpose::STANDARD.encode(&decoded) != *base64 {
                    return Err("capsule base64 is not in canonical padded form".into());
                }
                Ok(decoded)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MatchCase {
    pub pattern: TextExpression,
    pub body: Node,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub(crate) enum FieldSplitting {
    None,
    PosixIfs { ifs: TextExpression },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GlobBehavior {
    Disabled,
    LiteralIfNoMatch,
    FailIfNoMatch,
    DropIfNoMatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub(crate) enum Redirection {
    Read {
        fd: u32,
        path: TextExpression,
    },
    Write {
        fd: u32,
        path: TextExpression,
        append: bool,
    },
    Duplicate {
        fd: u32,
        target_fd: u32,
    },
    Close {
        fd: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StateBinding {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: ValueType,
    pub value: TextExpression,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvironmentBinding {
    pub name: String,
    pub value: Option<TextExpression>,
    pub secret: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClockKind {
    Realtime,
    Monotonic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "type")]
pub(crate) enum Operation {
    Exec {
        argv: Vec<TextExpression>,
        environment: Vec<NamedExpression>,
        working_directory: Option<TextExpression>,
    },
    ExpandWords {
        name: String,
        value: TextExpression,
        field_splitting: FieldSplitting,
        glob: GlobBehavior,
    },
    Redirect {
        redirections: Vec<Redirection>,
        body: Box<Node>,
    },
    Pipeline {
        nodes: Vec<Node>,
        status: PipelineStatus,
    },
    Sequence {
        nodes: Vec<Node>,
    },
    Parallel {
        nodes: Vec<Node>,
    },
    Condition {
        predicate: Box<Node>,
        if_true: Box<Node>,
        if_false: Option<Box<Node>>,
    },
    Match {
        value: TextExpression,
        cases: Vec<MatchCase>,
        default: Option<Box<Node>>,
    },
    Foreach {
        variable: String,
        items: Vec<TextExpression>,
        body: Box<Node>,
    },
    Scope {
        variables: Vec<StateBinding>,
        environment: Vec<EnvironmentBinding>,
        working_directory: Option<TextExpression>,
        body: Box<Node>,
    },
    TryFinally {
        body: Box<Node>,
        finalizer: Box<Node>,
    },
    TaskCall {
        task: String,
        arguments: Vec<NamedExpression>,
    },
    SetVariable {
        name: String,
        value_type: ValueType,
        value: TextExpression,
    },
    SetEnvironment {
        name: String,
        value: Option<TextExpression>,
        secret: bool,
    },
    SetWorkingDirectory {
        path: TextExpression,
    },
    CaptureStdout {
        name: String,
        value_type: PrimitiveType,
        body: Box<Node>,
    },
    Spawn {
        handle: String,
        body: Box<Node>,
    },
    Wait {
        handle: String,
    },
    SendSignal {
        handle: String,
        signal: u32,
        process_group: bool,
    },
    FileRead {
        path: TextExpression,
    },
    FileWrite {
        path: TextExpression,
        contents: TextExpression,
        append: bool,
    },
    FileRemove {
        path: TextExpression,
    },
    FileMetadata {
        path: TextExpression,
        output: String,
        follow_symlinks: bool,
    },
    FileSetMetadata {
        path: TextExpression,
        permissions: Option<u32>,
        executable: Option<bool>,
        follow_symlinks: bool,
    },
    NetworkRequest {
        method: TextExpression,
        uri: TextExpression,
    },
    ClockRead {
        clock: ClockKind,
        output: String,
    },
    RandomBytes {
        output: String,
        length: u64,
    },
    InterpreterCall {
        interpreter: String,
        interpreter_pin: String,
        source: SourceBytes,
        source_span: SourceSpan,
        capabilities: Vec<String>,
        reason: String,
    },
    OpaqueCapsule {
        interpreter: String,
        source: SourceBytes,
        path: Option<String>,
    },
}

impl Operation {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Exec { .. } => "exec",
            Self::ExpandWords { .. } => "expand_words",
            Self::Redirect { .. } => "redirect",
            Self::Pipeline { .. } => "pipeline",
            Self::Sequence { .. } => "sequence",
            Self::Parallel { .. } => "parallel",
            Self::Condition { .. } => "condition",
            Self::Match { .. } => "match",
            Self::Foreach { .. } => "foreach",
            Self::Scope { .. } => "scope",
            Self::TryFinally { .. } => "try_finally",
            Self::TaskCall { .. } => "task_call",
            Self::SetVariable { .. } => "set_variable",
            Self::SetEnvironment { .. } => "set_environment",
            Self::SetWorkingDirectory { .. } => "set_working_directory",
            Self::CaptureStdout { .. } => "capture_stdout",
            Self::Spawn { .. } => "spawn",
            Self::Wait { .. } => "wait",
            Self::SendSignal { .. } => "send_signal",
            Self::FileRead { .. } => "file_read",
            Self::FileWrite { .. } => "file_write",
            Self::FileRemove { .. } => "file_remove",
            Self::FileMetadata { .. } => "file_metadata",
            Self::FileSetMetadata { .. } => "file_set_metadata",
            Self::NetworkRequest { .. } => "network_request",
            Self::ClockRead { .. } => "clock_read",
            Self::RandomBytes { .. } => "random_bytes",
            Self::InterpreterCall { .. } => "interpreter_call",
            Self::OpaqueCapsule { .. } => "opaque_capsule",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Node {
    pub id: String,
    pub operation: Operation,
    pub guarantee: Guarantee,
    pub source: Option<SourceSpan>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: ValueType,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Invocation {
    pub style: InvocationStyle,
    pub accepts_common_parameters: bool,
    pub parameters: Vec<InvocationParameter>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InvocationStyle {
    Powershell,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationParameter {
    pub input: String,
    pub position: Option<u64>,
    pub required: bool,
    #[serde(rename = "switch")]
    pub is_switch: bool,
    pub default: Option<TextExpression>,
    pub validations: Vec<InvocationValidation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub(crate) enum InvocationValidation {
    AllowEmptyString,
    NotNullOrEmpty,
    StringSet {
        values: Vec<String>,
        ignore_case: bool,
    },
    IntRange {
        minimum: i64,
        maximum: i64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Task {
    pub name: String,
    pub inputs: Vec<Binding>,
    pub outputs: Vec<Binding>,
    pub environment: Vec<String>,
    pub secrets: Vec<String>,
    pub platform_capabilities: Vec<String>,
    pub cacheable: bool,
    pub invocation: Option<Invocation>,
    pub body: Node,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    pub schema_version: u32,
    pub generator: String,
    pub entrypoint: String,
    pub tasks: Vec<Task>,
}

impl Plan {
    pub(crate) fn decode(input: &[u8]) -> Result<Self, Vec<String>> {
        let plan: Self = crate::strict_json::decode(input).map_err(|error| vec![error])?;
        plan.validate()?;
        Ok(plan)
    }

    pub(crate) fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        self.validate().map_err(|errors| errors.join("; "))?;
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        crate::canonical_json::pretty_bytes(&value)
    }

    pub(crate) fn assign_node_ids(&mut self) -> Result<(), String> {
        let mut preorder = 0_u64;
        for task in &mut self.tasks {
            assign_node_id(&mut task.body, &mut preorder)?;
        }
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != SCHEMA_VERSION {
            errors.push(format!(
                "schema_version must be {SCHEMA_VERSION} (found {})",
                self.schema_version
            ));
        }
        require_nonempty("generator", &self.generator, &mut errors);
        require_nonempty("entrypoint", &self.entrypoint, &mut errors);
        if self.tasks.is_empty() {
            errors.push("plan must contain at least one task".into());
        }

        let task_names = duplicate_strings(
            "task",
            self.tasks.iter().map(|task| task.name.as_str()),
            &mut errors,
        );
        for task in &self.tasks {
            require_nonempty("task name", &task.name, &mut errors);
        }
        if !task_names.contains(&self.entrypoint) {
            errors.push(format!("entrypoint task not found: {}", self.entrypoint));
        }

        let task_table: BTreeMap<&str, &Task> = self
            .tasks
            .iter()
            .map(|task| (task.name.as_str(), task))
            .collect();
        let mut seen_ids = BTreeSet::new();
        let mut preorder = 0_u64;
        for task in &self.tasks {
            validate_task(task, &task_table, &mut seen_ids, &mut preorder, &mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

pub(crate) fn node_id(
    normalized_path: &str,
    start_byte: u64,
    end_byte: u64,
    operation: &str,
    preorder: u64,
) -> Result<String, String> {
    if end_byte < start_byte {
        return Err("node ID byte span is reversed".into());
    }
    let path = if normalized_path.is_empty() {
        String::new()
    } else {
        normalize_path(normalized_path)?
    };
    if path != normalized_path {
        return Err(format!("node ID path is not normalized: {normalized_path}"));
    }
    if operation.is_empty()
        || !operation
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return Err(format!("node ID operation name is invalid: {operation}"));
    }
    let mut digest = Sha256::new();
    digest.update(b"deshell.node-id.v1\0");
    update_framed(&mut digest, path.as_bytes());
    update_framed(&mut digest, operation.as_bytes());
    digest.update(start_byte.to_be_bytes());
    digest.update(end_byte.to_be_bytes());
    digest.update(preorder.to_be_bytes());
    let bytes = digest.finalize();
    Ok(hex(&bytes[..16]))
}

pub(crate) fn normalize_path(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("path must not be empty".into());
    }
    if path.contains('\0') {
        return Err("path must not contain NUL".into());
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(format!("path must be project-relative: {path}"));
    }
    if normalized
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("path is not normalized: {path}"));
    }
    Ok(normalized)
}

fn update_framed(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn assign_node_id(node: &mut Node, preorder: &mut u64) -> Result<(), String> {
    let (path, start, end) = match &node.source {
        Some(span) => (span.file.as_str(), span.start_byte, span.end_byte),
        None => ("", 0, 0),
    };
    node.id = node_id(path, start, end, node.operation.name(), *preorder)?;
    *preorder = preorder
        .checked_add(1)
        .ok_or_else(|| "node preorder overflow".to_owned())?;
    visit_children_mut(&mut node.operation, |child| assign_node_id(child, preorder))
}

fn visit_children_mut<E>(
    operation: &mut Operation,
    mut visit: impl FnMut(&mut Node) -> Result<(), E>,
) -> Result<(), E> {
    match operation {
        Operation::Pipeline { nodes, .. }
        | Operation::Sequence { nodes }
        | Operation::Parallel { nodes } => {
            for node in nodes {
                visit(node)?;
            }
        }
        Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            visit(predicate)?;
            visit(if_true)?;
            if let Some(node) = if_false {
                visit(node)?;
            }
        }
        Operation::Match { cases, default, .. } => {
            for case in cases {
                visit(&mut case.body)?;
            }
            if let Some(node) = default {
                visit(node)?;
            }
        }
        Operation::Foreach { body, .. }
        | Operation::Scope { body, .. }
        | Operation::Redirect { body, .. }
        | Operation::CaptureStdout { body, .. }
        | Operation::Spawn { body, .. } => visit(body)?,
        Operation::TryFinally { body, finalizer } => {
            visit(body)?;
            visit(finalizer)?;
        }
        Operation::Exec { .. }
        | Operation::ExpandWords { .. }
        | Operation::TaskCall { .. }
        | Operation::SetVariable { .. }
        | Operation::SetEnvironment { .. }
        | Operation::SetWorkingDirectory { .. }
        | Operation::Wait { .. }
        | Operation::SendSignal { .. }
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
    Ok(())
}

fn validate_task<'a>(
    task: &Task,
    task_table: &BTreeMap<&'a str, &'a Task>,
    seen_ids: &mut BTreeSet<String>,
    preorder: &mut u64,
    errors: &mut Vec<String>,
) {
    let input_names = duplicate_strings(
        "input",
        task.inputs.iter().map(|binding| binding.name.as_str()),
        errors,
    );
    duplicate_strings(
        "output",
        task.outputs.iter().map(|binding| binding.name.as_str()),
        errors,
    );
    let environment = duplicate_strings(
        "task environment",
        task.environment.iter().map(String::as_str),
        errors,
    );
    duplicate_strings("secret", task.secrets.iter().map(String::as_str), errors);
    duplicate_strings(
        "platform capability",
        task.platform_capabilities.iter().map(String::as_str),
        errors,
    );
    for binding in task.inputs.iter().chain(&task.outputs) {
        require_nonempty("binding name", &binding.name, errors);
        validate_value_type(&binding.value_type, 0, errors);
    }
    for name in &task.environment {
        if !valid_identifier(name) {
            errors.push(format!("task environment name is invalid: {name}"));
        }
    }
    for secret in &task.secrets {
        if !input_names.contains(secret) && !environment.contains(secret) {
            errors.push(format!(
                "secret {secret} is not a task input or environment value"
            ));
        }
    }
    if let Some(invocation) = &task.invocation {
        let parameters = duplicate_strings(
            "invocation parameter",
            invocation
                .parameters
                .iter()
                .map(|parameter| parameter.input.as_str()),
            errors,
        );
        for input in &input_names {
            if !parameters.contains(input) {
                errors.push(format!(
                    "task {} input {input} is missing invocation metadata",
                    task.name
                ));
            }
        }
        for parameter in &invocation.parameters {
            if !input_names.contains(&parameter.input) {
                errors.push(format!(
                    "task {} invocation has unknown input {}",
                    task.name, parameter.input
                ));
            }
            for validation in &parameter.validations {
                if let InvocationValidation::StringSet { values, .. } = validation {
                    duplicate_strings(
                        "validation value",
                        values.iter().map(String::as_str),
                        errors,
                    );
                    if values.is_empty() {
                        errors.push("string_set validation requires values".into());
                    }
                }
                if let InvocationValidation::IntRange { minimum, maximum } = validation
                    && minimum > maximum
                {
                    errors.push(format!(
                        "invocation int range is reversed for {}",
                        parameter.input
                    ));
                }
            }
        }
    }
    validate_node(
        &task.body,
        &input_names,
        task_table,
        seen_ids,
        preorder,
        errors,
    );
}

fn validate_node<'a>(
    node: &Node,
    inputs: &BTreeSet<String>,
    task_table: &BTreeMap<&'a str, &'a Task>,
    seen_ids: &mut BTreeSet<String>,
    preorder: &mut u64,
    errors: &mut Vec<String>,
) {
    if !seen_ids.insert(node.id.clone()) {
        errors.push(format!("duplicate node id: {}", node.id));
    }
    let (path, start, end) = match &node.source {
        Some(span) => {
            match normalize_path(&span.file) {
                Ok(path) if path == span.file => {}
                Ok(_) => errors.push(format!("source path is not normalized: {}", span.file)),
                Err(error) => errors.push(format!("invalid source path {}: {error}", span.file)),
            }
            if span.end_byte < span.start_byte
                || span.start_line == 0
                || span.end_line == 0
                || span.end_line < span.start_line
                || (span.end_line == span.start_line && span.end_column < span.start_column)
                || span.end_byte > i64::MAX as u64
            {
                errors.push(format!(
                    "source span is not well formed for node {}",
                    node.id
                ));
            }
            (span.file.as_str(), span.start_byte, span.end_byte)
        }
        None => ("", 0, 0),
    };
    match node_id(path, start, end, node.operation.name(), *preorder) {
        Ok(expected) if node.id != expected => errors.push(format!(
            "node id {} is not deterministic; expected {expected}",
            node.id
        )),
        Err(error) => errors.push(error),
        Ok(_) => {}
    }
    *preorder = preorder.saturating_add(1);

    match &node.guarantee {
        Guarantee::Native { semantic_model } => {
            require_nonempty("native semantic model", semantic_model, errors)
        }
        Guarantee::Delegated { reason } => require_nonempty("delegation reason", reason, errors),
        Guarantee::Residual { reason } => require_nonempty("residual reason", reason, errors),
    }
    match &node.operation {
        Operation::InterpreterCall { .. }
            if !matches!(node.guarantee, Guarantee::Delegated { .. }) =>
        {
            errors.push("interpreter_call must use a delegated guarantee".into());
        }
        Operation::OpaqueCapsule { .. }
            if !matches!(node.guarantee, Guarantee::Residual { .. }) =>
        {
            errors.push("opaque_capsule must use a residual guarantee".into());
        }
        Operation::InterpreterCall { .. } | Operation::OpaqueCapsule { .. } => {}
        _ if !matches!(node.guarantee, Guarantee::Native { .. }) => {
            errors.push(format!(
                "{} must use a native guarantee",
                node.operation.name()
            ));
        }
        _ => {}
    }

    let expression = |value: &TextExpression, errors: &mut Vec<String>| {
        if let Err(error) = validate_expression(value, Some(inputs)) {
            errors.push(error);
        }
    };
    match &node.operation {
        Operation::Exec {
            argv,
            environment,
            working_directory,
        } => {
            if argv.is_empty() {
                errors.push("Exec argv must not be empty".into());
            }
            for value in argv {
                expression(value, errors);
            }
            let names = duplicate_strings(
                "Exec environment name",
                environment.iter().map(|value| value.name.as_str()),
                errors,
            );
            for value in environment {
                if !valid_identifier(&value.name) {
                    errors.push(format!("Exec environment name is invalid: {}", value.name));
                }
                expression(&value.value, errors);
            }
            let _ = names;
            if let Some(directory) = working_directory {
                expression(directory, errors);
            }
        }
        Operation::ExpandWords {
            name,
            value,
            field_splitting,
            ..
        } => {
            if !valid_identifier(name) {
                errors.push(format!("expanded word binding is invalid: {name}"));
            }
            expression(value, errors);
            if let FieldSplitting::PosixIfs { ifs } = field_splitting {
                expression(ifs, errors);
            }
        }
        Operation::Redirect { redirections, body } => {
            if redirections.is_empty() {
                errors.push("redirect must contain at least one ordered redirection".into());
            }
            for redirection in redirections {
                match redirection {
                    Redirection::Read { fd, path } | Redirection::Write { fd, path, .. } => {
                        if *fd > 1024 {
                            errors.push(format!("redirection file descriptor is too large: {fd}"));
                        }
                        expression(path, errors);
                    }
                    Redirection::Duplicate { fd, target_fd } => {
                        if *fd > 1024 || *target_fd > 1024 {
                            errors
                                .push("redirection duplicate file descriptor is too large".into());
                        }
                    }
                    Redirection::Close { fd } => {
                        if *fd > 1024 {
                            errors.push(format!("redirection file descriptor is too large: {fd}"));
                        }
                    }
                }
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::Pipeline { nodes, .. } | Operation::Parallel { nodes } => {
            if nodes.is_empty() {
                errors.push(format!(
                    "{} must contain at least one node",
                    node.operation.name()
                ));
            }
            if nodes.iter().any(contains_state_mutation) {
                errors.push(format!(
                    "{} state mutation is undefined",
                    node.operation.name()
                ));
            }
            for child in nodes {
                validate_node(child, inputs, task_table, seen_ids, preorder, errors);
            }
        }
        Operation::Sequence { nodes } => {
            if nodes.is_empty() {
                errors.push("sequence must contain at least one node".into());
            }
            for child in nodes {
                validate_node(child, inputs, task_table, seen_ids, preorder, errors);
            }
        }
        Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            validate_node(predicate, inputs, task_table, seen_ids, preorder, errors);
            validate_node(if_true, inputs, task_table, seen_ids, preorder, errors);
            if let Some(child) = if_false {
                validate_node(child, inputs, task_table, seen_ids, preorder, errors);
            }
        }
        Operation::Match {
            value,
            cases,
            default,
        } => {
            expression(value, errors);
            let mut patterns = BTreeSet::new();
            for case in cases {
                expression(&case.pattern, errors);
                if let Some(pattern) = literal_value(&case.pattern)
                    && !patterns.insert(pattern)
                {
                    errors.push("duplicate literal match case".into());
                }
                validate_node(&case.body, inputs, task_table, seen_ids, preorder, errors);
            }
            if let Some(child) = default {
                validate_node(child, inputs, task_table, seen_ids, preorder, errors);
            }
        }
        Operation::Foreach {
            variable,
            items,
            body,
        } => {
            if !valid_identifier(variable) {
                errors.push(format!("foreach variable is invalid: {variable}"));
            }
            for item in items {
                expression(item, errors);
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::Scope {
            variables,
            environment,
            working_directory,
            body,
        } => {
            duplicate_strings(
                "scope variable",
                variables.iter().map(|binding| binding.name.as_str()),
                errors,
            );
            for binding in variables {
                if !valid_identifier(&binding.name) {
                    errors.push(format!("scope variable is invalid: {}", binding.name));
                }
                validate_value_type(&binding.value_type, 0, errors);
                expression(&binding.value, errors);
            }
            duplicate_strings(
                "scope environment",
                environment.iter().map(|binding| binding.name.as_str()),
                errors,
            );
            for binding in environment {
                if !valid_identifier(&binding.name) {
                    errors.push(format!(
                        "scope environment name is invalid: {}",
                        binding.name
                    ));
                }
                if let Some(value) = &binding.value {
                    expression(value, errors);
                }
            }
            if let Some(directory) = working_directory {
                expression(directory, errors);
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::TryFinally { body, finalizer } => {
            if contains_state_mutation(body) || contains_state_mutation(finalizer) {
                errors.push("try/finally state mutation is undefined across failure paths".into());
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
            validate_node(finalizer, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::TaskCall { task, arguments } => {
            require_nonempty("task call target", task, errors);
            let names = duplicate_strings(
                "task argument",
                arguments.iter().map(|argument| argument.name.as_str()),
                errors,
            );
            for argument in arguments {
                expression(&argument.value, errors);
            }
            if let Some(target) = task_table.get(task.as_str()) {
                let expected: BTreeSet<String> = target
                    .inputs
                    .iter()
                    .map(|input| input.name.clone())
                    .collect();
                for unknown in names.difference(&expected) {
                    errors.push(format!("unknown argument {unknown} for task {task}"));
                }
                for missing in expected.difference(&names) {
                    errors.push(format!("missing argument {missing} for task {task}"));
                }
            } else if !task.is_empty() {
                errors.push(format!("task not found: {task}"));
            }
        }
        Operation::SetVariable {
            name,
            value_type,
            value,
        } => {
            if !valid_identifier(name) {
                errors.push(format!("runtime variable name is invalid: {name}"));
            }
            validate_value_type(value_type, 0, errors);
            expression(value, errors);
        }
        Operation::SetEnvironment { name, value, .. } => {
            if !valid_identifier(name) {
                errors.push(format!("runtime environment name is invalid: {name}"));
            }
            if let Some(value) = value {
                expression(value, errors);
            }
        }
        Operation::SetWorkingDirectory { path } => expression(path, errors),
        Operation::CaptureStdout {
            name,
            value_type,
            body,
        } => {
            if !valid_identifier(name) {
                errors.push(format!("runtime variable name is invalid: {name}"));
            }
            if *value_type != PrimitiveType::Text {
                errors.push("stdout capture value_type must be text".into());
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::Spawn { handle, body } => {
            if !valid_identifier(handle) {
                errors.push(format!("spawn handle is invalid: {handle}"));
            }
            if contains_state_mutation(body) {
                errors.push("spawned state mutation is undefined".into());
            }
            validate_node(body, inputs, task_table, seen_ids, preorder, errors);
        }
        Operation::Wait { handle } => {
            if !valid_identifier(handle) {
                errors.push(format!("wait handle is invalid: {handle}"));
            }
        }
        Operation::SendSignal { handle, signal, .. } => {
            if !valid_identifier(handle) {
                errors.push(format!("signal handle is invalid: {handle}"));
            }
            if !(1..=64).contains(signal) {
                errors.push(format!("signal number is outside 1..64: {signal}"));
            }
        }
        Operation::FileRead { path } | Operation::FileRemove { path } => expression(path, errors),
        Operation::FileWrite { path, contents, .. } => {
            expression(path, errors);
            expression(contents, errors);
        }
        Operation::FileMetadata { path, output, .. } => {
            expression(path, errors);
            if !valid_identifier(output) {
                errors.push(format!("file metadata output is invalid: {output}"));
            }
        }
        Operation::FileSetMetadata {
            path,
            permissions,
            executable,
            ..
        } => {
            expression(path, errors);
            if permissions.is_none() && executable.is_none() {
                errors.push("file_set_metadata must change permissions or executable state".into());
            }
            if permissions.is_some_and(|mode| mode > 0o777) {
                errors.push("file_set_metadata permissions exceed 0777".into());
            }
        }
        Operation::NetworkRequest { method, uri } => {
            expression(method, errors);
            expression(uri, errors);
        }
        Operation::ClockRead { output, .. } => {
            if !valid_identifier(output) {
                errors.push(format!("clock output is invalid: {output}"));
            }
        }
        Operation::RandomBytes { output, length } => {
            if !valid_identifier(output) {
                errors.push(format!("random output is invalid: {output}"));
            }
            if *length == 0 || *length > 1024 * 1024 {
                errors.push("random_bytes length must be between 1 and 1048576".into());
            }
        }
        Operation::InterpreterCall {
            interpreter,
            interpreter_pin,
            source,
            source_span,
            capabilities,
            reason,
        } => {
            require_nonempty("delegated interpreter", interpreter, errors);
            if !valid_pin(interpreter_pin) {
                errors.push("delegated interpreter_pin must be sha256:<64 lowercase hex>".into());
            }
            let bytes = match source.to_bytes() {
                Ok(bytes) => Some(bytes),
                Err(_) => {
                    errors.push("delegated source encoding is invalid".into());
                    None
                }
            };
            require_nonempty("delegation reason", reason, errors);
            duplicate_strings(
                "delegated capability",
                capabilities.iter().map(String::as_str),
                errors,
            );
            for capability in capabilities {
                require_nonempty("delegated capability", capability, errors);
            }
            if node.source.as_ref() != Some(source_span) {
                errors.push("interpreter_call source_span must equal the node source span".into());
            }
            if let Some(bytes) = bytes
                && source_span.end_byte.saturating_sub(source_span.start_byte) != bytes.len() as u64
            {
                errors.push("interpreter_call source bytes must exactly cover source_span".into());
            }
        }
        Operation::OpaqueCapsule {
            interpreter,
            source,
            path,
        } => {
            require_nonempty("capsule interpreter", interpreter, errors);
            if source.to_bytes().is_err() {
                errors.push("capsule source encoding is invalid".into());
            }
            if let Some(path) = path {
                match normalize_path(path) {
                    Ok(normalized) if normalized == *path => {}
                    Ok(_) => errors.push(format!("capsule path is not normalized: {path}")),
                    Err(error) => errors.push(format!("invalid capsule path {path}: {error}")),
                }
            }
        }
    }
}

fn contains_state_mutation(node: &Node) -> bool {
    match &node.operation {
        Operation::ExpandWords { .. }
        | Operation::SetVariable { .. }
        | Operation::SetEnvironment { .. }
        | Operation::SetWorkingDirectory { .. }
        | Operation::CaptureStdout { .. }
        | Operation::Wait { .. }
        | Operation::SendSignal { .. }
        | Operation::FileMetadata { .. }
        | Operation::ClockRead { .. }
        | Operation::RandomBytes { .. } => true,
        Operation::Pipeline { nodes, .. }
        | Operation::Sequence { nodes }
        | Operation::Parallel { nodes } => nodes.iter().any(contains_state_mutation),
        Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            contains_state_mutation(predicate)
                || contains_state_mutation(if_true)
                || if_false.as_deref().is_some_and(contains_state_mutation)
        }
        Operation::Match { cases, default, .. } => {
            cases.iter().any(|case| contains_state_mutation(&case.body))
                || default.as_deref().is_some_and(contains_state_mutation)
        }
        Operation::Foreach { body, .. }
        | Operation::Scope { body, .. }
        | Operation::Redirect { body, .. }
        | Operation::Spawn { body, .. } => contains_state_mutation(body),
        Operation::TryFinally { body, finalizer } => {
            contains_state_mutation(body) || contains_state_mutation(finalizer)
        }
        Operation::Exec { .. }
        | Operation::TaskCall { .. }
        | Operation::FileRead { .. }
        | Operation::FileWrite { .. }
        | Operation::FileRemove { .. }
        | Operation::FileSetMetadata { .. }
        | Operation::NetworkRequest { .. }
        | Operation::InterpreterCall { .. }
        | Operation::OpaqueCapsule { .. } => false,
    }
}

fn validate_expression(
    expression: &TextExpression,
    inputs: Option<&BTreeSet<String>>,
) -> Result<(), String> {
    if expression.parts.is_empty() {
        return Err("text expression must contain at least one part".into());
    }
    for (index, part) in expression.parts.iter().enumerate() {
        match part {
            TextPart::Literal { value } => {
                if expression.parts.len() > 1 && value.is_empty() {
                    return Err("empty literal is only valid as the sole expression part".into());
                }
                if index > 0 && matches!(expression.parts[index - 1], TextPart::Literal { .. }) {
                    return Err("adjacent literal expression parts must be merged".into());
                }
            }
            TextPart::Variable { name } => {
                if !valid_identifier(name) {
                    return Err(format!("variable name is invalid: {name}"));
                }
            }
            TextPart::Argument { name } => {
                require_nonempty_result("argument name", name)?;
                if let Some(inputs) = inputs
                    && !inputs.contains(name)
                {
                    return Err(format!(
                        "expression references unknown task argument: {name}"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn literal_value(expression: &TextExpression) -> Option<String> {
    match expression.parts.as_slice() {
        [TextPart::Literal { value }] => Some(value.clone()),
        _ => None,
    }
}

fn duplicate_strings<'a>(
    label: &str,
    values: impl Iterator<Item = &'a str>,
    errors: &mut Vec<String>,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.to_owned()) {
            errors.push(format!("duplicate {label}: {value}"));
        }
    }
    seen
}

fn valid_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_pin(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(crate::digest::valid_sha256)
}

fn validate_value_type(value: &ValueType, depth: usize, errors: &mut Vec<String>) {
    if depth >= 32 {
        errors.push("value type nesting exceeds 32 levels".into());
        return;
    }
    match value {
        ValueType::Primitive(_) => {}
        ValueType::List { list } | ValueType::Secret { secret: list } => {
            validate_value_type(list, depth + 1, errors);
        }
        ValueType::Record { record } => {
            if record.is_empty() {
                errors.push("record value type must contain at least one field".into());
            }
            let mut names = BTreeSet::new();
            for field in record {
                if !valid_identifier(&field.name) {
                    errors.push(format!("record field name is invalid: {}", field.name));
                }
                if !names.insert(field.name.as_str()) {
                    errors.push(format!("duplicate record field: {}", field.name));
                }
                validate_value_type(&field.value_type, depth + 1, errors);
            }
        }
    }
}

fn require_nonempty(label: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{label} must not be empty"));
    }
}

fn require_nonempty_result(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(parts: Vec<TextPart>) -> TextExpression {
        TextExpression { parts }
    }

    fn sample_plan() -> Plan {
        Plan {
            schema_version: 1,
            generator: "deshell/0.1.0".into(),
            entrypoint: "main".into(),
            tasks: vec![Task {
                name: "main".into(),
                inputs: vec![Binding {
                    name: "target".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Text),
                }],
                outputs: vec![],
                environment: vec!["HOME".into()],
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                invocation: None,
                body: Node {
                    id: String::new(),
                    operation: Operation::Exec {
                        argv: vec![
                            TextExpression::literal("printf"),
                            expression(vec![
                                TextPart::Literal {
                                    value: "$HOME:".into(),
                                },
                                TextPart::Variable {
                                    name: "HOME".into(),
                                },
                                TextPart::Literal { value: ":".into() },
                                TextPart::Argument {
                                    name: "target".into(),
                                },
                            ]),
                        ],
                        environment: vec![],
                        working_directory: None,
                    },
                    guarantee: Guarantee::Native {
                        semantic_model: "test-v1".into(),
                    },
                    source: Some(SourceSpan {
                        file: "scripts/build.sh".into(),
                        start_line: 1,
                        start_column: 0,
                        end_line: 1,
                        end_column: 6,
                        start_byte: 0,
                        end_byte: 6,
                    }),
                },
            }],
        }
    }

    fn native(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Native {
                semantic_model: "test-v1".into(),
            },
            source: None,
        }
    }

    fn delegated(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Delegated {
                reason: "test delegation".into(),
            },
            source: None,
        }
    }

    fn residual(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Residual {
                reason: "test residual".into(),
            },
            source: None,
        }
    }

    fn exec() -> Node {
        native(Operation::Exec {
            argv: vec![TextExpression::literal("true")],
            environment: vec![],
            working_directory: None,
        })
    }

    #[test]
    fn expression_evaluation_never_reparses_expanded_text() {
        let expression = expression(vec![
            TextPart::Variable {
                name: "FIRST".into(),
            },
            TextPart::Argument {
                name: "name".into(),
            },
        ]);
        let variables = BTreeMap::from([
            ("FIRST".into(), "$SECOND".into()),
            ("SECOND".into(), "must-not-appear".into()),
        ]);
        let arguments = BTreeMap::from([("name".into(), "-${SECOND}".into())]);
        assert_eq!(
            expression.evaluate(&variables, &arguments).unwrap(),
            "$SECOND-${SECOND}"
        );
    }

    #[test]
    fn deterministic_node_id_has_a_fixed_vector() {
        assert_eq!(
            node_id("scripts/build.sh", 12, 34, "exec", 5).unwrap(),
            "680482a635998b2ac7bb4bd0782fb5a8"
        );
        assert!(node_id("scripts/../escape.sh", 0, 1, "exec", 0).is_err());
    }

    #[test]
    fn normalized_paths_are_portable_and_traversal_safe() {
        assert_eq!(
            normalize_path("scripts\\build.sh").unwrap(),
            "scripts/build.sh"
        );
        for invalid in ["", "/tmp/a", "C:/a", "a//b", "./a", "a/../b", "a\0b"] {
            assert!(normalize_path(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn source_capsules_preserve_non_utf8_bytes_losslessly() {
        let original = b"echo \xff\0tail";
        let source = SourceBytes::from_bytes(original);
        assert!(matches!(source, SourceBytes::Base64 { .. }));
        assert_eq!(source.to_bytes().unwrap(), original);
        let utf8 = SourceBytes::from_bytes("echo 日本語\n".as_bytes());
        assert!(matches!(utf8, SourceBytes::Utf8 { .. }));
        assert_eq!(utf8.to_bytes().unwrap(), "echo 日本語\n".as_bytes());
    }

    #[test]
    fn v1_round_trip_is_strict_and_canonical() {
        let mut plan = sample_plan();
        plan.assign_node_ids().unwrap();
        plan.validate().unwrap();
        let encoded = plan.encode_pretty().unwrap();
        assert!(encoded.ends_with(b"\n"));
        assert_eq!(Plan::decode(&encoded).unwrap(), plan);

        let unknown =
            String::from_utf8(encoded)
                .unwrap()
                .replacen("{\n", "{\n  \"future\": true,\n", 1);
        assert!(
            Plan::decode(unknown.as_bytes())
                .unwrap_err()
                .join("; ")
                .contains("unknown field")
        );
    }

    #[test]
    fn bytes_list_record_and_secret_types_round_trip_and_validate_recursively() {
        let mut plan = sample_plan();
        plan.tasks[0].inputs[0].value_type = ValueType::Secret {
            secret: Box::new(ValueType::List {
                list: Box::new(ValueType::Record {
                    record: vec![RecordField {
                        name: "payload".into(),
                        value_type: ValueType::Primitive(PrimitiveType::Bytes),
                    }],
                }),
            }),
        };
        plan.assign_node_ids().unwrap();
        let encoded = plan.encode_pretty().unwrap();
        assert_eq!(Plan::decode(&encoded).unwrap(), plan);

        let ValueType::Secret { secret } = &mut plan.tasks[0].inputs[0].value_type else {
            unreachable!()
        };
        let ValueType::List { list } = secret.as_mut() else {
            unreachable!()
        };
        let ValueType::Record { record } = list.as_mut() else {
            unreachable!()
        };
        record.push(record[0].clone());
        assert!(
            plan.validate()
                .unwrap_err()
                .iter()
                .any(|error| error == "duplicate record field: payload")
        );
    }

    #[test]
    fn effect_algebra_round_trips_expansion_redirection_state_async_metadata_and_entropy() {
        fn native(operation: Operation) -> Node {
            Node {
                id: String::new(),
                operation,
                guarantee: Guarantee::Native {
                    semantic_model: "effect-algebra-v1".into(),
                },
                source: None,
            }
        }

        let mut plan = sample_plan();
        plan.tasks[0].body = native(Operation::Sequence {
            nodes: vec![
                native(Operation::ExpandWords {
                    name: "words".into(),
                    value: TextExpression::literal("$value/*.txt"),
                    field_splitting: FieldSplitting::PosixIfs {
                        ifs: TextExpression::literal(" \t\n"),
                    },
                    glob: GlobBehavior::LiteralIfNoMatch,
                }),
                native(Operation::Redirect {
                    redirections: vec![
                        Redirection::Read {
                            fd: 0,
                            path: TextExpression::literal("input.txt"),
                        },
                        Redirection::Write {
                            fd: 1,
                            path: TextExpression::literal("output.txt"),
                            append: false,
                        },
                        Redirection::Duplicate {
                            fd: 2,
                            target_fd: 1,
                        },
                    ],
                    body: Box::new(native(Operation::Exec {
                        argv: vec![TextExpression::literal("filter")],
                        environment: vec![],
                        working_directory: None,
                    })),
                }),
                native(Operation::Scope {
                    variables: vec![StateBinding {
                        name: "local".into(),
                        value_type: ValueType::Primitive(PrimitiveType::Text),
                        value: TextExpression::literal("value"),
                    }],
                    environment: vec![EnvironmentBinding {
                        name: "MODE".into(),
                        value: Some(TextExpression::literal("strict")),
                        secret: false,
                    }],
                    working_directory: Some(TextExpression::literal("workspace")),
                    body: Box::new(native(Operation::SetVariable {
                        name: "local".into(),
                        value_type: ValueType::Primitive(PrimitiveType::Text),
                        value: TextExpression::literal("updated"),
                    })),
                }),
                native(Operation::SetEnvironment {
                    name: "MODE".into(),
                    value: Some(TextExpression::literal("release")),
                    secret: false,
                }),
                native(Operation::SetWorkingDirectory {
                    path: TextExpression::literal("workspace"),
                }),
                native(Operation::Spawn {
                    handle: "job".into(),
                    body: Box::new(native(Operation::Exec {
                        argv: vec![TextExpression::literal("worker")],
                        environment: vec![],
                        working_directory: None,
                    })),
                }),
                native(Operation::Wait {
                    handle: "job".into(),
                }),
                native(Operation::SendSignal {
                    handle: "job".into(),
                    signal: 15,
                    process_group: true,
                }),
                native(Operation::FileMetadata {
                    path: TextExpression::literal("artifact"),
                    output: "metadata".into(),
                    follow_symlinks: false,
                }),
                native(Operation::FileSetMetadata {
                    path: TextExpression::literal("artifact"),
                    permissions: Some(0o755),
                    executable: Some(true),
                    follow_symlinks: false,
                }),
                native(Operation::ClockRead {
                    clock: ClockKind::Monotonic,
                    output: "now".into(),
                }),
                native(Operation::RandomBytes {
                    output: "nonce".into(),
                    length: 16,
                }),
            ],
        });
        plan.assign_node_ids().unwrap();
        let encoded = plan.encode_pretty().unwrap();
        assert_eq!(Plan::decode(&encoded).unwrap(), plan);
        let text = String::from_utf8(encoded).unwrap();
        for operation in [
            "expand_words",
            "redirect",
            "scope",
            "set_environment",
            "set_working_directory",
            "spawn",
            "wait",
            "send_signal",
            "file_metadata",
            "file_set_metadata",
            "clock_read",
            "random_bytes",
        ] {
            assert!(text.contains(&format!("\"type\": \"{operation}\"")));
        }
    }

    #[test]
    fn rejects_legacy_versions_and_duplicate_named_values() {
        let legacy = br#"{"schema_version":3,"generator":"old","entrypoint":"main","tasks":[]}"#;
        let error = Plan::decode(legacy).unwrap_err().join("; ");
        assert!(error.contains("schema_version"), "{error}");

        let mut plan = sample_plan();
        if let Operation::Exec { environment, .. } = &mut plan.tasks[0].body.operation {
            environment.push(NamedExpression {
                name: "A".into(),
                value: TextExpression::literal("1"),
            });
            environment.push(NamedExpression {
                name: "A".into(),
                value: TextExpression::literal("2"),
            });
        }
        plan.assign_node_ids().unwrap();
        let errors = plan.validate().unwrap_err().join("; ");
        assert!(
            errors.contains("duplicate Exec environment name: A"),
            "{errors}"
        );
    }

    #[test]
    fn differential_guarantee_cannot_be_decoded_into_a_plan() {
        let mut plan = sample_plan();
        plan.assign_node_ids().unwrap();
        let mut value = serde_json::to_value(plan).unwrap();
        value["tasks"][0]["body"]["guarantee"] = serde_json::json!({
            "level": "differential",
            "observation_digest": "bad",
            "scenarios": ["default"]
        });
        let differential = crate::canonical_json::pretty_bytes(&value).unwrap();
        assert!(Plan::decode(&differential).is_err());
    }

    #[test]
    fn node_id_expression_source_and_value_type_boundaries_fail_closed() {
        for (path, start, end, operation, expected) in [
            ("build.sh", 2, 1, "exec", "reversed"),
            (r"scripts\build.sh", 0, 1, "exec", "not normalized"),
            ("build.sh", 0, 1, "", "operation name"),
            ("build.sh", 0, 1, "Exec", "operation name"),
        ] {
            let error = node_id(path, start, end, operation, 0).unwrap_err();
            assert!(error.contains(expected), "unexpected {error:?}");
        }

        assert!(
            SourceBytes::Base64 { base64: "!".into() }
                .to_bytes()
                .unwrap_err()
                .contains("invalid capsule base64")
        );
        assert!(
            SourceBytes::Base64 {
                base64: "Zh==".into()
            }
            .to_bytes()
            .is_err()
        );

        let inputs = BTreeSet::from(["known".to_owned()]);
        let invalid_expressions = [
            expression(vec![]),
            expression(vec![
                TextPart::Literal {
                    value: String::new(),
                },
                TextPart::Variable { name: "A".into() },
            ]),
            expression(vec![
                TextPart::Literal { value: "a".into() },
                TextPart::Literal { value: "b".into() },
            ]),
            expression(vec![TextPart::Variable {
                name: "bad-name".into(),
            }]),
            expression(vec![TextPart::Argument {
                name: String::new(),
            }]),
            expression(vec![TextPart::Argument {
                name: "unknown".into(),
            }]),
        ];
        for invalid in invalid_expressions {
            assert!(validate_expression(&invalid, Some(&inputs)).is_err());
        }
        assert_eq!(
            literal_value(&TextExpression::literal("value")),
            Some("value".into())
        );
        assert_eq!(
            literal_value(&expression(vec![TextPart::Variable { name: "A".into() }])),
            None
        );
        assert!(
            expression(vec![TextPart::Variable {
                name: "MISSING".into()
            }])
            .evaluate(&BTreeMap::new(), &BTreeMap::new())
            .unwrap_err()
            .contains("runtime variable")
        );
        assert!(
            expression(vec![TextPart::Argument {
                name: "missing".into()
            }])
            .evaluate(&BTreeMap::new(), &BTreeMap::new())
            .unwrap_err()
            .contains("task argument")
        );

        let mut errors = Vec::new();
        validate_value_type(&ValueType::Record { record: vec![] }, 0, &mut errors);
        validate_value_type(
            &ValueType::Record {
                record: vec![
                    RecordField {
                        name: "bad-name".into(),
                        value_type: ValueType::Primitive(PrimitiveType::Text),
                    },
                    RecordField {
                        name: "bad-name".into(),
                        value_type: ValueType::Primitive(PrimitiveType::Text),
                    },
                ],
            },
            0,
            &mut errors,
        );
        let mut deeply_nested = ValueType::Primitive(PrimitiveType::Text);
        for _ in 0..33 {
            deeply_nested = ValueType::List {
                list: Box::new(deeply_nested),
            };
        }
        validate_value_type(&deeply_nested, 0, &mut errors);
        let errors = errors.join("; ");
        for expected in [
            "at least one field",
            "field name is invalid",
            "duplicate record",
            "nesting",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }
    }

    #[test]
    fn task_invocation_source_and_guarantee_validation_aggregate_all_contract_errors() {
        let mut plan = sample_plan();
        let mut worker = plan.tasks[0].clone();
        worker.name = "worker".into();
        worker.inputs = vec![Binding {
            name: "needed".into(),
            value_type: ValueType::Primitive(PrimitiveType::Text),
        }];
        worker.environment.clear();
        worker.body = exec();
        plan.tasks.push(worker);
        plan.assign_node_ids().unwrap();

        plan.schema_version = 2;
        plan.generator.clear();
        plan.entrypoint = "missing".into();
        plan.tasks[0].outputs = vec![
            Binding {
                name: String::new(),
                value_type: ValueType::Record { record: vec![] },
            },
            Binding {
                name: String::new(),
                value_type: ValueType::Primitive(PrimitiveType::Text),
            },
        ];
        plan.tasks[0].environment = vec!["BAD-NAME".into(), "BAD-NAME".into()];
        plan.tasks[0].secrets = vec!["unbound".into(), "unbound".into()];
        plan.tasks[0].platform_capabilities = vec!["process".into(), "process".into()];
        plan.tasks[0].invocation = Some(Invocation {
            style: InvocationStyle::Powershell,
            accepts_common_parameters: false,
            parameters: vec![
                InvocationParameter {
                    input: "ghost".into(),
                    position: Some(0),
                    required: false,
                    is_switch: false,
                    default: None,
                    validations: vec![
                        InvocationValidation::StringSet {
                            values: vec![],
                            ignore_case: false,
                        },
                        InvocationValidation::IntRange {
                            minimum: 2,
                            maximum: 1,
                        },
                    ],
                },
                InvocationParameter {
                    input: "ghost".into(),
                    position: Some(1),
                    required: false,
                    is_switch: false,
                    default: None,
                    validations: vec![InvocationValidation::StringSet {
                        values: vec!["same".into(), "same".into()],
                        ignore_case: false,
                    }],
                },
            ],
        });
        plan.tasks[0].body.guarantee = Guarantee::Native {
            semantic_model: String::new(),
        };
        plan.tasks[0].body.source = Some(SourceSpan {
            file: r"scripts\build.sh".into(),
            start_line: 0,
            start_column: 4,
            end_line: 0,
            end_column: 3,
            start_byte: 9,
            end_byte: i64::MAX as u64 + 1,
        });
        plan.tasks[1].body.id = plan.tasks[0].body.id.clone();
        plan.tasks[1].body.source = Some(SourceSpan {
            file: "../escape.sh".into(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 1,
        });
        let errors = plan.validate().unwrap_err().join("; ");
        for expected in [
            "schema_version",
            "generator must not be empty",
            "entrypoint task not found",
            "duplicate output",
            "binding name must not be empty",
            "record value type",
            "duplicate task environment",
            "task environment name is invalid",
            "duplicate secret",
            "not a task input or environment",
            "duplicate platform capability",
            "duplicate invocation parameter",
            "missing invocation metadata",
            "unknown input",
            "string_set validation requires values",
            "duplicate validation value",
            "int range is reversed",
            "source path is not normalized",
            "invalid source path",
            "source span is not well formed",
            "duplicate node id",
            "native semantic model must not be empty",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }
    }

    #[test]
    fn operation_validation_rejects_every_ambiguous_effect_boundary() {
        let invalid_expression = || TextExpression { parts: vec![] };
        let state_mutation = || {
            native(Operation::SetVariable {
                name: "value".into(),
                value_type: ValueType::Primitive(PrimitiveType::Text),
                value: TextExpression::literal("changed"),
            })
        };
        let source_span = SourceSpan {
            file: "build.sh".into(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 4,
            start_byte: 0,
            end_byte: 4,
        };
        let interpreter = delegated(Operation::InterpreterCall {
            interpreter: String::new(),
            interpreter_pin: "bad".into(),
            source: SourceBytes::Base64 { base64: "!".into() },
            source_span: source_span.clone(),
            capabilities: vec![String::new(), String::new()],
            reason: String::new(),
        });
        let short_interpreter = Node {
            source: Some(source_span.clone()),
            ..delegated(Operation::InterpreterCall {
                interpreter: "sh".into(),
                interpreter_pin: format!("sha256:{}", "a".repeat(64)),
                source: SourceBytes::from_bytes(b"true"),
                source_span: SourceSpan {
                    end_byte: 3,
                    ..source_span.clone()
                },
                capabilities: vec![],
                reason: "pinned".into(),
            })
        };
        let invalid_native_guarantee = Node {
            guarantee: Guarantee::Delegated {
                reason: "wrong".into(),
            },
            ..exec()
        };
        let invalid_interpreter_guarantee = Node {
            guarantee: Guarantee::Native {
                semantic_model: "wrong".into(),
            },
            ..delegated(Operation::InterpreterCall {
                interpreter: "sh".into(),
                interpreter_pin: format!("sha256:{}", "a".repeat(64)),
                source: SourceBytes::from_bytes(b"true"),
                source_span: source_span.clone(),
                capabilities: vec![],
                reason: "pinned".into(),
            })
        };
        let invalid_capsule_guarantee = Node {
            guarantee: Guarantee::Native {
                semantic_model: "wrong".into(),
            },
            ..residual(Operation::OpaqueCapsule {
                interpreter: "sh".into(),
                source: SourceBytes::from_bytes(b"true"),
                path: None,
            })
        };

        let mut plan = sample_plan();
        plan.tasks[0].body = native(Operation::Sequence {
            nodes: vec![
                native(Operation::Exec {
                    argv: vec![],
                    environment: vec![
                        NamedExpression {
                            name: "BAD-NAME".into(),
                            value: invalid_expression(),
                        },
                        NamedExpression {
                            name: "BAD-NAME".into(),
                            value: TextExpression::literal("duplicate"),
                        },
                    ],
                    working_directory: Some(invalid_expression()),
                }),
                native(Operation::ExpandWords {
                    name: "bad-name".into(),
                    value: invalid_expression(),
                    field_splitting: FieldSplitting::PosixIfs {
                        ifs: invalid_expression(),
                    },
                    glob: GlobBehavior::Disabled,
                }),
                native(Operation::Redirect {
                    redirections: vec![],
                    body: Box::new(exec()),
                }),
                native(Operation::Redirect {
                    redirections: vec![
                        Redirection::Read {
                            fd: 1025,
                            path: invalid_expression(),
                        },
                        Redirection::Write {
                            fd: 1025,
                            path: invalid_expression(),
                            append: false,
                        },
                        Redirection::Duplicate {
                            fd: 1025,
                            target_fd: 1025,
                        },
                        Redirection::Close { fd: 1025 },
                    ],
                    body: Box::new(exec()),
                }),
                native(Operation::Pipeline {
                    nodes: vec![state_mutation()],
                    status: PipelineStatus::Pipefail,
                }),
                native(Operation::Parallel {
                    nodes: vec![native(Operation::ClockRead {
                        clock: ClockKind::Realtime,
                        output: "now".into(),
                    })],
                }),
                native(Operation::Pipeline {
                    nodes: vec![],
                    status: PipelineStatus::Last,
                }),
                native(Operation::Parallel { nodes: vec![] }),
                native(Operation::Sequence { nodes: vec![] }),
                native(Operation::Condition {
                    predicate: Box::new(exec()),
                    if_true: Box::new(exec()),
                    if_false: Some(Box::new(exec())),
                }),
                native(Operation::Match {
                    value: invalid_expression(),
                    cases: vec![
                        MatchCase {
                            pattern: TextExpression::literal("same"),
                            body: exec(),
                        },
                        MatchCase {
                            pattern: TextExpression::literal("same"),
                            body: exec(),
                        },
                    ],
                    default: Some(Box::new(exec())),
                }),
                native(Operation::Foreach {
                    variable: "bad-name".into(),
                    items: vec![invalid_expression()],
                    body: Box::new(exec()),
                }),
                native(Operation::Scope {
                    variables: vec![
                        StateBinding {
                            name: "bad-name".into(),
                            value_type: ValueType::Record { record: vec![] },
                            value: invalid_expression(),
                        },
                        StateBinding {
                            name: "bad-name".into(),
                            value_type: ValueType::Primitive(PrimitiveType::Text),
                            value: TextExpression::literal("duplicate"),
                        },
                    ],
                    environment: vec![
                        EnvironmentBinding {
                            name: "BAD-NAME".into(),
                            value: Some(invalid_expression()),
                            secret: false,
                        },
                        EnvironmentBinding {
                            name: "BAD-NAME".into(),
                            value: None,
                            secret: false,
                        },
                    ],
                    working_directory: Some(invalid_expression()),
                    body: Box::new(exec()),
                }),
                native(Operation::TryFinally {
                    body: Box::new(state_mutation()),
                    finalizer: Box::new(exec()),
                }),
                native(Operation::TaskCall {
                    task: String::new(),
                    arguments: vec![],
                }),
                native(Operation::TaskCall {
                    task: "missing".into(),
                    arguments: vec![
                        NamedExpression {
                            name: "arg".into(),
                            value: invalid_expression(),
                        },
                        NamedExpression {
                            name: "arg".into(),
                            value: TextExpression::literal("duplicate"),
                        },
                    ],
                }),
                native(Operation::SetVariable {
                    name: "bad-name".into(),
                    value_type: ValueType::Record { record: vec![] },
                    value: invalid_expression(),
                }),
                native(Operation::SetEnvironment {
                    name: "bad-name".into(),
                    value: Some(invalid_expression()),
                    secret: false,
                }),
                native(Operation::SetWorkingDirectory {
                    path: invalid_expression(),
                }),
                native(Operation::CaptureStdout {
                    name: "bad-name".into(),
                    value_type: PrimitiveType::Bytes,
                    body: Box::new(exec()),
                }),
                native(Operation::Spawn {
                    handle: "bad-handle".into(),
                    body: Box::new(state_mutation()),
                }),
                native(Operation::Wait {
                    handle: "bad-handle".into(),
                }),
                native(Operation::SendSignal {
                    handle: "bad-handle".into(),
                    signal: 0,
                    process_group: false,
                }),
                native(Operation::FileRead {
                    path: invalid_expression(),
                }),
                native(Operation::FileWrite {
                    path: invalid_expression(),
                    contents: invalid_expression(),
                    append: false,
                }),
                native(Operation::FileRemove {
                    path: invalid_expression(),
                }),
                native(Operation::FileMetadata {
                    path: invalid_expression(),
                    output: "bad-name".into(),
                    follow_symlinks: false,
                }),
                native(Operation::FileSetMetadata {
                    path: invalid_expression(),
                    permissions: None,
                    executable: None,
                    follow_symlinks: false,
                }),
                native(Operation::FileSetMetadata {
                    path: TextExpression::literal("file"),
                    permissions: Some(0o1000),
                    executable: None,
                    follow_symlinks: false,
                }),
                native(Operation::NetworkRequest {
                    method: invalid_expression(),
                    uri: invalid_expression(),
                }),
                native(Operation::ClockRead {
                    clock: ClockKind::Realtime,
                    output: "bad-name".into(),
                }),
                native(Operation::RandomBytes {
                    output: "bad-name".into(),
                    length: 0,
                }),
                native(Operation::RandomBytes {
                    output: "nonce".into(),
                    length: 1024 * 1024 + 1,
                }),
                interpreter,
                short_interpreter,
                residual(Operation::OpaqueCapsule {
                    interpreter: String::new(),
                    source: SourceBytes::Base64 { base64: "!".into() },
                    path: Some(r"scripts\capsule.sh".into()),
                }),
                residual(Operation::OpaqueCapsule {
                    interpreter: "sh".into(),
                    source: SourceBytes::from_bytes(b"true"),
                    path: Some("../capsule.sh".into()),
                }),
                invalid_native_guarantee,
                invalid_interpreter_guarantee,
                invalid_capsule_guarantee,
            ],
        });
        let mut worker = Task {
            name: "worker".into(),
            inputs: vec![Binding {
                name: "needed".into(),
                value_type: ValueType::Primitive(PrimitiveType::Text),
            }],
            outputs: vec![],
            environment: vec![],
            secrets: vec![],
            platform_capabilities: vec![],
            cacheable: false,
            invocation: None,
            body: exec(),
        };
        if let Operation::Sequence { nodes } = &mut plan.tasks[0].body.operation {
            nodes.push(native(Operation::TaskCall {
                task: "worker".into(),
                arguments: vec![NamedExpression {
                    name: "unknown".into(),
                    value: TextExpression::literal("value"),
                }],
            }));
        } else {
            unreachable!()
        }
        worker.body = exec();
        plan.tasks.push(worker);
        plan.assign_node_ids().unwrap();
        let errors = plan.validate().unwrap_err().join("; ");
        for expected in [
            "Exec argv must not be empty",
            "duplicate Exec environment name",
            "Exec environment name is invalid",
            "text expression must contain",
            "expanded word binding is invalid",
            "redirect must contain",
            "file descriptor is too large",
            "pipeline state mutation",
            "parallel state mutation",
            "must contain at least one node",
            "sequence must contain",
            "duplicate literal match case",
            "foreach variable is invalid",
            "duplicate scope variable",
            "scope variable is invalid",
            "duplicate scope environment",
            "scope environment name is invalid",
            "try/finally state mutation",
            "task call target must not be empty",
            "duplicate task argument",
            "task not found",
            "unknown argument unknown for task worker",
            "missing argument needed for task worker",
            "runtime variable name is invalid",
            "runtime environment name is invalid",
            "stdout capture value_type must be text",
            "spawn handle is invalid",
            "spawned state mutation",
            "wait handle is invalid",
            "signal handle is invalid",
            "signal number is outside",
            "file metadata output is invalid",
            "must change permissions",
            "permissions exceed",
            "clock output is invalid",
            "random output is invalid",
            "random_bytes length",
            "delegated interpreter must not be empty",
            "interpreter_pin",
            "delegated source encoding",
            "duplicate delegated capability",
            "delegated capability must not be empty",
            "source_span must equal",
            "source bytes must exactly cover",
            "capsule interpreter must not be empty",
            "capsule source encoding",
            "capsule path is not normalized",
            "invalid capsule path",
            "exec must use a native guarantee",
            "interpreter_call must use a delegated guarantee",
            "opaque_capsule must use a residual guarantee",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }
    }
}
