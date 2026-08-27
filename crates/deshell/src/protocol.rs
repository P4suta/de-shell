use base64::Engine as _;
use std::io::{BufRead, Write};

pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_CHUNK_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentKind {
    Process,
    Observer,
    Nushell,
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
    let request = process_request(parameters)?;
    let root = std::env::current_dir().map_err(|error| {
        (
            -32010,
            format!("cannot resolve observer workspace: {error}"),
        )
    })?;
    if let Some(values) = parameters.get("fixtures") {
        let fixtures = fixture_params(values)?;
        crate::workspace::materialize(&root, &fixtures).map_err(|error| (-32602, error))?;
    }
    let before = crate::workspace::capture(&root).map_err(|error| (-32010, error))?;
    let outcome = crate::agent_process::execute(&root, request).map_err(|error| (-32010, error))?;
    let after = crate::workspace::capture(&root).map_err(|error| (-32010, error))?;
    if let Some(values) = parameters.get("expected_files") {
        let expected = expected_file_params(values)?;
        crate::workspace::validate_expected(&root, &expected)
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
    let root = std::env::current_dir().map_err(|error| {
        (
            -32010,
            format!("cannot resolve observer workspace: {error}"),
        )
    })?;
    if let Some(values) = parameters.get("fixtures") {
        let fixtures = fixture_params(values)?;
        crate::workspace::materialize(&root, &fixtures).map_err(|error| (-32602, error))?;
    }
    let before = crate::workspace::capture(&root).map_err(|error| (-32010, error))?;
    let config =
        crate::project::load_config(&root).map_err(|errors| (-32010, errors.join("; ")))?;
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
    let (mut plan, _) = crate::project::load_entry_artifacts(&root, entrypoint)
        .map_err(|errors| (-32010, errors.join("; ")))?;
    if let Some(node_id) = node_id {
        plan = select_plan_node(plan, node_id).map_err(|error| (-32602, error))?;
    }
    let lock = crate::project::load_lock(&root).map_err(|errors| (-32010, errors.join("; ")))?;
    let backend = crate::local_backend::LocalBackend::with_pinned_interpreters(
        &root,
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
    let after = crate::workspace::capture(&root).map_err(|error| (-32010, error))?;
    if let Some(values) = parameters.get("expected_files") {
        let expected = expected_file_params(values)?;
        crate::workspace::validate_expected(&root, &expected)
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
}
