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
        entry: Option<String>,
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
    },
    /// Run the canonical Effect IR plan.
    Run {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        node: Option<String>,
        #[arg(long)]
        allow_residual: bool,
        #[arg(long)]
        allow_file_read: bool,
        #[arg(long)]
        allow_file_write: bool,
        #[arg(long)]
        allow_network: bool,
        #[arg(long = "arg", allow_hyphen_values = true)]
        arguments: Vec<String>,
        #[arg(last = true, allow_hyphen_values = true)]
        trailing: Vec<String>,
    },
    /// Export the canonical plan without dropping effects.
    Export {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum)]
        target: ExportTarget,
        #[arg(long)]
        bridge: bool,
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
enum ExportTarget {
    Internal,
    Dagger,
    #[value(name = "nu")]
    Nushell,
    Cwl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum SchemaName {
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
    match dispatch(cli.command, stdout, stderr) {
        Ok(code) => code,
        Err(failure) => {
            let diagnostic = crate::diagnostics::Diagnostic::error(failure.code, failure.message);
            if crate::diagnostics::emit(stderr, cli.diagnostics, &diagnostic).is_err() {
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
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    match command {
        Command::Init { root } => {
            let result = crate::project::init(&root).map_err(Failure::io)?;
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
            Ok(0)
        }
        Command::Scan { root, format } => {
            let findings = crate::project::scan(&root).map_err(Failure::io)?;
            match format {
                OutputFormat::Json => {
                    let value = serde_json::to_value(&findings)
                        .map_err(|error| Failure::internal(error.to_string()))?;
                    write_io(
                        stdout,
                        &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
                    )?;
                }
                OutputFormat::Human => {
                    for finding in &findings {
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
                        format_args!("{} shell location(s) found", findings.len()),
                    )?;
                }
            }
            Ok(0)
        }
        Command::Analyze { root, entry } => {
            let entry = selected_entry(&root, entry)?;
            let result = crate::project::analyze(&root, &entry).map_err(classify_project_error)?;
            writeln_io(stdout, format_args!("wrote {}", result.plan_path.display()))?;
            writeln_io(
                stdout,
                format_args!("wrote {}", result.evidence_path.display()),
            )?;
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
        Command::Verify { root } => {
            let (plan, evidence) =
                crate::project::load_artifacts(&root).map_err(classify_project_errors)?;
            let report = crate::verify::audit(&plan, Some(&evidence))
                .map_err(|errors| Failure::invalid(errors.join("; ")))?;
            writeln_io(
                stdout,
                format_args!(
                    "formal={} exhaustive={} residual={} observations={}",
                    report.formal, report.exhaustive, report.residual, report.observations
                ),
            )?;
            for reason in report.residual_reasons {
                writeln_io(stdout, format_args!("residual: {reason}"))?;
            }
            if evidence.observations.iter().any(|observation| {
                observation.status == crate::evidence::ObservationStatus::Different
            }) {
                return Err(Failure::difference(
                    "recorded differential observation does not match the plan",
                ));
            }
            Ok(0)
        }
        Command::Run {
            root,
            node,
            allow_residual,
            allow_file_read,
            allow_file_write,
            allow_network,
            mut arguments,
            trailing,
        } => {
            arguments.extend(trailing);
            run_plan(
                RunOptions {
                    root: &root,
                    node_id: node.as_deref(),
                    policy: crate::runner::Policy {
                        allow_file_read,
                        allow_file_write,
                        allow_network,
                        allow_opaque: allow_residual,
                    },
                    arguments: &arguments,
                },
                stdout,
                stderr,
            )
        }
        Command::Export {
            root,
            target,
            bridge,
            output,
        } => {
            let (plan, _) =
                crate::project::load_artifacts(&root).map_err(classify_project_errors)?;
            let artifact = crate::exporter::export(&plan, exporter_target(target), bridge)
                .map_err(|message| {
                    if message.contains("strict exporter") || message.contains("requires exactly") {
                        Failure::policy(message)
                    } else {
                        Failure::invalid(message)
                    }
                })?;
            if let Some(output) = output {
                let path = if output.is_absolute() {
                    output
                } else {
                    root.join(output)
                };
                atomic_write(&path, artifact.content)?;
                writeln_io(stdout, format_args!("wrote {}", path.display()))?;
            } else {
                write_io(stdout, &artifact.content)?;
            }
            Ok(0)
        }
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
        } => modernize_command(&root, &profile, apply, stdout),
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

struct RunOptions<'a> {
    root: &'a Path,
    node_id: Option<&'a str>,
    policy: crate::runner::Policy,
    arguments: &'a [String],
}

fn run_plan(
    options: RunOptions<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<i32, Failure> {
    let (plan, _) =
        crate::project::load_artifacts(options.root).map_err(classify_project_errors)?;
    let plan = select_node(plan, options.node_id).map_err(Failure::invalid)?;
    let backend = crate::local_backend::LocalBackend::new(options.root).map_err(Failure::io)?;
    let mut environment = std::collections::BTreeMap::new();
    for name in plan.tasks.iter().flat_map(|task| &task.environment) {
        if let Ok(value) = std::env::var(name) {
            environment.insert(name.clone(), value);
        }
    }
    let result = crate::runner::run_plan(
        &backend,
        options.policy,
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
        crate::ir::Operation::Pipeline { nodes }
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
        crate::ir::Operation::Pipeline { nodes }
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
        crate::patch::apply_all(&[
            crate::patch::prepare(&path, result.output.into_bytes()).map_err(Failure::io)?
        ])
        .map_err(Failure::io)?;
        writeln_io(
            stdout,
            format_args!("{entry}: applied {} equivalent edit(s)", result.edits.len()),
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
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let profiles = parse_profiles(profile)?;
    let findings = crate::project::scan(root).map_err(Failure::io)?;
    let mut paths = findings
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
            writeln_io(
                stdout,
                format_args!("{}: {}: {}", entry, finding.rule, finding.message),
            )?;
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
        writeln_io(
            stdout,
            format_args!("modernization proposal: no applicable changes"),
        )?;
    } else if apply {
        crate::patch::apply_all(&proposals).map_err(Failure::io)?;
        for (entry, _, _, count) in changes {
            writeln_io(
                stdout,
                format_args!("{entry}: applied {count} modernization edit(s)"),
            )?;
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
    let mut analysis = crate::project::analyze(root, &entry).map_err(classify_project_error)?;
    if observe {
        let scenario_path = root.join(".deshell/scenarios/default.toml");
        let scenario_text = std::fs::read_to_string(&scenario_path).map_err(|error| {
            Failure::io(format!("cannot read {}: {error}", scenario_path.display()))
        })?;
        let scenario = crate::config::Scenario::decode(&scenario_text)
            .map_err(|errors| Failure::invalid(errors.join("; ")))?;
        let backend = crate::local_backend::LocalBackend::new(root).map_err(Failure::io)?;
        let observer = UnconfiguredObserver;
        let outcome = crate::differential::evaluate(
            &observer,
            &backend,
            crate::runner::Policy::default(),
            &analysis.plan,
            &scenario,
            &mut analysis.evidence,
        )
        .map_err(Failure::internal)?;
        crate::project::save_evidence(root, &analysis.evidence).map_err(classify_project_error)?;
        match outcome {
            crate::differential::Outcome::Verified => {}
            crate::differential::Outcome::Different => {
                return Err(Failure::difference(
                    "migration stopped because observed behavior differs",
                ));
            }
            crate::differential::Outcome::Unavailable => {
                return Err(Failure::unavailable(
                    "no observation provider is configured",
                ));
            }
            crate::differential::Outcome::Failed => {
                return Err(Failure::io("observation provider failed"));
            }
        }
    }
    let artifact = crate::exporter::export(&analysis.plan, exporter_target(target), false)
        .map_err(Failure::policy)?;
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

struct UnconfiguredObserver;

impl crate::differential::Observer for UnconfiguredObserver {
    fn observe(
        &self,
        _scenario: &crate::config::Scenario,
    ) -> Result<crate::runner::RunResult, crate::differential::ProviderFailure> {
        Err(crate::differential::ProviderFailure {
            kind: crate::differential::ProviderFailureKind::Unavailable,
            message: "no observation provider is configured".into(),
        })
    }

    fn name(&self) -> &str {
        "unconfigured"
    }
}

fn preview(path: &str, before: &str, after: &str) -> String {
    format!(
        "--- a/{path}\n+++ b/{path}\n-{}\n+{}\n",
        before.trim_end_matches('\n').replace('\n', "\n-"),
        after.trim_end_matches('\n').replace('\n', "\n+")
    )
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
            b"#!/bin/sh\nprintf '%s' \"$NAME\"\n",
        );
        let analyzed = invoke_owned(vec![
            "deshell".into(),
            "analyze".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(analyzed.0, 0, "{}", String::from_utf8_lossy(&analyzed.2));
        assert!(directory.path().join(".deshell/plan.json").is_file());
        assert!(directory.path().join(".deshell/evidence.json").is_file());
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
        configure(residual.path(), "build.unknown", b"dynamic syntax\n");
        crate::project::analyze(residual.path(), "build.unknown").unwrap();
        let denied = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            path(residual.path()),
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
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let (code, stdout, stderr) = invoke_owned(vec![
            "deshell".into(),
            "run".into(),
            "--root".into(),
            path(directory.path()),
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
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let args = vec![
            "deshell".to_owned(),
            "run".to_owned(),
            "--root".to_owned(),
            path(directory.path()),
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
        configure(directory.path(), "build.sh", b"printf hello\n");
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
        assert_eq!(inventory.as_array().unwrap()[0]["path"], "build.sh");
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
        assert!(
            String::from_utf8(preview.1)
                .unwrap()
                .contains("+echo $(printf hi)")
        );
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
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"#!/bin/sh\necho hello\n"
        );
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
    fn verify_and_explain_report_v1_guarantees_without_a_differential_plan_level() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"printf hello\n");
        crate::project::analyze(directory.path(), "build.sh").unwrap();
        let root = path(directory.path());
        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(verified.0, 0);
        let report = String::from_utf8(verified.1).unwrap();
        assert!(report.contains("formal=1"));
        assert!(report.contains("observations=0"));
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
        configure(directory.path(), "build.sh", b"printf hello\n");
        let analysis = crate::project::analyze(directory.path(), "build.sh").unwrap();
        let mut evidence = analysis.evidence;
        evidence
            .append_observation(crate::evidence::ObservationEvidence {
                scenarios: vec!["default".into()],
                status: crate::evidence::ObservationStatus::Different,
                provider: Some("test-provider".into()),
                reason: Some("stdout differs".into()),
                digest: Some("a".repeat(64)),
            })
            .unwrap();
        std::fs::write(
            directory.path().join(".deshell/evidence.json"),
            evidence.encode_pretty().unwrap(),
        )
        .unwrap();
        let verified = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(verified.0, 5);
        assert!(verified.1.starts_with(b"formal="));
        assert!(!verified.2.is_empty());
    }

    #[test]
    fn migrate_reports_unavailable_observer_and_writes_exports_only_with_apply() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"printf hello\n");
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
        assert!(
            !directory
                .path()
                .join(".deshell/export/deshell.cwl")
                .exists()
        );
        let evidence = crate::evidence::Evidence::decode(
            &std::fs::read(directory.path().join(".deshell/evidence.json")).unwrap(),
        )
        .unwrap();
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
