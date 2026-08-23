use nu_protocol::engine::StateWorkingSet;
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: i64 = 1;
const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn result(id: Value, value: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": value })
}

fn parse_source(path: &str, source: &str) -> Value {
    let engine_state = nu_cmd_lang::create_default_context();
    let mut working_set = StateWorkingSet::new(&engine_state);
    let block = nu_parser::parse(&mut working_set, Some(path), source.as_bytes(), false);
    let diagnostics: Vec<Value> = working_set
        .parse_errors
        .iter()
        .map(|diagnostic| json!({ "message": format!("{diagnostic:?}") }))
        .collect();
    let compile_diagnostics: Vec<Value> = working_set
        .compile_errors
        .iter()
        .map(|diagnostic| json!({ "message": format!("{diagnostic:?}") }))
        .collect();

    json!({
        "valid": diagnostics.is_empty(),
        "parser": "nu-parser/0.115.0",
        "pipeline_count": block.pipelines.len(),
        "diagnostics": diagnostics,
        "compile_diagnostics": compile_diagnostics
    })
}

fn handle(request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if request.get("jsonrpc") != Some(&Value::String("2.0".to_owned())) {
        return error(id, -32600, "invalid JSON-RPC request");
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return error(id, -32600, "request method must be a string");
    };
    let params = request.get("params").and_then(Value::as_object);

    match method {
        "deshell.handshake" => {
            let version = params
                .and_then(|values| values.get("protocol_version"))
                .and_then(Value::as_i64);
            if version != Some(PROTOCOL_VERSION) {
                error(id, -32001, "unsupported protocol version")
            } else {
                result(
                    id,
                    json!({
                        "protocol_version": PROTOCOL_VERSION,
                        "server": {
                            "name": "deshell-nushell-official-parser",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": ["frontend.detect", "frontend.parse"]
                    }),
                )
            }
        }
        "frontend.detect" => result(
            id,
            json!({ "interpreter": "nu", "confidence": "certain" }),
        ),
        "frontend.parse" => {
            let path = params
                .and_then(|values| values.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("source.nu");
            match params
                .and_then(|values| values.get("source"))
                .and_then(Value::as_str)
            {
                Some(source) => result(id, parse_source(path, source)),
                None => error(id, -32602, "params.source must be a string"),
            }
        }
        _ => error(id, -32601, "method not found"),
    }
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let response = if line.len() > MAX_MESSAGE_BYTES {
            error(Value::Null, -32002, "adapter message exceeds the byte limit")
        } else {
            match serde_json::from_str::<Value>(&line) {
                Ok(request) => handle(request),
                Err(parse_error) => error(Value::Null, -32700, parse_error.to_string()),
            }
        };
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_and_invalid_nushell_with_pinned_parser() {
        let valid = parse_source("build.nu", "^git status");
        assert_eq!(valid["valid"], true, "{valid:#}");
        assert_eq!(valid["parser"], "nu-parser/0.115.0");
        assert!(valid["pipeline_count"].as_u64().unwrap_or_default() > 0);

        let invalid = parse_source("build.nu", "def broken [");
        assert_eq!(invalid["valid"], false);
        assert!(!invalid["diagnostics"].as_array().unwrap().is_empty());
    }

    #[test]
    fn handshake_is_versioned_and_extensible() {
        let response = handle(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "deshell.handshake",
            "params": { "protocol_version": 1, "future": true },
            "future": true
        }));
        assert_eq!(response["id"], 7);
        assert_eq!(response["result"]["protocol_version"], 1);
    }
}
