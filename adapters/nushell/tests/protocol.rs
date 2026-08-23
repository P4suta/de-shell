use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn exchange(stdin: &mut impl Write, stdout: &mut impl BufRead, request: Value) -> Value {
    serde_json::to_writer(&mut *stdin, &request).unwrap();
    stdin.write_all(b"\n").unwrap();
    stdin.flush().unwrap();
    let mut response = String::new();
    stdout.read_line(&mut response).unwrap();
    serde_json::from_str(&response).unwrap()
}

#[test]
fn stdio_json_rpc_contract() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_deshell-nushell-adapter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let handshake = exchange(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "deshell.handshake",
            "params": { "protocol_version": 1 },
            "future": true
        }),
    );
    assert_eq!(handshake["id"], 1);
    assert_eq!(handshake["result"]["protocol_version"], 1);

    let parsed = exchange(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "frontend.parse",
            "params": { "path": "build.nu", "source": "^git status" }
        }),
    );
    assert_eq!(parsed["id"], 2);
    assert_eq!(parsed["result"]["valid"], true);
    assert_eq!(parsed["result"]["parser"], "nu-parser/0.115.0");

    let exported_module = exchange(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "frontend.parse",
            "params": {
                "path": "deshell.nu",
                "source": "export def main [] {\n  run-external \"printf\" \"hello world\"\n}\n"
            }
        }),
    );
    assert_eq!(exported_module["result"]["valid"], true);
    assert_eq!(exported_module["result"]["diagnostics"], json!([]));

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
