use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "deshell",
    version = env!("CARGO_PKG_VERSION"),
    about = "Compile shell automation behavior into typed, evidence-carrying Effect IR."
)]
struct Cli {
    #[arg(long, global = true, value_enum, default_value = "human")]
    diagnostics: crate::diagnostics::Mode,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize canonical de-shell project files.
    Init {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long = "entry")]
        entries: Vec<String>,
    },
    /// Inventory shell files, embedded shell, and candidates.
    Scan {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Lower an entrypoint into canonical Effect IR.
    Analyze {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Vec<String>,
    },
    /// Preview or apply meaning-preserving shell rewrites.
    Rewrite {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        equivalent: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Propose explicitly behavior-changing improvements.
    Modernize {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        apply: bool,
    },
    /// Analyze and export an entrypoint.
    Migrate {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        observe: bool,
        #[arg(long, value_enum)]
        target: ExportTarget,
        #[arg(long)]
        apply: bool,
    },
    /// Audit static guarantees and recorded observations.
    Verify {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long = "entry")]
        entry: Vec<String>,
        #[arg(long, value_enum)]
        require: Option<GuaranteeRequirement>,
    },
    /// Observe original and lowered behavior in a disposable provider.
    Observe {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        scenario: Vec<String>,
    },
    /// Diagnose locks, runtimes, and disposable-provider readiness.
    Doctor {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Run the canonical Effect IR plan.
    Run {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long, value_enum, default_value = "disposable")]
        backend: BackendKind,
        #[arg(long)]
        node: Option<String>,
        #[arg(long = "arg", allow_hyphen_values = true)]
        arguments: Vec<String>,
        #[arg(last = true, allow_hyphen_values = true)]
        trailing: Vec<String>,
    },
    /// Export the canonical plan without dropping effects.
    Export {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long, value_enum)]
        target: ExportTarget,
        #[arg(long, value_enum, default_value = "strict")]
        mode: ExportMode,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Validate config, lock, scenarios, plan, and evidence.
    Check {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Explain a plan or an individual guarantee.
    Explain {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        node_id: Option<String>,
    },
    /// Write an embedded v1 JSON Schema to stdout.
    Schema {
        #[arg(value_enum)]
        name: SchemaName,
    },
    #[command(hide = true, name = "__process-agent")]
    ProcessAgent,
    #[command(hide = true, name = "__observer-agent")]
    ObserverAgent,
    #[command(hide = true, name = "__nushell-adapter")]
    NushellAdapter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum GuaranteeRequirement {
    Native,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BackendKind {
    Disposable,
    Local,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ExportMode {
    Strict,
    Bundle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ExportTarget {
    Internal,
    Dagger,
    #[value(name = "nu")]
    Nushell,
    Cwl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SchemaName {
    Inventory,
    Manifest,
    Bundle,
    #[value(name = "effect-ir")]
    EffectIr,
    Evidence,
    Diagnostic,
    Protocol,
    Project,
    Scenario,
    Lock,
    Replay,
    #[value(name = "corpus-audit")]
    CorpusAudit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Failure {
    exit: i32,
    code: &'static str,
    message: String,
}

impl Failure {
    fn io(message: impl Into<String>) -> Self {
        Self {
            exit: 1,
            code: "DESHELL_IO",
            message: message.into(),
        }
    }
    fn limit(message: impl Into<String>) -> Self {
        Self {
            exit: 1,
            code: "DESHELL_LIMIT_EXCEEDED",
            message: message.into(),
        }
    }
    fn usage(message: impl Into<String>) -> Self {
        Self {
            exit: 2,
            code: "DESHELL_USAGE",
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            exit: 3,
            code: "DESHELL_INVALID_CONTRACT",
            message: message.into(),
        }
    }
    fn policy(message: impl Into<String>) -> Self {
        Self {
            exit: 4,
            code: "DESHELL_POLICY",
            message: message.into(),
        }
    }
    fn difference(message: impl Into<String>) -> Self {
        Self {
            exit: 5,
            code: "DESHELL_DIFFERENCE",
            message: message.into(),
        }
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            exit: 6,
            code: "DESHELL_PROVIDER_UNAVAILABLE",
            message: message.into(),
        }
    }
    fn internal(message: impl Into<String>) -> Self {
        Self {
            exit: 70,
            code: "DESHELL_INTERNAL",
            message: message.into(),
        }
    }
}

pub(crate) fn run_from<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let fallback_mode = requested_diagnostic_mode(&args);
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            use clap::error::ErrorKind;
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                return match stdout.write_all(error.to_string().as_bytes()) {
                    Ok(()) => 0,
                    Err(_) => 1,
                };
            }
            let diagnostic = crate::diagnostics::Diagnostic::error(
                "DESHELL_USAGE",
                error.to_string().trim().to_owned(),
            );
            let _ = crate::diagnostics::emit(stderr, fallback_mode, &diagnostic);
            return 2;
        }
    };
    let diagnostic_mode = cli.diagnostics;
    match dispatch(cli.command, diagnostic_mode, stdout, stderr) {
        Ok(code) => code,
        Err(failure) => {
            let diagnostic = crate::diagnostics::Diagnostic::error(failure.code, failure.message);
            if crate::diagnostics::emit(stderr, diagnostic_mode, &diagnostic).is_err() {
                70
            } else {
                failure.exit
            }
        }
    }
}

fn requested_diagnostic_mode(args: &[OsString]) -> crate::diagnostics::Mode {
    let mut values = args.iter().filter_map(|value| value.to_str());
    while let Some(value) = values.next() {
        if value == "--diagnostics=jsonl" {
            return crate::diagnostics::Mode::Jsonl;
        }
        if value == "--diagnostics" && values.next() == Some("jsonl") {
            return crate::diagnostics::Mode::Jsonl;
        }
    }
    crate::diagnostics::Mode::Human
}

fn dispatch(
    command: Command,
    diagnostic_mode: crate::diagnostics::Mode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    match command {
        Command::Init { root, entries } => {
            let result = if entries.is_empty() {
                crate::project::init(&root)
            } else {
                crate::project::init_with_entries(&root, &entries)
            }
            .map_err(Failure::io)?;
            if result.created.is_empty() {
                writeln_io(
                    stdout,
                    format_args!("de-shell already initialized in {}", root.display()),
                )?;
            } else {
                writeln_io(
                    stdout,
                    format_args!("initialized de-shell in {}", root.display()),
                )?;
                for path in result.created {
                    writeln_io(stdout, format_args!("created {}", path.display()))?;
                }
            }
            for entry in result.entrypoints {
                writeln_io(stdout, format_args!("entrypoint {entry}"))?;
            }
            Ok(0)
        }
        Command::Scan { root, format } => {
            let inventory = crate::project::scan(&root).map_err(Failure::io)?;
            match format {
                OutputFormat::Json => {
                    let value = serde_json::to_value(&inventory)
                        .map_err(|error| Failure::internal(error.to_string()))?;
                    write_io(
                        stdout,
                        &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
                    )?;
                }
                OutputFormat::Human => {
                    for finding in &inventory.findings {
                        let locator = finding
                            .locator
                            .as_ref()
                            .map_or(String::new(), |value| format!("#{value}"));
                        writeln_io(
                            stdout,
                            format_args!(
                                "{}\t{}{}",
                                finding_kind(&finding.kind),
                                finding.path,
                                locator
                            ),
                        )?;
                    }
                    writeln_io(
                        stdout,
                        format_args!(
                            "{} shell location(s) found; {} skipped; {} error(s)",
                            inventory.findings.len(),
                            inventory.skipped.len(),
                            inventory.errors.len()
                        ),
                    )?;
                }
            }
            Ok(0)
        }
        Command::Analyze { root, entry } => {
            for entry in selected_entries(&root, entry)? {
                let result =
                    crate::project::analyze(&root, &entry).map_err(classify_project_error)?;
                writeln_io(stdout, format_args!("wrote {}", result.plan_path.display()))?;
                writeln_io(
                    stdout,
                    format_args!("wrote {}", result.evidence_path.display()),
                )?;
            }
            Ok(0)
        }
        Command::Check { root } => {
            crate::project::check(&root).map_err(classify_project_errors)?;
            writeln_io(
                stdout,
                format_args!("{}: project artifacts are valid", root.display()),
            )?;
            Ok(0)
        }
        Command::Verify {
            root,
            entry,
            require,
        } => {
            let project =
                crate::project::ValidatedProject::load(&root).map_err(classify_project_errors)?;
            let entries = selected_entries_from_config(&project.config, entry)?;
            let multiple = entries.len() > 1;
            let mut has_difference = false;
            let mut has_unavailable = false;
            let mut has_failed = false;
            let mut has_unobserved = false;
            let mut has_residual = false;
            let scenario_digests = project
                .scenarios
                .iter()
                .map(|scenario| (scenario.scenario.name.clone(), scenario.digest.clone()))
                .collect::<std::collections::BTreeMap<_, _>>();
            let lock = &project.lock;
            let provider_name = if crate::lab::digest_pinned(&lock.lab.image) {
                crate::lab::select(crate::lab::platform_of_host(), &crate::lab::SystemProbe)
                    .ok()
                    .filter(|provider| crate::lab::execution_connected(*provider))
                    .map(crate::lab::provider_name)
                    .unwrap_or("unavailable")
            } else {
                "unavailable"
            };
            let provider_fingerprint = crate::digest::sha256(
                format!("deshell-provider-v1:{provider_name}:{}", lock.lab.image).as_bytes(),
            );
            for entry in entries {
                let validated = project.entry(&entry).map_err(classify_project_errors)?;
                let report = crate::verify::audit_current(
                    &validated.plan,
                    &validated.evidence,
                    crate::verify::AuditContext {
                        source_path: &entry,
                        source_bytes: validated.source.len(),
                        scenario_digests: &scenario_digests,
                        runtime_lock_digest: &project.runtime_lock_digest,
                        lab_image: &lock.lab.image,
                        provider_fingerprint: &provider_fingerprint,
                    },
                )
                .map_err(|errors| Failure::invalid(errors.join("; ")))?;
                let prefix = if multiple {
                    format!("entry={entry} ")
                } else {
                    String::new()
                };
                writeln_io(
                    stdout,
                    format_args!(
                        "{prefix}native={} delegated={} residual={} observations={} stale={} unobserved={} verified={} different={} unavailable={} failed={} nondeterministic={} source_bytes={} native_bytes={} delegated_bytes={} residual_bytes={} uncovered_bytes={}",
                        report.native,
                        report.delegated,
                        report.residual,
                        report.observations,
                        report.stale,
                        report.unobserved,
                        report.verified,
                        report.different,
                        report.unavailable,
                        report.failed,
                        report.nondeterministic,
                        report.source_bytes,
                        report.native_bytes,
                        report.delegated_bytes,
                        report.residual_bytes,
                        report.uncovered_bytes
                    ),
                )?;
                for reason in report.residual_reasons {
                    writeln_io(stdout, format_args!("{prefix}residual: {reason}"))?;
                }
                has_residual |= report.residual != 0 || report.residual_bytes != 0;
                if require == Some(GuaranteeRequirement::Native)
                    && (report.delegated != 0 || report.residual != 0)
                {
                    return Err(Failure::policy(format!(
                        "entrypoint {entry} does not satisfy --require native"
                    )));
                }
                has_difference |= report.different != 0 || report.nondeterministic != 0;
                has_unavailable |= report.unavailable != 0;
                has_failed |= report.failed != 0;
                has_unobserved |= report.unobserved != 0;
            }
            if has_residual {
                return Err(Failure::policy(
                    "verification rejected non-executable residual source",
                ));
            }
            if has_difference {
                return Err(Failure::difference(
                    "recorded observation differs from the plan or is nondeterministic",
                ));
            }
            if has_unavailable {
                return Err(Failure::unavailable(
                    "a current scenario observation has no disposable provider",
                ));
            }
            if has_failed {
                return Err(Failure::io(
                    "a current scenario observation failed during execution",
                ));
            }
            if has_unobserved {
                return Err(Failure::invalid(
                    "one or more current scenarios have not been observed",
                ));
            }
            Ok(0)
        }
        Command::Run {
            root,
            entry,
            backend,
            node,
            mut arguments,
            trailing,
        } => {
            arguments.extend(trailing);
            run_plan(
                RunOptions {
                    root: &root,
                    entrypoint: entry.as_deref(),
                    node_id: node.as_deref(),
                    backend,
                    arguments: &arguments,
                },
                stdout,
                stderr,
            )
        }
        Command::Export {
            root,
            entry,
            target,
            mode,
            output,
        } => {
            let project =
                crate::project::ValidatedProject::load(&root).map_err(classify_project_errors)?;
            let selected = selected_entry_from_config(&project.config, entry)?;
            let validated = project.entry(&selected).map_err(classify_project_errors)?;
            if mode == ExportMode::Bundle
                && project.config.export.mode != crate::config::ExportMode::Bundle
            {
                return Err(Failure::policy(
                    "bundle export is disabled by project policy; set export.mode = \"bundle\"",
                ));
            }
            if mode == ExportMode::Bundle {
                let output = output.ok_or_else(|| {
                    Failure::invalid("bundle export requires project-relative --output")
                })?;
                let path = safe_output_path(&root, &output)?;
                let request = bundle_request(&root, &selected, &project, exporter_target(target))?;
                atomic_bundle_write(&path, request)?;
                writeln_io(stdout, format_args!("wrote {}", path.display()))?;
                return Ok(0);
            }
            let artifact = crate::exporter::export(
                &validated.plan,
                exporter_target(target),
                crate::exporter::Mode::Strict,
                export_runtime(&project.lock, target),
            )
            .map_err(|message| {
                if message.contains("strict exporter") || message.contains("requires exactly") {
                    Failure::policy(message)
                } else {
                    Failure::invalid(message)
                }
            })?;
            if let Some(output) = output {
                let path = safe_output_path(&root, &output)?;
                atomic_write(&path, artifact.content)?;
                writeln_io(stdout, format_args!("wrote {}", path.display()))?;
            } else {
                write_io(stdout, &artifact.content)?;
            }
            Ok(0)
        }
        Command::Observe {
            root,
            entry,
            scenario,
        } => observe_command(&root, entry, &scenario, stdout),
        Command::Doctor { root, format } => doctor_command(&root, format, stdout),
        Command::Explain { root, node_id } => explain(&root, node_id.as_deref(), stdout),
        Command::Schema { name } => {
            write_io(stdout, schema(name))?;
            Ok(0)
        }
        Command::Rewrite {
            root,
            entry,
            equivalent,
            apply,
        } => rewrite_command(&root, entry, equivalent, apply, stdout),
        Command::Modernize {
            root,
            profile,
            apply,
        } => modernize_command(&root, &profile, apply, diagnostic_mode, stdout, stderr),
        Command::Migrate {
            root,
            entry,
            observe,
            target,
            apply,
        } => migrate_command(&root, entry, observe, target, apply, stdout),
        Command::ProcessAgent => {
            crate::protocol::serve_stdio(crate::protocol::AgentKind::Process, stdout)
                .map_err(Failure::invalid)
        }
        Command::ObserverAgent => {
            crate::protocol::serve_stdio(crate::protocol::AgentKind::Observer, stdout)
                .map_err(Failure::invalid)
        }
        Command::NushellAdapter => {
            crate::protocol::serve_stdio(crate::protocol::AgentKind::Nushell, stdout)
                .map_err(Failure::invalid)
        }
    }
}

fn schema(name: SchemaName) -> &'static [u8] {
    match name {
        SchemaName::Inventory => {
            include_bytes!("../../../contracts/schema/inventory-v1.schema.json")
        }
        SchemaName::Manifest => {
            include_bytes!("../../../contracts/schema/manifest-v1.schema.json")
        }
        SchemaName::Bundle => include_bytes!("../../../contracts/schema/bundle-v1.schema.json"),
        SchemaName::EffectIr => {
            include_bytes!("../../../contracts/schema/effect-ir-v1.schema.json")
        }
        SchemaName::Evidence => include_bytes!("../../../contracts/schema/evidence-v1.schema.json"),
        SchemaName::Diagnostic => {
            include_bytes!("../../../contracts/schema/diagnostic-v1.schema.json")
        }
        SchemaName::Protocol => include_bytes!("../../../contracts/schema/protocol-v1.schema.json"),
        SchemaName::Project => include_bytes!("../../../contracts/schema/project-v1.schema.json"),
        SchemaName::Scenario => include_bytes!("../../../contracts/schema/scenario-v1.schema.json"),
        SchemaName::Lock => include_bytes!("../../../contracts/schema/lock-v1.schema.json"),
        SchemaName::Replay => include_bytes!("../../../contracts/schema/replay-v1.schema.json"),
        SchemaName::CorpusAudit => {
            include_bytes!("../../../contracts/schema/corpus-audit-v1.schema.json")
        }
    }
}

fn selected_entry(root: &Path, entry: Option<String>) -> Result<String, Failure> {
    entry.map_or_else(
        || crate::project::configured_entry(root).map_err(classify_project_error),
        Ok,
    )
}

fn selected_entry_from_config(
    config: &crate::config::ProjectConfig,
    entry: Option<String>,
) -> Result<String, Failure> {
    match (entry, config.entrypoints.as_slice()) {
        (Some(entry), _) => Ok(entry),
        (None, [entry]) => Ok(entry.clone()),
        (None, []) => Err(Failure::invalid(
            "no entrypoint was supplied and project.toml entrypoints is empty",
        )),
        (None, _) => Err(Failure::invalid(
            "project.toml contains multiple entrypoints; select one with --entry",
        )),
    }
}

fn selected_entries(root: &Path, mut entries: Vec<String>) -> Result<Vec<String>, Failure> {
    if entries.is_empty() {
        entries = crate::project::load_config(root)
            .map_err(classify_project_errors)?
            .entrypoints;
    }
    entries.sort();
    entries.dedup();
    if entries.is_empty() {
        Err(Failure::invalid(
            "no entrypoint was supplied and project.toml entrypoints is empty",
        ))
    } else {
        Ok(entries)
    }
}

fn selected_entries_from_config(
    config: &crate::config::ProjectConfig,
    mut entries: Vec<String>,
) -> Result<Vec<String>, Failure> {
    if entries.is_empty() {
        entries.clone_from(&config.entrypoints);
    }
    entries.sort();
    entries.dedup();
    if entries.is_empty() {
        Err(Failure::invalid(
            "no entrypoint was supplied and project.toml entrypoints is empty",
        ))
    } else {
        Ok(entries)
    }
}

fn classify_project_errors(errors: Vec<String>) -> Failure {
    let message = errors.join("; ");
    if message.contains("rejected by policy") {
        Failure::policy(message)
    } else if errors.iter().all(|error| is_io_message(error)) {
        Failure::io(message)
    } else {
        Failure::invalid(message)
    }
}

fn classify_project_error(message: String) -> Failure {
    if message.contains("rejected by policy") {
        Failure::policy(message)
    } else if is_io_message(&message) {
        Failure::io(message)
    } else {
        Failure::invalid(message)
    }
}

fn is_io_message(message: &str) -> bool {
    [
        "cannot read",
        "cannot inspect",
        "cannot resolve",
        "No such file",
        "missing required file",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

fn finding_kind(kind: &crate::scanner::FindingKind) -> &'static str {
    match kind {
        crate::scanner::FindingKind::ShellFile => "shell_file",
        crate::scanner::FindingKind::EmbeddedShell => "embedded_shell",
        crate::scanner::FindingKind::Candidate => "candidate",
    }
}

fn exporter_target(target: ExportTarget) -> crate::exporter::Target {
    match target {
        ExportTarget::Internal => crate::exporter::Target::Internal,
        ExportTarget::Dagger => crate::exporter::Target::Dagger,
        ExportTarget::Nushell => crate::exporter::Target::Nushell,
        ExportTarget::Cwl => crate::exporter::Target::Cwl,
    }
}

fn export_runtime(lock: &crate::config::Lockfile, target: ExportTarget) -> Option<&str> {
    match target {
        ExportTarget::Dagger => Some(&lock.targets.dagger_image),
        ExportTarget::Internal | ExportTarget::Nushell | ExportTarget::Cwl => None,
    }
}

fn bundle_export_runtime(
    lock: &crate::config::Lockfile,
    target: crate::exporter::Target,
) -> Option<&str> {
    match target {
        crate::exporter::Target::Dagger => Some(&lock.targets.dagger_image),
        _ => None,
    }
}

struct RunOptions<'a> {
    root: &'a Path,
    entrypoint: Option<&'a str>,
    node_id: Option<&'a str>,
    backend: BackendKind,
    arguments: &'a [String],
}

fn run_plan(
    options: RunOptions<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    let project =
        crate::project::ValidatedProject::load(options.root).map_err(classify_project_errors)?;
    let entrypoint =
        selected_entry_from_config(&project.config, options.entrypoint.map(str::to_owned))?;
    if options.backend == BackendKind::Disposable {
        return run_disposable(
            options.root,
            &entrypoint,
            options.node_id,
            options.arguments,
            stdout,
            stderr,
        );
    }
    let validated = project
        .entry(&entrypoint)
        .map_err(classify_project_errors)?;
    let plan = select_node(validated.plan.clone(), options.node_id).map_err(Failure::invalid)?;
    let config = &project.config;
    if !config.sandbox.allow_local {
        return Err(Failure::policy(
            "local execution requires both sandbox.allow_local = true and --backend local",
        ));
    }
    let backend = crate::local_backend::LocalBackend::for_validated_project(&project);
    let mut environment = std::collections::BTreeMap::new();
    for name in plan.tasks.iter().flat_map(|task| &task.environment) {
        if let Ok(value) = std::env::var(name) {
            environment.insert(name.clone(), value);
        }
    }
    let result = crate::runner::run_plan(
        &backend,
        policy_from_config(config, false),
        &plan,
        &environment,
        &std::collections::BTreeMap::new(),
        options.arguments,
    )
    .map_err(|error| match error.kind {
        crate::runner::RunErrorKind::Execution
            if error.message.contains("provider is unavailable") =>
        {
            Failure::unavailable(error.message)
        }
        crate::runner::RunErrorKind::Execution if error.message.contains("limit_exceeded") => {
            Failure::limit(error.message)
        }
        crate::runner::RunErrorKind::Execution => Failure::io(error.message),
        crate::runner::RunErrorKind::Invalid => Failure::invalid(error.message),
        crate::runner::RunErrorKind::Policy => Failure::policy(error.message),
    })?;
    // Once execution returns, its status is the command status. Diagnostics mode
    // never transforms the plan's raw stdout or stderr.
    let _ = stdout.write_all(&result.stdout);
    let _ = stderr.write_all(&result.stderr);
    Ok(result.exit_code)
}

fn policy_from_config(
    config: &crate::config::ProjectConfig,
    disposable: bool,
) -> crate::runner::Policy {
    crate::runner::Policy {
        allow_file_read: matches!(
            config.policy.file_read,
            crate::config::FileReadPolicy::Project
        ),
        allow_file_write: disposable
            && matches!(
                config.policy.file_write,
                crate::config::FileWritePolicy::Sandbox
            ),
        allow_network: disposable
            && matches!(
                config.policy.network,
                crate::config::NetworkPolicy::RecordReplay
            ),
        allow_delegation: disposable
            && matches!(
                config.policy.delegation,
                crate::config::DelegationPolicy::Pinned
            ),
    }
}

fn disposable_provider(lock: &crate::config::Lockfile) -> Result<crate::lab::Provider, Failure> {
    if lock.lab.image == "unconfigured" {
        return Err(Failure::unavailable(
            "deshell.lock lab.image is unconfigured; install a signed digest-pinned lab bundle",
        ));
    }
    if !crate::lab::digest_pinned(&lock.lab.image) {
        return Err(Failure::invalid(
            "deshell.lock lab.image is not pinned by an OCI sha256 digest",
        ));
    }
    let provider = crate::lab::select(crate::lab::platform_of_host(), &crate::lab::SystemProbe)
        .map_err(Failure::unavailable)?;
    if !crate::lab::execution_connected(provider) {
        return Err(Failure::unavailable(format!(
            "{} launch contract is present, but its signed helper transport is not connected in this build",
            crate::lab::provider_name(provider)
        )));
    }
    Ok(provider)
}

#[allow(clippy::too_many_arguments)]
fn run_disposable(
    root: &Path,
    entrypoint: &str,
    node_id: Option<&str>,
    arguments: &[String],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    let workspace = crate::workspace::private_snapshot(root).map_err(Failure::io)?;
    let project = crate::project::ValidatedProject::load(workspace.path())
        .map_err(classify_project_errors)?;
    let validated = project.entry(entrypoint).map_err(classify_project_errors)?;
    let plan = select_node(validated.plan.clone(), node_id).map_err(Failure::invalid)?;
    let provider = disposable_provider(&project.lock)?;
    let output = tempfile::Builder::new()
        .prefix("deshell-result-")
        .tempdir()
        .map_err(|error| Failure::io(format!("cannot create result mount: {error}")))?;
    let environment = plan
        .tasks
        .iter()
        .flat_map(|task| &task.environment)
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
        .collect();
    let request = crate::lab::Request {
        workspace: path_string(workspace.path(), "private workspace")?,
        result_path: path_string(&output.path().join("result.json"), "result path")?,
        target: crate::lab::Target::Plan {
            entrypoint: entrypoint.into(),
            node_id: node_id.map(str::to_owned),
        },
        arguments: arguments.to_vec(),
        named_inputs: vec![],
        environment,
        stdin: vec![],
        working_directory: None,
        fixtures: vec![],
        expected_files: vec![],
        limits: project.config.limits,
        network: lab_network(&project.config),
        image: project.lock.lab.image.clone(),
    };
    let result = crate::lab::execute(provider, &request).map_err(classify_lab_failure)?;
    let _ = stdout.write_all(&result.stdout);
    let _ = stderr.write_all(&result.stderr);
    Ok(result.exit_code)
}

fn lab_network(config: &crate::config::ProjectConfig) -> crate::lab::Network {
    match config.policy.network {
        crate::config::NetworkPolicy::Deny => crate::lab::Network::Deny,
        crate::config::NetworkPolicy::RecordReplay => crate::lab::Network::Replay {
            proxy: "http://deshell-replay:8080".into(),
            tape: "/workspace/.deshell/replay.json".into(),
        },
    }
}

fn classify_lab_failure(error: crate::lab::ExecutionFailure) -> Failure {
    match error.kind {
        crate::lab::ExecutionFailureKind::Unavailable => Failure::unavailable(error.message),
        crate::lab::ExecutionFailureKind::Failed => Failure::io(error.message),
    }
}

fn path_string(path: &Path, label: &str) -> Result<String, Failure> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Failure::invalid(format!("{label} is not valid UTF-8: {}", path.display())))
}

fn doctor_command(
    root: &Path,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let binary = std::env::current_exe()
        .map_err(|error| Failure::io(format!("cannot resolve current executable: {error}")))?;
    let binary_ok = binary.is_file();
    let config = crate::project::load_config(root);
    let lock = crate::project::load_lock(root);
    let provider = crate::lab::select(crate::lab::platform_of_host(), &crate::lab::SystemProbe)
        .and_then(|provider| {
            if crate::lab::execution_connected(provider) {
                Ok(provider)
            } else {
                Err(format!(
                    "{} launch contract is present, but its signed helper transport is not connected in this build",
                    crate::lab::provider_name(provider)
                ))
            }
        });
    let (image, image_pinned) = match &lock {
        Ok(lock) => (
            lock.lab.image.clone(),
            crate::lab::digest_pinned(&lock.lab.image),
        ),
        Err(_) => ("unavailable".into(), false),
    };
    let provider_name = provider
        .as_ref()
        .ok()
        .map(|provider| crate::lab::provider_name(*provider).to_owned());
    let provider_error = provider.as_ref().err().cloned();
    let config_errors = config.as_ref().err().cloned().unwrap_or_default();
    let lock_errors = lock.as_ref().err().cloned().unwrap_or_default();
    let lock_digest = crate::digest::file_sha256(&root.join("deshell.lock"))
        .ok()
        .map(|(_, digest)| digest);
    let mut bundle_assets = Vec::new();
    let mut runtime_asset_ready = false;
    let interpreter_pins = lock
        .as_ref()
        .ok()
        .and_then(|lock| serde_json::to_value(&lock.interpreters).ok());
    let target_pins = lock
        .as_ref()
        .ok()
        .and_then(|lock| serde_json::to_value(&lock.targets).ok());
    if let Ok(lock) = &lock {
        for asset in lock.lab.assets.iter().filter(|asset| {
            asset.operating_system == std::env::consts::OS
                && asset.architecture == std::env::consts::ARCH
        }) {
            let inspected = crate::project::project_file_path(root, &asset.path)
                .and_then(|path| crate::digest::file_sha256(&path));
            let (actual, bytes, error) = match inspected {
                Ok((bytes, digest)) => (Some(digest), Some(bytes), None),
                Err(error) => (None, None, Some(error)),
            };
            let valid = actual
                .as_deref()
                .is_some_and(|digest| asset.sha256 == format!("sha256:{digest}"));
            runtime_asset_ready |= valid && asset.role == crate::config::LabAssetRole::Runtime;
            bundle_assets.push(serde_json::json!({
                "actual_sha256": actual,
                "bytes": bytes,
                "error": error,
                "expected_sha256": asset.sha256,
                "name": asset.name,
                "path": asset.path,
                "role": asset.role,
                "valid": valid
            }));
        }
    }
    let bundle_ready = image_pinned && runtime_asset_ready;
    let ready = binary_ok && config.is_ok() && lock.is_ok() && image_pinned && provider.is_ok();
    match format {
        OutputFormat::Json => {
            let value = serde_json::json!({
                "binary": {"path": binary, "valid": binary_ok},
                "bundle": {"assets": bundle_assets, "ready": bundle_ready},
                "config": {"errors": config_errors, "valid": config.is_ok()},
                "interpreters": interpreter_pins,
                "lab_image": {"pin": image, "valid": image_pinned},
                "lock": {"digest": lock_digest, "errors": lock_errors, "valid": lock.is_ok()},
                "provider": {"error": provider_error, "name": provider_name},
                "ready": ready,
                "schema_version": 1,
                "targets": target_pins
            });
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        OutputFormat::Human => {
            writeln_io(
                stdout,
                format_args!("binary: {}", if binary_ok { "ok" } else { "invalid" }),
            )?;
            writeln_io(
                stdout,
                format_args!("config: {}", if config.is_ok() { "ok" } else { "invalid" }),
            )?;
            writeln_io(
                stdout,
                format_args!("lock: {}", if lock.is_ok() { "ok" } else { "invalid" }),
            )?;
            writeln_io(
                stdout,
                format_args!(
                    "lab image: {}",
                    if image_pinned {
                        "pinned"
                    } else {
                        "unconfigured or invalid"
                    }
                ),
            )?;
            writeln_io(
                stdout,
                format_args!(
                    "Dagger target: {}",
                    if lock
                        .as_ref()
                        .ok()
                        .is_some_and(|lock| crate::lab::digest_pinned(&lock.targets.dagger_image))
                    {
                        "pinned"
                    } else {
                        "unconfigured or invalid"
                    }
                ),
            )?;
            writeln_io(
                stdout,
                format_args!(
                    "bundle assets: {}",
                    if bundle_ready { "ready" } else { "not ready" }
                ),
            )?;
            match (provider_name, provider_error) {
                (Some(name), _) => writeln_io(stdout, format_args!("provider: {name}"))?,
                (_, Some(error)) => {
                    writeln_io(stdout, format_args!("provider: unavailable ({error})"))?
                }
                _ => writeln_io(stdout, format_args!("provider: unavailable"))?,
            }
            writeln_io(
                stdout,
                format_args!(
                    "disposable execution: {}",
                    if ready { "ready" } else { "not ready" }
                ),
            )?;
        }
    }
    Ok(if ready { 0 } else { 6 })
}

fn observe_command(
    root: &Path,
    entry: Option<String>,
    selected_scenarios: &[String],
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let base_workspace = crate::workspace::private_snapshot(root).map_err(Failure::io)?;
    let base = base_workspace.path();
    let project = crate::project::ValidatedProject::load(base).map_err(classify_project_errors)?;
    let entry = selected_entry_from_config(&project.config, entry)?;
    let validated = project.entry(&entry).map_err(classify_project_errors)?;
    let mut evidence = validated.evidence.clone();
    let scenarios = select_scenarios(&project.scenarios, selected_scenarios)?;
    let interpreter = crate::frontend::detect(&entry, &validated.source)
        .name()
        .to_owned();
    let config = &project.config;
    let lock = &project.lock;
    let provider = if lock.lab.image == "unconfigured" {
        Err(
            "deshell.lock lab.image is unconfigured; install a signed digest-pinned lab bundle"
                .to_owned(),
        )
    } else if !crate::lab::digest_pinned(&lock.lab.image) {
        return Err(Failure::invalid(
            "deshell.lock lab.image is not pinned by an OCI sha256 digest",
        ));
    } else {
        crate::lab::select(crate::lab::platform_of_host(), &crate::lab::SystemProbe).and_then(
            |provider| {
                if crate::lab::execution_connected(provider) {
                    Ok(provider)
                } else {
                    Err(format!(
                        "{} launch contract is present, but its signed helper transport is not connected in this build",
                        crate::lab::provider_name(provider)
                    ))
                }
            },
        )
    };
    let provider = match provider {
        Ok(provider) => provider,
        Err(reason) => {
            let provider_name = "unavailable";
            let fingerprint = crate::digest::sha256(
                format!("deshell-provider-v1:{provider_name}:{}", lock.lab.image).as_bytes(),
            );
            for scenario in &scenarios {
                evidence
                    .append_observation(crate::evidence::ObservationEvidence {
                        scenario: scenario.scenario.name.clone(),
                        key: crate::evidence::ObservationKey {
                            scenario_digest: scenario.digest.clone(),
                            provider_fingerprint: fingerprint.clone(),
                            runtime_lock_digest: project.runtime_lock_digest.clone(),
                        },
                        status: crate::evidence::ObservationStatus::Unavailable,
                        provider: provider_name.into(),
                        reason: Some(reason.clone()),
                        digest: None,
                    })
                    .map_err(Failure::invalid)?;
                writeln_io(
                    stdout,
                    format_args!("{}: unavailable", scenario.scenario.name),
                )?;
            }
            crate::project::save_evidence(root, &evidence).map_err(classify_project_error)?;
            return Ok(6);
        }
    };
    let provider_name = crate::lab::provider_name(provider);
    let provider_fingerprint = crate::digest::sha256(
        format!("deshell-provider-v1:{provider_name}:{}", lock.lab.image).as_bytes(),
    );
    let mut exit = 0;
    for validated_scenario in scenarios {
        let scenario = validated_scenario.scenario;
        let key = crate::evidence::ObservationKey {
            scenario_digest: validated_scenario.digest,
            provider_fingerprint: provider_fingerprint.clone(),
            runtime_lock_digest: project.runtime_lock_digest.clone(),
        };
        let original_workspace = crate::workspace::private_snapshot(base).map_err(Failure::io)?;
        let actual_workspace = crate::workspace::private_snapshot(base).map_err(Failure::io)?;
        let original = lab_scenario_request(
            original_workspace.path(),
            crate::lab::Target::Original {
                interpreter: interpreter.clone(),
                script: entry.clone(),
            },
            &scenario,
            config,
            &lock.lab.image,
        )?;
        let actual = lab_scenario_request(
            actual_workspace.path(),
            crate::lab::Target::Plan {
                entrypoint: entry.clone(),
                node_id: None,
            },
            &scenario,
            config,
            &lock.lab.image,
        )?;
        let expected = match crate::lab::execute(provider, &original) {
            Ok(result) => result,
            Err(error) => {
                let status = match error.kind {
                    crate::lab::ExecutionFailureKind::Unavailable => {
                        exit = exit.max(6);
                        crate::evidence::ObservationStatus::Unavailable
                    }
                    crate::lab::ExecutionFailureKind::Failed => {
                        exit = exit.max(1);
                        crate::evidence::ObservationStatus::Failed
                    }
                };
                evidence
                    .append_observation(crate::evidence::ObservationEvidence {
                        scenario: scenario.name.clone(),
                        key,
                        status,
                        provider: provider_name.into(),
                        reason: Some(error.message),
                        digest: None,
                    })
                    .map_err(Failure::invalid)?;
                continue;
            }
        };
        if let Some(reason) = crate::differential::expectation_failure(&scenario, &expected) {
            evidence
                .append_observation(crate::evidence::ObservationEvidence {
                    scenario: scenario.name.clone(),
                    key,
                    status: crate::evidence::ObservationStatus::Failed,
                    provider: provider_name.into(),
                    reason: Some(reason),
                    digest: None,
                })
                .map_err(Failure::invalid)?;
            exit = exit.max(1);
            continue;
        }
        let actual = match crate::lab::execute(provider, &actual) {
            Ok(result) => result,
            Err(error) => {
                evidence
                    .append_observation(crate::evidence::ObservationEvidence {
                        scenario: scenario.name.clone(),
                        key,
                        status: crate::evidence::ObservationStatus::Failed,
                        provider: provider_name.into(),
                        reason: Some(format!("plan execution failed: {}", error.message)),
                        digest: None,
                    })
                    .map_err(Failure::invalid)?;
                exit = exit.max(1);
                continue;
            }
        };
        let comparison = crate::verify::compare(&expected, &actual).map_err(Failure::internal)?;
        let status = crate::verify::record_comparison(
            &mut evidence,
            &scenario.name,
            provider_name,
            key,
            &comparison,
        )
        .map_err(Failure::invalid)?;
        writeln_io(
            stdout,
            format_args!("{}: {}", scenario.name, observation_status(status)),
        )?;
        if matches!(
            status,
            crate::evidence::ObservationStatus::Different
                | crate::evidence::ObservationStatus::Nondeterministic
        ) {
            exit = 5;
        }
    }
    crate::project::save_evidence(root, &evidence).map_err(classify_project_error)?;
    Ok(exit)
}

fn lab_scenario_request(
    workspace: &Path,
    target: crate::lab::Target,
    scenario: &crate::config::Scenario,
    config: &crate::config::ProjectConfig,
    image: &str,
) -> Result<crate::lab::Request, Failure> {
    let output = workspace.join(".deshell/provider-result.json");
    Ok(crate::lab::Request {
        workspace: path_string(workspace, "private workspace")?,
        result_path: path_string(&output, "provider result path")?,
        target,
        arguments: scenario.argv.clone(),
        named_inputs: scenario
            .arguments
            .iter()
            .map(|value| (value.name.clone(), value.value.clone()))
            .collect(),
        environment: scenario
            .environment
            .iter()
            .map(|value| (value.name.clone(), value.value.clone()))
            .collect(),
        stdin: scenario
            .stdin
            .as_ref()
            .map(crate::config::BinaryData::bytes)
            .transpose()
            .map_err(Failure::invalid)?
            .unwrap_or_default(),
        working_directory: scenario.cwd.clone(),
        fixtures: scenario.fixtures.clone(),
        expected_files: scenario.expect.files.clone(),
        limits: scenario.limits,
        network: lab_network(config),
        image: image.into(),
    })
}

fn select_scenarios(
    scenarios: &[crate::project::ValidatedScenario],
    selected: &[String],
) -> Result<Vec<crate::project::ValidatedScenario>, Failure> {
    let matching = scenarios
        .iter()
        .filter(|scenario| selected.is_empty() || selected.contains(&scenario.scenario.name))
        .cloned()
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return Err(Failure::invalid("no matching scenario was found"));
    }
    for name in selected {
        if !matching
            .iter()
            .any(|scenario| &scenario.scenario.name == name)
        {
            return Err(Failure::invalid(format!("scenario not found: {name}")));
        }
    }
    Ok(matching)
}

fn observation_status(status: crate::evidence::ObservationStatus) -> &'static str {
    match status {
        crate::evidence::ObservationStatus::Verified => "verified",
        crate::evidence::ObservationStatus::Different => "different",
        crate::evidence::ObservationStatus::Unavailable => "unavailable",
        crate::evidence::ObservationStatus::Failed => "failed",
        crate::evidence::ObservationStatus::Nondeterministic => "nondeterministic",
    }
}

fn bundle_request<'a>(
    root: &Path,
    entrypoint: &'a str,
    project: &'a crate::project::ValidatedProject,
    target: crate::exporter::Target,
) -> Result<crate::exporter::BundleRequest<'a>, Failure> {
    let lock = &project.lock;
    let plan = &project
        .entry(entrypoint)
        .map_err(classify_project_errors)?
        .plan;
    if !crate::lab::digest_pinned(&lock.lab.image) {
        return Err(Failure::unavailable(
            "bundle export requires a digest-pinned lab.image",
        ));
    }
    let platform_assets = lock
        .lab
        .assets
        .iter()
        .filter(|asset| {
            asset.operating_system == std::env::consts::OS
                && asset.architecture == std::env::consts::ARCH
        })
        .collect::<Vec<_>>();
    if !platform_assets
        .iter()
        .any(|asset| asset.role == crate::config::LabAssetRole::Runtime)
    {
        return Err(Failure::unavailable(format!(
            "deshell.lock has no runtime asset for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    }
    let mut files = vec![
        bundle_project_file(root, ".deshell/project.toml", None, false)?,
        bundle_project_file(root, ".deshell/manifest.json", None, false)?,
        bundle_project_file(root, "deshell.lock", None, false)?,
    ];
    if !project
        .manifest
        .entries
        .iter()
        .any(|entry| entry.entrypoint == entrypoint)
    {
        return Err(Failure::invalid(format!(
            "manifest entry not found: {entrypoint}"
        )));
    }
    for validated in &project.entries {
        let entry = &validated.manifest;
        files.push(bundle_project_file(
            root,
            &entry.entrypoint,
            Some(entry.source_digest.clone()),
            false,
        )?);
        files.push(bundle_project_file(
            root,
            &entry.plan_path,
            Some(crate::digest::sha256(
                &validated.plan.encode_pretty().map_err(Failure::internal)?,
            )),
            false,
        )?);
        files.push(bundle_project_file(
            root,
            &entry.evidence_path,
            Some(crate::digest::sha256(
                &validated
                    .evidence
                    .encode_pretty()
                    .map_err(Failure::internal)?,
            )),
            false,
        )?);
    }
    let scenarios = crate::project::project_directory_path(root, ".deshell/scenarios")
        .map_err(Failure::invalid)?;
    let entries = std::fs::read_dir(&scenarios).map_err(|error| {
        Failure::io(format!(
            "cannot enumerate bundle scenarios {}: {error}",
            scenarios.display()
        ))
    })?;
    let mut scenario_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| Failure::io(format!("cannot read scenario: {error}")))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Failure::invalid("scenario filename is not valid UTF-8"))?;
        if Path::new(&name)
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            scenario_paths.push(format!(".deshell/scenarios/{name}"));
        }
    }
    scenario_paths.sort();
    for path in scenario_paths {
        files.push(bundle_project_file(root, &path, None, false)?);
    }
    let executable = std::env::current_exe()
        .map_err(|error| Failure::io(format!("cannot resolve current executable: {error}")))?;
    let metadata = executable.symlink_metadata().map_err(|error| {
        Failure::io(format!(
            "cannot inspect current executable {}: {error}",
            executable.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Failure::invalid(
            "current deshell executable is not a regular non-symlink file",
        ));
    }
    files.push(crate::exporter::BundleFile {
        archive_path: if cfg!(windows) {
            "bin/deshell.exe".into()
        } else {
            "bin/deshell".into()
        },
        source: crate::exporter::BundleSource::File(executable),
        expected_sha256: None,
        executable: true,
    });
    let mut runtime_assets = Vec::new();
    for asset in platform_assets {
        let path =
            crate::project::project_file_path(root, &asset.path).map_err(Failure::unavailable)?;
        let archive_path = format!("project/{}", asset.path);
        if asset.role == crate::config::LabAssetRole::Runtime {
            runtime_assets.push(archive_path.clone());
        }
        files.push(crate::exporter::BundleFile {
            archive_path,
            source: crate::exporter::BundleSource::File(path),
            expected_sha256: Some(asset.sha256.clone()),
            executable: asset.executable,
        });
    }
    if let Ok(artifact) = crate::exporter::export(
        plan,
        target,
        crate::exporter::Mode::Strict,
        bundle_export_runtime(lock, target),
    ) {
        files.push(crate::exporter::BundleFile {
            archive_path: format!("target/{}", artifact.filename),
            source: crate::exporter::BundleSource::Bytes(artifact.content),
            expected_sha256: None,
            executable: false,
        });
    }
    files.push(crate::exporter::BundleFile {
        archive_path: "README.txt".into(),
        source: crate::exporter::BundleSource::Bytes(
            format!(
                "Verify bundle-manifest.json digests, load its locked runtime asset, then run bin/deshell run --root project --entry {entrypoint}.\n"
            )
            .into_bytes(),
        ),
        expected_sha256: None,
        executable: false,
    });
    Ok(crate::exporter::BundleRequest {
        plan,
        entrypoint,
        target,
        runtime_image: &lock.lab.image,
        runtime_assets,
        files,
    })
}

fn bundle_project_file(
    root: &Path,
    relative: &str,
    expected_sha256: Option<String>,
    executable: bool,
) -> Result<crate::exporter::BundleFile, Failure> {
    let source = crate::project::project_file_path(root, relative).map_err(Failure::invalid)?;
    Ok(crate::exporter::BundleFile {
        archive_path: format!("project/{relative}"),
        source: crate::exporter::BundleSource::File(source),
        expected_sha256,
        executable,
    })
}

fn atomic_bundle_write(
    path: &Path,
    request: crate::exporter::BundleRequest<'_>,
) -> Result<(), Failure> {
    let parent = path
        .parent()
        .ok_or_else(|| Failure::invalid("bundle output has no parent directory"))?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".deshell-bundle-")
        .tempfile_in(parent)
        .map_err(|error| Failure::io(format!("cannot create bundle output: {error}")))?;
    crate::exporter::write_bundle(request, temporary.as_file_mut()).map_err(|message| {
        if message.contains("residual") {
            Failure::policy(message)
        } else if message.starts_with("cannot write")
            || message.starts_with("cannot flush")
            || message.starts_with("cannot finish")
        {
            Failure::io(message)
        } else {
            Failure::invalid(message)
        }
    })?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| Failure::io(format!("cannot sync bundle output: {error}")))?;
    temporary.persist(path).map_err(|error| {
        Failure::io(format!(
            "cannot atomically persist bundle {}: {}",
            path.display(),
            error.error
        ))
    })?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| Failure::io(format!("cannot sync bundle directory: {error}")))?;
    Ok(())
}

fn safe_output_path(root: &Path, output: &Path) -> Result<PathBuf, Failure> {
    if output.is_absolute() {
        return Err(Failure::policy("export --output must be project-relative"));
    }
    let raw = output
        .to_str()
        .ok_or_else(|| Failure::invalid("export --output must be valid UTF-8"))?;
    let normalized = crate::ir::normalize_path(raw).map_err(Failure::policy)?;
    if normalized != raw {
        return Err(Failure::policy(
            "export --output must be a normalized project-relative path",
        ));
    }
    let root = root
        .canonicalize()
        .map_err(|error| Failure::io(format!("cannot resolve project root: {error}")))?;
    let path = root.join(raw);
    let parent = path
        .parent()
        .ok_or_else(|| Failure::policy("export output has no project-relative parent"))?;
    let relative_parent = parent
        .strip_prefix(&root)
        .map_err(|_| Failure::policy("export output escapes the project"))?;
    let mut current = root.clone();
    for component in relative_parent.components() {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => {
                return Err(Failure::policy(format!(
                    "export output parent is not a regular directory: {}",
                    current.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    Failure::io(format!("cannot create {}: {error}", current.display()))
                })?;
            }
            Err(error) => {
                return Err(Failure::io(format!(
                    "cannot inspect {}: {error}",
                    current.display()
                )));
            }
        }
        let canonical = current.canonicalize().map_err(|error| {
            Failure::io(format!("cannot resolve {}: {error}", current.display()))
        })?;
        if !canonical.starts_with(&root) {
            return Err(Failure::policy("export output parent escapes the project"));
        }
    }
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(Failure::policy("export output must not be a symlink"))
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(Failure::policy("export output must be a regular file"))
        }
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(Failure::io(format!(
            "cannot inspect export output: {error}"
        ))),
    }
}

fn select_node(
    mut plan: crate::ir::Plan,
    node_id: Option<&str>,
) -> Result<crate::ir::Plan, String> {
    let Some(node_id) = node_id else {
        return Ok(plan);
    };
    let mut selected = None;
    for (index, task) in plan.tasks.iter().enumerate() {
        if let Some(node) = find_node(&task.body, node_id) {
            selected = Some((index, node.clone()));
            break;
        }
    }
    let (owner, node) = selected.ok_or_else(|| format!("node not found: {node_id}"))?;
    plan.entrypoint = plan.tasks[owner].name.clone();
    plan.tasks[owner].body = node;
    plan.assign_node_ids()?;
    plan.validate().map_err(|errors| errors.join("; "))?;
    Ok(plan)
}

fn find_node<'a>(node: &'a crate::ir::Node, id: &str) -> Option<&'a crate::ir::Node> {
    if node.id == id {
        return Some(node);
    }
    match &node.operation {
        crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Sequence { nodes }
        | crate::ir::Operation::Parallel { nodes } => {
            nodes.iter().find_map(|node| find_node(node, id))
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => find_node(predicate, id)
            .or_else(|| find_node(if_true, id))
            .or_else(|| if_false.as_deref().and_then(|node| find_node(node, id))),
        crate::ir::Operation::Match { cases, default, .. } => cases
            .iter()
            .find_map(|case| find_node(&case.body, id))
            .or_else(|| default.as_deref().and_then(|node| find_node(node, id))),
        crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. } => find_node(body, id),
        crate::ir::Operation::TryFinally { body, finalizer } => {
            find_node(body, id).or_else(|| find_node(finalizer, id))
        }
        _ => None,
    }
}

fn explain(root: &Path, node_id: Option<&str>, stdout: &mut dyn Write) -> Result<i32, Failure> {
    let (plan, _) = crate::project::load_artifacts(root).map_err(classify_project_errors)?;
    let nodes: Vec<_> = plan
        .tasks
        .iter()
        .flat_map(|task| {
            let mut values = Vec::new();
            collect_nodes(&task.body, &mut values);
            values
        })
        .collect();
    match node_id {
        None => {
            writeln_io(stdout, format_args!("entrypoint: {}", plan.entrypoint))?;
            writeln_io(stdout, format_args!("tasks: {}", plan.tasks.len()))?;
            writeln_io(stdout, format_args!("nodes: {}", nodes.len()))?;
        }
        Some(id) => {
            let node = nodes
                .into_iter()
                .find(|node| node.id == id)
                .ok_or_else(|| Failure::invalid(format!("node not found: {id}")))?;
            writeln_io(stdout, format_args!("{}", node.id))?;
            let value = serde_json::to_value(&node.guarantee)
                .map_err(|error| Failure::internal(error.to_string()))?;
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
    }
    Ok(0)
}

fn collect_nodes<'a>(node: &'a crate::ir::Node, values: &mut Vec<&'a crate::ir::Node>) {
    values.push(node);
    match &node.operation {
        crate::ir::Operation::Pipeline { nodes, .. }
        | crate::ir::Operation::Sequence { nodes }
        | crate::ir::Operation::Parallel { nodes } => {
            for node in nodes {
                collect_nodes(node, values);
            }
        }
        crate::ir::Operation::Condition {
            predicate,
            if_true,
            if_false,
        } => {
            collect_nodes(predicate, values);
            collect_nodes(if_true, values);
            if let Some(node) = if_false {
                collect_nodes(node, values);
            }
        }
        crate::ir::Operation::Match { cases, default, .. } => {
            for case in cases {
                collect_nodes(&case.body, values);
            }
            if let Some(node) = default {
                collect_nodes(node, values);
            }
        }
        crate::ir::Operation::Foreach { body, .. }
        | crate::ir::Operation::CaptureStdout { body, .. } => collect_nodes(body, values),
        crate::ir::Operation::TryFinally { body, finalizer } => {
            collect_nodes(body, values);
            collect_nodes(finalizer, values);
        }
        _ => {}
    }
}

fn rewrite_command(
    root: &Path,
    entry: Option<String>,
    equivalent: bool,
    apply: bool,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    if !equivalent {
        return Err(Failure::usage("rewrite requires --equivalent"));
    }
    let entry = selected_entry(root, entry)?;
    let (_, path) = crate::project::resolve_entry(root, &entry).map_err(classify_project_error)?;
    let source = std::fs::read(&path)
        .map_err(|error| Failure::io(format!("cannot read {}: {error}", path.display())))?;
    let source = String::from_utf8(source)
        .map_err(|_| Failure::invalid(format!("rewrite source is not valid UTF-8: {entry}")))?;
    let result = crate::rewrite::equivalent(&entry, &source);
    if result.edits.is_empty() {
        writeln_io(
            stdout,
            format_args!("{entry}: no equivalent rewrite available"),
        )?;
    } else if apply {
        let config = crate::project::load_config(root).map_err(classify_project_errors)?;
        let lock = crate::project::load_lock(root).map_err(classify_project_errors)?;
        let mut preflight = crate::frontend::lower(
            &entry,
            result.output.as_bytes(),
            config.policy.unknown_interpreter,
        )
        .map_err(classify_project_error)?;
        crate::frontend::bind_interpreter_pins(&mut preflight, &lock.interpreters)
            .map_err(classify_project_error)?;
        let replacement_digest = crate::digest::sha256(result.output.as_bytes());
        crate::patch::apply_all(&[
            crate::patch::prepare(&path, result.output.into_bytes()).map_err(Failure::io)?
        ])
        .map_err(Failure::io)?;
        let analysis = match crate::project::analyze(root, &entry) {
            Ok(analysis) => analysis,
            Err(error) => {
                let rollback =
                    crate::patch::prepare_expected(&path, &replacement_digest, source.into_bytes())
                        .and_then(|proposal| crate::patch::apply_all(&[proposal]));
                if let Err(rollback) = rollback {
                    return Err(Failure::io(format!(
                        "{error}; source rollback also failed: {rollback}"
                    )));
                }
                return Err(classify_project_error(error));
            }
        };
        writeln_io(
            stdout,
            format_args!("{entry}: applied {} equivalent edit(s)", result.edits.len()),
        )?;
        writeln_io(
            stdout,
            format_args!("wrote {}", analysis.plan_path.display()),
        )?;
    } else {
        write_io(stdout, preview(&entry, &source, &result.output).as_bytes())?;
    }
    Ok(0)
}

fn modernize_command(
    root: &Path,
    profile: &str,
    apply: bool,
    diagnostic_mode: crate::diagnostics::Mode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    let profiles = parse_profiles(profile)?;
    let inventory = crate::project::scan(root).map_err(Failure::io)?;
    if !inventory.errors.is_empty() || !inventory.skipped.is_empty() {
        let errors = inventory
            .errors
            .iter()
            .map(|error| {
                format!(
                    "{}:{}:{}",
                    error.path.as_deref().unwrap_or("<root>"),
                    error.stage,
                    error.message
                )
            })
            .chain(
                inventory
                    .skipped
                    .iter()
                    .map(|skipped| format!("{}:skipped:{}", skipped.path, skipped.reason)),
            )
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Failure::invalid(format!(
            "modernize requires a complete Inventory v1 scan: {errors}"
        )));
    }
    let mut paths = inventory
        .findings
        .into_iter()
        .filter(|finding| finding.kind == crate::scanner::FindingKind::ShellFile)
        .map(|finding| finding.path)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut proposals = Vec::new();
    let mut changes = Vec::new();
    for entry in paths {
        let (_, path) =
            crate::project::resolve_entry(root, &entry).map_err(classify_project_error)?;
        let source = std::fs::read_to_string(&path)
            .map_err(|error| Failure::io(format!("cannot read {}: {error}", path.display())))?;
        let result = crate::rewrite::modernize(&entry, &source, &profiles);
        for finding in result.findings {
            let mut diagnostic = crate::diagnostics::Diagnostic::warning(
                "DESHELL_MODERNIZE_FINDING",
                finding.message,
            );
            diagnostic.context.insert("path".into(), entry.clone());
            diagnostic.context.insert("rule".into(), finding.rule);
            diagnostic.context.insert(
                "span".into(),
                format!("{}..{}", finding.span.start_byte, finding.span.end_byte),
            );
            crate::diagnostics::emit(stderr, diagnostic_mode, &diagnostic)
                .map_err(|error| Failure::io(error.to_string()))?;
        }
        if result.output != source {
            proposals.push(
                crate::patch::prepare(&path, result.output.clone().into_bytes())
                    .map_err(Failure::io)?,
            );
            changes.push((entry, source, result.output, result.edits.len()));
        }
    }
    if changes.is_empty() {
        if apply {
            writeln_io(stdout, format_args!("no applicable modernization changes"))?;
        }
    } else if apply {
        let config = crate::project::load_config(root).map_err(classify_project_errors)?;
        let lock = crate::project::load_lock(root).map_err(classify_project_errors)?;
        let manifest = crate::project::load_manifest(root).map_err(classify_project_errors)?;
        let analyzed = manifest
            .entries
            .iter()
            .map(|entry| entry.entrypoint.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for (entry, _, output, _) in &changes {
            if config.entrypoints.contains(entry) || analyzed.contains(entry.as_str()) {
                let mut preflight = crate::frontend::lower(
                    entry,
                    output.as_bytes(),
                    config.policy.unknown_interpreter.clone(),
                )
                .map_err(classify_project_error)?;
                crate::frontend::bind_interpreter_pins(&mut preflight, &lock.interpreters)
                    .map_err(classify_project_error)?;
            }
        }
        let manifest_path = crate::project::project_file_path(root, ".deshell/manifest.json")
            .map_err(Failure::invalid)?;
        let manifest_before = std::fs::read(&manifest_path).map_err(|error| {
            Failure::io(format!("cannot read {}: {error}", manifest_path.display()))
        })?;
        crate::patch::apply_all(&proposals).map_err(Failure::io)?;
        let mut analyzed_paths = Vec::new();
        for (entry, _, _, _) in &changes {
            if config.entrypoints.contains(entry) || analyzed.contains(entry.as_str()) {
                match crate::project::analyze(root, entry) {
                    Ok(analysis) => analyzed_paths.push((entry.clone(), analysis.plan_path)),
                    Err(error) => {
                        let mut rollback = Vec::new();
                        for (rollback_entry, before, after, _) in &changes {
                            let (_, path) = crate::project::resolve_entry(root, rollback_entry)
                                .map_err(classify_project_error)?;
                            rollback.push(
                                crate::patch::prepare_expected(
                                    &path,
                                    &crate::digest::sha256(after.as_bytes()),
                                    before.as_bytes().to_vec(),
                                )
                                .map_err(Failure::io)?,
                            );
                        }
                        let manifest_current =
                            std::fs::read(&manifest_path).map_err(|read_error| {
                                Failure::io(format!(
                                    "{error}; cannot read manifest for rollback: {read_error}"
                                ))
                            })?;
                        if manifest_current != manifest_before {
                            rollback.push(
                                crate::patch::prepare_expected(
                                    &manifest_path,
                                    &crate::digest::sha256(&manifest_current),
                                    manifest_before.clone(),
                                )
                                .map_err(Failure::io)?,
                            );
                        }
                        if let Err(rollback_error) = crate::patch::apply_all(&rollback) {
                            return Err(Failure::io(format!(
                                "{error}; modernization rollback also failed: {rollback_error}"
                            )));
                        }
                        return Err(classify_project_error(error));
                    }
                }
            }
        }
        for (entry, _, _, count) in &changes {
            writeln_io(
                stdout,
                format_args!("{entry}: applied {count} modernization edit(s)"),
            )?;
            if let Some((_, plan_path)) = analyzed_paths.iter().find(|(value, _)| value == entry) {
                writeln_io(stdout, format_args!("wrote {}", plan_path.display()))?;
            }
        }
    } else {
        for (entry, before, after, _) in changes {
            write_io(stdout, preview(&entry, &before, &after).as_bytes())?;
        }
    }
    Ok(0)
}

fn parse_profiles(value: &str) -> Result<Vec<crate::rewrite::Profile>, Failure> {
    let mut output = Vec::new();
    for name in value.split(',').map(str::trim) {
        let profile = match name {
            "portable" => crate::rewrite::Profile::Portable,
            "secure" => crate::rewrite::Profile::Secure,
            "reproducible" => crate::rewrite::Profile::Reproducible,
            _ => {
                return Err(Failure::usage(format!(
                    "unknown modernization profile: {name}"
                )));
            }
        };
        if !output.contains(&profile) {
            output.push(profile);
        }
    }
    if output.is_empty() {
        Err(Failure::usage(
            "at least one modernization profile is required",
        ))
    } else {
        Ok(output)
    }
}

fn migrate_command(
    root: &Path,
    entry: Option<String>,
    observe: bool,
    target: ExportTarget,
    apply: bool,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let entry = selected_entry(root, entry)?;
    let analysis = crate::project::analyze(root, &entry).map_err(classify_project_error)?;
    let mut observation_summary = Vec::new();
    if observe {
        match observe_command(root, Some(entry.clone()), &[], &mut observation_summary)? {
            0 => {}
            5 => {
                return Err(Failure::difference(
                    "migration stopped because observation differs or is nondeterministic",
                ));
            }
            6 => return Err(Failure::unavailable("observation provider is unavailable")),
            _ => return Err(Failure::io("observation provider failed")),
        }
    }
    let artifact = crate::exporter::export(
        &analysis.plan,
        exporter_target(target),
        crate::exporter::Mode::Strict,
        export_runtime(
            &crate::project::load_lock(root).map_err(classify_project_errors)?,
            target,
        ),
    )
    .map_err(Failure::policy)?;
    if apply || target == ExportTarget::Internal {
        write_io(stdout, &observation_summary)?;
    }
    if target == ExportTarget::Internal {
        writeln_io(
            stdout,
            format_args!("plan: {}", analysis.plan_path.display()),
        )?;
    } else if apply {
        let directory = root.join(".deshell/export");
        ensure_output_directory(&directory)?;
        let path = directory.join(&artifact.filename);
        atomic_write(&path, artifact.content)?;
        writeln_io(stdout, format_args!("wrote {}", path.display()))?;
    } else {
        write_io(stdout, &artifact.content)?;
    }
    Ok(0)
}

fn preview(path: &str, before: &str, after: &str) -> String {
    #[derive(Clone, Copy)]
    enum Change<'a> {
        Equal(&'a str),
        Remove(&'a str),
        Add(&'a str),
    }
    fn lines(value: &str) -> Vec<&str> {
        if value.is_empty() {
            Vec::new()
        } else {
            value.split_inclusive('\n').collect()
        }
    }
    let left = lines(before);
    let right = lines(after);
    let mut changes = Vec::new();
    if left.len().saturating_mul(right.len()) <= 4_000_000 {
        let mut table = vec![vec![0_u32; right.len() + 1]; left.len() + 1];
        for left_index in (0..left.len()).rev() {
            for right_index in (0..right.len()).rev() {
                table[left_index][right_index] = if left[left_index] == right[right_index] {
                    table[left_index + 1][right_index + 1] + 1
                } else {
                    table[left_index + 1][right_index].max(table[left_index][right_index + 1])
                };
            }
        }
        let (mut left_index, mut right_index) = (0, 0);
        while left_index < left.len() || right_index < right.len() {
            if left_index < left.len()
                && right_index < right.len()
                && left[left_index] == right[right_index]
            {
                changes.push(Change::Equal(left[left_index]));
                left_index += 1;
                right_index += 1;
            } else if right_index < right.len()
                && (left_index == left.len()
                    || table[left_index][right_index + 1] >= table[left_index + 1][right_index])
            {
                changes.push(Change::Add(right[right_index]));
                right_index += 1;
            } else {
                changes.push(Change::Remove(left[left_index]));
                left_index += 1;
            }
        }
    } else {
        let prefix = left
            .iter()
            .zip(&right)
            .take_while(|(left, right)| left == right)
            .count();
        let suffix = left[prefix..]
            .iter()
            .rev()
            .zip(right[prefix..].iter().rev())
            .take_while(|(left, right)| left == right)
            .count();
        changes.extend(left[..prefix].iter().map(|line| Change::Equal(line)));
        changes.extend(
            left[prefix..left.len() - suffix]
                .iter()
                .map(|line| Change::Remove(line)),
        );
        changes.extend(
            right[prefix..right.len() - suffix]
                .iter()
                .map(|line| Change::Add(line)),
        );
        changes.extend(
            left[left.len() - suffix..]
                .iter()
                .map(|line| Change::Equal(line)),
        );
    }
    let changed = changes
        .iter()
        .enumerate()
        .filter_map(|(index, change)| (!matches!(change, Change::Equal(_))).then_some(index))
        .collect::<Vec<_>>();
    let mut output = format!("--- a/{path}\n+++ b/{path}\n");
    let mut groups = Vec::new();
    for index in changed {
        let start = index.saturating_sub(3);
        let end = (index + 4).min(changes.len());
        if let Some((_, previous_end)) = groups.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            groups.push((start, end));
        }
    }
    for (start, end) in groups {
        let old_start = 1 + changes[..start]
            .iter()
            .filter(|change| !matches!(change, Change::Add(_)))
            .count();
        let new_start = 1 + changes[..start]
            .iter()
            .filter(|change| !matches!(change, Change::Remove(_)))
            .count();
        let old_count = changes[start..end]
            .iter()
            .filter(|change| !matches!(change, Change::Add(_)))
            .count();
        let new_count = changes[start..end]
            .iter()
            .filter(|change| !matches!(change, Change::Remove(_)))
            .count();
        output.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for change in &changes[start..end] {
            let (prefix, line) = match change {
                Change::Equal(line) => (' ', *line),
                Change::Remove(line) => ('-', *line),
                Change::Add(line) => ('+', *line),
            };
            output.push(prefix);
            output.push_str(line);
            if !line.ends_with('\n') {
                output.push('\n');
                output.push_str("\\ No newline at end of file\n");
            }
        }
    }
    output
}

fn ensure_output_directory(path: &Path) -> Result<(), Failure> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(Failure::io(format!(
            "output path is not a regular directory: {}",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(path)
            .map_err(|error| Failure::io(format!("cannot create {}: {error}", path.display()))),
        Err(error) => Err(Failure::io(format!(
            "cannot inspect {}: {error}",
            path.display()
        ))),
    }
}

fn atomic_write(path: &Path, contents: Vec<u8>) -> Result<(), Failure> {
    let proposal = match path.symlink_metadata() {
        Ok(_) => crate::patch::prepare(path, contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::patch::prepare_create(path, contents, 0o644)
        }
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
    .map_err(Failure::io)?;
    crate::patch::apply_all(&[proposal]).map_err(Failure::io)
}

fn write_io(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), Failure> {
    writer
        .write_all(bytes)
        .map_err(|error| Failure::io(error.to_string()))
}

fn writeln_io(writer: &mut dyn Write, arguments: std::fmt::Arguments<'_>) -> Result<(), Failure> {
    writer
        .write_fmt(arguments)
        .and_then(|()| writer.write_all(b"\n"))
        .map_err(|error| Failure::io(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use std::path::Path;

    fn invoke(args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_from(args.iter().copied(), &mut stdout, &mut stderr);
        (code, stdout, stderr)
    }

    fn invoke_owned(args: Vec<String>) -> (i32, Vec<u8>, Vec<u8>) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_from(args, &mut stdout, &mut stderr);
        (code, stdout, stderr)
    }

    fn path(root: &Path) -> String {
        root.to_string_lossy().into_owned()
    }

    fn configure(root: &Path, entry: &str, source: &[u8]) {
        std::fs::write(root.join(entry), source).unwrap();
        let config = ProjectConfig::default_text()
            .replace("entrypoints = []", &format!("entrypoints = [\"{entry}\"]"));
        std::fs::write(root.join(".deshell/project.toml"), config).unwrap();
    }

    fn enable_local(root: &Path) {
        let path = root.join(".deshell/project.toml");
        let config = std::fs::read_to_string(&path)
            .unwrap()
            .replace("allow_local = false", "allow_local = true");
        std::fs::write(path, config).unwrap();
    }

    fn enable_bundle(root: &Path) {
        let runtime_path = root.join(".deshell/runtime/lab.asset");
        std::fs::create_dir_all(runtime_path.parent().unwrap()).unwrap();
        std::fs::write(&runtime_path, b"pinned runtime asset").unwrap();
        let config_path = root.join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("mode = \"strict\"", "mode = \"bundle\"");
        std::fs::write(config_path, config).unwrap();
        let image = concat!(
            "ghcr.io/deshell-lang/lab@sha256:",
            "14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce"
        );
        let asset_pin = crate::digest::sha256(b"pinned runtime asset");
        let lock_path = root.join("deshell.lock");
        let lock = std::fs::read_to_string(&lock_path)
            .unwrap()
            .replace("image = \"unconfigured\"", &format!("image = \"{image}\""))
            .replace(
                "assets = []",
                &format!(
                    "assets = [{{ name = \"lab.asset\", role = \"runtime\", operating_system = \"{}\", architecture = \"{}\", path = \".deshell/runtime/lab.asset\", sha256 = \"sha256:{asset_pin}\", executable = false }}]",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            );
        std::fs::write(lock_path, lock).unwrap();
    }

    #[test]
    fn version_and_help_expose_the_rust_multicall_cli() {
        let (code, stdout, stderr) = invoke(&["deshell", "--version"]);
        assert_eq!(
            (code, stdout, stderr),
            (0, b"deshell 0.1.0\n".to_vec(), vec![])
        );
        let (code, stdout, stderr) = invoke(&["deshell", "--help"]);
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        let help = String::from_utf8(stdout).unwrap();
        for command in [
            "init",
            "scan",
            "analyze",
            "rewrite",
            "modernize",
            "migrate",
            "verify",
            "observe",
            "doctor",
            "run",
            "export",
            "check",
            "explain",
            "schema",
        ] {
            assert!(help.contains(command), "help omitted {command}");
        }
    }

    #[test]
    fn embedded_schema_stdout_is_identical_in_both_diagnostic_modes() {
        let human = invoke(&["deshell", "schema", "effect-ir"]);
        let jsonl = invoke(&["deshell", "--diagnostics", "jsonl", "schema", "effect-ir"]);
        assert_eq!(human.0, 0);
        assert_eq!(
            human.1,
            include_bytes!("../../../contracts/schema/effect-ir-v1.schema.json")
        );
        assert_eq!(human.1, jsonl.1);
        assert!(human.2.is_empty() && jsonl.2.is_empty());
    }

    #[test]
    fn every_named_v1_schema_is_embedded_byte_for_byte() {
        for name in [
            "inventory",
            "manifest",
            "bundle",
            "effect-ir",
            "evidence",
            "diagnostic",
            "protocol",
            "project",
            "scenario",
            "lock",
            "replay",
            "corpus-audit",
        ] {
            let (code, stdout, stderr) = invoke(&["deshell", "schema", name]);
            assert_eq!(code, 0, "{name}: {}", String::from_utf8_lossy(&stderr));
            assert!(stderr.is_empty(), "{name}");
            assert_eq!(
                stdout,
                std::fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join(format!("contracts/schema/{name}-v1.schema.json"))
                )
                .unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn usage_errors_are_exit_two_and_jsonl_stays_on_stderr() {
        let (code, stdout, stderr) =
            invoke(&["deshell", "schema", "unknown", "--diagnostics=jsonl"]);
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        let diagnostic: serde_json::Value = crate::strict_json::parse(&stderr).unwrap();
        assert_eq!(diagnostic["code"], "DESHELL_USAGE");
        assert_eq!(diagnostic["severity"], "error");
    }

    #[test]
    fn init_analyze_and_check_form_a_complete_v1_lifecycle() {
        let directory = tempfile::tempdir().unwrap();
        let root = path(directory.path());
        assert_eq!(
            invoke_owned(vec![
                "deshell".into(),
                "init".into(),
                "--root".into(),
                root.clone()
            ])
            .0,
            0
        );
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf '%s' \"$NAME\"\n",
        );
        let analyzed = invoke_owned(vec![
            "deshell".into(),
            "analyze".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(analyzed.0, 0, "{}", String::from_utf8_lossy(&analyzed.2));
        let manifest = crate::project::load_manifest(directory.path()).unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert!(
            directory
                .path()
                .join(&manifest.entries[0].plan_path)
                .is_file()
        );
        assert!(
            directory
                .path()
                .join(&manifest.entries[0].evidence_path)
                .is_file()
        );
        let checked = invoke_owned(vec![
            "deshell".into(),
            "check".into(),
            "--root".into(),
            root,
        ]);
        assert_eq!(checked.0, 0, "{}", String::from_utf8_lossy(&checked.2));
        assert!(checked.2.is_empty());
    }

    #[test]
    fn failures_use_the_fixed_io_invalid_and_policy_categories() {
        let missing = tempfile::tempdir().unwrap().path().join("gone");
        let checked = invoke_owned(vec![
            "deshell".into(),
            "check".into(),
            "--root".into(),
            path(&missing),
        ]);
        assert_eq!(checked.0, 1);
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join(".deshell/project.toml"),
            "version = 99\n",
        )
        .unwrap();
        let invalid = invoke_owned(vec![
            "deshell".into(),
            "check".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(invalid.0, 3);

        let residual = tempfile::tempdir().unwrap();
        crate::project::init(residual.path()).unwrap();
        configure(residual.path(), "build.sh", b"eval 'printf bad'\n");
        enable_local(residual.path());
        crate::project::analyze(residual.path(), "build.sh").unwrap();
        let denied = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            path(residual.path()),
            "--backend".into(),
            "local".into(),
        ]);
        assert_eq!(denied.0, 4);
        assert!(denied.1.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn run_returns_the_plan_exit_code_after_execution_starts() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"sh -c 'printf run-output; exit 7'\n",
        );
        enable_local(directory.path());
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let (code, stdout, stderr) = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            path(directory.path()),
            "--backend".into(),
            "local".into(),
        ]);
        assert_eq!(code, 7);
        assert_eq!(stdout, b"run-output");
        assert!(stderr.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn run_keeps_the_plan_exit_code_when_output_sinks_close_after_start() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "closed",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"sh -c 'printf output; printf error >&2; exit 7'\n",
        );
        enable_local(directory.path());
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let args = vec![
            "deshell".to_owned(),
            "run".to_owned(),
            "--root".to_owned(),
            path(directory.path()),
            "--backend".to_owned(),
            "local".to_owned(),
        ];
        let mut closed_stdout = Closed;
        let mut closed_stderr = Closed;
        assert_eq!(
            run_from(args.clone(), &mut closed_stdout, &mut Vec::new()),
            7
        );
        assert_eq!(run_from(args, &mut Vec::new(), &mut closed_stderr), 7);
    }

    #[test]
    fn scan_json_and_export_are_stdout_artifacts_only() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        let root = path(directory.path());
        let scan = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(scan.0, 0);
        assert!(scan.2.is_empty());
        let inventory: serde_json::Value = crate::strict_json::parse(&scan.1).unwrap();
        assert_eq!(inventory["schema_version"], 1);
        assert_eq!(inventory["findings"][0]["path"], "build.sh");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let exported = invoke_owned(vec![
            "deshell".into(),
            "export".into(),
            "--root".into(),
            root,
            "--target".into(),
            "cwl".into(),
        ]);
        assert_eq!(exported.0, 0, "{}", String::from_utf8_lossy(&exported.2));
        assert!(exported.2.is_empty());
        let cwl: serde_json::Value = crate::strict_json::parse(&exported.1).unwrap();
        assert_eq!(cwl["cwlVersion"], "v1.2");
    }

    #[test]
    fn disposable_is_default_local_is_double_opt_in_and_doctor_is_machine_readable() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let root = path(directory.path());
        let default_run = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(default_run.0, 6);
        let local = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            root.clone(),
            "--backend".into(),
            "local".into(),
        ]);
        assert_eq!(local.0, 4);
        let doctor = invoke_owned(vec![
            "deshell".into(),
            "doctor".into(),
            "--root".into(),
            root,
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(doctor.0, 6);
        assert!(doctor.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&doctor.1).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["ready"], false);
        assert_eq!(report["lab_image"]["valid"], false);
    }

    #[test]
    fn unavailable_observation_is_keyed_and_persisted_as_evidence() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let observed = invoke_owned(vec![
            "deshell".into(),
            "observe".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(observed.0, 6);
        assert!(observed.2.is_empty());
        assert!(
            String::from_utf8(observed.1)
                .unwrap()
                .contains("default: unavailable")
        );
        let (_, evidence) =
            crate::project::load_entry_artifacts(directory.path(), "build.sh").unwrap();
        assert_eq!(evidence.observations.len(), 1);
        assert_eq!(
            evidence.observations[0].status,
            crate::evidence::ObservationStatus::Unavailable
        );
        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(verified.0, 6);
        assert!(
            String::from_utf8(verified.1)
                .unwrap()
                .contains("unavailable=1")
        );
    }

    #[test]
    fn export_output_rejects_absolute_traversal_and_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let root = path(directory.path());
        for output in ["../escape.json", "/absolute.json"] {
            let exported = invoke_owned(vec![
                "deshell".into(),
                "export".into(),
                "--root".into(),
                root.clone(),
                "--target".into(),
                "internal".into(),
                "--output".into(),
                output.into(),
            ]);
            assert_eq!(exported.0, 4, "{}", String::from_utf8_lossy(&exported.2));
        }
        #[cfg(unix)]
        {
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::os::unix::fs::symlink(outside.path(), directory.path().join("output.json"))
                .unwrap();
            let exported = invoke_owned(vec![
                "deshell".into(),
                "export".into(),
                "--root".into(),
                root,
                "--target".into(),
                "internal".into(),
                "--output".into(),
                "output.json".into(),
            ]);
            assert_eq!(exported.0, 4);
        }
    }

    #[test]
    fn bundle_export_requires_locked_assets_and_writes_a_self_contained_tar() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("build.sh"),
            b"/usr/bin/printf hello\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("verify.sh"),
            b"/usr/bin/printf second-entry\n",
        )
        .unwrap();
        let project_path = directory.path().join(".deshell/project.toml");
        let project = ProjectConfig::default_text().replace(
            "entrypoints = []",
            "entrypoints = [\"build.sh\", \"verify.sh\"]",
        );
        std::fs::write(project_path, project).unwrap();
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        crate::project::analyze(directory.path(), "verify.sh").unwrap();
        let root = path(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("mode = \"strict\"", "mode = \"bundle\"");
        std::fs::write(config_path, config).unwrap();
        let unavailable = invoke_owned(vec![
            "deshell".into(),
            "export".into(),
            "--root".into(),
            root.clone(),
            "--entry".into(),
            "build.sh".into(),
            "--target".into(),
            "internal".into(),
            "--mode".into(),
            "bundle".into(),
            "--output".into(),
            "before.tar".into(),
        ]);
        assert_eq!(unavailable.0, 6);
        assert!(unavailable.1.is_empty());
        assert!(!directory.path().join("before.tar").exists());

        enable_bundle(directory.path());
        std::fs::write(
            directory.path().join(".deshell/runtime/lab.asset"),
            b"tampered runtime asset",
        )
        .unwrap();
        let tampered = invoke_owned(vec![
            "deshell".into(),
            "export".into(),
            "--root".into(),
            root.clone(),
            "--entry".into(),
            "build.sh".into(),
            "--target".into(),
            "internal".into(),
            "--mode".into(),
            "bundle".into(),
            "--output".into(),
            "tampered.tar".into(),
        ]);
        assert_eq!(tampered.0, 3);
        assert!(!directory.path().join("tampered.tar").exists());
        std::fs::write(
            directory.path().join(".deshell/runtime/lab.asset"),
            b"pinned runtime asset",
        )
        .unwrap();
        let bundled = invoke_owned(vec![
            "deshell".into(),
            "export".into(),
            "--root".into(),
            root,
            "--entry".into(),
            "build.sh".into(),
            "--target".into(),
            "internal".into(),
            "--mode".into(),
            "bundle".into(),
            "--output".into(),
            "release/deshell-bundle.tar".into(),
        ]);
        assert_eq!(bundled.0, 0, "{}", String::from_utf8_lossy(&bundled.2));
        assert!(bundled.2.is_empty());
        let archive = std::fs::read(directory.path().join("release/deshell-bundle.tar")).unwrap();
        assert!(archive.starts_with(b"deshell-bundle/"));
        assert!(
            archive
                .windows(b"deshell-bundle/project/verify.sh".len())
                .any(|window| window == b"deshell-bundle/project/verify.sh")
        );
        assert!(
            archive
                .windows(b"second-entry".len())
                .any(|window| window == b"second-entry")
        );
        assert!(
            archive
                .windows(b"deshell-bundle-v1".len())
                .any(|window| window == b"deshell-bundle-v1")
        );
        assert!(archive.ends_with(&[0_u8; 1024]));
    }

    #[test]
    fn rewrite_is_preview_first_and_apply_is_transactional() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"echo `printf hi`\n");
        let root = path(directory.path());
        let missing_guard = invoke_owned(vec![
            "deshell".into(),
            "rewrite".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(missing_guard.0, 2);
        let preview = invoke_owned(vec![
            "deshell".into(),
            "rewrite".into(),
            "--root".into(),
            root.clone(),
            "--equivalent".into(),
        ]);
        assert_eq!(preview.0, 0);
        let preview_bytes = String::from_utf8(preview.1).unwrap();
        assert!(preview_bytes.contains("+echo $(printf hi)"));
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"echo `printf hi`\n"
        );
        let applied = invoke_owned(vec![
            "deshell".into(),
            "rewrite".into(),
            "--root".into(),
            root,
            "--equivalent".into(),
            "--apply".into(),
        ]);
        assert_eq!(applied.0, 0);
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"echo $(printf hi)\n"
        );
    }

    #[test]
    fn modernize_requires_named_profiles_and_never_applies_a_preview() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"#!/bin/sh\necho hello\n");
        let root = path(directory.path());
        let unknown = invoke_owned(vec![
            "deshell".into(),
            "modernize".into(),
            "--root".into(),
            root.clone(),
            "--profile".into(),
            "future".into(),
        ]);
        assert_eq!(unknown.0, 2);
        let preview = invoke_owned(vec![
            "deshell".into(),
            "modernize".into(),
            "--root".into(),
            root.clone(),
            "--profile".into(),
            "secure".into(),
        ]);
        assert_eq!(preview.0, 0);
        assert!(preview.1.starts_with(b"--- a/build.sh\n+++ b/build.sh\n"));
        assert!(String::from_utf8_lossy(&preview.2).contains("DESHELL_MODERNIZE_FINDING"));
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"#!/bin/sh\necho hello\n"
        );
        let jsonl = invoke_owned(vec![
            "deshell".into(),
            "--diagnostics=jsonl".into(),
            "modernize".into(),
            "--root".into(),
            root.clone(),
            "--profile".into(),
            "secure".into(),
        ]);
        assert_eq!(jsonl.0, 0);
        assert!(jsonl.1.starts_with(b"--- a/build.sh\n+++ b/build.sh\n"));
        let diagnostic: serde_json::Value = crate::strict_json::parse(&jsonl.2).unwrap();
        assert_eq!(diagnostic["severity"], "warning");
        assert_eq!(diagnostic["code"], "DESHELL_MODERNIZE_FINDING");
        let applied = invoke_owned(vec![
            "deshell".into(),
            "modernize".into(),
            "--root".into(),
            root,
            "--profile".into(),
            "secure".into(),
            "--apply".into(),
        ]);
        assert_eq!(applied.0, 0);
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"#!/bin/sh\nset -eu\necho hello\n"
        );
    }

    #[test]
    fn modernize_rolls_back_sources_when_reanalysis_cannot_commit() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = b"#!/bin/sh\necho hello\n";
        configure(directory.path(), "build.sh", source);
        let manifest_path = directory.path().join(".deshell/manifest.json");
        let manifest_before = std::fs::read(&manifest_path).unwrap();
        std::fs::write(directory.path().join(".deshell/artifacts"), b"blocked").unwrap();

        let result = invoke_owned(vec![
            "deshell".into(),
            "modernize".into(),
            "--root".into(),
            path(directory.path()),
            "--profile".into(),
            "secure".into(),
            "--apply".into(),
        ]);

        assert_ne!(result.0, 0);
        assert!(!String::from_utf8(result.1).unwrap().contains(": applied "));
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            source
        );
        assert_eq!(std::fs::read(manifest_path).unwrap(), manifest_before);
    }

    #[test]
    fn verify_reports_unobserved_scenarios_and_explain_reports_static_guarantees() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let root = path(directory.path());
        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(verified.0, 3);
        let report = String::from_utf8(verified.1).unwrap();
        assert!(report.contains("native=1"));
        assert!(report.contains("delegated=0"));
        assert!(report.contains("observations=0"));
        assert!(report.contains("unobserved=1"));
        assert!(!report.contains("differential="));
        let explained = invoke_owned(vec![
            "deshell".into(),
            "explain".into(),
            "--root".into(),
            root,
        ]);
        assert_eq!(explained.0, 0);
        assert!(String::from_utf8(explained.1).unwrap().contains("nodes: 1"));
    }

    #[test]
    fn verify_uses_exit_five_for_recorded_differences() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        let analysis = crate::project::analyze(directory.path(), "build.sh").unwrap();
        let mut evidence = analysis.evidence;
        let scenario = crate::config::Scenario::decode(
            &std::fs::read_to_string(directory.path().join(".deshell/scenarios/default.toml"))
                .unwrap(),
        )
        .unwrap();
        let lock_bytes = std::fs::read(directory.path().join("deshell.lock")).unwrap();
        let lock = crate::project::load_lock(directory.path()).unwrap();
        let provider = "unavailable";
        evidence
            .append_observation(crate::evidence::ObservationEvidence {
                scenario: "default".into(),
                key: crate::evidence::ObservationKey {
                    scenario_digest: scenario.digest().unwrap(),
                    provider_fingerprint: crate::digest::sha256(
                        format!("deshell-provider-v1:{provider}:{}", lock.lab.image).as_bytes(),
                    ),
                    runtime_lock_digest: crate::digest::sha256(&lock_bytes),
                },
                status: crate::evidence::ObservationStatus::Different,
                provider: provider.into(),
                reason: Some("stdout differs".into()),
                digest: Some("a".repeat(64)),
            })
            .unwrap();
        crate::project::save_evidence(directory.path(), &evidence).unwrap();
        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(verified.0, 5);
        assert!(verified.1.starts_with(b"native="));
        assert!(!verified.2.is_empty());
    }

    #[test]
    fn verify_ignores_stale_observations_once_every_current_scenario_is_observed() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        let analysis = crate::project::analyze(directory.path(), "build.sh").unwrap();
        let mut evidence = analysis.evidence;
        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let old =
            crate::config::Scenario::decode(&std::fs::read_to_string(&scenario_path).unwrap())
                .unwrap();
        let lock_bytes = std::fs::read(directory.path().join("deshell.lock")).unwrap();
        let lock = crate::project::load_lock(directory.path()).unwrap();
        let provider = "unavailable";
        let fingerprint = crate::digest::sha256(
            format!("deshell-provider-v1:{provider}:{}", lock.lab.image).as_bytes(),
        );
        let runtime_lock_digest = crate::digest::sha256(&lock_bytes);
        let observation =
            |scenario: &crate::config::Scenario| crate::evidence::ObservationEvidence {
                scenario: scenario.name.clone(),
                key: crate::evidence::ObservationKey {
                    scenario_digest: scenario.digest().unwrap(),
                    provider_fingerprint: fingerprint.clone(),
                    runtime_lock_digest: runtime_lock_digest.clone(),
                },
                status: crate::evidence::ObservationStatus::Verified,
                provider: provider.into(),
                reason: None,
                digest: Some(crate::digest::sha256(b"result")),
            };
        evidence.append_observation(observation(&old)).unwrap();
        let updated_text = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("argv = []", "argv = [\"current\"]");
        std::fs::write(&scenario_path, &updated_text).unwrap();
        let current = crate::config::Scenario::decode(&updated_text).unwrap();
        evidence.append_observation(observation(&current)).unwrap();
        crate::project::save_evidence(directory.path(), &evidence).unwrap();

        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let report = String::from_utf8(verified.1).unwrap();
        assert!(report.contains("stale=1"), "{report}");
        assert!(report.contains("unobserved=0"), "{report}");
        assert!(report.contains("verified=1"), "{report}");
    }

    #[test]
    fn migrate_reports_unavailable_observer_and_writes_exports_only_with_apply() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        let root = path(directory.path());
        let unavailable = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "--root".into(),
            root.clone(),
            "--target".into(),
            "cwl".into(),
            "--observe".into(),
        ]);
        assert_eq!(unavailable.0, 6);
        assert!(unavailable.1.is_empty());
        assert!(
            !directory
                .path()
                .join(".deshell/export/deshell.cwl")
                .exists()
        );
        let (_, evidence) = crate::project::load_artifacts(directory.path()).unwrap();
        assert_eq!(evidence.observations.len(), 1);
        assert_eq!(
            evidence.observations[0].status,
            crate::evidence::ObservationStatus::Unavailable
        );
        let preview = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "--root".into(),
            root.clone(),
            "--target".into(),
            "cwl".into(),
        ]);
        assert_eq!(preview.0, 0);
        let preview_bytes = String::from_utf8(preview.1).unwrap();
        let preview_document: serde_json::Value = serde_json::from_str(&preview_bytes).unwrap();
        assert_eq!(preview_document["cwlVersion"], "v1.2");
        assert!(
            !directory
                .path()
                .join(".deshell/export/deshell.cwl")
                .exists()
        );
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "--root".into(),
            root,
            "--target".into(),
            "cwl".into(),
            "--apply".into(),
        ]);
        assert_eq!(applied.0, 0);
        assert!(
            directory
                .path()
                .join(".deshell/export/deshell.cwl")
                .is_file()
        );
    }
}
