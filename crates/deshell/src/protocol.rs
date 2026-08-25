use base64::Engine as _;
use std::io::{BufRead, Write};

pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

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
    let method = fields["method"].as_str().expect("validated method");
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
            Some(Frame::Message(message)) => output
                .write_all(&handle_message(kind, &message))
                .map_err(|error| format!("cannot write RPC response: {error}"))?,
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
        .expect("process outcome object")
        .insert("files".into(), serde_json::Value::Array(changes));
    Ok(result)
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
                .get("contents")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid("fixture.contents must be a string"))?;
            let executable = fields.get("executable").map_or(Ok(false), |value| {
                value
                    .as_bool()
                    .ok_or_else(|| invalid("fixture.executable must be a boolean"))
            })?;
            Ok(crate::config::Fixture {
                path: path.into(),
                contents: contents.into(),
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
    Ok(crate::agent_process::Request {
        argv,
        environment,
        working_directory,
        stdin,
        timeout_ms,
    })
}

fn process_outcome(outcome: &crate::agent_process::Outcome) -> serde_json::Value {
    serde_json::json!({
        "exit_code": outcome.exit_code,
        "signal": outcome.signal,
        "stderr_base64": base64::engine::general_purpose::STANDARD.encode(&outcome.stderr),
        "stdout_base64": base64::engine::general_purpose::STANDARD.encode(&outcome.stdout),
        "timed_out": outcome.timed_out
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
    let mut bytes = crate::canonical_json::canonical_bytes(&value)
        .expect("JSON-RPC response contains only canonical integer JSON");
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
                    "source": {"encoding": "utf8", "text": "^printf hello\\n"},
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
