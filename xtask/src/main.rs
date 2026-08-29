use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

const PERFORMANCE_WARMUPS: usize = 5;
const PERFORMANCE_SAMPLES: usize = 20;
const PERFORMANCE_HOST_FILES: usize = 4_096;
const REQUIRED_CONTRACTS: &[&str] = &[
    "contracts/README.md",
    "contracts/canonical-json-v1.md",
    "contracts/diagnostics-v1.md",
    "contracts/effect-ir-v1.md",
    "contracts/json-rpc-v1.md",
    "contracts/project-v1.md",
    "contracts/cli/cases.json",
    "contracts/golden/frontend-v1.json",
    "contracts/golden/transform-export-v1.json",
    "contracts/schema/effect-ir-v1.schema.json",
    "contracts/schema/inventory-v1.schema.json",
    "contracts/schema/manifest-v1.schema.json",
    "contracts/schema/bundle-v1.schema.json",
    "contracts/schema/evidence-v1.schema.json",
    "contracts/schema/diagnostic-v1.schema.json",
    "contracts/schema/protocol-v1.schema.json",
    "contracts/schema/project-v1.schema.json",
    "contracts/schema/scenario-v1.schema.json",
    "contracts/schema/lock-v1.schema.json",
    "contracts/schema/replay-v1.schema.json",
    "contracts/schema/corpus-audit-v1.schema.json",
    "contracts/schema/generator-protocol-v1.schema.json",
    "contracts/schema/migration-request-v1.schema.json",
    "contracts/schema/proposal-v1.schema.json",
    "contracts/schema/migration-plan-v1.schema.json",
    "contracts/schema/migration-evidence-v1.schema.json",
    "contracts/schema/archive-manifest-v1.schema.json",
    "contracts/schema/audit-finding-v1.schema.json",
    "contracts/schema/harden-plan-v1.schema.json",
    "contracts/schema/harden-approval-v1.schema.json",
    "contracts/schema/harden-evidence-v1.schema.json",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliContract {
    schema_version: u32,
    diagnostic_modes: Vec<String>,
    exit_codes: ExitCodes,
    cases: Vec<CliCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExitCodes {
    success: i32,
    execution_io: i32,
    usage: i32,
    invalid_contract: i32,
    policy: i32,
    difference: i32,
    provider_unavailable: i32,
    internal: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CliCase {
    argv: Vec<String>,
    exit: i32,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    stdout_artifact: bool,
    #[serde(default)]
    stderr_only: bool,
    #[serde(default)]
    fixture: Option<String>,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("repository root")
}

fn validate_contract_tree(_root: &Path) -> Result<CliContract, Vec<String>> {
    let root = _root;
    let mut errors = Vec::new();
    for relative in REQUIRED_CONTRACTS {
        let path = root.join(relative);
        match std::fs::read(&path) {
            Err(error) => errors.push(format!("missing or unreadable {relative}: {error}")),
            Ok(bytes) => {
                if !bytes.ends_with(b"\n") {
                    errors.push(format!("contract file must end in LF: {relative}"));
                }
                if relative.ends_with(".json")
                    && serde_json::from_slice::<serde_json::Value>(&bytes).is_err()
                {
                    errors.push(format!("contract file is not valid JSON: {relative}"));
                }
            }
        }
    }
    let cases_path = root.join("contracts/cli/cases.json");
    let contract = match std::fs::read(&cases_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CliContract>(&bytes).ok())
    {
        Some(contract) => contract,
        None => {
            errors.push("contracts/cli/cases.json does not match the CLI contract shape".into());
            return Err(errors);
        }
    };
    if contract.schema_version != 1 {
        errors.push("CLI contract schema_version must be 1".into());
    }
    if contract.diagnostic_modes != ["human", "jsonl"] {
        errors.push("CLI diagnostic modes must be human,jsonl".into());
    }
    let exits = &contract.exit_codes;
    if (
        exits.success,
        exits.execution_io,
        exits.usage,
        exits.invalid_contract,
        exits.policy,
        exits.difference,
        exits.provider_unavailable,
        exits.internal,
    ) != (0, 1, 2, 3, 4, 5, 6, 70)
    {
        errors.push("CLI exit code table does not match v1".into());
    }
    let allowed = [0, 1, 2, 3, 4, 5, 6, 70];
    let mut seen = std::collections::BTreeSet::new();
    for case in &contract.cases {
        if case.argv.is_empty() {
            errors.push("CLI case argv must not be empty".into());
        }
        let key = case.argv.join("\u{0}");
        if !seen.insert(key) {
            errors.push(format!("duplicate CLI case: {:?}", case.argv));
        }
        if !allowed.contains(&case.exit) {
            errors.push(format!(
                "CLI case uses an undeclared exit code: {}",
                case.exit
            ));
        }
        if case.stdout.is_some() && (case.stdout_artifact || case.stderr_only) {
            errors.push(format!(
                "CLI case has conflicting output assertions: {:?}",
                case.argv
            ));
        }
        if let Some(fixture) = &case.fixture
            && fixture != "reject-unknown"
        {
            errors.push(format!("unknown CLI fixture: {fixture}"));
        }
    }
    if errors.is_empty() {
        Ok(contract)
    } else {
        Err(errors)
    }
}

fn run_conformance(root: &Path, binary: &Path) -> Result<(), Vec<String>> {
    let contract = validate_contract_tree(root)?;
    let binary = if binary.is_file() {
        binary.canonicalize().map_err(|error| {
            vec![format!(
                "cannot resolve deshell binary {}: {error}",
                binary.display()
            )]
        })?
    } else if cfg!(windows) && binary.with_extension("exe").is_file() {
        binary
            .with_extension("exe")
            .canonicalize()
            .map_err(|error| {
                vec![format!(
                    "cannot resolve deshell binary {}: {error}",
                    binary.with_extension("exe").display()
                )]
            })?
    } else {
        return Err(vec![format!(
            "deshell binary does not exist: {}",
            binary.display()
        )]);
    };
    let mut errors = Vec::new();
    for case in &contract.cases {
        let directory = match tempfile::tempdir() {
            Ok(directory) => directory,
            Err(error) => {
                errors.push(format!("cannot create CLI fixture: {error}"));
                continue;
            }
        };
        if let Some(fixture) = &case.fixture
            && let Err(error) = prepare_fixture(&binary, directory.path(), fixture)
        {
            errors.push(format!("case {:?}: {error}", case.argv));
            continue;
        }
        let output = match std::process::Command::new(&binary)
            .args(&case.argv)
            .current_dir(directory.path())
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                errors.push(format!("case {:?}: could not execute: {error}", case.argv));
                continue;
            }
        };
        let exit = output.status.code().unwrap_or(1);
        if exit != case.exit {
            errors.push(format!(
                "case {:?}: expected exit {}, found {exit}; stderr={}",
                case.argv,
                case.exit,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if let Some(expected) = &case.stdout
            && output.stdout != expected.as_bytes()
        {
            errors.push(format!("case {:?}: stdout bytes differ", case.argv));
        }
        if case.stdout_artifact
            && serde_json::from_slice::<serde_json::Value>(&output.stdout).is_err()
        {
            errors.push(format!(
                "case {:?}: stdout is not a JSON artifact",
                case.argv
            ));
        }
        if case.stderr_only && (!output.stdout.is_empty() || output.stderr.is_empty()) {
            errors.push(format!(
                "case {:?}: expected empty stdout and non-empty stderr",
                case.argv
            ));
        }
    }
    for mode in [
        "__process-agent",
        "__observer-agent",
        "__nushell-adapter",
        "__generator",
    ] {
        if let Err(error) = smoke_agent(&binary, mode) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn prepare_fixture(binary: &Path, root: &Path, fixture: &str) -> Result<(), String> {
    match fixture {
        "reject-unknown" => {
            let output = std::process::Command::new(binary)
                .args(["init", "--root", "."])
                .current_dir(root)
                .output()
                .map_err(|error| error.to_string())?;
            if !output.status.success() {
                return Err(format!(
                    "fixture init failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            let config_path = root.join(".deshell/project.toml");
            let config = std::fs::read_to_string(&config_path)
                .map_err(|error| format!("cannot read initialized fixture config: {error}"))?;
            if !config.contains("entrypoints = []") {
                return Err("initialized fixture config omitted the empty entrypoint list".into());
            }
            let config = config.replacen("entrypoints = []", "entrypoints = [\"unknown.ext\"]", 1);
            std::fs::write(config_path, config).map_err(|error| error.to_string())?;
            std::fs::write(root.join("unknown.ext"), b"dynamic syntax\n")
                .map_err(|error| error.to_string())
        }
        other => Err(format!("unknown fixture: {other}")),
    }
}

fn smoke_agent(binary: &Path, mode: &str) -> Result<(), String> {
    use std::io::Write as _;
    let mut child = std::process::Command::new(binary)
        .arg(mode)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("{mode}: could not start: {error}"))?;
    let request = b"{\"id\":1,\"jsonrpc\":\"2.0\",\"method\":\"deshell.handshake\",\"params\":{\"protocol_version\":1}}\n";
    child
        .stdin
        .take()
        .ok_or_else(|| format!("{mode}: stdin unavailable"))?
        .write_all(request)
        .map_err(|error| format!("{mode}: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{mode}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{mode}: handshake process failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("{mode}: invalid handshake JSON: {error}"))?;
    let valid_result = if mode == "__generator" {
        response["result"]["schema_version"] == 1
            && response["result"]["protocol"] == "deshell.generator.v1"
    } else {
        response["result"]["protocol_version"] == 1
    };
    if response["id"] != 1 || !valid_result {
        return Err(format!("{mode}: invalid handshake response"));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct PerformanceReport {
    schema_version: u32,
    operating_system: &'static str,
    architecture: &'static str,
    binary: PerformanceBinary,
    settings: PerformanceSettings,
    benchmarks: PerformanceBenchmarks,
}

#[derive(Debug, Serialize)]
struct PerformanceBinary {
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct PerformanceSettings {
    warmup_iterations: usize,
    measured_iterations: usize,
    scan_host_files: usize,
}

#[derive(Debug, Serialize)]
struct PerformanceBenchmarks {
    scan: PerformanceMetric,
    simple_run: PerformanceMetric,
}

#[derive(Debug, Serialize)]
struct PerformanceMetric {
    median_ms: f64,
    p95_ms: f64,
}

fn run_performance(binary: &Path) -> Result<(), Vec<String>> {
    let binary = resolve_binary(binary)?;
    let binary_size = std::fs::metadata(&binary)
        .map_err(|error| vec![format!("cannot inspect {}: {error}", binary.display())])?
        .len();
    let fixture = tempfile::tempdir()
        .map_err(|error| vec![format!("cannot create performance fixture: {error}")])?;
    let scan_root = fixture.path().join("scan-corpus");
    let project_root = fixture.path().join("simple-run");
    prepare_scan_corpus(&scan_root).map_err(|error| vec![error])?;
    prepare_simple_run_project(&binary, &project_root).map_err(|error| vec![error])?;

    let scan_arguments = [
        "scan".to_owned(),
        "--root".to_owned(),
        scan_root.to_string_lossy().into_owned(),
    ];
    let run_arguments = [
        "run".to_owned(),
        "--root".to_owned(),
        project_root.to_string_lossy().into_owned(),
        "--backend".to_owned(),
        "local".to_owned(),
    ];
    let scan = measure_command(&binary, &scan_arguments, "scan")?;
    let simple_run = measure_command(&binary, &run_arguments, "simple-run")?;
    let report = PerformanceReport {
        schema_version: 1,
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        binary: PerformanceBinary {
            path: binary.to_string_lossy().into_owned(),
            size_bytes: binary_size,
        },
        settings: PerformanceSettings {
            warmup_iterations: PERFORMANCE_WARMUPS,
            measured_iterations: PERFORMANCE_SAMPLES,
            scan_host_files: PERFORMANCE_HOST_FILES,
        },
        benchmarks: PerformanceBenchmarks { scan, simple_run },
    };
    serde_json::to_writer_pretty(std::io::stdout().lock(), &report)
        .map_err(|error| vec![format!("cannot encode performance report: {error}")])?;
    println!();
    Ok(())
}

fn resolve_binary(binary: &Path) -> Result<PathBuf, Vec<String>> {
    let candidate = if binary.is_file() {
        binary.to_path_buf()
    } else if cfg!(windows) && binary.with_extension("exe").is_file() {
        binary.with_extension("exe")
    } else {
        return Err(vec![format!(
            "deshell binary does not exist: {}",
            binary.display()
        )]);
    };
    candidate.canonicalize().map_err(|error| {
        vec![format!(
            "cannot resolve deshell binary {}: {error}",
            candidate.display()
        )]
    })
}

fn prepare_scan_corpus(root: &Path) -> Result<(), String> {
    for index in 0..PERFORMANCE_HOST_FILES {
        let language = if index % 2 == 0 {
            "python"
        } else {
            "javascript"
        };
        let directory = root.join(language).join(format!("{:02}", index % 64));
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        let (extension, source) = if index % 2 == 0 {
            (
                "py",
                concat!(
                    "# deterministic host-heavy scanner corpus: 日本語\n",
                    "import os, subprocess\n",
                    "os.system('printf one')\n",
                    "subprocess.run(\"printf two\")\n",
                    "subprocess.call(command)\n",
                    "subprocess.Popen('printf three')\n",
                ),
            )
        } else {
            (
                "js",
                concat!(
                    "// deterministic host-heavy scanner corpus: 日本語\n",
                    "const child_process = require('child_process');\n",
                    "child_process.exec('printf one');\n",
                    "execSync(\"printf two\");\n",
                    "exec(command);\n",
                    "child_process.execSync('printf three');\n",
                ),
            )
        };
        let path = directory.join(format!("host-{index:05}.{extension}"));
        std::fs::write(&path, source)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn prepare_simple_run_project(binary: &Path, root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("cannot create {}: {error}", root.display()))?;
    let entry = "benchmark.sh";
    let source = if cfg!(windows) {
        b"cmd.exe /d /c exit 0\n".as_slice()
    } else {
        b"/bin/true\n".as_slice()
    };
    std::fs::write(root.join(entry), source)
        .map_err(|error| format!("cannot write simple-run entrypoint: {error}"))?;
    command_success(
        binary,
        &[
            "init".to_owned(),
            "--root".to_owned(),
            root.to_string_lossy().into_owned(),
            "--entry".to_owned(),
            entry.to_owned(),
        ],
        "simple-run init",
    )?;
    let config_path = root.join(".deshell/project.toml");
    let config = std::fs::read_to_string(&config_path)
        .map_err(|error| format!("cannot read {}: {error}", config_path.display()))?
        .replace("allow_local = false", "allow_local = true");
    std::fs::write(&config_path, config)
        .map_err(|error| format!("cannot write {}: {error}", config_path.display()))?;
    command_success(
        binary,
        &[
            "analyze".to_owned(),
            "--root".to_owned(),
            root.to_string_lossy().into_owned(),
            "--entry".to_owned(),
            entry.to_owned(),
        ],
        "simple-run analyze",
    )
}

fn command_success(binary: &Path, arguments: &[String], label: &str) -> Result<(), String> {
    let output = std::process::Command::new(binary)
        .args(arguments)
        .output()
        .map_err(|error| format!("cannot execute {label}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{label} failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn measure_command(
    binary: &Path,
    arguments: &[String],
    label: &str,
) -> Result<PerformanceMetric, Vec<String>> {
    for _ in 0..PERFORMANCE_WARMUPS {
        timed_command(binary, arguments, label)?;
    }
    let mut samples = Vec::with_capacity(PERFORMANCE_SAMPLES);
    for _ in 0..PERFORMANCE_SAMPLES {
        samples.push(timed_command(binary, arguments, label)?);
    }
    samples.sort_unstable();
    let middle = samples.len() / 2;
    let median_ns = (samples[middle - 1] as f64 + samples[middle] as f64) / 2.0;
    let p95_index = (samples.len() * 95).div_ceil(100) - 1;
    Ok(PerformanceMetric {
        median_ms: median_ns / 1_000_000.0,
        p95_ms: samples[p95_index] as f64 / 1_000_000.0,
    })
}

fn timed_command(binary: &Path, arguments: &[String], label: &str) -> Result<u128, Vec<String>> {
    let start = Instant::now();
    let status = std::process::Command::new(binary)
        .args(arguments)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| vec![format!("cannot execute performance {label}: {error}")])?;
    let elapsed = start.elapsed().as_nanos();
    if !status.success() {
        return Err(vec![format!(
            "performance {label} failed with status {status}"
        )]);
    }
    Ok(elapsed)
}

struct MigrationFixture {
    path: &'static str,
    source: &'static [u8],
}

struct EmbeddedMigrationFixture {
    path: &'static str,
    source: String,
    command: String,
}

fn migration_fixture(interpreter: &str) -> Result<MigrationFixture, String> {
    match interpreter {
        "sh" => Ok(MigrationFixture {
            path: "corpus.sh",
            source: concat!(
                "#!/bin/sh\n",
                "/usr/bin/printf '%s:%s\\n' \"$1\" \"$CORPUS_ENV\"\n",
                "/usr/bin/test \"$1\" = pass && /usr/bin/printf '%s\\n' branch\n",
            )
            .as_bytes(),
        }),
        "bash" => Ok(MigrationFixture {
            path: "corpus.sh",
            source: concat!(
                "#!/usr/bin/env bash\n",
                "/usr/bin/printf '%s:%s\\n' \"$1\" \"$CORPUS_ENV\"\n",
                "/usr/bin/test \"$1\" = pass && /usr/bin/printf '%s\\n' branch\n",
            )
            .as_bytes(),
        }),
        "zsh" => Ok(MigrationFixture {
            path: "corpus.zsh",
            source: concat!(
                "#!/usr/bin/env zsh\n",
                "/usr/bin/printf '%s:%s\\n' \"$1\" \"$CORPUS_ENV\"\n",
                "/usr/bin/test \"$1\" = pass && /usr/bin/printf '%s\\n' branch\n",
            )
            .as_bytes(),
        }),
        "fish" => Ok(MigrationFixture {
            path: "corpus.fish",
            source: concat!(
                "#!/usr/bin/env fish\n",
                "command /usr/bin/printf '%s:%s\\n' \"$argv[1]\" \"$CORPUS_ENV\"\n",
                "command /usr/bin/test \"$argv[1]\" = pass && command /usr/bin/printf '%s\\n' branch\n",
            )
            .as_bytes(),
        }),
        "powershell" if cfg!(windows) => Ok(MigrationFixture {
            path: "corpus.ps1",
            source: concat!(
                "& '.\\target\\deshell-corpus-helper.exe' 'emit' $args[0] $env:CORPUS_ENV\r\n",
                "& '.\\target\\deshell-corpus-helper.exe' 'test' $args[0] && & '.\\target\\deshell-corpus-helper.exe' 'branch'\r\n",
                "exit $LASTEXITCODE\r\n",
            )
            .as_bytes(),
        }),
        "powershell" => Ok(MigrationFixture {
            path: "corpus.ps1",
            source: concat!(
                "& './target/deshell-corpus-helper' 'emit' $args[0] $env:CORPUS_ENV\n",
                "& './target/deshell-corpus-helper' 'test' $args[0] && & './target/deshell-corpus-helper' 'branch'\n",
                "exit $LASTEXITCODE\n",
            )
            .as_bytes(),
        }),
        "cmd" => Ok(MigrationFixture {
            path: "corpus.cmd",
            source: concat!(
                "@echo off\r\n",
                "target\\deshell-corpus-helper.exe emit \"%~1\" \"%CORPUS_ENV%\"\r\n",
                "target\\deshell-corpus-helper.exe test \"%~1\" && target\\deshell-corpus-helper.exe branch\r\n",
            )
            .as_bytes(),
        }),
        "nu" => Ok(MigrationFixture {
            path: "corpus.nu",
            source: concat!(
                "def main [value: string] {\n",
                "  ^/usr/bin/printf '%s:%s\\n' $value $env.CORPUS_ENV\n",
                "  ^/usr/bin/test $value '=' pass\n",
                "  if $env.LAST_EXIT_CODE == 0 {\n",
                "    ^/usr/bin/printf '%s\\n' branch\n",
                "  } else {\n",
                "    ^/usr/bin/false\n",
                "  }\n",
                "}\n",
            )
            .as_bytes(),
        }),
        other => Err(format!("unsupported migration E2E interpreter: {other}")),
    }
}

fn migration_embedded_fixture(interpreter: &str) -> Result<EmbeddedMigrationFixture, String> {
    let unix_helper = "./target/deshell-corpus-helper";
    let (shell, command) = match interpreter {
        "sh" => ("sh {0}", format!("{unix_helper} branch")),
        "bash" => ("bash {0}", format!("{unix_helper} branch")),
        "zsh" => ("zsh {0}", format!("{unix_helper} branch")),
        "fish" => ("fish {0}", format!("command {unix_helper} branch")),
        "powershell" if cfg!(windows) => (
            "pwsh",
            "& '.\\target\\deshell-corpus-helper.exe' 'branch'".into(),
        ),
        "powershell" => ("pwsh", "& './target/deshell-corpus-helper' 'branch'".into()),
        "cmd" => (
            "cmd",
            "@echo off\ntarget\\deshell-corpus-helper.exe branch".into(),
        ),
        "nu" => ("nu {0}", format!("^{unix_helper} branch")),
        other => return Err(format!("unsupported embedded interpreter: {other}")),
    };
    let indented_command = command.replace('\n', "\n          ");
    let source = format!(
        concat!(
            "defaults:\n",
            "  run:\n",
            "    shell: {shell}\n",
            "jobs:\n",
            "  embedded:\n",
            "    runs-on: fixture\n",
            "    steps:\n",
            "      - name: embedded\n",
            "        run: |-\n",
            "          {command}\n",
        ),
        shell = shell,
        command = indented_command,
    );
    Ok(EmbeddedMigrationFixture {
        path: ".github/workflows/embedded.yml",
        source,
        command,
    })
}

fn prepare_migration_helper(root: &Path) -> Result<(), String> {
    let target = root.join("target");
    std::fs::create_dir_all(&target)
        .map_err(|error| format!("cannot create migration helper directory: {error}"))?;
    let source = target.join("deshell_corpus_helper.rs");
    std::fs::write(
        &source,
        concat!(
            "use std::{env, process};\n",
            "fn main() {\n",
            "    let mut args = env::args().skip(1);\n",
            "    match args.next().as_deref() {\n",
            "        Some(\"emit\") => println!(\"{}:{}\", args.next().unwrap_or_default(), args.next().unwrap_or_default()),\n",
            "        Some(\"test\") => process::exit(i32::from(args.next().as_deref() != Some(\"pass\"))),\n",
            "        Some(\"branch\") => println!(\"branch\"),\n",
            "        _ => process::exit(64),\n",
            "    }\n",
            "}\n",
        ),
    )
    .map_err(|error| format!("cannot write migration helper source: {error}"))?;
    let executable = target.join(if cfg!(windows) {
        "deshell-corpus-helper.exe"
    } else {
        "deshell-corpus-helper"
    });
    let output = std::process::Command::new("rustc")
        .arg("--edition=2024")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .map_err(|error| format!("cannot start rustc for migration helper: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot build migration helper: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn powershell_runtime_path() -> Result<String, String> {
    let current = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
    let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
    let executable = if cfg!(windows) { "pwsh.exe" } else { "pwsh" };
    let Some(index) = paths.iter().position(|path| {
        path.join(executable).is_file()
            && !path
                .components()
                .any(|component| component.as_os_str() == "shims")
    }) else {
        return current
            .into_string()
            .map_err(|_| "PATH is not valid Unicode".into());
    };
    let runtime = paths.remove(index);
    paths.insert(0, runtime);
    std::env::join_paths(paths)
        .map_err(|error| format!("cannot construct PowerShell runtime PATH: {error}"))?
        .into_string()
        .map_err(|_| "PowerShell runtime PATH is not valid Unicode".into())
}

fn migration_interpreter_supported_on(interpreter: &str, operating_system: &str) -> bool {
    match interpreter {
        "powershell" => true,
        "cmd" => operating_system == "windows",
        "sh" | "bash" | "zsh" | "fish" | "nu" => operating_system != "windows",
        _ => false,
    }
}

fn run_migration_e2e(
    repository: &Path,
    interpreter: &str,
    generator: &str,
) -> Result<(), Vec<String>> {
    if !matches!(generator, "rust" | "go") {
        return Err(vec![format!(
            "unsupported migration E2E generator: {generator}"
        )]);
    }
    if !migration_interpreter_supported_on(interpreter, std::env::consts::OS) {
        return Err(vec![format!(
            "migration E2E interpreter {interpreter} is assigned to the wrong operating system"
        )]);
    }
    let fixture = migration_fixture(interpreter).map_err(|error| vec![error])?;
    let embedded = migration_embedded_fixture(interpreter).map_err(|error| vec![error])?;
    let directory = tempfile::tempdir()
        .map_err(|error| vec![format!("cannot create migration E2E project: {error}")])?;
    let mut binary = repository.join("target/debug/deshell");
    if cfg!(windows) {
        binary.set_extension("exe");
    }
    let binary = binary
        .canonicalize()
        .map_err(|error| vec![format!("cannot resolve {}: {error}", binary.display())])?;
    std::fs::write(directory.path().join(fixture.path), fixture.source)
        .map_err(|error| vec![format!("cannot write {}: {error}", fixture.path)])?;
    std::fs::create_dir_all(directory.path().join(".github/workflows")).map_err(|error| {
        vec![format!(
            "cannot create embedded workflow directory: {error}"
        )]
    })?;
    std::fs::write(directory.path().join(embedded.path), &embedded.source)
        .map_err(|error| vec![format!("cannot write {}: {error}", embedded.path)])?;
    prepare_migration_helper(directory.path()).map_err(|error| vec![error])?;
    run_deshell(
        &binary,
        directory.path(),
        &["init", "--root", ".", "--entry", fixture.path],
    )?;
    let config_path = directory.path().join(".deshell/project.toml");
    let mut config = std::fs::read_to_string(&config_path)
        .map_err(|error| vec![format!("cannot read {}: {error}", config_path.display())])?;
    let embedded_start = embedded
        .source
        .find("        run: |-")
        .ok_or_else(|| vec!["embedded migration fixture omitted its run span".into()])?;
    let encoded_command = embedded.command.replace('\n', "\n          ");
    if !embedded.source.contains(&encoded_command) {
        return Err(vec![
            "embedded migration fixture source map omitted its decoded command".into(),
        ]);
    }
    let embedded_end = embedded
        .source
        .strip_suffix('\n')
        .map_or(embedded.source.len(), str::len);
    config = config.replace(
        "location_overrides = []",
        &format!(
            "location_overrides = [{{ path = \"{}\", start_byte = {embedded_start}, end_byte = {embedded_end}, generator = \"host\", target = \"host\", module_root = \"host\" }}]",
            embedded.path,
        ),
    );
    config = config.replace(
        "platform_cells = []",
        &format!(
            "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}]",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    let module_root = if generator == "go" { "cmd" } else { "src/bin" };
    if generator == "go" {
        config = config
            .replacen("generator = \"rust\"", "generator = \"go\"", 1)
            .replacen("target = \"rust\"", "target = \"go\"", 1)
            .replacen("module_root = \"src/bin\"", "module_root = \"cmd\"", 1);
    }
    std::fs::write(&config_path, config)
        .map_err(|error| vec![format!("cannot write {}: {error}", config_path.display())])?;
    std::fs::create_dir_all(directory.path().join(module_root))
        .map_err(|error| vec![format!("cannot create {module_root}: {error}")])?;
    let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
    let mut scenario_template = std::fs::read_to_string(&scenario_path)
        .map_err(|error| vec![format!("cannot read {}: {error}", scenario_path.display())])?;
    if interpreter == "powershell" {
        scenario_template =
            scenario_template.replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
    }
    let rich_matrix = matches!(
        interpreter,
        "sh" | "bash" | "zsh" | "fish" | "powershell" | "cmd" | "nu"
    );
    let scenario_environment = if interpreter == "powershell" {
        let runtime_path =
            serde_json::to_string(&powershell_runtime_path().map_err(|error| vec![error])?)
                .map_err(|error| vec![format!("cannot encode PowerShell runtime PATH: {error}")])?;
        format!(
            "environment = [{{ name = \"CORPUS_ENV\", value = \"matrix\" }}, {{ name = \"PATH\", value = {runtime_path} }}]"
        )
    } else {
        "environment = [{ name = \"CORPUS_ENV\", value = \"matrix\" }]".into()
    };
    let mut scenario = scenario_template.replace("approval = \"draft\"", "approval = \"approved\"");
    if rich_matrix {
        scenario = scenario
            .replace(
                "arguments = []",
                "arguments = [{ name = \"1\", value = \"pass\" }]",
            )
            .replace("argv = []", "argv = [\"pass\"]")
            .replace("environment = []", &scenario_environment);

        let failure = scenario_template
            .replace("name = \"default\"", "name = \"failure\"")
            .replace("approval = \"draft\"", "approval = \"approved\"")
            .replace(
                "arguments = []",
                "arguments = [{ name = \"1\", value = \"fail\" }]",
            )
            .replace("argv = []", "argv = [\"fail\"]")
            .replace("environment = []", &scenario_environment);
        let failure_path = directory.path().join(".deshell/scenarios/failure.toml");
        std::fs::write(&failure_path, failure)
            .map_err(|error| vec![format!("cannot write {}: {error}", failure_path.display())])?;
    }
    std::fs::write(&scenario_path, scenario)
        .map_err(|error| vec![format!("cannot write {}: {error}", scenario_path.display())])?;

    let planned = run_deshell(
        &binary,
        directory.path(),
        &["migrate", "plan", "--root", "."],
    )?;
    if planned.contains("blocker ") {
        return Err(vec![format!(
            "{interpreter}/{generator} migration plan was blocked:\n{planned}"
        )]);
    }
    let digest = planned
        .lines()
        .find_map(|line| line.strip_prefix("plan "))
        .ok_or_else(|| vec![format!("migration plan omitted its digest: {planned}")])?;
    if let Err(mut errors) = run_deshell(
        &binary,
        directory.path(),
        &[
            "migrate",
            "verify",
            "--root",
            ".",
            "--plan",
            digest,
            "--cell",
            "host",
            "--output",
            "evidence.json",
        ],
    ) {
        if let Ok(evidence) = std::fs::read_to_string(directory.path().join("evidence.json")) {
            errors.push(format!("migration Evidence:\n{evidence}"));
        }
        return Err(errors);
    }
    if rich_matrix {
        let evidence_path = directory.path().join("evidence.json");
        let evidence: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&evidence_path).map_err(|error| {
                vec![format!("cannot read {}: {error}", evidence_path.display())]
            })?)
            .map_err(|error| vec![format!("invalid migration Evidence JSON: {error}")])?;
        let outcomes = evidence["checks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|check| check["source"]["path"].as_str() == Some(fixture.path))
            .filter_map(|check| {
                Some((
                    check["scenario"].as_str()?.to_owned(),
                    check["comparisons"][0]["original"]["exit_code"].as_i64()?,
                ))
            })
            .collect::<std::collections::BTreeSet<_>>();
        let expected = std::collections::BTreeSet::from([
            ("default".to_owned(), 0),
            ("failure".to_owned(), 1),
        ]);
        if outcomes != expected {
            return Err(vec![format!(
                "{interpreter}/{generator} Evidence omitted success/failure branch outcomes: {outcomes:?}"
            )]);
        }
    }
    run_deshell(
        &binary,
        directory.path(),
        &[
            "migrate",
            "evidence",
            "import",
            "--root",
            ".",
            "--plan",
            digest,
            "evidence.json",
        ],
    )?;
    run_deshell(
        &binary,
        directory.path(),
        &["migrate", "apply", "--root", ".", "--plan", digest],
    )?;
    run_deshell(
        &binary,
        directory.path(),
        &["verify", "--root", ".", "--require", "shell-free"],
    )?;
    let manifest_path = directory.path().join(".deshell/archive/manifest.json");
    if directory.path().join(fixture.path).exists() || !manifest_path.is_file() {
        return Err(vec![format!(
            "{interpreter}/{generator} did not atomically retire and archive {}",
            fixture.path
        )]);
    }
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path)
            .map_err(|error| vec![format!("cannot read {}: {error}", manifest_path.display())])?,
    )
    .map_err(|error| vec![format!("invalid archive manifest JSON: {error}")])?;
    let entries = manifest["entries"]
        .as_array()
        .ok_or_else(|| vec!["archive manifest omitted entries".into()])?;
    let embedded_entry = entries
        .iter()
        .find(|entry| entry["original"]["path"].as_str() == Some(embedded.path));
    if entries.len() != 2 || embedded_entry.is_none() {
        return Err(vec![format!(
            "{interpreter}/{generator} archive manifest did not contain exactly the shell file and embedded snippet"
        )]);
    }
    let archive_path = embedded_entry.unwrap()["archive_path"]
        .as_str()
        .ok_or_else(|| vec!["embedded archive entry omitted its path".into()])?;
    let archived = std::fs::read(directory.path().join(archive_path)).map_err(|error| {
        vec![format!(
            "cannot read embedded archive {archive_path}: {error}"
        )]
    })?;
    if archived != embedded.command.as_bytes() {
        return Err(vec![format!(
            "{interpreter}/{generator} archived host bytes instead of the decoded embedded snippet"
        )]);
    }
    let workflow = std::fs::read_to_string(directory.path().join(embedded.path))
        .map_err(|error| vec![format!("cannot read migrated {}: {error}", embedded.path)])?;
    if !workflow.contains("uses: ./.github/actions/deshell-") || workflow.contains("run: |-") {
        return Err(vec![format!(
            "{interpreter}/{generator} did not replace the embedded run step with a local action"
        )]);
    }
    Ok(())
}

fn run_deshell(binary: &Path, root: &Path, arguments: &[&str]) -> Result<String, Vec<String>> {
    let output = std::process::Command::new(binary)
        .args(arguments)
        .current_dir(root)
        .output()
        .map_err(|error| {
            vec![format!(
                "cannot execute {} {}: {error}",
                binary.display(),
                arguments.join(" ")
            )]
        })?;
    if !output.status.success() {
        return Err(vec![format!(
            "{} {} failed with {}: {}",
            binary.display(),
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )]);
    }
    String::from_utf8(output.stdout)
        .map_err(|error| vec![format!("deshell stdout is not UTF-8: {error}")])
}

fn dispatch(root: &Path, arguments: &[std::ffi::OsString]) -> Result<(), Vec<String>> {
    match arguments.first().and_then(|value| value.to_str()) {
        Some("conformance") => {
            let binary = arguments
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/debug/deshell"));
            run_conformance(root, &binary)
        }
        Some("validate-contracts") => validate_contract_tree(root).map(|_| ()),
        Some("performance") => {
            let binary = arguments
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/release/deshell"));
            run_performance(&binary)
        }
        Some("migration-e2e") => {
            let interpreter = arguments
                .get(1)
                .and_then(|value| value.to_str())
                .ok_or_else(|| vec!["migration-e2e requires INTERPRETER".into()]);
            let generator = arguments
                .get(2)
                .and_then(|value| value.to_str())
                .ok_or_else(|| vec!["migration-e2e requires GENERATOR".into()]);
            match (interpreter, generator) {
                (Ok(interpreter), Ok(generator)) => {
                    run_migration_e2e(root, interpreter, generator)
                }
                (Err(mut left), Err(right)) => {
                    left.extend(right);
                    Err(left)
                }
                (Err(errors), _) | (_, Err(errors)) => Err(errors),
            }
        }
        _ => Err(vec![
            "usage: cargo run -p xtask -- conformance [DESHELL_BINARY] | migration-e2e INTERPRETER GENERATOR | performance [DESHELL_BINARY] | validate-contracts".into(),
        ]),
    }
}

fn main() {
    let root = repository_root();
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = dispatch(&root, &arguments);
    if let Err(errors) = result {
        for error in errors {
            eprintln!("xtask: {error}");
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_fake_deshell(path: &Path) {
        let source = path.with_extension("rs");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &source,
            r###"
use std::path::{Path, PathBuf};

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|value| value == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn project_root(args: &[String]) -> PathBuf {
    let root = option(args, "--root").unwrap_or_else(|| ".".into());
    let root = PathBuf::from(root);
    if root.is_absolute() {
        root
    } else {
        std::env::current_dir().unwrap().join(root)
    }
}

fn corpus_path() -> PathBuf {
    std::fs::read_dir(".")
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("corpus."))
        })
        .unwrap()
}

fn initialize(args: &[String]) {
    let root = project_root(args);
    std::fs::create_dir_all(root.join(".deshell/scenarios")).unwrap();
    std::fs::write(
        root.join(".deshell/project.toml"),
        concat!(
            "entrypoints = []\n",
            "generator = \"rust\"\n",
            "target = \"rust\"\n",
            "module_root = \"src/bin\"\n",
            "location_overrides = []\n",
            "platform_cells = []\n",
            "allow_local = false\n",
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(".deshell/scenarios/default.toml"),
        concat!(
            "name = \"default\"\n",
            "approval = \"draft\"\n",
            "arguments = []\n",
            "argv = []\n",
            "environment = []\n",
            "memory_bytes = 1073741824\n",
        ),
    )
    .unwrap();
}

fn write_evidence(args: &[String]) {
    let output = option(args, "--output").unwrap();
    let source = corpus_path();
    let source = source.file_name().unwrap().to_string_lossy();
    let evidence = format!(
        "{{\"checks\":[{{\"comparisons\":[{{\"original\":{{\"exit_code\":0}}}}],\"scenario\":\"default\",\"source\":{{\"path\":\"{source}\"}}}},{{\"comparisons\":[{{\"original\":{{\"exit_code\":1}}}}],\"scenario\":\"failure\",\"source\":{{\"path\":\"{source}\"}}}}]}}"
    );
    std::fs::write(output, evidence).unwrap();
}

fn apply_migration() {
    let source = corpus_path();
    let source_name = source.file_name().unwrap().to_string_lossy();
    let workflow_path = Path::new(".github/workflows/embedded.yml");
    let workflow = std::fs::read_to_string(workflow_path).unwrap();
    let mut lines = workflow.lines();
    while lines.next().is_some_and(|line| line.trim() != "run: |-") {}
    let command = lines
        .take_while(|line| line.starts_with("          "))
        .map(|line| line.strip_prefix("          ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!command.is_empty());
    std::fs::create_dir_all(".deshell/archive").unwrap();
    std::fs::write(".deshell/archive/source.bin", std::fs::read(&source).unwrap()).unwrap();
    std::fs::write(".deshell/archive/snippet.bin", command.as_bytes()).unwrap();
    let manifest = format!(
        "{{\"entries\":[{{\"archive_path\":\".deshell/archive/source.bin\",\"original\":{{\"path\":\"{source_name}\"}}}},{{\"archive_path\":\".deshell/archive/snippet.bin\",\"original\":{{\"path\":\".github/workflows/embedded.yml\"}}}}]}}"
    );
    std::fs::write(".deshell/archive/manifest.json", manifest).unwrap();
    std::fs::remove_file(source).unwrap();
    std::fs::write(
        workflow_path,
        "jobs:\n  embedded:\n    steps:\n      - uses: ./.github/actions/deshell-fake\n",
    )
    .unwrap();
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("__generator") => println!(
            "{{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{{\"protocol\":\"deshell.generator.v1\",\"schema_version\":1}}}}"
        ),
        Some(mode) if mode.starts_with("__") => println!(
            "{{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{{\"protocol_version\":1}}}}"
        ),
        Some("--version") => println!("deshell 0.1.0"),
        Some("schema") if args.get(1).map(String::as_str) == Some("unknown") => {
            eprintln!("unknown schema");
            std::process::exit(2);
        }
        Some("schema") => println!("{{}}"),
        Some("check") => {
            eprintln!("missing project");
            std::process::exit(1);
        }
        Some("doctor") => {
            println!("{{}}");
            std::process::exit(6);
        }
        Some("init") => initialize(&args),
        Some("analyze") if option(&args, "--entry").as_deref() == Some("unknown.ext") => {
            eprintln!("unknown interpreter");
            std::process::exit(4);
        }
        Some("analyze" | "scan" | "run" | "verify") => {}
        Some("migrate") if args.get(1).map(String::as_str) == Some("plan") => {
            println!("plan {}", "a".repeat(64));
        }
        Some("migrate") if args.get(1).map(String::as_str) == Some("verify") => {
            write_evidence(&args);
        }
        Some("migrate") if args.get(1).map(String::as_str) == Some("apply") => {
            apply_migration();
        }
        Some("migrate") => {}
        _ => {
            eprintln!("unsupported fake invocation: {args:?}");
            std::process::exit(64);
        }
    }
}
"###,
        )
        .unwrap();
        let output = std::process::Command::new("rustc")
            .args(["--edition=2024", "-o"])
            .arg(path)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn compile_test_program(path: &Path, source_text: &str) {
        let source = path.with_extension("rs");
        std::fs::write(&source, source_text).unwrap();
        let output = std::process::Command::new("rustc")
            .args(["--edition=2024", "-o"])
            .arg(path)
            .arg(source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write_contract_fixture(root: &Path, cases: serde_json::Value) {
        let repository = repository_root();
        for relative in REQUIRED_CONTRACTS {
            let destination = root.join(relative);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(repository.join(relative), destination).unwrap();
        }
        let mut bytes = serde_json::to_vec_pretty(&cases).unwrap();
        bytes.push(b'\n');
        std::fs::write(root.join("contracts/cli/cases.json"), bytes).unwrap();
    }

    #[test]
    fn checked_in_contract_tree_is_complete_and_consistent() {
        let contract = validate_contract_tree(&repository_root()).unwrap();
        assert_eq!(contract.schema_version, 1);
        assert_eq!(contract.diagnostic_modes, ["human", "jsonl"]);
        assert_eq!(
            (
                contract.exit_codes.success,
                contract.exit_codes.execution_io,
                contract.exit_codes.usage,
                contract.exit_codes.invalid_contract,
                contract.exit_codes.policy,
                contract.exit_codes.difference,
                contract.exit_codes.provider_unavailable,
                contract.exit_codes.internal
            ),
            (0, 1, 2, 3, 4, 5, 6, 70)
        );
        assert!(contract.cases.iter().any(|case| case.argv == ["--version"]));
        assert!(
            contract
                .cases
                .iter()
                .any(|case| case.fixture.as_deref() == Some("reject-unknown"))
        );
    }

    #[test]
    fn migration_oracle_contracts_are_package_gate_inputs() {
        for relative in [
            "contracts/schema/generator-protocol-v1.schema.json",
            "contracts/schema/migration-request-v1.schema.json",
            "contracts/schema/proposal-v1.schema.json",
            "contracts/schema/migration-plan-v1.schema.json",
            "contracts/schema/migration-evidence-v1.schema.json",
            "contracts/schema/archive-manifest-v1.schema.json",
            "contracts/schema/audit-finding-v1.schema.json",
            "contracts/schema/harden-plan-v1.schema.json",
            "contracts/schema/harden-approval-v1.schema.json",
            "contracts/schema/harden-evidence-v1.schema.json",
        ] {
            assert!(REQUIRED_CONTRACTS.contains(&relative), "omitted {relative}");
        }
    }

    #[test]
    fn public_docs_describe_the_retirement_oracle_and_release_gate() {
        let root = repository_root();
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("migration oracle"));
        assert!(readme.contains("deshell migrate plan"));
        assert!(readme.contains("deshell verify --require shell-free"));
        assert!(!readme.contains("behavioral compiler"));
        assert!(!readme.contains("74%"));
        assert!(readme.contains("90% release floor"));
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("description = \"Shell retirement migration oracle"));
        assert!(!manifest.contains("Behavioral compiler"));

        let roadmap = std::fs::read_to_string(root.join("ROADMAP.md")).unwrap();
        assert!(roadmap.contains("seven interpreters"));
        assert!(roadmap.contains("90%"));
        assert!(!roadmap.contains("74%"));
        assert!(roadmap.contains("- [x] Enforce measured line coverage"));
        assert!(roadmap.contains("- [x] Complete project-native Make/package/task"));
        assert!(roadmap.contains("- [x] Add enforced network record/replay observations"));
        assert!(roadmap.contains("must not be published"));
    }

    #[test]
    fn independent_schema_gate_validates_migration_oracle_instances() {
        let script =
            std::fs::read_to_string(repository_root().join("scripts/validate-json-contracts.py"))
                .unwrap();
        for schema in [
            "migration-request-v1.schema.json",
            "proposal-v1.schema.json",
            "migration-plan-v1.schema.json",
            "migration-evidence-v1.schema.json",
            "archive-manifest-v1.schema.json",
            "audit-finding-v1.schema.json",
            "harden-plan-v1.schema.json",
            "harden-approval-v1.schema.json",
            "harden-evidence-v1.schema.json",
        ] {
            assert!(script.contains(schema), "validator omitted {schema}");
        }
    }

    #[test]
    fn validator_rejects_non_json_and_missing_contract_assets() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("contracts/cli")).unwrap();
        std::fs::write(
            directory.path().join("contracts/cli/cases.json"),
            b"not-json",
        )
        .unwrap();
        let errors = validate_contract_tree(directory.path()).unwrap_err();
        assert!(!errors.is_empty());
    }

    #[test]
    fn default_build_test_cli_and_ci_are_rust_first() {
        let root = repository_root();
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        assert!(mise.contains("cargo build --locked --workspace"));
        assert!(mise.contains("cargo run --locked -p deshell --"));
        assert!(mise.contains("cargo test --locked --workspace -- --test-threads=1"));
        assert!(mise.contains("cargo test --locked -p deshell lab::tests"));
        assert!(mise.contains("[tasks.performance]"));
        assert!(mise.contains("cargo run --locked -p xtask -- performance target/release/deshell"));
        assert!(mise.contains("[tasks.\"reference:test\"]"));
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("cargo test --locked --workspace -- --test-threads=1"));
        assert!(ci.contains("cargo run --locked -p xtask -- conformance"));
        assert!(!ci.contains("Create project-local OCaml switch"));
        assert!(!ci.contains("Build opam package"));
        assert!(ci.contains("reference-conformance:"));
        assert!(ci.contains("official-exporters:"));
        assert!(ci.contains("security-and-coverage:"));
        assert!(ci.contains("mise run reference:setup"));
        assert!(ci.contains("mise run reference:build"));
        assert!(ci.contains("mise run reference:test"));
        assert!(
            ci.contains("install_args: rust@1.98.0 go@1.27.0 actionlint@1.7.12 powershell@7.6.5")
        );
        assert!(ci.contains("install_args: opam@2.5.2"));
        assert!(ci.contains("install_args: rust@1.98.0 powershell@7.6.5 dagger@0.21.8"));
        assert!(ci.contains("mise run test:official-exporters"));
        assert!(ci.contains("- reference-conformance"));
        assert!(ci.contains("- official-exporters"));
        assert!(ci.contains("- security-and-coverage"));
        assert!(ci.contains("cargo audit --db target/cargo-audit-db --deny warnings"));
        assert!(ci.contains("cargo deny --locked check"));
        assert!(ci.contains("mise run test:schema-validator"));
        assert!(!ci.contains("--fail-under-lines 74"));
        assert!(ci.contains(
            "cargo llvm-cov --locked --workspace --all-targets --summary-only --fail-under-lines 90 -- --test-threads=1"
        ));
        assert!(!mise.contains("--fail-under-lines 74"));
        assert!(mise.contains(
            "cargo llvm-cov --locked --workspace --all-targets --summary-only --fail-under-lines 90 -- --test-threads=1"
        ));
        assert!(ci.contains("MISE_AUTO_INSTALL: \"false\""));
    }

    #[test]
    fn package_verification_uses_an_isolated_target_directory() {
        let mise = std::fs::read_to_string(repository_root().join("mise.toml")).unwrap();
        assert!(mise.contains("cargo package --locked --target-dir target/package-build"));
    }

    #[test]
    fn reference_setup_disables_opam_sandbox_only_on_isolated_ci() {
        let root = repository_root();
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        let init = mise
            .find("${CI:+--disable-sandboxing}")
            .expect("hosted CI must select opam's no-sandbox initialization mode");
        let switch = mise
            .find("opam switch create . 5.5.0")
            .expect("reference setup must create the private compiler switch");
        assert!(
            init < switch,
            "opam must be initialized before switch creation"
        );

        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("mise run reference:setup"));
        assert!(!ci.contains("bubblewrap"));
    }

    #[test]
    fn rust_ci_installs_lint_components_before_running_lint() {
        let root = repository_root();
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        let install = ci
            .find("rustup component add clippy rustfmt")
            .expect("Rust CI must install the clippy and rustfmt components");
        let lint = ci
            .find("run: mise run lint")
            .expect("Rust CI must run the repository lint gate");
        assert!(
            install < lint,
            "clippy and rustfmt must be installed before lint"
        );
    }

    #[test]
    fn solo_repository_ruleset_keeps_pr_checks_without_independent_approval() {
        let root = repository_root();
        let ruleset: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(".github/rulesets/default-branch.json")).unwrap(),
        )
        .unwrap();
        let rules = ruleset["rules"].as_array().unwrap();
        let pull_request = rules
            .iter()
            .find(|rule| rule["type"] == "pull_request")
            .expect("the default branch must still require pull requests");
        let review = &pull_request["parameters"];
        assert_eq!(review["required_approving_review_count"], 0);
        assert_eq!(review["require_code_owner_review"], false);
        assert_eq!(review["require_last_push_approval"], false);
        assert_eq!(review["required_review_thread_resolution"], true);

        let status_checks = rules
            .iter()
            .find(|rule| rule["type"] == "required_status_checks")
            .expect("the default branch must still require status checks");
        assert_eq!(
            status_checks["parameters"]["required_status_checks"][0]["context"],
            "Required gate"
        );
        assert_eq!(
            status_checks["parameters"]["strict_required_status_checks_policy"],
            true
        );
    }

    #[test]
    fn release_workflow_declares_six_archives_checksums_signing_and_provenance() {
        let root = repository_root();
        let workflow = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        for target in [
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ] {
            assert!(workflow.contains(target), "release matrix omitted {target}");
        }
        assert!(workflow.contains("sha256"));
        assert!(workflow.contains("attest-build-provenance"));
        assert!(workflow.contains("cosign"));
        assert!(workflow.contains("syft@1.51.0"));
        assert!(workflow.contains("deshell-0.1.0.cdx.json"));
        assert!(workflow.contains("CycloneDX"));
        assert!(workflow.contains("cargo publish"));
        assert!(workflow.contains("v0.1.0-rc.1"));
        assert!(workflow.contains(
            "$archivePath = Join-Path (Resolve-Path 'dist').Path '${{ matrix.archive }}'"
        ));
        assert!(workflow.contains("Compress-Archive -Path $stage -DestinationPath $archivePath"));
        assert!(workflow.contains("tar -C dist -czf $archivePath 'deshell-0.1.0'"));
        assert!(
            workflow.contains("Expand-Archive -Path $archivePath -DestinationPath $installRoot")
        );
        assert!(workflow.contains("$installed = Join-Path $installRoot (Join-Path 'deshell-0.1.0' '${{ matrix.executable }}')"));
        assert!(workflow.contains("archive schema smoke test failed"));
        assert!(workflow.contains("archive $mode handshake failed"));
        assert_eq!(workflow.matches("install_args: rust@1.98.0").count(), 4);
        assert!(workflow.contains("MISE_AUTO_INSTALL: \"false\""));
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        assert!(mise.contains("actionlint .github/workflows/ci.yml .github/workflows/release.yml"));
        assert!(mise.contains("pipx:check-jsonschema"));
        assert!(mise.contains("scripts/validate-json-contracts.py"));
    }

    #[test]
    fn release_signing_action_is_present_in_the_selected_action_allowlist() {
        let root = repository_root();
        let release = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let selected: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join(".github/settings/selected-actions.json")).unwrap(),
        )
        .unwrap();
        let installer = "sigstore/cosign-installer@6f9f17788090df1f26f669e9d70d6ae9567deba6";
        assert!(release.contains(installer));
        assert!(
            selected["patterns_allowed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|pattern| pattern == installer),
            "the pinned release signing action must be executable under repository policy"
        );
    }

    #[test]
    fn ci_installs_the_exact_external_runtimes_exercised_by_the_test_suite() {
        let root = repository_root();
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        let release = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        let nightly = std::fs::read_to_string(root.join(".github/workflows/nightly.yml")).unwrap();
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        let mise_lock = std::fs::read_to_string(root.join("mise.lock")).unwrap();
        let installer_path = root.join("scripts/install-nushell.ps1");
        let installer = std::fs::read_to_string(&installer_path).unwrap();

        assert!(ci.matches("go@1.27.0").count() >= 2);
        assert!(release.matches("go@1.27.0").count() >= 4);
        assert!(mise.contains("go = \"1.27.0\""));
        assert!(mise_lock.contains("[[tools.go]]"));
        assert!(mise_lock.contains("version = \"1.27.0\""));
        for platform in [
            "linux-arm64",
            "linux-x64",
            "macos-arm64",
            "macos-x64",
            "windows-arm64",
            "windows-x64",
        ] {
            assert!(
                mise_lock.contains(&format!("[tools.go.\"platforms.{platform}\"]")),
                "mise.lock omitted the Go artifact for {platform}"
            );
            assert!(
                mise_lock.contains(&format!("[tools.powershell.\"platforms.{platform}\"]")),
                "mise.lock omitted the PowerShell artifact for {platform}"
            );
        }
        for (name, workflow, minimum) in [
            ("ci", ci.as_str(), 4),
            ("release", release.as_str(), 4),
            ("nightly", nightly.as_str(), 1),
        ] {
            assert!(
                workflow.matches("scripts/install-nushell.ps1").count() >= minimum,
                "{name} must provision pinned Nushell for every parser/test job"
            );
        }
        for (asset, digest) in [
            (
                "nu-0.115.1-x86_64-unknown-linux-gnu.tar.gz",
                "d11d825241f6504a3617c535fa725a9dd6d009c86d7b19fb3168b47635b9d8b0",
            ),
            (
                "nu-0.115.1-aarch64-unknown-linux-gnu.tar.gz",
                "5c4a5bca0af5b070e903a68fa014cc24e6419d0ac9cec03a2948494b2d310e08",
            ),
            (
                "nu-0.115.1-x86_64-apple-darwin.tar.gz",
                "0292f4b92af29cfe5d9c4b2ec06eeb325b705d1d6c19536a8bec2b75859b3485",
            ),
            (
                "nu-0.115.1-aarch64-apple-darwin.tar.gz",
                "2e6ed1eb043869ff05b5f2448a8c443e4d3a93557ba4303b21008a0523c96734",
            ),
            (
                "nu-0.115.1-x86_64-pc-windows-msvc.zip",
                "b83009cbc88021f4dc293c49320118886b78363f9a4bb14933d33c8803241f46",
            ),
            (
                "nu-0.115.1-aarch64-pc-windows-msvc.zip",
                "8f185bc965828208fc9824de32a2e65aa39fa59ebf0a3927dbd0bad1daeb24a1",
            ),
        ] {
            assert!(installer.contains(asset), "installer omitted {asset}");
            assert!(
                installer.contains(digest),
                "installer omitted digest for {asset}"
            );
        }
        assert!(installer.contains("Get-FileHash"));
        assert!(installer.contains("github.com/nushell/nushell/releases/download/0.115.1"));
        assert!(installer.contains("GITHUB_PATH"));
    }

    #[test]
    fn executable_corpus_fixtures_use_portable_system_paths() {
        let root = repository_root();
        for relative in [
            "crates/deshell/src/cli.rs",
            "crates/deshell/src/frontend.rs",
        ] {
            let source = std::fs::read_to_string(root.join(relative)).unwrap();
            assert!(
                !source.contains("/usr/bin/test"),
                "{relative} contains a fixture path absent on macOS"
            );
        }
    }

    #[test]
    fn manually_dispatched_release_qualification_cannot_publish() {
        let workflow =
            std::fs::read_to_string(repository_root().join(".github/workflows/release.yml"))
                .unwrap();
        let publish = &workflow[workflow.find("  publish:").unwrap()..];
        assert!(publish.contains(
            "if: ${{ github.ref == 'refs/tags/v0.1.0-rc.1' || github.ref == 'refs/tags/v0.1.0' }}"
        ));
    }

    #[test]
    fn final_release_requires_ninety_percent_coverage_and_the_seven_interpreter_corpus() {
        let workflow =
            std::fs::read_to_string(repository_root().join(".github/workflows/release.yml"))
                .unwrap();
        assert!(workflow.contains("release-qualification:"));
        assert!(workflow.contains("migration-e2e:"));
        assert!(workflow.contains("--fail-under-lines 90"));
        assert!(!workflow.contains("--fail-under-lines 74"));
        assert!(workflow.contains("-- --test-threads=1"));
        assert!(workflow.contains("scripts/check-release-coverage.py"));
        assert!(workflow.contains("cargo run --locked -p xtask -- migration-e2e"));
        for interpreter in ["sh", "bash", "zsh", "fish", "powershell", "cmd", "nu"] {
            assert!(
                workflow.contains(&format!("interpreter: {interpreter}")),
                "release migration corpus omitted {interpreter}"
            );
        }
        for generator in ["rust", "go"] {
            assert!(
                workflow.contains(&format!("generator: {generator}")),
                "release migration corpus omitted {generator}"
            );
        }
        let publish = workflow.find("  publish:").unwrap();
        let publish_workflow = &workflow[publish..];
        assert!(publish_workflow.contains("- release-qualification"));
        assert!(publish_workflow.contains("- migration-e2e"));
        let checker =
            std::fs::read_to_string(repository_root().join("scripts/check-release-coverage.py"))
                .unwrap();
        assert!(checker.contains("MINIMUM = 90.0"));
        for module in [
            "scanner.rs",
            "frontend.rs",
            "runner.rs",
            "protocol.rs",
            "lab.rs",
            "patch.rs",
        ] {
            assert!(
                checker.contains(module),
                "coverage checker omitted {module}"
            );
        }
    }

    #[test]
    fn migration_e2e_fixture_contract_has_one_source_for_each_interpreter() {
        let expected = [
            ("sh", ".sh"),
            ("bash", ".sh"),
            ("zsh", ".zsh"),
            ("fish", ".fish"),
            ("powershell", ".ps1"),
            ("cmd", ".cmd"),
            ("nu", ".nu"),
        ];
        for (interpreter, suffix) in expected {
            let fixture = migration_fixture(interpreter).unwrap();
            assert!(fixture.path.ends_with(suffix));
            assert!(!fixture.source.is_empty());
        }
        assert!(migration_fixture("unknown").is_err());
        for interpreter in ["sh", "bash", "zsh"] {
            let fixture = migration_fixture(interpreter).unwrap();
            let source = std::str::from_utf8(fixture.source).unwrap();
            assert!(
                source.contains("$1"),
                "{interpreter} omitted argument input"
            );
            assert!(
                source.contains("$CORPUS_ENV"),
                "{interpreter} omitted environment input"
            );
            assert!(source.contains("&&"), "{interpreter} omitted a branch");
        }
        let fish = migration_fixture("fish").unwrap();
        let fish_source = std::str::from_utf8(fish.source).unwrap();
        assert!(fish_source.contains("$argv[1]"));
        assert!(fish_source.contains("$CORPUS_ENV"));
        assert!(fish_source.contains("&&"));
        let nushell = migration_fixture("nu").unwrap();
        let nushell_source = std::str::from_utf8(nushell.source).unwrap();
        assert!(nushell_source.contains("def main [value: string]"));
        assert!(nushell_source.contains("$env.CORPUS_ENV"));
        assert!(nushell_source.contains("$env.LAST_EXIT_CODE"));
        let powershell = migration_fixture("powershell").unwrap();
        let powershell_source = std::str::from_utf8(powershell.source).unwrap();
        assert!(powershell_source.contains("$args[0]"));
        assert!(powershell_source.contains("$env:CORPUS_ENV"));
        assert!(powershell_source.contains("&&"));
        assert!(powershell_source.contains("corpus-helper"));
        let cmd = migration_fixture("cmd").unwrap();
        let cmd_source = std::str::from_utf8(cmd.source).unwrap();
        assert!(cmd_source.contains("%~1"));
        assert!(cmd_source.contains("%CORPUS_ENV%"));
        assert!(cmd_source.contains("&&"));
        assert!(cmd_source.contains("deshell-corpus-helper.exe"));
        let embedded_cmd = migration_embedded_fixture("cmd").unwrap();
        assert_eq!(
            embedded_cmd.command,
            "@echo off\ntarget\\deshell-corpus-helper.exe branch"
        );
        assert!(
            embedded_cmd.source.contains(
                "          @echo off\n          target\\deshell-corpus-helper.exe branch\n"
            )
        );
        for interpreter in ["sh", "bash", "zsh", "fish", "powershell", "cmd", "nu"] {
            let embedded = migration_embedded_fixture(interpreter).unwrap();
            assert!(embedded.path.starts_with(".github/workflows/"));
            assert!(embedded.source.contains("run: |-"));
            assert!(embedded.source.contains("shell:"));
            assert!(!embedded.command.is_empty());
        }
    }

    #[test]
    fn migration_e2e_platform_assignment_allows_powershell_on_both_host_families() {
        for interpreter in ["sh", "bash", "zsh", "fish", "nu"] {
            assert!(migration_interpreter_supported_on(interpreter, "linux"));
            assert!(migration_interpreter_supported_on(interpreter, "macos"));
            assert!(!migration_interpreter_supported_on(interpreter, "windows"));
        }
        assert!(migration_interpreter_supported_on("powershell", "linux"));
        assert!(migration_interpreter_supported_on("powershell", "windows"));
        assert!(!migration_interpreter_supported_on("cmd", "linux"));
        assert!(migration_interpreter_supported_on("cmd", "windows"));
    }

    #[test]
    fn release_dynamic_analysis_uses_real_saved_corpus_targets_and_blocks_publish() {
        let root = repository_root();
        for target in ["frontend", "scanner", "protocol", "schema"] {
            assert!(
                root.join(format!("fuzz/fuzz_targets/{target}.rs"))
                    .is_file(),
                "missing fuzz target {target}"
            );
            assert!(
                root.join(format!("fuzz/corpus/{target}")).is_dir(),
                "missing saved corpus for {target}"
            );
        }
        let release = std::fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
        assert!(release.contains("dynamic-analysis:"));
        let nightly_fuzz = "cargo +nightly-2026-07-15 fuzz run";
        assert!(release.contains(nightly_fuzz));
        assert!(
            release.contains("rustup component add --toolchain nightly-2026-07-15 miri rust-src")
        );
        assert!(release.contains("cargo +nightly-2026-07-15 miri test"));
        assert!(release.contains("-Zsanitizer=address"));
        assert!(release.contains("--cfg deshell_sanitizer_address"));
        assert!(release.contains("-fsanitize=undefined"));
        assert!(release.contains("-fno-sanitize=function"));
        assert!(release.contains("--cfg deshell_sanitizer_undefined"));
        assert_eq!(
            release
                .matches(
                    "cargo +nightly-2026-07-15 test --locked -p deshell --target x86_64-unknown-linux-gnu -- --test-threads=1"
                )
                .count(),
            2,
            "ASan and UBSan must serialize resource-limit tests"
        );
        assert!(release.contains("nightly-2026-07-15"));
        let publish = &release[release.find("  publish:").unwrap()..];
        assert!(publish.contains("- dynamic-analysis"));
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("fuzz-smoke:"));
        assert!(ci.contains(nightly_fuzz));
        assert!(ci.contains("nightly-2026-07-15"));
        assert!(ci.contains("- fuzz-smoke"));
        let nightly = std::fs::read_to_string(root.join(".github/workflows/nightly.yml")).unwrap();
        assert!(nightly.contains("schedule:"));
        assert!(nightly.contains(nightly_fuzz));
        assert!(nightly.contains("nightly-2026-07-15"));
        let install = "cargo install --locked --version 0.13.2 cargo-fuzz";
        for (name, workflow) in [
            ("release", release.as_str()),
            ("ci", ci.as_str()),
            ("nightly", nightly.as_str()),
        ] {
            assert!(
                workflow.contains(install),
                "{name} must install the exact cargo-fuzz crate through Cargo's checksummed registry"
            );
            assert!(
                !workflow.contains("tool: cargo-fuzz"),
                "{name} must not ask install-action for an unsupported tool"
            );
        }
        for target in ["frontend", "scanner", "protocol", "schema"] {
            assert!(
                nightly.contains(&format!("fuzz/corpus/{target}")),
                "nightly workflow omitted saved corpus {target}"
            );
        }
    }

    #[test]
    fn cargo_package_surface_is_allowlisted_from_the_repository_root() {
        let root = repository_root();
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(manifest.contains("\"/crates/deshell/src/**\""));
        assert!(manifest.contains("\"/contracts/**\""));
        assert!(manifest.contains("\"/adapters/powershell/adapter.ps1\""));
        assert!(manifest.contains("\"/README.md\""));
        assert!(manifest.contains("\"/LICENSE\""));
        assert!(!manifest.lines().any(|line| line.trim() == "\"README.md\","));
        assert!(!manifest.lines().any(|line| line.trim() == "\"LICENSE\","));
    }

    #[test]
    fn public_docs_and_dependency_updates_are_rust_first_v1() {
        let root = repository_root();
        let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
        assert!(readme.contains("`de-shell` is a Rust"));
        assert!(readme.contains("Effect IR v1"));
        assert!(readme.contains("Evidence v1"));
        assert!(readme.contains("Rust 1.98"));
        assert!(readme.contains("disposable-lab launch contracts"));
        assert!(readme.contains("unpublished OCaml reference implementation"));
        assert!(readme.contains("mise run performance"));
        assert!(readme.contains("five warm-up runs and twenty measured runs"));
        for stale in [
            "Effect IR v3",
            "v0/v1/v2-to-v3",
            "Build the OCaml project",
            "Format OCaml and Dune",
        ] {
            assert!(
                !readme.contains(stale),
                "README retained stale claim: {stale}"
            );
        }

        let roadmap = std::fs::read_to_string(root.join("ROADMAP.md")).unwrap();
        assert!(roadmap.contains("0.1.0"));
        assert!(roadmap.contains("six release archives"));
        assert!(roadmap.contains("48-repository"));
        assert!(roadmap.contains("disposable-lab launch contracts"));

        let audit = std::fs::read_to_string(root.join("docs/corpus-audit.md")).unwrap();
        assert!(audit.contains("pre-cutover baseline"));
        assert!(audit.contains("must be rerun with the Rust implementation"));

        let contributing = std::fs::read_to_string(root.join("CONTRIBUTING.md")).unwrap();
        assert!(contributing.contains("Red"));
        assert!(contributing.contains("Green"));
        assert!(contributing.contains("cargo test --locked --workspace"));

        let dependabot = std::fs::read_to_string(root.join(".github/dependabot.yml")).unwrap();
        assert!(dependabot.contains("package-ecosystem: cargo\n    directory: /\n"));
        assert!(!dependabot.contains("directory: /adapters/nushell"));
    }

    #[test]
    fn ocaml_reference_declares_no_public_or_install_targets() {
        let root = repository_root();
        for relative in [
            "bin/dune",
            "lib/dune",
            "schema/dune",
            "scripts/dune",
            "adapters/powershell/dune",
            "adapters/nushell/dune",
        ] {
            let dune = std::fs::read_to_string(root.join(relative)).unwrap();
            assert!(
                !dune.contains("(public_name"),
                "{relative} still exports a public OCaml name"
            );
            assert!(
                !dune.contains("(public_names"),
                "{relative} still exports public OCaml names"
            );
            assert!(
                !dune.contains("(install"),
                "{relative} still installs a reference artifact"
            );
        }
        let package = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(package.contains("default-run = \"deshell\""));
        assert!(!package.contains("crate-type"));
    }

    #[test]
    fn ocaml_reference_gate_targets_the_shared_v1_golden_contract() {
        let root = repository_root();
        let library = std::fs::read_to_string(root.join("lib/dune")).unwrap();
        assert!(library.contains("(name deshell_reference_v1)"));
        assert!(library.contains("(modules sha256 reference_v1)"));
        let executable = std::fs::read_to_string(root.join("bin/dune")).unwrap();
        assert!(executable.contains("reference_conformance"));
        let reference = std::fs::read_to_string(root.join("lib/reference_v1.ml")).unwrap();
        assert!(reference.contains("Effect IR v1"));
        assert!(reference.contains("frontend-v1.json"));
        assert!(reference.contains("transform-export-v1.json"));
        let dune = std::fs::read_to_string(root.join("dune")).unwrap();
        assert!(dune.contains("contracts/golden/transform-export-v1.json"));
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        assert!(mise.contains("dune build @reference-v1"));
        assert!(mise.contains("dune exec bin/reference_conformance.exe"));
        assert!(mise.contains("[tasks.\"reference:fmt:check\"]"));
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("mise run reference:fmt:check"));
    }

    #[test]
    fn pre_v1_ocaml_runtime_and_compatibility_sources_are_removed() {
        let root = repository_root();
        let names = |relative: &str| {
            let mut values = std::fs::read_dir(root.join(relative))
                .unwrap()
                .filter_map(|entry| {
                    let entry = entry.unwrap();
                    entry
                        .file_type()
                        .unwrap()
                        .is_file()
                        .then(|| entry.file_name().to_string_lossy().into_owned())
                })
                .collect::<Vec<_>>();
            values.sort();
            values
        };
        assert_eq!(names("lib"), ["dune", "reference_v1.ml", "sha256.ml"]);
        assert_eq!(names("bin"), ["dune", "reference_conformance.ml"]);
        assert_eq!(names("test"), ["dune"]);
        assert_eq!(names("schema"), ["dune"]);
        assert_eq!(names("adapters/nushell"), ["dune"]);
        let reference = std::fs::read_to_string(root.join("lib/reference_v1.ml")).unwrap();
        for stale in [
            "current_schema_version = 3",
            "v0_migration",
            "v1_migration",
            "v2_migration",
        ] {
            assert!(!reference.contains(stale));
        }
        for relative in ["de-shell.opam", "de-shell.opam.locked"] {
            let opam = std::fs::read_to_string(root.join(relative)).unwrap();
            assert!(opam.contains("Unpublished OCaml Effect IR v1 reference"));
            for stale_dependency in ["alcotest", "cmdliner", "conf-rust", "qcheck"] {
                assert!(
                    !opam.contains(stale_dependency),
                    "{relative} retained {stale_dependency}"
                );
            }
        }
    }

    #[test]
    fn conformance_executes_every_cli_and_hidden_agent_contract() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join(if cfg!(windows) {
            "fake-deshell.exe"
        } else {
            "fake-deshell"
        });
        compile_fake_deshell(&binary);
        run_conformance(&repository_root(), &binary).unwrap();
        assert!(prepare_fixture(&binary, directory.path(), "unknown").is_err());
        assert!(run_conformance(&repository_root(), &directory.path().join("missing")).is_err());
        assert!(smoke_agent(&directory.path().join("missing"), "__process-agent").is_err());
    }

    #[test]
    fn performance_orchestration_prepares_and_measures_real_processes() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join(if cfg!(windows) {
            "fake-deshell.exe"
        } else {
            "fake-deshell"
        });
        compile_fake_deshell(&binary);
        run_performance(&binary).unwrap();
        assert!(resolve_binary(&directory.path().join("missing")).is_err());

        let blocked = directory.path().join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        assert!(prepare_scan_corpus(&blocked).is_err());
        assert!(prepare_simple_run_project(&binary, &blocked).is_err());

        let failure = if cfg!(windows) {
            (
                PathBuf::from("cmd.exe"),
                vec!["/d".into(), "/c".into(), "exit 9".into()],
            )
        } else {
            (PathBuf::from("/bin/false"), Vec::new())
        };
        assert!(timed_command(&failure.0, &failure.1, "expected failure").is_err());
        assert!(command_success(&failure.0, &failure.1, "expected failure").is_err());
    }

    #[test]
    fn migration_release_orchestration_retires_file_and_embedded_shell() {
        let repository = tempfile::tempdir().unwrap();
        let mut binary = repository.path().join("target/debug/deshell");
        if cfg!(windows) {
            binary.set_extension("exe");
        }
        compile_fake_deshell(&binary);
        let interpreter = if cfg!(windows) { "cmd" } else { "sh" };
        run_migration_e2e(repository.path(), interpreter, "rust").unwrap();
        run_migration_e2e(repository.path(), "powershell", "go").unwrap();
        assert!(run_migration_e2e(repository.path(), interpreter, "unknown").is_err());
        let wrong_platform = if cfg!(windows) { "sh" } else { "cmd" };
        assert!(run_migration_e2e(repository.path(), wrong_platform, "rust").is_err());

        let missing = tempfile::tempdir().unwrap();
        assert!(run_migration_e2e(missing.path(), interpreter, "rust").is_err());
        assert!(migration_embedded_fixture("unknown").is_err());
        assert!(!powershell_runtime_path().unwrap().is_empty());
    }

    #[test]
    fn fake_oracle_archives_the_complete_multiline_embedded_snippet() {
        let repository = tempfile::tempdir().unwrap();
        let mut binary = repository.path().join("target/debug/deshell");
        if cfg!(windows) {
            binary.set_extension("exe");
        }
        compile_fake_deshell(&binary);
        std::fs::write(repository.path().join("corpus.sh"), b"#!/bin/sh\ntrue\n").unwrap();
        let workflow = repository.path().join(".github/workflows/embedded.yml");
        std::fs::create_dir_all(workflow.parent().unwrap()).unwrap();
        std::fs::write(
            &workflow,
            concat!(
                "jobs:\n",
                "  embedded:\n",
                "    steps:\n",
                "      - name: embedded\n",
                "        run: |-\n",
                "          @one.exe\n",
                "          @two.exe\n",
            ),
        )
        .unwrap();

        let output = std::process::Command::new(binary)
            .args(["migrate", "apply"])
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(repository.path().join(".deshell/archive/snippet.bin")).unwrap(),
            b"@one.exe\n@two.exe"
        );
    }

    #[test]
    fn command_dispatch_reports_usage_and_routes_each_subcommand() {
        use std::ffi::OsString;

        let root = repository_root();
        assert!(dispatch(&root, &[OsString::from("validate-contracts")]).is_ok());
        assert!(dispatch(&root, &[]).unwrap_err()[0].contains("usage:"));
        assert_eq!(
            dispatch(&root, &[OsString::from("migration-e2e")])
                .unwrap_err()
                .len(),
            2
        );
        assert!(
            dispatch(
                &root,
                &[OsString::from("migration-e2e"), OsString::from("sh")]
            )
            .unwrap_err()[0]
                .contains("GENERATOR")
        );
        assert!(
            dispatch(
                &root,
                &[
                    OsString::from("migration-e2e"),
                    OsString::from(if cfg!(windows) { "cmd" } else { "sh" }),
                    OsString::from("invalid")
                ]
            )
            .is_err()
        );
        assert!(
            dispatch(
                &root,
                &[
                    OsString::from("performance"),
                    OsString::from("definitely-missing")
                ]
            )
            .is_err()
        );
        assert!(
            dispatch(
                &root,
                &[
                    OsString::from("conformance"),
                    OsString::from("definitely-missing")
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn contract_validation_reports_all_semantic_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        write_contract_fixture(
            directory.path(),
            serde_json::json!({
                "cases": [
                    {
                        "argv": [],
                        "exit": 9,
                        "fixture": "future-fixture",
                        "stdout": "conflict",
                        "stdout_artifact": true
                    },
                    {"argv": ["same"], "exit": 0},
                    {"argv": ["same"], "exit": 0}
                ],
                "diagnostic_modes": ["human"],
                "exit_codes": {
                    "success": 1,
                    "execution_io": 1,
                    "usage": 2,
                    "invalid_contract": 3,
                    "policy": 4,
                    "difference": 5,
                    "provider_unavailable": 6,
                    "internal": 70
                },
                "schema_version": 2
            }),
        );
        let errors = validate_contract_tree(directory.path())
            .unwrap_err()
            .join("; ");
        for expected in [
            "schema_version",
            "diagnostic modes",
            "exit code table",
            "argv must not be empty",
            "duplicate CLI case",
            "undeclared exit code",
            "conflicting output",
            "unknown CLI fixture",
        ] {
            assert!(errors.contains(expected), "{expected}: {errors}");
        }
    }

    #[test]
    fn conformance_aggregates_bad_cli_output_and_agent_failures() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join(if cfg!(windows) {
            "fake-deshell.exe"
        } else {
            "fake-deshell"
        });
        compile_fake_deshell(&binary);
        let contracts = directory.path().join("contracts-root");
        write_contract_fixture(
            &contracts,
            serde_json::json!({
                "cases": [
                    {"argv": ["--version"], "exit": 1, "stdout": "wrong\n"},
                    {"argv": ["unsupported"], "exit": 0, "stdout_artifact": true},
                    {"argv": ["schema", "inventory"], "exit": 0, "stderr_only": true},
                    {
                        "argv": ["analyze", "--entry", "unknown.ext"],
                        "exit": 4,
                        "fixture": "reject-unknown",
                        "stderr_only": true
                    }
                ],
                "diagnostic_modes": ["human", "jsonl"],
                "exit_codes": {
                    "success": 0,
                    "execution_io": 1,
                    "usage": 2,
                    "invalid_contract": 3,
                    "policy": 4,
                    "difference": 5,
                    "provider_unavailable": 6,
                    "internal": 70
                },
                "schema_version": 1
            }),
        );
        let errors = run_conformance(&contracts, &binary).unwrap_err().join("; ");
        for expected in [
            "expected exit",
            "stdout bytes differ",
            "stdout is not a JSON artifact",
            "expected empty stdout",
        ] {
            assert!(errors.contains(expected), "{expected}: {errors}");
        }

        let read_stdin = "use std::io::Read as _; let mut value=String::new(); std::io::stdin().read_to_string(&mut value).unwrap();";
        let failed = directory.path().join(if cfg!(windows) {
            "failed.exe"
        } else {
            "failed"
        });
        compile_test_program(
            &failed,
            &format!("fn main() {{ {read_stdin} eprintln!(\"failed\"); std::process::exit(9); }}"),
        );
        assert!(
            smoke_agent(&failed, "__process-agent")
                .unwrap_err()
                .contains("process failed")
        );
        assert!(
            prepare_fixture(&failed, directory.path(), "reject-unknown")
                .unwrap_err()
                .contains("init failed")
        );
        assert!(
            run_deshell(&failed, directory.path(), &[]).unwrap_err()[0].contains("failed with")
        );
        assert!(
            run_deshell(
                &directory.path().join("missing-program"),
                directory.path(),
                &[]
            )
            .unwrap_err()[0]
                .contains("cannot execute")
        );

        let invalid_json = directory.path().join(if cfg!(windows) {
            "invalid-json.exe"
        } else {
            "invalid-json"
        });
        compile_test_program(
            &invalid_json,
            &format!("fn main() {{ {read_stdin} println!(\"not-json\"); }}"),
        );
        assert!(
            smoke_agent(&invalid_json, "__observer-agent")
                .unwrap_err()
                .contains("invalid handshake JSON")
        );

        let invalid_result = directory.path().join(if cfg!(windows) {
            "invalid-result.exe"
        } else {
            "invalid-result"
        });
        compile_test_program(
            &invalid_result,
            &format!("fn main() {{ {read_stdin} println!(\"{{{{}}}}\"); }}"),
        );
        assert!(
            smoke_agent(&invalid_result, "__generator")
                .unwrap_err()
                .contains("invalid handshake response")
        );

        let invalid_utf8 = directory.path().join(if cfg!(windows) {
            "invalid-utf8.exe"
        } else {
            "invalid-utf8"
        });
        compile_test_program(
            &invalid_utf8,
            "use std::io::Write as _; fn main() { std::io::stdout().write_all(&[255]).unwrap(); }",
        );
        assert!(
            run_deshell(&invalid_utf8, directory.path(), &[]).unwrap_err()[0].contains("not UTF-8")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let blocked = directory.path().join("not-executable");
            std::fs::write(&blocked, b"not executable").unwrap();
            std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(
                run_conformance(&contracts, &blocked)
                    .unwrap_err()
                    .iter()
                    .any(|error| error.contains("could not execute"))
            );
        }
    }
}
