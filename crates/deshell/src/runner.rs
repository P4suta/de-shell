use crate::ir::{
    Guarantee, NamedExpression, Node, Operation, Plan, PrimitiveType, Task, TextExpression,
    ValueType,
};
use base64::Engine as _;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessRequest {
    pub argv: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub working_directory: Option<String>,
    pub stdin: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InterpreterRequest {
    pub interpreter: String,
    pub interpreter_pin: String,
    pub source: Vec<u8>,
    pub capabilities: Vec<String>,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub stdin: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(crate) trait Backend: Sync {
    fn execute(&self, request: ProcessRequest) -> Result<ProcessResult, String>;
    fn execute_pipeline(
        &self,
        requests: Vec<ProcessRequest>,
    ) -> Result<Vec<ProcessResult>, String> {
        let mut results = Vec::with_capacity(requests.len());
        let mut stdin = Vec::new();
        for (index, mut request) in requests.into_iter().enumerate() {
            if index > 0 {
                request.stdin = stdin;
            }
            let result = self.execute(request)?;
            stdin = result.stdout.clone();
            results.push(result);
        }
        Ok(results)
    }
    fn execute_interpreter(&self, request: InterpreterRequest) -> Result<ProcessResult, String>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>, String>;
    fn write_file(&self, path: &str, contents: &[u8], append: bool) -> Result<(), String>;
    fn remove_file(&self, path: &str) -> Result<(), String>;
    fn network_request(&self, method: &str, uri: &str) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Policy {
    pub allow_file_read: bool,
    pub allow_file_write: bool,
    pub allow_network: bool,
    pub allow_delegation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunErrorKind {
    Execution,
    Invalid,
    Policy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunError {
    pub kind: RunErrorKind,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TraceEvent {
    Process {
        argv: Vec<String>,
        exit_code: i32,
    },
    FileRead {
        path: String,
    },
    FileWrite {
        path: String,
    },
    FileRemove {
        path: String,
    },
    Network {
        method: String,
        uri: String,
    },
    Delegated {
        interpreter: String,
        interpreter_pin: String,
        exit_code: i32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub trace: Vec<TraceEvent>,
}

#[derive(Clone, Copy)]
pub(crate) struct RunInputs<'a> {
    pub host_environment: &'a BTreeMap<String, String>,
    pub named_inputs: &'a BTreeMap<String, String>,
    pub arguments: &'a [String],
    pub stdin: &'a [u8],
    pub default_working_directory: Option<&'a str>,
}

pub(crate) fn run_plan(
    backend: &dyn Backend,
    policy: Policy,
    plan: &Plan,
    host_environment: &BTreeMap<String, String>,
    named_inputs: &BTreeMap<String, String>,
    arguments: &[String],
) -> Result<RunResult, RunError> {
    run_plan_with_io(
        backend,
        policy,
        plan,
        RunInputs {
            host_environment,
            named_inputs,
            arguments,
            stdin: &[],
            default_working_directory: None,
        },
    )
}

pub(crate) fn run_plan_with_io(
    backend: &dyn Backend,
    policy: Policy,
    plan: &Plan,
    inputs: RunInputs<'_>,
) -> Result<RunResult, RunError> {
    plan.validate()
        .map_err(|errors| invalid(errors.join("; ")))?;
    let tasks: BTreeMap<&str, &Task> = plan
        .tasks
        .iter()
        .map(|task| (task.name.as_str(), task))
        .collect();
    let executor = Executor {
        backend,
        policy,
        tasks,
        host_environment: inputs.host_environment,
        script_arguments: inputs.arguments,
        default_working_directory: inputs.default_working_directory,
    };
    executor.run_task(
        &plan.entrypoint,
        inputs.named_inputs,
        inputs.arguments,
        inputs.stdin.to_vec(),
        &[],
    )
}

#[derive(Clone)]
struct Context {
    variables: BTreeMap<String, String>,
    arguments: BTreeMap<String, String>,
    process_environment: BTreeMap<String, String>,
    secret_names: BTreeSet<String>,
    secret_values: Vec<String>,
}

struct Executor<'a> {
    backend: &'a dyn Backend,
    policy: Policy,
    tasks: BTreeMap<&'a str, &'a Task>,
    host_environment: &'a BTreeMap<String, String>,
    script_arguments: &'a [String],
    default_working_directory: Option<&'a str>,
}

impl Executor<'_> {
    fn run_task(
        &self,
        name: &str,
        provided: &BTreeMap<String, String>,
        positional: &[String],
        stdin: Vec<u8>,
        stack: &[String],
    ) -> Result<RunResult, RunError> {
        if stack.iter().any(|item| item == name) {
            return Err(invalid(format!(
                "recursive task call detected: {} -> {name}",
                stack.join(" -> ")
            )));
        }
        let task = self
            .tasks
            .get(name)
            .ok_or_else(|| invalid(format!("task not found: {name}")))?;
        let arguments = bind_task_arguments(task, provided, positional)?;
        let expected: BTreeSet<&str> = task
            .inputs
            .iter()
            .map(|binding| binding.name.as_str())
            .collect();
        for name in provided.keys() {
            if !expected.contains(name.as_str()) {
                return Err(invalid(format!(
                    "task {} received unknown input {name}",
                    task.name
                )));
            }
        }
        for binding in &task.inputs {
            let value = arguments.get(&binding.name).ok_or_else(|| {
                invalid(format!(
                    "task {} is missing input {}",
                    task.name, binding.name
                ))
            })?;
            normalize_value(&binding.name, &binding.value_type, value).map_err(invalid)?;
        }

        let mut variables = BTreeMap::new();
        let mut process_environment = BTreeMap::new();
        for name in &task.environment {
            if let Some(value) = self.host_environment.get(name) {
                variables.insert(name.clone(), value.clone());
                process_environment.insert(name.clone(), value.clone());
            }
        }
        let secret_names: BTreeSet<String> = task.secrets.iter().cloned().collect();
        let mut secret_values = Vec::new();
        for secret in &secret_names {
            if let Some(value) = arguments.get(secret).or_else(|| variables.get(secret))
                && !value.is_empty()
            {
                secret_values.push(value.clone());
            }
        }
        secret_values.sort_by_key(|value| std::cmp::Reverse(value.len()));
        secret_values.dedup();
        let context = Context {
            variables,
            arguments,
            process_environment,
            secret_names,
            secret_values,
        };
        let mut next_stack = stack.to_vec();
        next_stack.push(name.to_owned());
        self.run_node(&task.body, context, stdin, &next_stack)
            .map(|(result, _)| result)
    }

    fn run_node(
        &self,
        node: &Node,
        context: Context,
        stdin: Vec<u8>,
        stack: &[String],
    ) -> Result<(RunResult, Context), RunError> {
        match &node.operation {
            Operation::Exec {
                argv,
                environment,
                working_directory,
            } => {
                let argv = evaluate_list(argv, &context)?;
                if argv.first().is_none_or(String::is_empty) {
                    return Err(invalid("Exec executable must not be empty"));
                }
                let mut process_environment = context.process_environment.clone();
                for value in environment {
                    process_environment
                        .insert(value.name.clone(), evaluate(&value.value, &context)?);
                }
                let working_directory = working_directory
                    .as_ref()
                    .map(|value| evaluate(value, &context))
                    .transpose()?;
                let working_directory =
                    working_directory.or_else(|| self.default_working_directory.map(str::to_owned));
                if let Some(directory) = &working_directory {
                    validate_runtime_path(directory)?;
                }
                let request = ProcessRequest {
                    argv: argv.clone(),
                    environment: process_environment.into_iter().collect(),
                    working_directory,
                    stdin,
                };
                let process = self
                    .backend
                    .execute(request)
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                let trace_argv = argv
                    .iter()
                    .map(|value| redact(value, &context.secret_values))
                    .collect();
                Ok((
                    RunResult {
                        exit_code: process.exit_code,
                        stdout: process.stdout,
                        stderr: process.stderr,
                        trace: vec![TraceEvent::Process {
                            argv: trace_argv,
                            exit_code: process.exit_code,
                        }],
                    },
                    context,
                ))
            }
            Operation::Pipeline { nodes, status } => {
                if nodes
                    .iter()
                    .all(|child| matches!(child.operation, Operation::Exec { .. }))
                {
                    let mut requests = Vec::with_capacity(nodes.len());
                    let mut trace_argv = Vec::with_capacity(nodes.len());
                    for (index, child) in nodes.iter().enumerate() {
                        let Operation::Exec {
                            argv,
                            environment,
                            working_directory,
                        } = &child.operation
                        else {
                            unreachable!("pipeline shape checked above")
                        };
                        let argv = evaluate_list(argv, &context)?;
                        if argv.first().is_none_or(String::is_empty) {
                            return Err(invalid("Exec executable must not be empty"));
                        }
                        let mut process_environment = context.process_environment.clone();
                        for value in environment {
                            process_environment
                                .insert(value.name.clone(), evaluate(&value.value, &context)?);
                        }
                        let working_directory = working_directory
                            .as_ref()
                            .map(|value| evaluate(value, &context))
                            .transpose()?;
                        let working_directory = working_directory
                            .or_else(|| self.default_working_directory.map(str::to_owned));
                        if let Some(directory) = &working_directory {
                            validate_runtime_path(directory)?;
                        }
                        trace_argv.push(
                            argv.iter()
                                .map(|value| redact(value, &context.secret_values))
                                .collect::<Vec<_>>(),
                        );
                        requests.push(ProcessRequest {
                            argv,
                            environment: process_environment.into_iter().collect(),
                            working_directory,
                            stdin: if index == 0 {
                                stdin.clone()
                            } else {
                                Vec::new()
                            },
                        });
                    }
                    let results = self
                        .backend
                        .execute_pipeline(requests)
                        .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                    if results.len() != nodes.len() {
                        return Err(execution("pipeline backend returned the wrong stage count"));
                    }
                    let mut stderr = Vec::new();
                    let mut trace = Vec::new();
                    let mut pipeline_exit = 0;
                    let mut stdout = Vec::new();
                    for (index, result) in results.into_iter().enumerate() {
                        stderr.extend(result.stderr);
                        if index + 1 == nodes.len() {
                            stdout = result.stdout;
                        }
                        match status {
                            crate::ir::PipelineStatus::Last if index + 1 == nodes.len() => {
                                pipeline_exit = result.exit_code;
                            }
                            crate::ir::PipelineStatus::Pipefail if result.exit_code != 0 => {
                                pipeline_exit = result.exit_code;
                            }
                            _ => {}
                        }
                        trace.push(TraceEvent::Process {
                            argv: trace_argv[index].clone(),
                            exit_code: result.exit_code,
                        });
                    }
                    return Ok((
                        RunResult {
                            exit_code: pipeline_exit,
                            stdout,
                            stderr,
                            trace,
                        },
                        context,
                    ));
                }
                let mut input = stdin;
                let mut stderr = Vec::new();
                let mut trace = Vec::new();
                let mut exit_code = 0;
                let mut stdout = Vec::new();
                for child in nodes {
                    let (result, _) = self.run_node(child, context.clone(), input, stack)?;
                    input = result.stdout.clone();
                    stdout = result.stdout;
                    stderr.extend(result.stderr);
                    trace.extend(result.trace);
                    exit_code = match status {
                        crate::ir::PipelineStatus::Last => result.exit_code,
                        crate::ir::PipelineStatus::Pipefail if result.exit_code != 0 => {
                            result.exit_code
                        }
                        crate::ir::PipelineStatus::Pipefail => exit_code,
                    };
                }
                Ok((
                    RunResult {
                        exit_code,
                        stdout,
                        stderr,
                        trace,
                    },
                    context,
                ))
            }
            Operation::Sequence { nodes } => {
                let mut aggregate = RunResult::empty();
                let mut next_context = context;
                let mut input = stdin;
                for child in nodes {
                    let (result, child_context) =
                        self.run_node(child, next_context, input, stack)?;
                    aggregate = combine(aggregate, result);
                    next_context = child_context;
                    input = Vec::new();
                }
                Ok((aggregate, next_context))
            }
            Operation::Parallel { nodes } => self.run_parallel(nodes, context, stdin, stack),
            Operation::Condition {
                predicate,
                if_true,
                if_false,
            } => {
                let (condition, predicate_context) =
                    self.run_node(predicate, context, stdin, stack)?;
                let branch = if condition.exit_code == 0 {
                    Some(if_true.as_ref())
                } else {
                    if_false.as_deref()
                };
                if let Some(branch) = branch {
                    let (result, branch_context) =
                        self.run_node(branch, predicate_context, Vec::new(), stack)?;
                    Ok((combine(condition, result), branch_context))
                } else {
                    Ok((condition, predicate_context))
                }
            }
            Operation::Match {
                value,
                cases,
                default,
            } => {
                let value = evaluate(value, &context)?;
                let mut selected = None;
                for case in cases {
                    if evaluate(&case.pattern, &context)? == value {
                        selected = Some(&case.body);
                        break;
                    }
                }
                if let Some(branch) = selected.or(default.as_deref()) {
                    self.run_node(branch, context, stdin, stack)
                } else {
                    Ok((RunResult::empty(), context))
                }
            }
            Operation::Foreach {
                variable,
                items,
                body,
            } => {
                let values = evaluate_list(items, &context)?;
                let previous = context.variables.get(variable).cloned();
                let mut next_context = context;
                let mut aggregate = RunResult::empty();
                for value in values {
                    next_context.variables.insert(variable.clone(), value);
                    let (result, child_context) =
                        self.run_node(body, next_context, stdin.clone(), stack)?;
                    aggregate = combine(aggregate, result);
                    next_context = child_context;
                }
                match previous {
                    Some(value) => {
                        next_context.variables.insert(variable.clone(), value);
                    }
                    None => {
                        next_context.variables.remove(variable);
                    }
                }
                Ok((aggregate, next_context))
            }
            Operation::TryFinally { body, finalizer } => {
                match self.run_node(body, context.clone(), stdin, stack) {
                    Err(body_error) => match self.run_node(finalizer, context, Vec::new(), stack) {
                        Ok(_) => Err(body_error),
                        Err(finalizer_error) => Err(RunError {
                            kind: body_error.kind,
                            message: format!(
                                "{}; finalizer also failed: {}",
                                body_error.message, finalizer_error.message
                            ),
                        }),
                    },
                    Ok((body_result, body_context)) => {
                        let (finalizer_result, finalizer_context) =
                            self.run_node(finalizer, body_context, Vec::new(), stack)?;
                        let exit_code = if finalizer_result.exit_code != 0 {
                            finalizer_result.exit_code
                        } else {
                            body_result.exit_code
                        };
                        let mut result = combine(body_result, finalizer_result);
                        result.exit_code = exit_code;
                        Ok((result, finalizer_context))
                    }
                }
            }
            Operation::TaskCall { task, arguments } => {
                let provided = evaluate_named(arguments, &context)?;
                let result = self.run_task(task, &provided, &[], Vec::new(), stack)?;
                Ok((result, context))
            }
            Operation::SetVariable {
                name,
                value_type,
                value,
            } => {
                let value = evaluate(value, &context)?;
                let normalized = normalize_value(name, value_type, &value).map_err(execution)?;
                let mut context = context;
                context.variables.insert(name.clone(), normalized.clone());
                if value_type_is_secret(value_type) {
                    context.secret_names.insert(name.clone());
                    if !normalized.is_empty() {
                        context.secret_values.push(normalized);
                        context
                            .secret_values
                            .sort_by_key(|value| std::cmp::Reverse(value.len()));
                        context.secret_values.dedup();
                    }
                }
                Ok((RunResult::empty(), context))
            }
            Operation::CaptureStdout {
                name,
                value_type,
                body,
            } => {
                let (mut captured, _) = self.run_node(body, context.clone(), stdin, stack)?;
                while captured.stdout.last() == Some(&b'\n') {
                    captured.stdout.pop();
                }
                let text = std::str::from_utf8(&captured.stdout).map_err(|error| {
                    execution(format!("stdout capture {name} is not valid UTF-8: {error}"))
                })?;
                let typed = ValueType::Primitive(value_type.clone());
                let normalized = normalize_value(name, &typed, text).map_err(execution)?;
                let mut context = context;
                context.variables.insert(name.clone(), normalized);
                captured.stdout.clear();
                Ok((captured, context))
            }
            Operation::FileRead { path } => {
                if !self.policy.allow_file_read {
                    return Err(policy("file read denied by policy"));
                }
                let path = evaluate(path, &context)?;
                validate_runtime_path(&path)?;
                let contents = self
                    .backend
                    .read_file(&path)
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                Ok((
                    RunResult {
                        exit_code: 0,
                        stdout: contents,
                        stderr: vec![],
                        trace: vec![TraceEvent::FileRead {
                            path: redact(&path, &context.secret_values),
                        }],
                    },
                    context,
                ))
            }
            Operation::FileWrite {
                path,
                contents,
                append,
            } => {
                if !self.policy.allow_file_write {
                    return Err(policy("file write denied by policy"));
                }
                let path = evaluate(path, &context)?;
                validate_runtime_path(&path)?;
                let contents = evaluate(contents, &context)?;
                self.backend
                    .write_file(&path, contents.as_bytes(), *append)
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                Ok((
                    RunResult {
                        exit_code: 0,
                        stdout: vec![],
                        stderr: vec![],
                        trace: vec![TraceEvent::FileWrite {
                            path: redact(&path, &context.secret_values),
                        }],
                    },
                    context,
                ))
            }
            Operation::FileRemove { path } => {
                if !self.policy.allow_file_write {
                    return Err(policy("file remove denied by policy"));
                }
                let path = evaluate(path, &context)?;
                validate_runtime_path(&path)?;
                self.backend
                    .remove_file(&path)
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                Ok((
                    RunResult {
                        exit_code: 0,
                        stdout: vec![],
                        stderr: vec![],
                        trace: vec![TraceEvent::FileRemove {
                            path: redact(&path, &context.secret_values),
                        }],
                    },
                    context,
                ))
            }
            Operation::NetworkRequest { method, uri } => {
                if !self.policy.allow_network {
                    return Err(policy("network request denied by policy"));
                }
                let method = evaluate(method, &context)?;
                let uri = evaluate(uri, &context)?;
                let response = self
                    .backend
                    .network_request(&method, &uri)
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                Ok((
                    RunResult {
                        exit_code: 0,
                        stdout: response,
                        stderr: vec![],
                        trace: vec![TraceEvent::Network {
                            method,
                            uri: redact(&uri, &context.secret_values),
                        }],
                    },
                    context,
                ))
            }
            Operation::ExpandWords { .. }
            | Operation::Redirect { .. }
            | Operation::Scope { .. }
            | Operation::SetEnvironment { .. }
            | Operation::SetWorkingDirectory { .. }
            | Operation::Spawn { .. }
            | Operation::Wait { .. }
            | Operation::SendSignal { .. }
            | Operation::FileMetadata { .. }
            | Operation::FileSetMetadata { .. }
            | Operation::ClockRead { .. }
            | Operation::RandomBytes { .. } => Err(execution(format!(
                "{} requires an Effect IR v1 backend capability that is unavailable",
                node.operation.name()
            ))),
            Operation::InterpreterCall {
                interpreter,
                interpreter_pin,
                source,
                capabilities,
                ..
            } => {
                if !self.policy.allow_delegation {
                    return Err(policy("pinned interpreter delegation denied by policy"));
                }
                if !matches!(node.guarantee, Guarantee::Delegated { .. }) {
                    return Err(invalid("interpreter call lacks a delegated guarantee"));
                }
                let result = self
                    .backend
                    .execute_interpreter(InterpreterRequest {
                        interpreter: interpreter.clone(),
                        interpreter_pin: interpreter_pin.clone(),
                        source: source.to_bytes().map_err(invalid)?,
                        capabilities: capabilities.clone(),
                        arguments: self.script_arguments.to_vec(),
                        environment: context.process_environment.clone().into_iter().collect(),
                        stdin,
                    })
                    .map_err(|message| execution(redact(&message, &context.secret_values)))?;
                Ok((
                    RunResult {
                        exit_code: result.exit_code,
                        stdout: result.stdout,
                        stderr: result.stderr,
                        trace: vec![TraceEvent::Delegated {
                            interpreter: interpreter.clone(),
                            interpreter_pin: interpreter_pin.clone(),
                            exit_code: result.exit_code,
                        }],
                    },
                    context,
                ))
            }
            Operation::OpaqueCapsule {
                interpreter,
                source,
                ..
            } => {
                let _ = (interpreter, source, stdin);
                Err(policy(
                    "opaque capsule is residual-only and cannot be executed",
                ))
            }
        }
    }

    fn run_parallel(
        &self,
        nodes: &[Node],
        context: Context,
        stdin: Vec<u8>,
        stack: &[String],
    ) -> Result<(RunResult, Context), RunError> {
        if nodes.is_empty() {
            return Ok((RunResult::empty(), context));
        }
        let count = nodes.len();
        let workers = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .clamp(1, 8)
            .min(count);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results = std::sync::Mutex::new(vec![None; count]);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                let next = &next;
                let results = &results;
                let context = context.clone();
                let stdin = stdin.clone();
                scope.spawn(move || {
                    loop {
                        use std::sync::atomic::Ordering;
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(node) = nodes.get(index) else { break };
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            self.run_node(node, context.clone(), stdin.clone(), stack)
                        }))
                        .unwrap_or_else(|_| Err(execution("parallel worker panicked")));
                        let Ok(mut output) = results.lock() else {
                            break;
                        };
                        output[index] = Some(result);
                    }
                });
            }
        });
        let mut aggregate = RunResult::empty();
        for result in results
            .into_inner()
            .map_err(|_| execution("parallel result lock poisoned"))?
        {
            let (result, _) =
                result.ok_or_else(|| execution("parallel worker omitted a result"))??;
            aggregate = combine(aggregate, result);
        }
        Ok((aggregate, context))
    }
}

impl RunResult {
    fn empty() -> Self {
        Self {
            exit_code: 0,
            stdout: vec![],
            stderr: vec![],
            trace: vec![],
        }
    }
}

fn combine(mut left: RunResult, right: RunResult) -> RunResult {
    left.exit_code = right.exit_code;
    left.stdout.extend(right.stdout);
    left.stderr.extend(right.stderr);
    left.trace.extend(right.trace);
    left
}

fn evaluate(expression: &TextExpression, context: &Context) -> Result<String, RunError> {
    expression
        .evaluate(&context.variables, &context.arguments)
        .map_err(invalid)
}

fn evaluate_list(
    expressions: &[TextExpression],
    context: &Context,
) -> Result<Vec<String>, RunError> {
    expressions
        .iter()
        .map(|expression| evaluate(expression, context))
        .collect()
}

fn evaluate_named(
    values: &[NamedExpression],
    context: &Context,
) -> Result<BTreeMap<String, String>, RunError> {
    let mut output = BTreeMap::new();
    for value in values {
        if output
            .insert(value.name.clone(), evaluate(&value.value, context)?)
            .is_some()
        {
            return Err(invalid(format!("duplicate named value: {}", value.name)));
        }
    }
    Ok(output)
}

fn bind_task_arguments(
    task: &Task,
    provided: &BTreeMap<String, String>,
    positional: &[String],
) -> Result<BTreeMap<String, String>, RunError> {
    if task.invocation.is_some() {
        return bind_powershell_arguments(task, provided, positional);
    }
    let mut output = provided.clone();
    if !positional.is_empty() && task.inputs.is_empty() {
        return Ok(output);
    }
    for (index, value) in positional.iter().enumerate() {
        let numeric = (index + 1).to_string();
        let binding = task
            .inputs
            .iter()
            .find(|binding| binding.name == numeric)
            .or_else(|| task.inputs.get(index));
        let Some(binding) = binding else {
            return Err(invalid(format!("unexpected positional argument: {value}")));
        };
        if output.insert(binding.name.clone(), value.clone()).is_some() {
            return Err(invalid(format!(
                "task input {} was specified more than once",
                binding.name
            )));
        }
    }
    Ok(output)
}

fn bind_powershell_arguments(
    task: &Task,
    provided: &BTreeMap<String, String>,
    positional: &[String],
) -> Result<BTreeMap<String, String>, RunError> {
    let invocation = task.invocation.as_ref().expect("checked by caller");
    let mut output = BTreeMap::new();
    for (name, value) in provided {
        let parameter = invocation
            .parameters
            .iter()
            .find(|parameter| parameter.input.eq_ignore_ascii_case(name))
            .ok_or_else(|| invalid(format!("unknown PowerShell parameter: -{name}")))?;
        if output
            .insert(parameter.input.clone(), value.clone())
            .is_some()
        {
            return Err(invalid(format!(
                "PowerShell parameter -{name} was specified more than once"
            )));
        }
    }
    let mut index = 0;
    while index < positional.len() {
        let argument = &positional[index];
        if let Some(raw) = argument
            .strip_prefix('-')
            .filter(|value| !value.is_empty() && !value.as_bytes()[0].is_ascii_digit())
        {
            let (name, inline) = raw
                .split_once(':')
                .map_or((raw, None), |(name, value)| (name, Some(value)));
            let mut prefix_match = None;
            let mut ambiguous = false;
            let mut exact = None;
            for parameter in &invocation.parameters {
                if parameter.input.eq_ignore_ascii_case(name) {
                    exact = Some(parameter);
                    break;
                }
                if parameter
                    .input
                    .get(..name.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                {
                    ambiguous |= prefix_match.is_some();
                    prefix_match.get_or_insert(parameter);
                }
            }
            let parameter = match (exact, prefix_match, ambiguous) {
                (Some(exact), _, _) => exact,
                (None, Some(parameter), false) => parameter,
                (None, None, _) => {
                    return Err(invalid(format!("unknown PowerShell parameter: -{name}")));
                }
                (None, Some(_), true) => {
                    return Err(invalid(format!("ambiguous PowerShell parameter: -{name}")));
                }
            };
            let value = if parameter.is_switch {
                inline.unwrap_or("true").to_owned()
            } else if let Some(value) = inline {
                value.to_owned()
            } else {
                index += 1;
                positional.get(index).cloned().ok_or_else(|| {
                    invalid(format!(
                        "PowerShell parameter -{} requires a value",
                        parameter.input
                    ))
                })?
            };
            if output.insert(parameter.input.clone(), value).is_some() {
                return Err(invalid(format!(
                    "PowerShell parameter -{} was specified more than once",
                    parameter.input
                )));
            }
        } else {
            let mut candidates: Vec<_> = invocation
                .parameters
                .iter()
                .filter_map(|parameter| parameter.position.map(|position| (position, parameter)))
                .collect();
            candidates.sort_by_key(|(position, _)| *position);
            let parameter = candidates
                .into_iter()
                .map(|(_, parameter)| parameter)
                .find(|parameter| !output.contains_key(&parameter.input))
                .ok_or_else(|| {
                    invalid(format!(
                        "unexpected positional PowerShell argument: {argument}"
                    ))
                })?;
            output.insert(parameter.input.clone(), argument.clone());
        }
        index += 1;
    }
    for parameter in &invocation.parameters {
        if !output.contains_key(&parameter.input) {
            if parameter.required {
                return Err(invalid(format!(
                    "missing mandatory PowerShell parameter -{}",
                    parameter.input
                )));
            }
            let value = if let Some(default) = &parameter.default {
                default
                    .evaluate(&BTreeMap::new(), &BTreeMap::new())
                    .map_err(invalid)?
            } else if parameter.is_switch {
                "false".into()
            } else {
                let binding = task
                    .inputs
                    .iter()
                    .find(|binding| binding.name == parameter.input)
                    .ok_or_else(|| invalid(format!("unknown task input: {}", parameter.input)))?;
                default_value(&binding.value_type)
                    .ok_or_else(|| invalid(format!("task input {} has no default", binding.name)))?
            };
            output.insert(parameter.input.clone(), value);
        }
    }
    let mut normalized = BTreeMap::new();
    for binding in &task.inputs {
        let raw = output.get(&binding.name).ok_or_else(|| {
            invalid(format!(
                "task {} is missing input {}",
                task.name, binding.name
            ))
        })?;
        normalized.insert(
            binding.name.clone(),
            normalize_value(&binding.name, &binding.value_type, raw).map_err(invalid)?,
        );
    }
    Ok(normalized)
}

fn default_value(value_type: &ValueType) -> Option<String> {
    match value_type {
        ValueType::Primitive(PrimitiveType::Text | PrimitiveType::Path) => Some(String::new()),
        ValueType::Primitive(PrimitiveType::Bool) => Some("false".into()),
        ValueType::Primitive(PrimitiveType::Int) => Some("0".into()),
        ValueType::Primitive(PrimitiveType::Bytes) => Some(String::new()),
        ValueType::List { .. } => Some("[]".into()),
        ValueType::Record { record } => {
            let mut value = serde_json::Map::new();
            for field in record {
                let default = default_value(&field.value_type)?;
                let parsed = typed_value_as_json(&field.value_type, &default).ok()?;
                value.insert(field.name.clone(), parsed);
            }
            crate::canonical_json::canonical_bytes(&serde_json::Value::Object(value))
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        }
        ValueType::Secret { secret } => default_value(secret),
    }
}

fn normalize_value(name: &str, value_type: &ValueType, value: &str) -> Result<String, String> {
    match value_type {
        ValueType::Primitive(PrimitiveType::Text) => Ok(value.to_owned()),
        ValueType::Primitive(PrimitiveType::Bool) => match value.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok("true".into()),
            "false" | "0" => Ok("false".into()),
            _ => Err(format!("{name} must be a boolean")),
        },
        ValueType::Primitive(PrimitiveType::Int) => {
            let parsed = value
                .trim()
                .parse::<i64>()
                .map_err(|_| format!("{name} must be a signed 64-bit integer"))?;
            Ok(parsed.to_string())
        }
        ValueType::Primitive(PrimitiveType::Path) => {
            let normalized = crate::ir::normalize_path(value)
                .map_err(|error| format!("{name} must be a normalized path: {error}"))?;
            if normalized != value {
                return Err(format!("{name} must use normalized / path separators"));
            }
            Ok(normalized)
        }
        ValueType::Primitive(PrimitiveType::Bytes) => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|_| format!("{name} must be canonical base64 bytes"))?;
            if base64::engine::general_purpose::STANDARD.encode(&decoded) != value {
                return Err(format!("{name} must be canonical padded base64 bytes"));
            }
            Ok(value.into())
        }
        ValueType::List { .. } | ValueType::Record { .. } => {
            let parsed = crate::strict_json::parse(value.as_bytes())
                .map_err(|error| format!("{name} must be strict JSON: {error}"))?;
            let normalized = normalize_typed_json(name, value_type, parsed)?;
            String::from_utf8(crate::canonical_json::canonical_bytes(&normalized)?)
                .map_err(|_| format!("{name} canonical JSON was not UTF-8"))
        }
        ValueType::Secret { secret } => normalize_value(name, secret, value),
    }
}

fn value_type_is_secret(value_type: &ValueType) -> bool {
    match value_type {
        ValueType::Secret { .. } => true,
        ValueType::List { list } => value_type_is_secret(list),
        ValueType::Record { record } => record
            .iter()
            .any(|field| value_type_is_secret(&field.value_type)),
        ValueType::Primitive(_) => false,
    }
}

fn typed_value_as_json(value_type: &ValueType, value: &str) -> Result<serde_json::Value, String> {
    match value_type {
        ValueType::Primitive(PrimitiveType::Text | PrimitiveType::Path | PrimitiveType::Bytes) => {
            Ok(serde_json::Value::String(value.into()))
        }
        ValueType::Primitive(PrimitiveType::Bool) => Ok(serde_json::Value::Bool(value == "true")),
        ValueType::Primitive(PrimitiveType::Int) => value
            .parse::<i64>()
            .map(Into::into)
            .map_err(|_| "invalid normalized integer".into()),
        ValueType::List { .. } | ValueType::Record { .. } => {
            crate::strict_json::parse(value.as_bytes())
        }
        ValueType::Secret { secret } => typed_value_as_json(secret, value),
    }
}

fn normalize_typed_json(
    name: &str,
    value_type: &ValueType,
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match value_type {
        ValueType::Primitive(primitive) => {
            let raw = match (primitive, value) {
                (
                    PrimitiveType::Text | PrimitiveType::Path | PrimitiveType::Bytes,
                    serde_json::Value::String(value),
                ) => value,
                (PrimitiveType::Bool, serde_json::Value::Bool(value)) => value.to_string(),
                (PrimitiveType::Int, serde_json::Value::Number(value)) if value.is_i64() => {
                    value.to_string()
                }
                _ => return Err(format!("{name} JSON value has the wrong type")),
            };
            typed_value_as_json(value_type, &normalize_value(name, value_type, &raw)?)
        }
        ValueType::Secret { secret } => normalize_typed_json(name, secret, value),
        ValueType::List { list } => {
            let serde_json::Value::Array(values) = value else {
                return Err(format!("{name} must be a JSON array"));
            };
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    normalize_typed_json(&format!("{name}[{index}]"), list, value)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(serde_json::Value::Array)
        }
        ValueType::Record { record } => {
            let serde_json::Value::Object(mut values) = value else {
                return Err(format!("{name} must be a JSON object"));
            };
            let mut normalized = serde_json::Map::new();
            for field in record {
                let value = values
                    .remove(&field.name)
                    .ok_or_else(|| format!("{name} is missing record field {}", field.name))?;
                normalized.insert(
                    field.name.clone(),
                    normalize_typed_json(
                        &format!("{name}.{}", field.name),
                        &field.value_type,
                        value,
                    )?,
                );
            }
            if let Some(unknown) = values.keys().next() {
                return Err(format!("{name} has unknown record field {unknown}"));
            }
            Ok(serde_json::Value::Object(normalized))
        }
    }
}

fn validate_runtime_path(path: &str) -> Result<(), RunError> {
    let normalized = crate::ir::normalize_path(path).map_err(invalid)?;
    if normalized != path {
        return Err(invalid(format!("runtime path is not normalized: {path}")));
    }
    Ok(())
}

fn redact(message: &str, secrets: &[String]) -> String {
    let mut output = message.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            output = output.replace(secret, "<redacted>");
        }
    }
    output
}

fn execution(message: impl Into<String>) -> RunError {
    RunError {
        kind: RunErrorKind::Execution,
        message: message.into(),
    }
}
fn invalid(message: impl Into<String>) -> RunError {
    RunError {
        kind: RunErrorKind::Invalid,
        message: message.into(),
    }
}
fn policy(message: impl Into<String>) -> RunError {
    RunError {
        kind: RunErrorKind::Policy,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        Binding, Guarantee, Invocation, InvocationParameter, InvocationStyle, MatchCase,
        NamedExpression, Node, Operation, PrimitiveType, RecordField, SourceBytes, SourceSpan,
        Task, TextExpression, TextPart, ValueType,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockBackend {
        calls: Mutex<Vec<ProcessRequest>>,
        delegated: Mutex<Vec<InterpreterRequest>>,
    }

    impl Backend for MockBackend {
        fn execute(&self, request: ProcessRequest) -> Result<ProcessResult, String> {
            self.calls.lock().unwrap().push(request.clone());
            match request.argv.as_slice() {
                [command, value] if command == "emit" => Ok(ProcessResult {
                    exit_code: 0,
                    stdout: value.as_bytes().to_vec(),
                    stderr: vec![],
                }),
                [command] if command == "upper" => Ok(ProcessResult {
                    exit_code: 0,
                    stdout: request.stdin.iter().map(u8::to_ascii_uppercase).collect(),
                    stderr: vec![],
                }),
                [command, code] if command == "fail" => Ok(ProcessResult {
                    exit_code: code.parse().unwrap(),
                    stdout: vec![],
                    stderr: b"failed".to_vec(),
                }),
                [command] if command == "invalid-utf8" => Ok(ProcessResult {
                    exit_code: 0,
                    stdout: vec![0xff],
                    stderr: vec![],
                }),
                [command] if command == "backend-secret-error" => {
                    Err("failed with super-secret-value".into())
                }
                [command] if command == "panic" => panic!("injected backend panic"),
                _ => Ok(ProcessResult {
                    exit_code: 0,
                    stdout: request.stdin,
                    stderr: vec![],
                }),
            }
        }
        fn execute_interpreter(
            &self,
            request: InterpreterRequest,
        ) -> Result<ProcessResult, String> {
            self.delegated.lock().unwrap().push(request.clone());
            Ok(ProcessResult {
                exit_code: 0,
                stdout: request.source,
                stderr: vec![],
            })
        }
        fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
            if path == "fail" {
                return Err("read failed".into());
            }
            Ok(format!("read:{path}").into_bytes())
        }
        fn write_file(&self, path: &str, _contents: &[u8], _append: bool) -> Result<(), String> {
            if path == "fail" {
                return Err("write failed".into());
            }
            Ok(())
        }
        fn remove_file(&self, path: &str) -> Result<(), String> {
            if path == "fail" {
                return Err("remove failed".into());
            }
            Ok(())
        }
        fn network_request(&self, _method: &str, uri: &str) -> Result<Vec<u8>, String> {
            if uri == "fail" {
                return Err("network failed".into());
            }
            Ok(format!("network:{uri}").into_bytes())
        }
    }

    fn node(operation: Operation) -> Node {
        Node {
            id: String::new(),
            operation,
            guarantee: Guarantee::Native {
                semantic_model: "runner-test-v1".into(),
            },
            source: None,
        }
    }

    fn plan(body: Node) -> Plan {
        let mut plan = Plan {
            schema_version: 1,
            generator: "test".into(),
            entrypoint: "main".into(),
            tasks: vec![Task {
                name: "main".into(),
                inputs: vec![],
                outputs: vec![],
                environment: vec![],
                secrets: vec![],
                platform_capabilities: vec![],
                cacheable: false,
                invocation: None,
                body,
            }],
        };
        plan.assign_node_ids().unwrap();
        plan
    }

    fn exec(argv: &[&str]) -> Node {
        node(Operation::Exec {
            argv: argv
                .iter()
                .map(|value| TextExpression::literal(*value))
                .collect(),
            environment: vec![],
            working_directory: None,
        })
    }

    fn variable(name: &str) -> TextExpression {
        TextExpression {
            parts: vec![TextPart::Variable { name: name.into() }],
        }
    }

    fn run(backend: &MockBackend, body: Node) -> Result<RunResult, RunError> {
        run_plan(
            backend,
            Policy::default(),
            &plan(body),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &[],
        )
    }

    #[test]
    fn pipeline_streams_raw_bytes_and_sequence_uses_last_status() {
        let backend = MockBackend::default();
        let pipeline = node(Operation::Pipeline {
            nodes: vec![exec(&["emit", "hello"]), exec(&["upper"])],
            status: crate::ir::PipelineStatus::Last,
        });
        assert_eq!(run(&backend, pipeline).unwrap().stdout, b"HELLO");
        let sequence = node(Operation::Sequence {
            nodes: vec![exec(&["fail", "9"]), exec(&["emit", "after"])],
        });
        let result = run(&backend, sequence).unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, b"after");
        assert_eq!(result.stderr, b"failed");
    }

    #[test]
    fn expression_values_are_never_reparsed_by_runner() {
        let backend = MockBackend::default();
        let body = node(Operation::Exec {
            argv: vec![
                TextExpression::literal("emit"),
                TextExpression {
                    parts: vec![
                        TextPart::Variable {
                            name: "FIRST".into(),
                        },
                        TextPart::Argument {
                            name: "input".into(),
                        },
                    ],
                },
            ],
            environment: vec![],
            working_directory: None,
        });
        let mut plan = plan(body);
        plan.tasks[0].environment = vec!["FIRST".into(), "SECOND".into()];
        plan.tasks[0].inputs = vec![Binding {
            name: "input".into(),
            value_type: ValueType::Primitive(PrimitiveType::Text),
        }];
        plan.tasks[0].secrets = vec![];
        plan.assign_node_ids().unwrap();
        let host = BTreeMap::from([
            ("FIRST".into(), "$SECOND".into()),
            ("SECOND".into(), "wrong".into()),
        ]);
        let inputs = BTreeMap::from([("input".into(), "-${SECOND}".into())]);
        let result = run_plan(&backend, Policy::default(), &plan, &host, &inputs, &[]).unwrap();
        assert_eq!(result.stdout, b"$SECOND-${SECOND}");
    }

    #[test]
    fn capture_validates_utf8_only_at_the_text_boundary() {
        let backend = MockBackend::default();
        let body = node(Operation::CaptureStdout {
            name: "CAPTURED".into(),
            value_type: PrimitiveType::Text,
            body: Box::new(exec(&["invalid-utf8"])),
        });
        let error = run(&backend, body).unwrap_err();
        assert_eq!(error.kind, RunErrorKind::Execution);
        assert!(error.message.contains("UTF-8"));
    }

    #[test]
    fn policy_denials_do_not_call_backends() {
        let backend = MockBackend::default();
        let body = node(Operation::FileRead {
            path: TextExpression::literal("input.txt"),
        });
        let error = run(&backend, body).unwrap_err();
        assert_eq!(error.kind, RunErrorKind::Policy);
        assert!(backend.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn opaque_capsules_are_residual_only_and_never_execute() {
        let backend = MockBackend::default();
        let source = vec![b'e', b'c', b'h', b'o', b' ', 0xff];
        let mut capsule = node(Operation::OpaqueCapsule {
            interpreter: "sh".into(),
            source: SourceBytes::from_bytes(&source),
            path: Some("build.sh".into()),
        });
        capsule.guarantee = Guarantee::Residual {
            reason: "non-UTF-8".into(),
        };
        let error = run_plan(
            &backend,
            Policy {
                ..Policy::default()
            },
            &plan(capsule),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &["one".into()],
        )
        .unwrap_err();
        assert_eq!(error.kind, RunErrorKind::Policy);
        assert!(error.message.contains("residual-only"));
    }

    #[test]
    fn secret_values_are_redacted_from_backend_errors_and_trace() {
        let backend = MockBackend::default();
        let body = node(Operation::Exec {
            argv: vec![TextExpression::literal("backend-secret-error")],
            environment: vec![NamedExpression {
                name: "TOKEN".into(),
                value: TextExpression {
                    parts: vec![TextPart::Variable {
                        name: "TOKEN".into(),
                    }],
                },
            }],
            working_directory: None,
        });
        let mut plan = plan(body);
        plan.tasks[0].environment = vec!["TOKEN".into()];
        plan.tasks[0].secrets = vec!["TOKEN".into()];
        plan.assign_node_ids().unwrap();
        let error = run_plan(
            &backend,
            Policy::default(),
            &plan,
            &BTreeMap::from([("TOKEN".into(), "super-secret-value".into())]),
            &BTreeMap::new(),
            &[],
        )
        .unwrap_err();
        assert!(!error.message.contains("super-secret-value"));
        assert!(error.message.contains("<redacted>"));
    }

    #[test]
    fn parallel_output_is_merged_in_declared_order() {
        let backend = MockBackend::default();
        let body = node(Operation::Parallel {
            nodes: vec![
                exec(&["emit", "a"]),
                exec(&["emit", "b"]),
                exec(&["emit", "c"]),
            ],
        });
        assert_eq!(run(&backend, body).unwrap().stdout, b"abc");
    }

    #[test]
    fn parallel_worker_panics_become_execution_errors() {
        let backend = MockBackend::default();
        let body = node(Operation::Parallel {
            nodes: vec![exec(&["panic"]), exec(&["emit", "after"])],
        });
        let error = run(&backend, body).unwrap_err();
        assert_eq!(error.kind, RunErrorKind::Execution);
        assert!(error.message.contains("parallel worker panicked"));
    }

    #[test]
    fn control_flow_and_effect_nodes_execute_with_typed_mutable_context() {
        let backend = MockBackend::default();
        let body = node(Operation::Sequence {
            nodes: vec![
                node(Operation::SetVariable {
                    name: "VALUE".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Text),
                    value: TextExpression::literal("seed"),
                }),
                node(Operation::CaptureStdout {
                    name: "CAPTURED".into(),
                    value_type: PrimitiveType::Text,
                    body: Box::new(exec(&["emit", "captured\n\n"])),
                }),
                node(Operation::Exec {
                    argv: vec![TextExpression::literal("emit"), variable("CAPTURED")],
                    environment: vec![],
                    working_directory: Some(TextExpression::literal("work")),
                }),
                node(Operation::Condition {
                    predicate: Box::new(exec(&["fail", "1"])),
                    if_true: Box::new(exec(&["emit", "wrong"])),
                    if_false: Some(Box::new(exec(&["emit", "false-branch"]))),
                }),
                node(Operation::Match {
                    value: TextExpression::literal("selected"),
                    cases: vec![
                        MatchCase {
                            pattern: TextExpression::literal("other"),
                            body: exec(&["emit", "wrong"]),
                        },
                        MatchCase {
                            pattern: TextExpression::literal("selected"),
                            body: exec(&["emit", "matched"]),
                        },
                    ],
                    default: Some(Box::new(exec(&["emit", "default"]))),
                }),
                node(Operation::Foreach {
                    variable: "ITEM".into(),
                    items: vec![TextExpression::literal("a"), TextExpression::literal("b")],
                    body: Box::new(node(Operation::Exec {
                        argv: vec![TextExpression::literal("emit"), variable("ITEM")],
                        environment: vec![],
                        working_directory: None,
                    })),
                }),
                node(Operation::FileWrite {
                    path: TextExpression::literal("out.txt"),
                    contents: variable("VALUE"),
                    append: false,
                }),
                node(Operation::FileRead {
                    path: TextExpression::literal("out.txt"),
                }),
                node(Operation::FileRemove {
                    path: TextExpression::literal("out.txt"),
                }),
                node(Operation::NetworkRequest {
                    method: TextExpression::literal("GET"),
                    uri: TextExpression::literal("https://example.invalid/value"),
                }),
            ],
        });
        let result = run_plan_with_io(
            &backend,
            Policy {
                allow_file_read: true,
                allow_file_write: true,
                allow_network: true,
                allow_delegation: false,
            },
            &plan(body),
            RunInputs {
                host_environment: &BTreeMap::new(),
                named_inputs: &BTreeMap::new(),
                arguments: &[],
                stdin: b"stdin",
                default_working_directory: Some("default"),
            },
        )
        .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.starts_with(b"capturedfalse-branchmatchedab"));
        assert!(
            result
                .stdout
                .ends_with(b"network:https://example.invalid/value")
        );
        assert_eq!(result.stderr, b"failed");
        assert!(
            result
                .trace
                .iter()
                .any(|event| matches!(event, TraceEvent::FileRead { .. }))
        );
        assert!(
            result
                .trace
                .iter()
                .any(|event| matches!(event, TraceEvent::FileWrite { .. }))
        );
        assert!(
            result
                .trace
                .iter()
                .any(|event| matches!(event, TraceEvent::FileRemove { .. }))
        );
        assert!(
            result
                .trace
                .iter()
                .any(|event| matches!(event, TraceEvent::Network { .. }))
        );
        assert_eq!(
            backend.calls.lock().unwrap()[1]
                .working_directory
                .as_deref(),
            Some("work")
        );
    }

    #[test]
    fn task_calls_try_finally_and_delegation_cover_success_and_failure_semantics() {
        let backend = MockBackend::default();
        let helper = Task {
            name: "helper".into(),
            inputs: vec![Binding {
                name: "message".into(),
                value_type: ValueType::Primitive(PrimitiveType::Text),
            }],
            outputs: vec![],
            environment: vec![],
            secrets: vec![],
            platform_capabilities: vec![],
            cacheable: false,
            invocation: None,
            body: node(Operation::Exec {
                argv: vec![
                    TextExpression::literal("emit"),
                    TextExpression {
                        parts: vec![TextPart::Argument {
                            name: "message".into(),
                        }],
                    },
                ],
                environment: vec![],
                working_directory: None,
            }),
        };
        let mut task_plan = plan(node(Operation::TaskCall {
            task: "helper".into(),
            arguments: vec![NamedExpression {
                name: "message".into(),
                value: TextExpression::literal("called"),
            }],
        }));
        task_plan.tasks.push(helper);
        task_plan.assign_node_ids().unwrap();
        assert_eq!(
            run_plan(
                &backend,
                Policy::default(),
                &task_plan,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &[],
            )
            .unwrap()
            .stdout,
            b"called"
        );

        let success = node(Operation::TryFinally {
            body: Box::new(exec(&["emit", "body"])),
            finalizer: Box::new(exec(&["fail", "7"])),
        });
        let result = run(&backend, success).unwrap();
        assert_eq!(result.exit_code, 7);
        assert_eq!(result.stdout, b"body");

        let body_error = node(Operation::TryFinally {
            body: Box::new(exec(&["backend-secret-error"])),
            finalizer: Box::new(exec(&["emit", "cleanup"])),
        });
        assert!(
            run(&backend, body_error)
                .unwrap_err()
                .message
                .contains("failed with")
        );
        let both_error = node(Operation::TryFinally {
            body: Box::new(exec(&["backend-secret-error"])),
            finalizer: Box::new(exec(&["backend-secret-error"])),
        });
        assert!(
            run(&backend, both_error)
                .unwrap_err()
                .message
                .contains("finalizer also failed")
        );

        let delegated_span = SourceSpan {
            file: "build.sh".into(),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 16,
            start_byte: 0,
            end_byte: 16,
        };
        let mut delegated = node(Operation::InterpreterCall {
            interpreter: "sh".into(),
            interpreter_pin: format!("sha256:{}", "a".repeat(64)),
            source: SourceBytes::from_bytes(b"printf delegated"),
            source_span: delegated_span.clone(),
            capabilities: vec!["process".into()],
            reason: "fixture".into(),
        });
        delegated.guarantee = Guarantee::Delegated {
            reason: "fixture".into(),
        };
        delegated.source = Some(delegated_span);
        let result = run_plan_with_io(
            &backend,
            Policy {
                allow_delegation: true,
                ..Policy::default()
            },
            &plan(delegated),
            RunInputs {
                host_environment: &BTreeMap::new(),
                named_inputs: &BTreeMap::new(),
                arguments: &["one".into()],
                stdin: b"input",
                default_working_directory: None,
            },
        )
        .unwrap();
        assert_eq!(result.stdout, b"printf delegated");
        assert!(matches!(
            result.trace.as_slice(),
            [TraceEvent::Delegated { .. }]
        ));
    }

    #[test]
    fn effect_policies_paths_and_backend_failures_are_fail_closed() {
        let backend = MockBackend::default();
        for operation in [
            Operation::FileWrite {
                path: TextExpression::literal("out"),
                contents: TextExpression::literal("value"),
                append: true,
            },
            Operation::FileRemove {
                path: TextExpression::literal("out"),
            },
            Operation::NetworkRequest {
                method: TextExpression::literal("GET"),
                uri: TextExpression::literal("https://example.invalid"),
            },
        ] {
            assert_eq!(
                run(&backend, node(operation)).unwrap_err().kind,
                RunErrorKind::Policy
            );
        }
        for operation in [
            Operation::FileRead {
                path: TextExpression::literal("../escape"),
            },
            Operation::FileWrite {
                path: TextExpression::literal("../escape"),
                contents: TextExpression::literal("value"),
                append: false,
            },
        ] {
            let error = run_plan(
                &backend,
                Policy {
                    allow_file_read: true,
                    allow_file_write: true,
                    ..Policy::default()
                },
                &plan(node(operation)),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &[],
            )
            .unwrap_err();
            assert_eq!(error.kind, RunErrorKind::Invalid);
        }
        for operation in [
            Operation::FileRead {
                path: TextExpression::literal("fail"),
            },
            Operation::FileWrite {
                path: TextExpression::literal("fail"),
                contents: TextExpression::literal("value"),
                append: false,
            },
            Operation::FileRemove {
                path: TextExpression::literal("fail"),
            },
            Operation::NetworkRequest {
                method: TextExpression::literal("GET"),
                uri: TextExpression::literal("fail"),
            },
        ] {
            let error = run_plan(
                &backend,
                Policy {
                    allow_file_read: true,
                    allow_file_write: true,
                    allow_network: true,
                    allow_delegation: false,
                },
                &plan(node(operation)),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &[],
            )
            .unwrap_err();
            assert_eq!(error.kind, RunErrorKind::Execution);
        }
        let unsupported = node(Operation::ExpandWords {
            name: "words".into(),
            value: TextExpression::literal("value"),
            field_splitting: crate::ir::FieldSplitting::None,
            glob: crate::ir::GlobBehavior::Disabled,
        });
        assert!(
            run(&backend, unsupported)
                .unwrap_err()
                .message
                .contains("unavailable")
        );
    }

    #[test]
    fn typed_values_and_powershell_binding_are_canonical_and_strict() {
        let list = ValueType::List {
            list: Box::new(ValueType::Primitive(PrimitiveType::Int)),
        };
        let record = ValueType::Record {
            record: vec![
                RecordField {
                    name: "enabled".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Bool),
                },
                RecordField {
                    name: "items".into(),
                    value_type: list.clone(),
                },
            ],
        };
        assert_eq!(
            normalize_value("flag", &ValueType::Primitive(PrimitiveType::Bool), "1").unwrap(),
            "true"
        );
        assert_eq!(
            normalize_value("flag", &ValueType::Primitive(PrimitiveType::Bool), "0").unwrap(),
            "false"
        );
        assert!(
            normalize_value("flag", &ValueType::Primitive(PrimitiveType::Bool), "maybe").is_err()
        );
        assert_eq!(
            normalize_value("count", &ValueType::Primitive(PrimitiveType::Int), " 7 ").unwrap(),
            "7"
        );
        assert!(
            normalize_value("count", &ValueType::Primitive(PrimitiveType::Int), "huge").is_err()
        );
        assert_eq!(
            normalize_value("path", &ValueType::Primitive(PrimitiveType::Path), "a/b").unwrap(),
            "a/b"
        );
        assert!(
            normalize_value("path", &ValueType::Primitive(PrimitiveType::Path), "a\\b").is_err()
        );
        assert_eq!(
            normalize_value("bytes", &ValueType::Primitive(PrimitiveType::Bytes), "AA==").unwrap(),
            "AA=="
        );
        assert!(
            normalize_value("bytes", &ValueType::Primitive(PrimitiveType::Bytes), "AA").is_err()
        );
        assert_eq!(normalize_value("items", &list, "[2,1]").unwrap(), "[2,1]");
        assert!(normalize_value("items", &list, "{}").is_err());
        assert_eq!(
            normalize_value("record", &record, r#"{"items":[1],"enabled":true}"#).unwrap(),
            r#"{"enabled":true,"items":[1]}"#
        );
        assert!(normalize_value("record", &record, r#"{"enabled":true}"#).is_err());
        assert!(
            normalize_value(
                "record",
                &record,
                r#"{"enabled":true,"items":[],"extra":1}"#
            )
            .is_err()
        );
        assert!(value_type_is_secret(&ValueType::List {
            list: Box::new(ValueType::Secret {
                secret: Box::new(ValueType::Primitive(PrimitiveType::Text)),
            }),
        }));
        assert_eq!(
            default_value(&record).unwrap(),
            r#"{"enabled":false,"items":[]}"#
        );

        let task = Task {
            name: "powershell".into(),
            inputs: vec![
                Binding {
                    name: "Name".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Text),
                },
                Binding {
                    name: "Flag".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Bool),
                },
                Binding {
                    name: "Count".into(),
                    value_type: ValueType::Primitive(PrimitiveType::Int),
                },
            ],
            outputs: vec![],
            environment: vec![],
            secrets: vec![],
            platform_capabilities: vec![],
            cacheable: false,
            invocation: Some(Invocation {
                style: InvocationStyle::Powershell,
                accepts_common_parameters: false,
                parameters: vec![
                    InvocationParameter {
                        input: "Name".into(),
                        position: Some(0),
                        required: true,
                        is_switch: false,
                        default: None,
                        validations: vec![],
                    },
                    InvocationParameter {
                        input: "Flag".into(),
                        position: None,
                        required: false,
                        is_switch: true,
                        default: None,
                        validations: vec![],
                    },
                    InvocationParameter {
                        input: "Count".into(),
                        position: None,
                        required: false,
                        is_switch: false,
                        default: Some(TextExpression::literal("7")),
                        validations: vec![],
                    },
                ],
            }),
            body: exec(&["emit", "unused"]),
        };
        let bound = bind_task_arguments(
            &task,
            &BTreeMap::new(),
            &["alice".into(), "-Fl".into(), "-Count:9".into()],
        )
        .unwrap();
        assert_eq!(bound["Name"], "alice");
        assert_eq!(bound["Flag"], "true");
        assert_eq!(bound["Count"], "9");
        assert!(bind_task_arguments(&task, &BTreeMap::new(), &[]).is_err());
        assert!(
            bind_task_arguments(
                &task,
                &BTreeMap::new(),
                &["alice".into(), "-Unknown".into()]
            )
            .is_err()
        );
        assert!(
            bind_task_arguments(
                &task,
                &BTreeMap::from([("Name".into(), "a".into())]),
                &["b".into()]
            )
            .is_err()
        );
    }
}
