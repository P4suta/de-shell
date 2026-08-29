#![allow(dead_code)]

#[path = "../../crates/deshell/src/agent_process.rs"]
mod agent_process;
#[path = "../../crates/deshell/src/audit.rs"]
mod audit;
#[path = "../../crates/deshell/src/canonical_json.rs"]
mod canonical_json;
#[path = "../../crates/deshell/src/config.rs"]
mod config;
#[path = "../../crates/deshell/src/contract.rs"]
mod contract;
#[path = "../../crates/deshell/src/diagnostics.rs"]
mod diagnostics;
#[path = "../../crates/deshell/src/differential.rs"]
mod differential;
#[path = "../../crates/deshell/src/digest.rs"]
mod digest;
#[path = "../../crates/deshell/src/evidence.rs"]
mod evidence;
#[path = "../../crates/deshell/src/exporter.rs"]
mod exporter;
#[path = "../../crates/deshell/src/frontend.rs"]
mod frontend;
#[path = "../../crates/deshell/src/harden.rs"]
mod harden;
#[path = "../../crates/deshell/src/ir.rs"]
mod ir;
#[path = "../../crates/deshell/src/lab.rs"]
mod lab;
#[path = "../../crates/deshell/src/local_backend.rs"]
mod local_backend;
#[path = "../../crates/deshell/src/migration.rs"]
mod migration;
#[path = "../../crates/deshell/src/patch.rs"]
mod patch;
#[path = "../../crates/deshell/src/project.rs"]
mod project;
#[path = "../../crates/deshell/src/protocol.rs"]
mod protocol;
#[path = "../../crates/deshell/src/replay.rs"]
mod replay;
#[path = "../../crates/deshell/src/replay_proxy.rs"]
mod replay_proxy;
#[path = "../../crates/deshell/src/rewrite.rs"]
mod rewrite;
#[path = "../../crates/deshell/src/runner.rs"]
mod runner;
#[path = "../../crates/deshell/src/scanner.rs"]
mod scanner;
#[path = "../../crates/deshell/src/strict_json.rs"]
mod strict_json;
#[path = "../../crates/deshell/src/verify.rs"]
mod verify;
#[path = "../../crates/deshell/src/workspace.rs"]
mod workspace;

pub fn fuzz_frontend(data: &[u8]) {
    let (selector, source) = data
        .split_first()
        .map_or((0, data), |(head, tail)| (*head, tail));
    let paths = [
        "fuzz/input.sh",
        "fuzz/input.zsh",
        "fuzz/input.fish",
        "fuzz/input.ps1",
        "fuzz/input.cmd",
        "fuzz/input.nu",
        "fuzz/input.unknown",
    ];
    if let Ok(plan) = frontend::lower(
        paths[usize::from(selector) % paths.len()],
        source,
        config::UnknownInterpreter::TraceOnly,
    ) {
        let _ = plan.validate();
        if let Ok(encoded) = plan.encode_pretty() {
            let _ = ir::Plan::decode(&encoded);
        }
    }
}

pub fn fuzz_scanner(data: &[u8]) {
    let (selector, source) = data
        .split_first()
        .map_or((0, data), |(head, tail)| (*head, tail));
    let paths = [
        "input.sh",
        "input.fish",
        "input.ps1",
        "input.cmd",
        "input.nu",
        "Dockerfile",
        "package.json",
        "workflow.yml",
    ];
    let Ok(directory) = tempfile::tempdir() else {
        return;
    };
    let bounded = &source[..source.len().min(64 * 1024)];
    if std::fs::write(
        directory
            .path()
            .join(paths[usize::from(selector) % paths.len()]),
        bounded,
    )
    .is_ok()
    {
        let _ = scanner::scan(directory.path());
    }
}

pub fn fuzz_protocol(data: &[u8]) {
    let _ = protocol::decode_response(data, &serde_json::json!(1));
}

pub fn fuzz_schema(data: &[u8]) {
    let _ = strict_json::parse(data);
    let _ = ir::Plan::decode(data);
}
