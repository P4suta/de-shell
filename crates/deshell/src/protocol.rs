use base64::Engine as _;
use std::io::{BufRead, Write};

pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentKind {
    Process,
    Observer,
    Nushell,
    Generator,
}

pub(crate) fn handle_message(kind: AgentKind, input: &[u8]) -> Vec<u8> {
    if input.len() > MAX_MESSAGE_BYTES {
        return response(error(
            serde_json::Value::Null,
            -32002,
            "message exceeds the 4194304 byte limit",
        ));
    }
    let request = match crate::strict_json::parse(input) {
        Ok(value) => value,
        Err(_) => {
            return response(error(
                serde_json::Value::Null,
                -32600,
                "invalid JSON-RPC request",
            ));
        }
    };
    let Some(fields) = request.as_object() else {
        return response(error(
            serde_json::Value::Null,
            -32600,
            "invalid JSON-RPC request",
        ));
    };
    let id = fields
        .get("id")
        .filter(|value| valid_id(value))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let valid_envelope = fields.get("jsonrpc").and_then(serde_json::Value::as_str) == Some("2.0")
        && fields.get("id").is_some_and(valid_id)
        && fields
            .get("method")
            .and_then(serde_json::Value::as_str)
            .is_some();
    if !valid_envelope {
        return response(error(id, -32600, "invalid JSON-RPC request"));
    }
    let Some(method) = fields.get("method").and_then(serde_json::Value::as_str) else {
        return response(error(id, -32600, "invalid JSON-RPC request"));
    };
    if method == "deshell.handshake" {
        let Some(parameters) = fields.get("params").and_then(serde_json::Value::as_object) else {
            return response(error(id, -32602, "handshake params must be an object"));
        };
        let Some(version) = parameters
            .get("protocol_version")
            .and_then(serde_json::Value::as_i64)
        else {
            return response(error(
                id,
                -32602,
                "params.protocol_version must be an integer",
            ));
        };
        if version != 1 {
            return response(error(
                id,
                -32001,
                &format!("unsupported protocol version {version}; supported version is 1"),
            ));
        }
        if kind == AgentKind::Generator {
            return result_response(
                id,
                serde_json::json!({
                    "generator": {
                        "capabilities": ["rust", "go"],
                        "digest": crate::migration::official_generator_digest(),
                        "name": "deshell-official",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "max_frame_bytes": MAX_MESSAGE_BYTES,
                    "protocol": "deshell.generator.v1",
                    "schema_version": 1
                }),
            );
        }
        let (name, capability) = agent_identity(kind);
        return result_response(
            id,
            serde_json::json!({
                "capabilities": [capability],
                "protocol_version": 1,
                "server": {"name": name, "version": env!("CARGO_PKG_VERSION")}
            }),
        );
    }
    let Some(parameters) = fields.get("params").and_then(serde_json::Value::as_object) else {
        return response(error(id, -32602, "method params must be an object"));
    };
    let result = match (kind, method) {
        (AgentKind::Process, "process.execute") => execute_process(parameters),
        (AgentKind::Observer, "observer.observe") => observe_process(parameters),
        (AgentKind::Observer, "observer.run_plan") => run_plan_process(parameters),
        (AgentKind::Nushell, "nushell.lower") => lower_nushell(parameters),
        (AgentKind::Generator, "generator.propose") => {
            crate::migration::generator_propose(parameters).map_err(|message| (-32602, message))
        }
        _ => return response(error(id, -32601, "method not found")),
    };
    match result {
        Ok(value) => result_response(id, value),
        Err((code, message)) => response(error(id, code, &message)),
    }
}

pub(crate) fn serve(
    kind: AgentKind,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<i32, String> {
    loop {
        match read_frame(input)? {
            None => return Ok(0),
            Some(Frame::Message(message)) => {
                let response = handle_message(kind, &message);
                for frame in response_frames(&response)? {
                    output
                        .write_all(&frame)
                        .map_err(|error| format!("cannot write RPC response: {error}"))?;
                }
            }
            Some(Frame::Oversized) => output
                .write_all(&response(error(
                    serde_json::Value::Null,
                    -32002,
                    "message exceeds the 4194304 byte limit",
                )))
                .map_err(|error| format!("cannot write RPC response: {error}"))?,
        }
        output
            .flush()
            .map_err(|error| format!("cannot flush RPC response: {error}"))?;
    }
}

pub(crate) fn serve_stdio(kind: AgentKind, output: &mut dyn Write) -> Result<i32, String> {
    let stdin = std::io::stdin();
    serve(kind, &mut stdin.lock(), output)
}

#[allow(dead_code)]
pub(crate) fn decode_response(
    input: &[u8],
    expected_id: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if input.len() > MAX_MESSAGE_BYTES {
        return Err(format!(
            "RPC response exceeds the {MAX_MESSAGE_BYTES} byte limit"
        ));
    }
    let value = crate::strict_json::parse(input)
        .map_err(|error| format!("invalid RPC response: {error}"))?;
    let fields = value.as_object().ok_or("RPC response must be an object")?;
    if fields.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0") {
        return Err("RPC response jsonrpc must be 2.0".into());
    }
    let id = fields.get("id").ok_or("RPC response is missing id")?;
    if id != expected_id {
        return Err(format!(
            "RPC response ID mismatch (expected {expected_id}, found {id})"
        ));
    }
    match (fields.get("result"), fields.get("error")) {
        (Some(result), None) if result.is_object() => Ok(result.clone()),
        (None, Some(error)) => {
            let fields = error.as_object().ok_or("RPC error must be an object")?;
            let code = fields
                .get("code")
                .and_then(serde_json::Value::as_i64)
                .ok_or("RPC error code must be an integer")?;
            let message = fields
                .get("message")
                .and_then(serde_json::Value::as_str)
                .ok_or("RPC error message must be a string")?;
            Err(format!("RPC error {code}: {message}"))
        }
        _ => Err("RPC response must contain exactly one of result or error".into()),
    }
}

pub(crate) fn decode_streamed_response(
    input: &[u8],
    expected_id: &serde_json::Value,
    stdout_limit: u64,
    stderr_limit: u64,
) -> Result<serde_json::Value, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut next_stdout = 0_u64;
    let mut next_stderr = 0_u64;
    let mut final_result = None;
    for raw in input.split_inclusive(|byte| *byte == b'\n') {
        let frame = raw.strip_suffix(b"\n").unwrap_or(raw);
        let frame = frame.strip_suffix(b"\r").unwrap_or(frame);
        if frame.is_empty() {
            continue;
        }
        if frame.len() > MAX_MESSAGE_BYTES {
            return Err(format!(
                "RPC response frame exceeds the {MAX_MESSAGE_BYTES} byte limit"
            ));
        }
        let value = crate::strict_json::parse(frame)
            .map_err(|error| format!("invalid RPC response frame: {error}"))?;
        if value.get("method").and_then(serde_json::Value::as_str) == Some("deshell.stream") {
            let parameters = value
                .get("params")
                .and_then(serde_json::Value::as_object)
                .ok_or("stream notification params must be an object")?;
            if parameters.get("request_id") != Some(expected_id) {
                return Err("stream notification request ID mismatch".into());
            }
            let stream = parameters
                .get("stream")
                .and_then(serde_json::Value::as_str)
                .ok_or("stream notification stream must be a string")?;
            let sequence = parameters
                .get("sequence")
                .and_then(serde_json::Value::as_u64)
                .ok_or("stream notification sequence must be an integer")?;
            let encoded = parameters
                .get("data_base64")
                .and_then(serde_json::Value::as_str)
                .ok_or("stream notification data_base64 must be a string")?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| format!("stream chunk is invalid base64: {error}"))?;
            if data.len() > MAX_CHUNK_BYTES {
                return Err("stream chunk exceeds the 262144 byte limit".into());
            }
            if base64::engine::general_purpose::STANDARD.encode(&data) != encoded {
                return Err("stream chunk base64 is not canonical".into());
            }
            let (output, next, limit) = match stream {
                "stdout" => (&mut stdout, &mut next_stdout, stdout_limit),
                "stderr" => (&mut stderr, &mut next_stderr, stderr_limit),
                _ => return Err(format!("unknown RPC stream: {stream}")),
            };
            if sequence != *next {
                return Err(format!(
                    "out-of-order {stream} chunk (expected {}, found {sequence})",
                    *next
                ));
            }
            *next += 1;
            if output.len() as u64 + data.len() as u64 > limit {
                return Err(format!("{stream} stream exceeds the configured byte limit"));
            }
            output.extend(data);
            continue;
        }
        if final_result.is_some() {
            return Err("RPC stream contains multiple final responses".into());
        }
        final_result = Some(decode_response(frame, expected_id)?);
    }
    let mut result = final_result.ok_or("RPC stream is missing a final response")?;
    let fields = result
        .as_object_mut()
        .ok_or("RPC final result must be an object")?;
    if let Some(streams) = fields.remove("streams") {
        validate_stream_metadata(&streams, "stdout", &stdout)?;
        validate_stream_metadata(&streams, "stderr", &stderr)?;
        fields.insert(
            "stdout_base64".into(),
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(stdout)),
        );
        fields.insert(
            "stderr_base64".into(),
            serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(stderr)),
        );
    } else if next_stdout != 0 || next_stderr != 0 {
        return Err("RPC final response is missing stream metadata".into());
    }
    Ok(result)
}

fn validate_stream_metadata(
    streams: &serde_json::Value,
    name: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let metadata = streams
        .get(name)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("RPC final response is missing {name} stream metadata"))?;
    if metadata.get("bytes").and_then(serde_json::Value::as_u64) != Some(bytes.len() as u64) {
        return Err(format!("RPC {name} stream byte count mismatch"));
    }
    if metadata.get("sha256").and_then(serde_json::Value::as_str)
        != Some(crate::digest::sha256(bytes).as_str())
    {
        return Err(format!("RPC {name} stream digest mismatch"));
    }
    Ok(())
}

fn response_frames(response: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if response.len() <= MAX_MESSAGE_BYTES {
        return Ok(vec![response.to_vec()]);
    }
    let mut value = crate::strict_json::parse(response)
        .map_err(|error| format!("cannot stream internal RPC response: {error}"))?;
    let id = value
        .get("id")
        .cloned()
        .ok_or("internal RPC response is missing id")?;
    let result = value
        .get_mut("result")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("oversized RPC errors or non-object results cannot be streamed")?;
    let stdout = take_stream(result, "stdout_base64")?;
    let stderr = take_stream(result, "stderr_base64")?;
    let mut frames = Vec::new();
    for (name, bytes) in [("stdout", stdout.as_slice()), ("stderr", stderr.as_slice())] {
        for (sequence, chunk) in bytes.chunks(MAX_CHUNK_BYTES).enumerate() {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "deshell.stream",
                "params": {
                    "data_base64": base64::engine::general_purpose::STANDARD.encode(chunk),
                    "request_id": id.clone(),
                    "sequence": sequence,
                    "stream": name
                }
            });
            let mut frame = crate::canonical_json::canonical_bytes(&notification)?;
            frame.push(b'\n');
            if frame.len() > MAX_MESSAGE_BYTES {
                return Err("internal stream frame exceeds the RPC frame limit".into());
            }
            frames.push(frame);
        }
    }
    result.insert(
        "streams".into(),
        serde_json::json!({
            "stderr": {"bytes": stderr.len(), "sha256": crate::digest::sha256(&stderr)},
            "stdout": {"bytes": stdout.len(), "sha256": crate::digest::sha256(&stdout)}
        }),
    );
    let mut final_frame = crate::canonical_json::canonical_bytes(&value)?;
    final_frame.push(b'\n');
    if final_frame.len() > MAX_MESSAGE_BYTES {
        return Err("streamed RPC final response exceeds the frame limit".into());
    }
    frames.push(final_frame);
    Ok(frames)
}

fn take_stream(
    result: &mut serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let encoded = result
        .remove(name)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| format!("oversized RPC result is missing {name}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .map_err(|error| format!("internal RPC {name} is invalid base64: {error}"))?;
    if base64::engine::general_purpose::STANDARD.encode(&bytes) != encoded {
        return Err(format!("internal RPC {name} is not canonical base64"));
    }
    Ok(bytes)
}

fn valid_id(value: &serde_json::Value) -> bool {
    value.as_str().is_some() || value.as_i64().is_some()
}

fn agent_identity(kind: AgentKind) -> (&'static str, &'static str) {
    match kind {
        AgentKind::Process => ("deshell-process-agent", "process.execute"),
        AgentKind::Observer => ("deshell-observer-agent", "observer.observe"),
        AgentKind::Nushell => ("deshell-nushell-adapter", "nushell.lower"),
        AgentKind::Generator => ("deshell-official", "generator.propose"),
    }
}

fn result_response(id: serde_json::Value, result: serde_json::Value) -> Vec<u8> {
    response(serde_json::json!({"id": id, "jsonrpc": "2.0", "result": result}))
}

type MethodError = (i64, String);

fn execute_process(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let request = process_request(parameters)?;
    let root = std::env::current_dir()
        .map_err(|error| (-32010, format!("cannot resolve agent workspace: {error}")))?;
    let outcome = crate::agent_process::execute(&root, request).map_err(|error| (-32010, error))?;
    Ok(process_outcome(&outcome))
}

fn observe_process(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let root = std::env::current_dir().map_err(|error| {
        (
            -32010,
            format!("cannot resolve observer workspace: {error}"),
        )
    })?;
    observe_process_at(&root, parameters)
}

fn observe_process_at(
    root: &std::path::Path,
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let request = process_request(parameters)?;
    if let Some(values) = parameters.get("fixtures") {
        let fixtures = fixture_params(values)?;
        crate::workspace::materialize(root, &fixtures).map_err(|error| (-32602, error))?;
    }
    let before = crate::workspace::capture(root).map_err(|error| (-32010, error))?;
    let outcome = crate::agent_process::execute(root, request).map_err(|error| (-32010, error))?;
    let after = crate::workspace::capture(root).map_err(|error| (-32010, error))?;
    if let Some(values) = parameters.get("expected_files") {
        let expected = expected_file_params(values)?;
        crate::workspace::validate_expected(root, &expected)
            .map_err(|errors| (-32011, errors.join("; ")))?;
    }
    let changes = crate::workspace::diff(&before, &after).into_iter().map(|change| {
        let kind = match change.kind { crate::workspace::ChangeKind::Created => "created", crate::workspace::ChangeKind::Modified => "modified", crate::workspace::ChangeKind::Removed => "removed" };
        serde_json::json!({"after_sha256": change.after_sha256, "before_sha256": change.before_sha256, "kind": kind, "path": change.path})
    }).collect::<Vec<_>>();
    let mut result = process_outcome(&outcome);
    result
        .as_object_mut()
        .ok_or_else(|| (-32010, "process outcome is not a JSON object".to_owned()))?
        .insert("files".into(), serde_json::Value::Array(changes));
    Ok(result)
}

fn run_plan_process(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let root = std::env::current_dir().map_err(|error| {
        (
            -32010,
            format!("cannot resolve observer workspace: {error}"),
        )
    })?;
    run_plan_process_at(&root, parameters)
}

fn run_plan_process_at(
    root: &std::path::Path,
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let invalid = |message: &str| (-32602, message.to_owned());
    let request = process_request(parameters)?;
    let entrypoint = parameters
        .get("entrypoint")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("params.entrypoint must be a string"))?;
    let node_id = match parameters.get("node_id") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid("params.node_id must be a non-empty string or null"))?,
        ),
    };
    let named_inputs = named_values(parameters.get("arguments"), "arguments")?;
    if let Some(values) = parameters.get("fixtures") {
        let fixtures = fixture_params(values)?;
        crate::workspace::materialize(root, &fixtures).map_err(|error| (-32602, error))?;
    }
    let before = crate::workspace::capture(root).map_err(|error| (-32010, error))?;
    let config = crate::project::load_config(root).map_err(|errors| (-32010, errors.join("; ")))?;
    let limits = crate::config::ResourceLimits {
        timeout_ms: request.limits.timeout_ms,
        memory_bytes: request.limits.memory_bytes,
        processes: request.limits.processes,
        stdout_bytes: request.limits.stdout_bytes,
        stderr_bytes: request.limits.stderr_bytes,
    };
    if !limits.narrows(config.limits) {
        return Err(invalid(
            "observer plan resource limits may only narrow project limits",
        ));
    }
    let (mut plan, _) = crate::project::load_entry_artifacts(root, entrypoint)
        .map_err(|errors| (-32010, errors.join("; ")))?;
    if let Some(node_id) = node_id {
        plan = select_plan_node(plan, node_id).map_err(|error| (-32602, error))?;
    }
    let lock = crate::project::load_lock(root).map_err(|errors| (-32010, errors.join("; ")))?;
    let backend = crate::local_backend::LocalBackend::with_pinned_interpreters(
        root,
        limits,
        lock.interpreters,
    )
    .map_err(|error| (-32010, error))?;
    let environment = request.environment.into_iter().collect();
    let result = crate::runner::run_plan_with_io(
        &backend,
        crate::runner::Policy {
            allow_file_read: matches!(
                config.policy.file_read,
                crate::config::FileReadPolicy::Project
            ),
            allow_file_write: matches!(
                config.policy.file_write,
                crate::config::FileWritePolicy::Sandbox
            ),
            allow_network: matches!(
                config.policy.network,
                crate::config::NetworkPolicy::RecordReplay
            ),
            allow_delegation: matches!(
                config.policy.delegation,
                crate::config::DelegationPolicy::Pinned
            ),
        },
        &plan,
        crate::runner::RunInputs {
            host_environment: &environment,
            named_inputs: &named_inputs,
            arguments: &request.argv,
            stdin: &request.stdin,
            default_working_directory: request.working_directory.as_deref(),
        },
    )
    .map_err(|error| (-32010, error.message))?;
    let after = crate::workspace::capture(root).map_err(|error| (-32010, error))?;
    if let Some(values) = parameters.get("expected_files") {
        let expected = expected_file_params(values)?;
        crate::workspace::validate_expected(root, &expected)
            .map_err(|errors| (-32011, errors.join("; ")))?;
    }
    Ok(run_result_value(
        &result,
        crate::workspace::diff(&before, &after),
    ))
}

fn named_values(
    value: Option<&serde_json::Value>,
    label: &str,
) -> Result<std::collections::BTreeMap<String, String>, MethodError> {
    let invalid = |message: String| (-32602, message);
    let values = value
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid(format!("params.{label} must be a name/value array")))?;
    let mut output = std::collections::BTreeMap::new();
    for value in values {
        let fields = value
            .as_object()
            .ok_or_else(|| invalid(format!("params.{label} entries must be objects")))?;
        let name = fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid(format!("params.{label}.name must be a non-empty string")))?;
        let value = fields
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid(format!("params.{label}.value must be a string")))?;
        if output.insert(name.into(), value.into()).is_some() {
            return Err(invalid(format!("duplicate params.{label} name: {name}")));
        }
    }
    Ok(output)
}

fn run_result_value(
    result: &crate::runner::RunResult,
    changes: Vec<crate::workspace::Change>,
) -> serde_json::Value {
    let files = changes
        .into_iter()
        .map(|change| {
            let kind = match change.kind {
                crate::workspace::ChangeKind::Created => "created",
                crate::workspace::ChangeKind::Modified => "modified",
                crate::workspace::ChangeKind::Removed => "removed",
            };
            serde_json::json!({
                "after_sha256": change.after_sha256,
                "before_sha256": change.before_sha256,
                "kind": kind,
                "path": change.path
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "exit_code": result.exit_code,
        "files": files,
        "signal": null,
        "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&result.stderr),
        "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&result.stdout),
        "timed_out": false,
        "limit_exceeded": null
    })
}

fn select_plan_node(mut plan: crate::ir::Plan, id: &str) -> Result<crate::ir::Plan, String> {
    fn find(node: &crate::ir::Node, id: &str) -> Option<crate::ir::Node> {
        if node.id == id {
            return Some(node.clone());
        }
        let mut children = Vec::new();
        match &node.operation {
            crate::ir::Operation::Pipeline { nodes, .. }
            | crate::ir::Operation::Sequence { nodes }
            | crate::ir::Operation::Parallel { nodes } => children.extend(nodes.iter()),
            crate::ir::Operation::Condition {
                predicate,
                if_true,
                if_false,
            } => {
                children.extend([predicate.as_ref(), if_true.as_ref()]);
                children.extend(if_false.as_deref());
            }
            crate::ir::Operation::Match { cases, default, .. } => {
                children.extend(cases.iter().map(|case| &case.body));
                children.extend(default.as_deref());
            }
            crate::ir::Operation::Foreach { body, .. }
            | crate::ir::Operation::CaptureStdout { body, .. } => children.push(body),
            crate::ir::Operation::TryFinally { body, finalizer } => {
                children.extend([body.as_ref(), finalizer.as_ref()]);
            }
            _ => {}
        }
        children.into_iter().find_map(|child| find(child, id))
    }
    let (task_index, node) = plan
        .tasks
        .iter()
        .enumerate()
        .find_map(|(index, task)| find(&task.body, id).map(|node| (index, node)))
        .ok_or_else(|| format!("node not found: {id}"))?;
    let task_name = plan.tasks[task_index].name.clone();
    plan.tasks[task_index].body = node;
    plan.entrypoint = task_name;
    plan.tasks.retain(|task| task.name == plan.entrypoint);
    Ok(plan)
}

fn fixture_params(value: &serde_json::Value) -> Result<Vec<crate::config::Fixture>, MethodError> {
    let invalid = |message: &str| (-32602, message.to_owned());
    let values = value
        .as_array()
        .ok_or_else(|| invalid("params.fixtures must be an array"))?;
    values
        .iter()
        .map(|value| {
            let fields = value
                .as_object()
                .ok_or_else(|| invalid("fixture must be an object"))?;
            let path = fields
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("fixture.path must be a string"))?;
            let contents = fields
                .get("contents_base64")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("fixture.contents_base64 must be a string"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(contents)
                .map_err(|_| invalid("fixture.contents_base64 is invalid"))?;
            if base64::engine::general_purpose::STANDARD.encode(&bytes) != contents {
                return Err(invalid("fixture.contents_base64 must be canonical"));
            }
            let executable = fields.get("executable").map_or(Ok(false), |value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid("fixture.executable must be a boolean"))
            })?;
            Ok(crate::config::Fixture {
                path: path.into(),
                contents: crate::config::BinaryData {
                    utf8: None,
                    base64: Some(contents.into()),
                },
                executable,
            })
        })
        .collect()
}

fn expected_file_params(
    value: &serde_json::Value,
) -> Result<Vec<crate::config::ExpectedFile>, MethodError> {
    let invalid = |message: &str| (-32602, message.to_owned());
    let values = value
        .as_array()
        .ok_or_else(|| invalid("params.expected_files must be an array"))?;
    values
        .iter()
        .map(|value| {
            let fields = value
                .as_object()
                .ok_or_else(|| invalid("expected file must be an object"))?;
            let path = fields
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("expected_file.path must be a string"))?;
            let sha256 = fields
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("expected_file.sha256 must be a string"))?;
            Ok(crate::config::ExpectedFile {
                path: path.into(),
                sha256: sha256.into(),
            })
        })
        .collect()
}

fn process_request(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<crate::agent_process::Request, MethodError> {
    let invalid = |message: &str| (-32602, message.to_owned());
    let argv = parameters
        .get("argv")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid("params.argv must be a string array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("params.argv must contain strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let environment_values = parameters
        .get("environment")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid("params.environment must be a name/value array"))?;
    let mut environment = Vec::new();
    for value in environment_values {
        let fields = value
            .as_object()
            .ok_or_else(|| invalid("params.environment entries must be objects"))?;
        let name = fields
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("environment.name must be a string"))?;
        let value = fields
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("environment.value must be a string"))?;
        environment.push((name.into(), value.into()));
    }
    let working_directory = match parameters.get("working_directory") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| invalid("params.working_directory must be a string or null"))?
                .into(),
        ),
    };
    let encoded = parameters
        .get("stdin_base64")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("params.stdin_base64 must be a string"))?;
    let stdin = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| invalid("params.stdin_base64 is invalid"))?;
    if base64::engine::general_purpose::STANDARD.encode(&stdin) != encoded {
        return Err(invalid(
            "params.stdin_base64 must use canonical padded base64",
        ));
    }
    let timeout_ms = parameters
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| invalid("params.timeout_ms must be a positive integer"))?;
    let defaults = crate::agent_process::Limits::default();
    let limit = |name: &str, default: u64| -> Result<u64, MethodError> {
        match parameters.get(name) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| invalid(&format!("params.{name} must be a positive integer"))),
        }
    };
    Ok(crate::agent_process::Request {
        argv,
        environment,
        working_directory,
        stdin,
        limits: crate::agent_process::Limits {
            timeout_ms,
            memory_bytes: limit("memory_bytes", defaults.memory_bytes)?,
            processes: limit("processes", defaults.processes)?,
            stdout_bytes: limit("stdout_bytes", defaults.stdout_bytes)?,
            stderr_bytes: limit("stderr_bytes", defaults.stderr_bytes)?,
        },
    })
}

fn process_outcome(outcome: &crate::agent_process::Outcome) -> serde_json::Value {
    serde_json::json!({
        "exit_code": outcome.exit_code,
        "signal": outcome.signal,
        "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&outcome.stderr),
        "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&outcome.stdout),
        "timed_out": outcome.timed_out,
        "limit_exceeded": outcome.limit_exceeded
    })
}

fn lower_nushell(
    parameters: &serde_json::Map<String, serde_json::Value>,
) -> Result<serde_json::Value, MethodError> {
    let invalid = |message: &str| (-32602, message.to_owned());
    let path = parameters
        .get("path")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("params.path must be a string"))?;
    let source = parameters
        .get("source")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| invalid("params.source must be an object"))?;
    let encoding = source
        .get("encoding")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("params.source.encoding must be a string"))?;
    let bytes = match encoding {
        "utf8" => source
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| invalid("UTF-8 source requires text"))?
            .as_bytes()
            .to_vec(),
        "base64" => {
            let encoded = source
                .get("base64")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("base64 source requires base64"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| invalid("source base64 is invalid"))?;
            if base64::engine::general_purpose::STANDARD.encode(&bytes) != encoded {
                return Err(invalid("source base64 must be canonical"));
            }
            bytes
        }
        _ => return Err(invalid("params.source.encoding must be utf8 or base64")),
    };
    let plan = crate::frontend::lower(path, &bytes, crate::config::UnknownInterpreter::Reject)
        .map_err(|error| (-32602, error))?;
    let plan = serde_json::to_value(plan).map_err(|error| (-32010, error.to_string()))?;
    Ok(serde_json::json!({"plan": plan}))
}

fn error(id: serde_json::Value, code: i64, message: &str) -> serde_json::Value {
    serde_json::json!({"error": {"code": code, "message": message}, "id": id, "jsonrpc": "2.0"})
}

fn response(value: serde_json::Value) -> Vec<u8> {
    let mut bytes = crate::canonical_json::canonical_bytes(&value).unwrap_or_else(|_| {
        br#"{"error":{"code":-32603,"message":"cannot serialize JSON-RPC response"},"id":null,"jsonrpc":"2.0"}"#.to_vec()
    });
    bytes.push(b'\n');
    bytes
}

enum Frame {
    Message(Vec<u8>),
    Oversized,
}

fn read_frame(input: &mut dyn BufRead) -> Result<Option<Frame>, String> {
    let mut message = Vec::new();
    let mut oversized = false;
    loop {
        let available = input
            .fill_buf()
            .map_err(|error| format!("cannot read RPC input: {error}"))?;
        if available.is_empty() {
            return if message.is_empty() && !oversized {
                Ok(None)
            } else {
                Err("RPC peer disconnected before terminating a message".into())
            };
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if !oversized {
                if message.len() + newline > MAX_MESSAGE_BYTES {
                    oversized = true;
                } else {
                    message.extend_from_slice(&available[..newline]);
                }
            }
            input.consume(newline + 1);
            if oversized {
                return Ok(Some(Frame::Oversized));
            }
            if message.last() == Some(&b'\r') {
                message.pop();
            }
            return Ok(Some(Frame::Message(message)));
        }
        let count = available.len();
        if !oversized {
            if message.len() + count > MAX_MESSAGE_BYTES {
                oversized = true;
            } else {
                message.extend_from_slice(available);
            }
        }
        input.consume(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i64, method: &str, params: serde_json::Value) -> Vec<u8> {
        crate::canonical_json::canonical_bytes(&serde_json::json!({
            "future_envelope": true,
            "id": id,
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .unwrap()
    }

    fn value(bytes: &[u8]) -> serde_json::Value {
        crate::strict_json::parse(bytes).unwrap()
    }

    #[test]
    fn handshake_accepts_unknown_fields_and_describes_each_internal_agent() {
        for (kind, name, capability) in [
            (
                AgentKind::Process,
                "deshell-process-agent",
                "process.execute",
            ),
            (
                AgentKind::Observer,
                "deshell-observer-agent",
                "observer.observe",
            ),
            (
                AgentKind::Nushell,
                "deshell-nushell-adapter",
                "nushell.lower",
            ),
        ] {
            let response = handle_message(
                kind,
                &request(
                    7,
                    "deshell.handshake",
                    serde_json::json!({"protocol_version": 1, "future": "ignored"}),
                ),
            );
            assert!(response.ends_with(b"\n"));
            let response = value(&response);
            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], 7);
            assert_eq!(response["result"]["protocol_version"], 1);
            assert_eq!(response["result"]["server"]["name"], name);
            assert!(
                response["result"]["capabilities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item == capability)
            );
        }
    }

    #[test]
    fn official_generator_handshake_uses_generator_protocol_v1() {
        let response = handle_message(
            AgentKind::Generator,
            &request(
                11,
                "deshell.handshake",
                serde_json::json!({"protocol_version": 1}),
            ),
        );
        let response = value(&response);
        let result = &response["result"];
        assert_eq!(result["schema_version"], 1);
        assert_eq!(result["protocol"], "deshell.generator.v1");
        assert_eq!(result["max_frame_bytes"], MAX_MESSAGE_BYTES);
        assert_eq!(result["generator"]["name"], "deshell-official");
        assert!(
            result["generator"]["digest"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(
            result["generator"]["capabilities"],
            serde_json::json!(["rust", "go"])
        );
    }

    #[test]
    fn official_generator_proposes_from_a_persisted_migration_request() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("build.sh"),
            b"#!/bin/sh\n/usr/bin/printf generated\n",
        )
        .unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("entrypoints = []", "entrypoints = [\"build.sh\"]")
            .replace(
                "platform_cells = []",
                &format!(
                    "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}]",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            );
        std::fs::write(config_path, config).unwrap();
        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("approval = \"draft\"", "approval = \"approved\"");
        std::fs::write(scenario_path, scenario).unwrap();
        std::fs::create_dir_all(directory.path().join("src/bin")).unwrap();
        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        let plan_directory = directory
            .path()
            .join(format!(".deshell/migrations/sha256/{}", planned.digest));
        let request_path = std::fs::read_dir(plan_directory.join("requests"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let proposal_path = std::fs::read_dir(plan_directory.join("proposals"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let migration_request =
            crate::strict_json::parse(&std::fs::read(request_path).unwrap()).unwrap();
        let expected = crate::strict_json::parse(&std::fs::read(proposal_path).unwrap()).unwrap();
        let response = handle_message(
            AgentKind::Generator,
            &request(
                12,
                "generator.propose",
                serde_json::json!({
                    "expected_digest": null,
                    "request": migration_request,
                    "target_path": "src/bin/deshell_build.rs",
                    "validation": []
                }),
            ),
        );
        let response = value(&response);
        assert_eq!(response["result"], expected, "{response:#}");
    }

    #[test]
    fn protocol_errors_use_fixed_json_rpc_codes() {
        let incompatible = value(&handle_message(
            AgentKind::Process,
            &request(
                1,
                "deshell.handshake",
                serde_json::json!({"protocol_version": 2}),
            ),
        ));
        assert_eq!(incompatible["error"]["code"], -32001);
        let bad_params = value(&handle_message(
            AgentKind::Process,
            &request(2, "deshell.handshake", serde_json::json!([])),
        ));
        assert_eq!(bad_params["error"]["code"], -32602);
        let unknown = value(&handle_message(
            AgentKind::Process,
            &request(3, "future.method", serde_json::json!({})),
        ));
        assert_eq!(unknown["error"]["code"], -32601);
        let invalid = value(&handle_message(
            AgentKind::Process,
            br#"{"jsonrpc":"1.0","id":4,"method":"deshell.handshake","params":{}}"#,
        ));
        assert_eq!(invalid["error"]["code"], -32600);
        assert_eq!(invalid["id"], 4);
    }

    #[test]
    fn duplicate_keys_malformed_utf8_and_oversize_are_rejected() {
        let duplicate = value(&handle_message(
            AgentKind::Process,
            br#"{"jsonrpc":"2.0","id":1,"id":2,"method":"x","params":{}}"#,
        ));
        assert_eq!(duplicate["error"]["code"], -32600);
        let utf8 = value(&handle_message(AgentKind::Process, &[0xff, 0xfe]));
        assert_eq!(utf8["error"]["code"], -32600);
        let oversized = value(&handle_message(
            AgentKind::Process,
            &vec![b' '; MAX_MESSAGE_BYTES + 1],
        ));
        assert_eq!(oversized["error"]["code"], -32002);
        assert_eq!(oversized["id"], serde_json::Value::Null);
    }

    #[test]
    fn server_handles_multiple_lines_and_treats_partial_disconnect_as_failure() {
        let first = request(
            1,
            "deshell.handshake",
            serde_json::json!({"protocol_version": 1}),
        );
        let second = request(2, "missing", serde_json::json!({}));
        let mut complete = first.clone();
        complete.push(b'\n');
        complete.extend(second);
        complete.push(b'\n');
        let mut output = Vec::new();
        assert_eq!(
            serve(
                AgentKind::Observer,
                &mut std::io::Cursor::new(complete),
                &mut output
            )
            .unwrap(),
            0
        );
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 2);
        let mut partial = std::io::Cursor::new(first);
        assert!(
            serve(AgentKind::Process, &mut partial, &mut Vec::new())
                .unwrap_err()
                .contains("disconnected")
        );
    }

    #[test]
    fn client_rejects_response_id_mismatch_duplicate_keys_and_non_responses() {
        let valid = br#"{"future":true,"id":"a","jsonrpc":"2.0","result":{"ok":true}}"#;
        assert_eq!(
            decode_response(valid, &serde_json::json!("a")).unwrap()["ok"],
            true
        );
        assert!(
            decode_response(valid, &serde_json::json!("b"))
                .unwrap_err()
                .contains("ID mismatch")
        );
        assert!(
            decode_response(
                br#"{"id":1,"id":1,"jsonrpc":"2.0","result":{}}"#,
                &serde_json::json!(1)
            )
            .is_err()
        );
        assert!(decode_response(br#"{"id":1,"jsonrpc":"2.0"}"#, &serde_json::json!(1)).is_err());
        assert!(
            decode_response(&vec![b' '; MAX_MESSAGE_BYTES + 1], &serde_json::json!(1))
                .unwrap_err()
                .contains("byte limit")
        );
    }

    #[test]
    fn oversized_results_use_ordered_bounded_chunks_with_verified_digests() {
        let stdout = vec![0xa5; MAX_MESSAGE_BYTES];
        let stderr = vec![0x5a; MAX_CHUNK_BYTES + 7];
        let response = result_response(
            serde_json::json!(9),
            serde_json::json!({
                "exit_code": 0,
                "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&stderr),
                "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&stdout)
            }),
        );
        let frames = response_frames(&response).unwrap();
        assert!(frames.len() > 3);
        assert!(frames.iter().all(|frame| frame.len() <= MAX_MESSAGE_BYTES));
        let joined = frames.concat();
        let result = decode_streamed_response(
            &joined,
            &serde_json::json!(9),
            stdout.len() as u64,
            stderr.len() as u64,
        )
        .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(result["stdout_base64"].as_str().unwrap())
                .unwrap(),
            stdout
        );
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(result["stderr_base64"].as_str().unwrap())
                .unwrap(),
            stderr
        );

        let mut lines = frames;
        let first: serde_json::Value = crate::strict_json::parse(&lines[0]).unwrap();
        let mut invalid = first;
        invalid["params"]["sequence"] = serde_json::json!(4);
        let mut encoded = crate::canonical_json::canonical_bytes(&invalid).unwrap();
        encoded.push(b'\n');
        lines[0] = encoded;
        assert!(
            decode_streamed_response(
                &lines.concat(),
                &serde_json::json!(9),
                stdout.len() as u64,
                stderr.len() as u64,
            )
            .unwrap_err()
            .contains("out-of-order")
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_agent_executes_exact_argv_with_raw_base64_stdio() {
        let response = value(&handle_message(
            AgentKind::Process,
            &request(
                9,
                "process.execute",
                serde_json::json!({
                    "argv": ["/bin/sh", "-c", "printf '\\000\\377'; printf err >&2"],
                    "environment": [],
                    "future": true,
                    "stdin_base64": "",
                    "timeout_ms": 5000,
                    "working_directory": null
                }),
            ),
        ));
        assert_eq!(response["id"], 9);
        assert_eq!(response["result"]["exit_code"], 0);
        assert_eq!(response["result"]["stdout_base64"], "AP8=");
        assert_eq!(response["result"]["stderr_base64"], "ZXJy");
        assert_eq!(response["result"]["timed_out"], false);
    }

    #[test]
    fn nushell_agent_returns_a_strict_effect_ir_v1_plan() {
        let response = value(&handle_message(
            AgentKind::Nushell,
            &request(
                10,
                "nushell.lower",
                serde_json::json!({
                    "path": "build.nu",
                    "source": {"encoding": "utf8", "text": "^printf hello\n"},
                    "unknown": "ignored"
                }),
            ),
        ));
        assert_eq!(response["id"], 10);
        assert_eq!(response["result"]["plan"]["schema_version"], 1);
        assert_eq!(
            response["result"]["plan"]["tasks"][0]["body"]["operation"]["type"],
            "exec"
        );
    }

    #[test]
    fn process_fixture_expected_file_and_named_value_parameters_are_strict() {
        let base = serde_json::json!({
            "argv": ["program", "literal"],
            "environment": [{"name": "MODE", "value": "test"}],
            "memory_bytes": 33554432,
            "processes": 4,
            "stderr_bytes": 2048,
            "stdin_base64": "AP8=",
            "stdout_bytes": 1024,
            "timeout_ms": 5000,
            "working_directory": "work"
        });
        let fields = base.as_object().unwrap();
        let request = process_request(fields).unwrap();
        assert_eq!(request.argv, ["program", "literal"]);
        assert_eq!(request.environment, [("MODE".into(), "test".into())]);
        assert_eq!(request.stdin, [0, 0xff]);
        assert_eq!(request.working_directory.as_deref(), Some("work"));
        assert_eq!(request.limits.stdout_bytes, 1024);

        for (field, invalid) in [
            ("argv", serde_json::json!(null)),
            ("environment", serde_json::json!([1])),
            ("stdin_base64", serde_json::json!("AA")),
            ("timeout_ms", serde_json::json!(0)),
            ("memory_bytes", serde_json::json!(0)),
            ("working_directory", serde_json::json!(false)),
        ] {
            let mut value = base.clone();
            value[field] = invalid;
            assert!(
                process_request(value.as_object().unwrap()).is_err(),
                "{field}"
            );
        }
        let mut bad_environment = base.clone();
        bad_environment["environment"] = serde_json::json!([{"name": 1, "value": "x"}]);
        assert!(process_request(bad_environment.as_object().unwrap()).is_err());
        bad_environment["environment"] = serde_json::json!([{"name": "A", "value": 1}]);
        assert!(process_request(bad_environment.as_object().unwrap()).is_err());

        let fixtures = fixture_params(&serde_json::json!([
            {"contents_base64": "dmFsdWU=", "executable": true, "path": "bin/tool"},
            {"contents_base64": "", "path": "empty"}
        ]))
        .unwrap();
        assert_eq!(fixtures.len(), 2);
        assert!(fixtures[0].executable);
        for invalid in [
            serde_json::json!({}),
            serde_json::json!([1]),
            serde_json::json!([{"contents_base64": "AA=="}]),
            serde_json::json!([{"contents_base64": "AA", "path": "x"}]),
            serde_json::json!([{"contents_base64": "", "executable": 1, "path": "x"}]),
        ] {
            assert!(fixture_params(&invalid).is_err(), "{invalid}");
        }

        let expected = expected_file_params(&serde_json::json!([
            {"path": "out", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
        ]))
        .unwrap();
        assert_eq!(expected[0].path, "out");
        for invalid in [
            serde_json::json!({}),
            serde_json::json!([1]),
            serde_json::json!([{"sha256": "a"}]),
            serde_json::json!([{"path": "out"}]),
        ] {
            assert!(expected_file_params(&invalid).is_err(), "{invalid}");
        }

        let named = named_values(
            Some(&serde_json::json!([
                {"name": "first", "value": "one"},
                {"name": "second", "value": "two"}
            ])),
            "arguments",
        )
        .unwrap();
        assert_eq!(named["second"], "two");
        for invalid in [
            None,
            Some(serde_json::json!({})),
            Some(serde_json::json!([1])),
            Some(serde_json::json!([{"name": "", "value": "x"}])),
            Some(serde_json::json!([{"name": "x", "value": 1}])),
            Some(serde_json::json!([
                {"name": "x", "value": "1"},
                {"name": "x", "value": "2"}
            ])),
        ] {
            assert!(named_values(invalid.as_ref(), "arguments").is_err());
        }
    }

    #[test]
    fn streamed_response_validation_rejects_every_unbound_or_corrupt_shape() {
        fn notification(
            id: serde_json::Value,
            stream: &str,
            sequence: u64,
            data_base64: &str,
        ) -> Vec<u8> {
            let mut bytes = crate::canonical_json::canonical_bytes(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "deshell.stream",
                "params": {
                    "data_base64": data_base64,
                    "request_id": id,
                    "sequence": sequence,
                    "stream": stream
                }
            }))
            .unwrap();
            bytes.push(b'\n');
            bytes
        }

        let id = serde_json::json!(4);
        assert!(
            decode_streamed_response(b"", &id, 10, 10)
                .unwrap_err()
                .contains("missing")
        );
        assert!(
            decode_streamed_response(
                &notification(serde_json::json!(5), "stdout", 0, "YQ=="),
                &id,
                10,
                10
            )
            .unwrap_err()
            .contains("ID mismatch")
        );
        assert!(
            decode_streamed_response(&notification(id.clone(), "unknown", 0, "YQ=="), &id, 10, 10)
                .unwrap_err()
                .contains("unknown RPC stream")
        );
        assert!(
            decode_streamed_response(&notification(id.clone(), "stdout", 0, "?"), &id, 10, 10)
                .is_err()
        );
        assert!(
            decode_streamed_response(&notification(id.clone(), "stdout", 0, "YQ=="), &id, 0, 10)
                .unwrap_err()
                .contains("configured byte limit")
        );

        let mut missing_metadata = notification(id.clone(), "stdout", 0, "YQ==");
        missing_metadata.extend(result_response(
            id.clone(),
            serde_json::json!({"exit_code": 0}),
        ));
        assert!(
            decode_streamed_response(&missing_metadata, &id, 10, 10)
                .unwrap_err()
                .contains("missing stream metadata")
        );

        let mut wrong_count = notification(id.clone(), "stdout", 0, "YQ==");
        wrong_count.extend(result_response(
            id.clone(),
            serde_json::json!({
                "streams": {
                    "stdout": {"bytes": 2, "sha256": crate::digest::sha256(b"a")},
                    "stderr": {"bytes": 0, "sha256": crate::digest::sha256(b"")}
                }
            }),
        ));
        assert!(
            decode_streamed_response(&wrong_count, &id, 10, 10)
                .unwrap_err()
                .contains("byte count")
        );
        let mut wrong_digest = notification(id.clone(), "stdout", 0, "YQ==");
        wrong_digest.extend(result_response(
            id.clone(),
            serde_json::json!({
                "streams": {
                    "stdout": {"bytes": 1, "sha256": "0".repeat(64)},
                    "stderr": {"bytes": 0, "sha256": crate::digest::sha256(b"")}
                }
            }),
        ));
        assert!(
            decode_streamed_response(&wrong_digest, &id, 10, 10)
                .unwrap_err()
                .contains("digest mismatch")
        );

        let mut duplicate_final = result_response(id.clone(), serde_json::json!({"ok": true}));
        duplicate_final.extend(result_response(id.clone(), serde_json::json!({"ok": true})));
        assert!(
            decode_streamed_response(&duplicate_final, &id, 10, 10)
                .unwrap_err()
                .contains("multiple final")
        );
    }

    #[test]
    fn node_selection_and_result_encoding_preserve_observable_structure() {
        use crate::ir::{Guarantee, Node, Operation, Plan, Task, TextExpression};

        fn native(operation: Operation) -> Node {
            Node {
                id: String::new(),
                operation,
                guarantee: Guarantee::Native {
                    semantic_model: "protocol-test-v1".into(),
                },
                source: None,
            }
        }
        fn emit(value: &str) -> Node {
            native(Operation::Exec {
                argv: vec![
                    TextExpression::literal("emit"),
                    TextExpression::literal(value),
                ],
                environment: vec![],
                working_directory: None,
            })
        }

        let target = emit("selected");
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
                body: native(Operation::Condition {
                    predicate: Box::new(emit("predicate")),
                    if_true: Box::new(native(Operation::Sequence {
                        nodes: vec![emit("first"), target],
                    })),
                    if_false: Some(Box::new(emit("false"))),
                }),
            }],
        };
        plan.assign_node_ids().unwrap();
        let selected_id = match &plan.tasks[0].body.operation {
            Operation::Condition { if_true, .. } => match &if_true.operation {
                Operation::Sequence { nodes } => nodes[1].id.clone(),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let selected = select_plan_node(plan.clone(), &selected_id).unwrap();
        assert_eq!(selected.tasks.len(), 1);
        assert_eq!(selected.tasks[0].body.id, selected_id);
        assert!(
            select_plan_node(plan, "missing")
                .unwrap_err()
                .contains("not found")
        );

        let encoded = run_result_value(
            &crate::runner::RunResult {
                exit_code: 3,
                stdout: vec![0, 0xff],
                stderr: b"error".to_vec(),
                trace: vec![],
            },
            vec![
                crate::workspace::Change {
                    path: "created".into(),
                    kind: crate::workspace::ChangeKind::Created,
                    before_sha256: None,
                    after_sha256: Some("a".repeat(64)),
                },
                crate::workspace::Change {
                    path: "modified".into(),
                    kind: crate::workspace::ChangeKind::Modified,
                    before_sha256: Some("b".repeat(64)),
                    after_sha256: Some("c".repeat(64)),
                },
                crate::workspace::Change {
                    path: "removed".into(),
                    kind: crate::workspace::ChangeKind::Removed,
                    before_sha256: Some("d".repeat(64)),
                    after_sha256: None,
                },
            ],
        );
        assert_eq!(encoded["exit_code"], 3);
        assert_eq!(encoded["stdout_base64"], "AP8=");
        assert_eq!(encoded["files"][0]["kind"], "created");
        assert_eq!(encoded["files"][1]["kind"], "modified");
        assert_eq!(encoded["files"][2]["kind"], "removed");

        let outcome = process_outcome(&crate::agent_process::Outcome {
            exit_code: 9,
            stdout: b"out".to_vec(),
            stderr: b"err".to_vec(),
            timed_out: true,
            limit_exceeded: Some("timeout".into()),
            signal: Some(15),
        });
        assert_eq!(outcome["signal"], 15);
        assert_eq!(outcome["limit_exceeded"], "timeout");
    }

    #[test]
    fn envelope_and_frame_errors_remain_bounded_and_machine_readable() {
        for message in [
            serde_json::json!(null),
            serde_json::json!({"id": true, "jsonrpc": "2.0", "method": "x", "params": {}}),
            serde_json::json!({"id": 1, "jsonrpc": "2.0", "params": {}}),
            serde_json::json!({"id": 1, "jsonrpc": "2.0", "method": "x", "params": []}),
            serde_json::json!({"id": 1, "jsonrpc": "2.0", "method": "deshell.handshake", "params": {}}),
        ] {
            let encoded = crate::canonical_json::canonical_bytes(&message).unwrap();
            assert!(value(&handle_message(AgentKind::Process, &encoded))["error"]["code"].is_i64());
        }

        let mut oversized = vec![b'x'; MAX_MESSAGE_BYTES + 1];
        oversized.push(b'\n');
        let mut output = Vec::new();
        assert_eq!(
            serve(
                AgentKind::Process,
                &mut std::io::Cursor::new(oversized),
                &mut output,
            )
            .unwrap(),
            0
        );
        assert_eq!(value(&output)["error"]["code"], -32002);

        let rpc_error = br#"{"error":{"code":-1,"message":"denied"},"id":1,"jsonrpc":"2.0"}"#;
        assert!(
            decode_response(rpc_error, &serde_json::json!(1))
                .unwrap_err()
                .contains("denied")
        );
        assert!(
            decode_response(
                br#"{"id":1,"jsonrpc":"1.0","result":{}}"#,
                &serde_json::json!(1)
            )
            .is_err()
        );
        assert!(
            decode_response(
                br#"{"error":1,"id":1,"jsonrpc":"2.0"}"#,
                &serde_json::json!(1)
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn observer_process_and_persisted_plan_run_in_an_explicit_workspace() {
        let observed = tempfile::tempdir().unwrap();
        let expected = crate::digest::sha256(b"created");
        let observe = serde_json::json!({
            "argv": ["/bin/sh", "-c", "printf created > output.txt; printf observed"],
            "environment": [{"name": "MODE", "value": "test"}],
            "expected_files": [{"path": "output.txt", "sha256": expected}],
            "fixtures": [{"contents_base64": "aW5wdXQ=", "executable": false, "path": "input.txt"}],
            "memory_bytes": 1073741824_u64,
            "processes": 512,
            "stderr_bytes": 16777216,
            "stdin_base64": "",
            "stdout_bytes": 16777216,
            "timeout_ms": 5000,
            "working_directory": null
        });
        let result = observe_process_at(observed.path(), observe.as_object().unwrap()).unwrap();
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout_base64"], "b2JzZXJ2ZWQ=");
        assert_eq!(result["files"][0]["kind"], "created");
        assert_eq!(result["files"][0]["path"], "output.txt");
        let mut mismatch = observe.clone();
        mismatch["expected_files"][0]["sha256"] = serde_json::json!("0".repeat(64));
        let mismatched = tempfile::tempdir().unwrap();
        assert_eq!(
            observe_process_at(mismatched.path(), mismatch.as_object().unwrap())
                .unwrap_err()
                .0,
            -32011
        );

        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("build.sh"),
            b"#!/bin/sh\n/usr/bin/printf '%s' \"$1\"\n",
        )
        .unwrap();
        crate::project::init_with_entries(project.path(), &["build.sh".into()]).unwrap();
        crate::project::analyze(project.path(), "build.sh").unwrap();
        let run = serde_json::json!({
            "arguments": [],
            "argv": ["native"],
            "entrypoint": "build.sh",
            "environment": [],
            "expected_files": [],
            "fixtures": [],
            "memory_bytes": 1073741824_u64,
            "node_id": null,
            "processes": 512,
            "stderr_bytes": 16777216,
            "stdin_base64": "",
            "stdout_bytes": 16777216,
            "timeout_ms": 5000,
            "working_directory": null
        });
        let result = run_plan_process_at(project.path(), run.as_object().unwrap()).unwrap();
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout_base64"], "bmF0aXZl");
        assert_eq!(result["files"], serde_json::json!([]));

        let mut invalid_node = run.clone();
        invalid_node["node_id"] = serde_json::json!("");
        assert_eq!(
            run_plan_process_at(project.path(), invalid_node.as_object().unwrap())
                .unwrap_err()
                .0,
            -32602
        );
        let mut broad_limits = run;
        broad_limits["memory_bytes"] = serde_json::json!(2_147_483_648_u64);
        assert_eq!(
            run_plan_process_at(project.path(), broad_limits.as_object().unwrap())
                .unwrap_err()
                .0,
            -32602
        );
    }
}
