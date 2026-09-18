mod agent_process;
mod approval;
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
#[expect(dead_code, reason = "constructed by contract paths that are exercised only under specific platforms or feature gates")]
mod lab;
mod local_backend;
mod migration;
// The transactional filesystem layer. Every raw API that `clippy.toml` bans
// elsewhere is implemented here exactly once, in terms of a named intent, so this
// is the only module that reaches for them directly.
#[expect(
    clippy::disallowed_methods,
    reason = "patch implements the intents the ban redirects callers to; it is the one place the raw APIs may appear"
)]
mod patch;
mod project;
#[cfg(test)]
mod properties;
mod protocol;
mod replay;
mod replay_proxy;
mod report;
mod rewrite;
mod runner;
mod scanner;
mod strict_json;
mod verify;
mod workspace;

fn main() -> std::process::ExitCode {
    let code = cli::run_from(
        std::env::args_os(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    // Returning rather than calling `std::process::exit` lets destructors run, so
    // a staged temporary file or a rollback backup held by the transactional layer
    // is released instead of being abandoned on the way out.
    //
    // Exit codes are the fixed categories in the CLI contract plus, for `run`, the
    // exit code of the plan itself; all fit in a byte. A value that does not is an
    // invariant violation, which is what 70 means.
    std::process::ExitCode::from(u8::try_from(code).unwrap_or(70))
}
