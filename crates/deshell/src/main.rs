mod agent_process;
mod audit;
mod canonical_json;
mod cli;
mod config;
mod contract;
mod diagnostics;
mod differential;
mod digest;
mod evidence;
mod exporter;
mod frontend;
mod harden;
mod ir;
#[allow(dead_code)]
mod lab;
mod local_backend;
mod migration;
mod patch;
mod project;
#[cfg(test)]
mod properties;
mod protocol;
mod replay;
mod replay_proxy;
mod rewrite;
mod runner;
mod scanner;
mod strict_json;
mod verify;
mod workspace;

fn main() {
    let code = cli::run_from(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code);
}
