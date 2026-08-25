use serde::Deserialize;
use std::path::{Path, PathBuf};

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
    let required = [
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
        "contracts/schema/evidence-v1.schema.json",
        "contracts/schema/diagnostic-v1.schema.json",
        "contracts/schema/protocol-v1.schema.json",
        "contracts/schema/project-v1.schema.json",
        "contracts/schema/scenario-v1.schema.json",
        "contracts/schema/lock-v1.schema.json",
        "contracts/schema/replay-v1.schema.json",
        "contracts/schema/corpus-audit-v1.schema.json",
    ];
    let mut errors = Vec::new();
    for relative in required {
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
    for mode in ["__process-agent", "__observer-agent", "__nushell-adapter"] {
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
            let config = concat!(
                "version = 1\nentrypoints = [\"unknown.ext\"]\n\n",
                "[policy]\nhost_write = \"deny\"\nnetwork = \"deny\"\nunknown_interpreter = \"reject\"\n\n",
                "[sandbox]\nmode = \"disposable\"\n\n",
                "[export]\nstrict = true\nbridge = false\n",
            );
            std::fs::write(root.join(".deshell/project.toml"), config)
                .map_err(|error| error.to_string())?;
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
    if response["id"] != 1 || response["result"]["protocol_version"] != 1 {
        return Err(format!("{mode}: invalid handshake response"));
    }
    Ok(())
}

fn main() {
    let root = repository_root();
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match arguments.first().and_then(|value| value.to_str()) {
        Some("conformance") => {
            let binary = arguments
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/debug/deshell"));
            run_conformance(&root, &binary)
        }
        Some("validate-contracts") => validate_contract_tree(&root).map(|_| ()),
        _ => Err(vec![
            "usage: cargo run -p xtask -- conformance [DESHELL_BINARY] | validate-contracts".into(),
        ]),
    };
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
        assert!(mise.contains("cargo test --locked --workspace"));
        assert!(mise.contains("cargo test --locked -p deshell lab::tests"));
        assert!(mise.contains("[tasks.\"reference:test\"]"));
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("cargo test --locked --workspace"));
        assert!(ci.contains("cargo run --locked -p xtask -- conformance"));
        assert!(!ci.contains("Create project-local OCaml switch"));
        assert!(!ci.contains("Build opam package"));
        assert!(ci.contains("reference-conformance:"));
        assert!(ci.contains("official-exporters:"));
        assert!(ci.contains("mise run reference:setup"));
        assert!(ci.contains("mise run reference:build"));
        assert!(ci.contains("mise run reference:test"));
        assert!(ci.contains("install_args: rust@1.98.0 actionlint@1.7.12 powershell@7.6.5"));
        assert!(ci.contains("install_args: opam@2.5.2"));
        assert!(ci.contains("install_args: rust@1.98.0 powershell@7.6.5 dagger@0.21.8"));
        assert!(ci.contains("mise run test:official-exporters"));
        assert!(ci.contains("- reference-conformance"));
        assert!(ci.contains("- official-exporters"));
        assert!(ci.contains("MISE_AUTO_INSTALL: \"false\""));
    }

    #[test]
    fn reference_ci_installs_the_opam_sandbox_dependency_before_setup() {
        let root = repository_root();
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        let install = ci
            .find("sudo apt-get install --yes bubblewrap")
            .expect("OCaml reference CI must install opam's bwrap sandbox dependency");
        let setup = ci
            .find("mise run reference:setup")
            .expect("OCaml reference CI must initialize the private switch");
        assert!(
            install < setup,
            "bubblewrap must be installed before opam init"
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
        assert_eq!(workflow.matches("install_args: rust@1.98.0").count(), 2);
        assert!(workflow.contains("MISE_AUTO_INSTALL: \"false\""));
        let mise = std::fs::read_to_string(root.join("mise.toml")).unwrap();
        assert!(mise.contains("actionlint .github/workflows/ci.yml .github/workflows/release.yml"));
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
}
