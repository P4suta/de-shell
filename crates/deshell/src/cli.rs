use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "deshell",
    version = env!("CARGO_PKG_VERSION"),
    about = "Independently verify project-native replacements with a shell retirement migration oracle.",
    after_help = "First retirement:\n  deshell init\n  deshell scenario list\n  deshell matrix list\n  deshell migrate plan\n  deshell migrate status\n  deshell verify --require shell-free\n\nReview commands print exact approval argv; migrate status prints the exact next argv."
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
    #[command(
        after_help = "Examples:\n  deshell init\n  deshell init --target rust\n  deshell init --target go --format json\n\nAuto writes nothing when standalone shell has no unique target and returns exact retry argv."
    )]
    Init {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long = "entry")]
        entries: Vec<String>,
        #[arg(long, value_enum, default_value = "auto")]
        target: InitTargetArg,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Inventory shell files, embedded shell, and candidates.
    Scan {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Diagnose shell risks without executing repository content.
    Audit {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: AuditOutputFormat,
        #[arg(long, value_enum, default_value = "pedantic")]
        persona: AuditPersona,
    },
    /// Create reviewable scenario drafts from repository call boundaries.
    #[command(
        after_help = "Review flow:\n  deshell scenario list\n  deshell scenario show --name NAME\n  deshell scenario approve --name NAME --digest sha256:REVIEW_DIGEST"
    )]
    Scenario {
        #[command(subcommand)]
        command: ScenarioCommand,
    },
    /// Review and approve platform/runtime matrix cells.
    #[command(
        after_help = "Review flow:\n  deshell matrix list\n  deshell matrix approve --cell CELL --digest sha256:REVIEW_DIGEST"
    )]
    Matrix {
        #[command(subcommand)]
        command: MatrixCommand,
    },
    /// Lower an entrypoint into canonical Effect IR.
    Analyze {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Vec<String>,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
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
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Propose explicitly behavior-changing improvements.
    Modernize {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        apply: bool,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Plan and verify intentional behavior changes separately from migration.
    Harden {
        #[command(subcommand)]
        command: HardenCommand,
    },
    /// Plan, verify, and atomically apply repository-wide shell retirement.
    #[command(
        after_help = "Retirement flow:\n  deshell migrate plan\n  deshell migrate status\n  deshell migrate verify --plan PLAN_DIGEST --cell CELL --output evidence.json\n  deshell migrate evidence import --plan PLAN_DIGEST evidence.json\n  deshell migrate apply --plan PLAN_DIGEST\n\nA blocker plan is saved and exits 4. Status uses only the active plan and prints exact next argv."
    )]
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
    /// Audit static guarantees and recorded observations.
    Verify {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long = "entry")]
        entry: Vec<String>,
        #[arg(long, value_enum)]
        require: Option<GuaranteeRequirement>,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Observe original and lowered behavior in a disposable provider.
    Observe {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        scenario: Vec<String>,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Diagnose locks, runtimes, and disposable-provider readiness.
    #[command(
        after_help = "Examples:\n  deshell doctor\n  deshell doctor --require planning\n  deshell doctor --require disposable --format json\n\nWithout --require this is an exit-0 capability report."
    )]
    Doctor {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
        #[arg(long, value_enum)]
        require: Option<DoctorRequirement>,
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
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Explain a plan or an individual guarantee.
    Explain {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        node_id: Option<String>,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
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
    #[command(hide = true, name = "__generator")]
    Generator,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AuditOutputFormat {
    Human,
    Json,
    Jsonl,
    Sarif,
    Github,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AuditPersona {
    Pedantic,
}

#[derive(Debug, Subcommand)]
enum ScenarioCommand {
    /// List scenario review digests and approval state.
    List {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Show one scenario review digest and approval state.
    Show {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Synthesize draft scenarios; write only when --apply is supplied.
    Synthesize {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        apply: bool,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Approve the exact scenario bytes shown by list/show.
    Approve {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        digest: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum MatrixCommand {
    /// List matrix review digests and approval state.
    List {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Approve the exact matrix cell shown by list.
    Approve {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        cell: String,
        #[arg(long)]
        digest: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum InitTargetArg {
    Auto,
    Rust,
    Go,
    Host,
}

#[derive(Debug, Subcommand)]
enum HardenCommand {
    Plan {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    Verify {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    Apply {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum MigrateCommand {
    /// Scan, lower, and obtain generator proposals for one repository-wide digest.
    Plan {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Verify original, Effect IR, and replacement in one approved matrix cell.
    Verify {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(long)]
        cell: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Import independently produced Evidence into a plan.
    Evidence {
        #[command(subcommand)]
        command: MigrateEvidenceCommand,
    },
    /// Atomically retire every source in a fully verified repository plan.
    Apply {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
    /// Summarize live, blocked, planned, verified, retired, and archived state.
    Status {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum MigrateEvidenceCommand {
    Import {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum GuaranteeRequirement {
    Native,
    ShellFree,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DoctorRequirement {
    Planning,
    Local,
    Disposable,
    Bundle,
    Dagger,
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
    Approval,
    #[value(name = "init-report")]
    InitReport,
    #[value(name = "scan-report")]
    ScanReport,
    #[value(name = "scenario-report")]
    ScenarioReport,
    #[value(name = "matrix-report")]
    MatrixReport,
    #[value(name = "audit-report")]
    AuditReport,
    #[value(name = "analyze-report")]
    AnalyzeReport,
    #[value(name = "check-report")]
    CheckReport,
    #[value(name = "verify-report")]
    VerifyReport,
    #[value(name = "observe-report")]
    ObserveReport,
    #[value(name = "doctor-report")]
    DoctorReport,
    #[value(name = "explain-report")]
    ExplainReport,
    #[value(name = "rewrite-report")]
    RewriteReport,
    #[value(name = "modernize-report")]
    ModernizeReport,
    #[value(name = "harden-report")]
    HardenReport,
    #[value(name = "migrate-report")]
    MigrateReport,
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
    #[value(name = "generator-protocol")]
    GeneratorProtocol,
    #[value(name = "migration-request")]
    MigrationRequest,
    Proposal,
    #[value(name = "migration-plan")]
    MigrationPlan,
    #[value(name = "migration-index")]
    MigrationIndex,
    #[value(name = "migration-evidence")]
    MigrationEvidence,
    #[value(name = "archive-manifest")]
    ArchiveManifest,
    #[value(name = "audit-finding")]
    AuditFinding,
    #[value(name = "harden-plan")]
    HardenPlan,
    #[value(name = "harden-approval")]
    HardenApproval,
    #[value(name = "harden-evidence")]
    HardenEvidence,
}

#[derive(Clone, Debug)]
struct ReportSpec {
    command: &'static str,
    format: OutputFormat,
    next_actions: Vec<crate::report::Action>,
}

impl Command {
    fn report_spec(&self) -> Option<ReportSpec> {
        fn root_value(root: &Path) -> String {
            root.to_string_lossy().into_owned()
        }
        fn action(argv: Vec<String>) -> crate::report::Action {
            crate::report::Action::Command { argv }
        }
        let spec = match self {
            Self::Init { root, format, .. } => ReportSpec {
                command: "init",
                format: *format,
                next_actions: vec![
                    action(vec![
                        "deshell".into(),
                        "scenario".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ]),
                    action(vec![
                        "deshell".into(),
                        "matrix".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ]),
                ],
            },
            Self::Scan { format, .. } => ReportSpec {
                command: "scan",
                format: *format,
                next_actions: Vec::new(),
            },
            Self::Audit { format, .. } => match format {
                AuditOutputFormat::Human => ReportSpec {
                    command: "audit",
                    format: OutputFormat::Human,
                    next_actions: Vec::new(),
                },
                AuditOutputFormat::Json => ReportSpec {
                    command: "audit",
                    format: OutputFormat::Json,
                    next_actions: Vec::new(),
                },
                AuditOutputFormat::Jsonl | AuditOutputFormat::Sarif | AuditOutputFormat::Github => {
                    return None;
                }
            },
            Self::Scenario { command } => {
                let (root, format) = match command {
                    ScenarioCommand::List { root, format }
                    | ScenarioCommand::Show { root, format, .. }
                    | ScenarioCommand::Synthesize { root, format, .. }
                    | ScenarioCommand::Approve { root, format, .. } => (root, *format),
                };
                ReportSpec {
                    command: "scenario",
                    format,
                    next_actions: vec![action(vec![
                        "deshell".into(),
                        "scenario".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ])],
                }
            }
            Self::Matrix { command } => {
                let (root, format) = match command {
                    MatrixCommand::List { root, format }
                    | MatrixCommand::Approve { root, format, .. } => (root, *format),
                };
                ReportSpec {
                    command: "matrix",
                    format,
                    next_actions: vec![action(vec![
                        "deshell".into(),
                        "matrix".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ])],
                }
            }
            Self::Analyze { root, format, .. } => ReportSpec {
                command: "analyze",
                format: *format,
                next_actions: vec![action(vec![
                    "deshell".into(),
                    "check".into(),
                    "--root".into(),
                    root_value(root),
                ])],
            },
            Self::Rewrite { format, .. } => ReportSpec {
                command: "rewrite",
                format: *format,
                next_actions: Vec::new(),
            },
            Self::Modernize { format, .. } => ReportSpec {
                command: "modernize",
                format: *format,
                next_actions: Vec::new(),
            },
            Self::Harden { command } => {
                let format = match command {
                    HardenCommand::Plan { format, .. }
                    | HardenCommand::Verify { format, .. }
                    | HardenCommand::Apply { format, .. } => *format,
                };
                ReportSpec {
                    command: "harden",
                    format,
                    next_actions: Vec::new(),
                }
            }
            Self::Migrate { command } => {
                let (root, format) = match command {
                    MigrateCommand::Plan { root, format }
                    | MigrateCommand::Verify { root, format, .. }
                    | MigrateCommand::Apply { root, format, .. }
                    | MigrateCommand::Status { root, format } => (root, *format),
                    MigrateCommand::Evidence { command } => match command {
                        MigrateEvidenceCommand::Import { root, format, .. } => (root, *format),
                    },
                };
                ReportSpec {
                    command: "migrate",
                    format,
                    next_actions: vec![action(vec![
                        "deshell".into(),
                        "migrate".into(),
                        "status".into(),
                        "--root".into(),
                        root_value(root),
                    ])],
                }
            }
            Self::Verify { root, format, .. } => ReportSpec {
                command: "verify",
                format: *format,
                next_actions: vec![action(vec![
                    "deshell".into(),
                    "verify".into(),
                    "--root".into(),
                    root_value(root),
                ])],
            },
            Self::Observe { root, format, .. } => ReportSpec {
                command: "observe",
                format: *format,
                next_actions: vec![action(vec![
                    "deshell".into(),
                    "verify".into(),
                    "--root".into(),
                    root_value(root),
                ])],
            },
            Self::Doctor { format, .. } => ReportSpec {
                command: "doctor",
                format: *format,
                next_actions: Vec::new(),
            },
            Self::Check { root, format } => ReportSpec {
                command: "check",
                format: *format,
                next_actions: vec![
                    action(vec![
                        "deshell".into(),
                        "scenario".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ]),
                    action(vec![
                        "deshell".into(),
                        "matrix".into(),
                        "list".into(),
                        "--root".into(),
                        root_value(root),
                    ]),
                ],
            },
            Self::Explain { format, .. } => ReportSpec {
                command: "explain",
                format: *format,
                next_actions: Vec::new(),
            },
            Self::Run { .. }
            | Self::Export { .. }
            | Self::Schema { .. }
            | Self::ProcessAgent
            | Self::ObserverAgent
            | Self::NushellAdapter
            | Self::Generator => return None,
        };
        Some(spec)
    }

    fn force_human_report_source(&mut self) {
        match self {
            Self::Init { format, .. }
            | Self::Scan { format, .. }
            | Self::Analyze { format, .. }
            | Self::Rewrite { format, .. }
            | Self::Modernize { format, .. }
            | Self::Verify { format, .. }
            | Self::Observe { format, .. }
            | Self::Doctor { format, .. }
            | Self::Check { format, .. }
            | Self::Explain { format, .. } => *format = OutputFormat::Human,
            Self::Audit { format, .. } => *format = AuditOutputFormat::Human,
            Self::Scenario { command } => match command {
                ScenarioCommand::List { format, .. }
                | ScenarioCommand::Show { format, .. }
                | ScenarioCommand::Synthesize { format, .. }
                | ScenarioCommand::Approve { format, .. } => *format = OutputFormat::Human,
            },
            Self::Matrix { command } => match command {
                MatrixCommand::List { format, .. } | MatrixCommand::Approve { format, .. } => {
                    *format = OutputFormat::Human
                }
            },
            Self::Harden { command } => match command {
                HardenCommand::Plan { format, .. }
                | HardenCommand::Verify { format, .. }
                | HardenCommand::Apply { format, .. } => *format = OutputFormat::Human,
            },
            Self::Migrate { command } => match command {
                MigrateCommand::Plan { format, .. }
                | MigrateCommand::Verify { format, .. }
                | MigrateCommand::Apply { format, .. }
                | MigrateCommand::Status { format, .. } => *format = OutputFormat::Human,
                MigrateCommand::Evidence { command } => match command {
                    MigrateEvidenceCommand::Import { format, .. } => *format = OutputFormat::Human,
                },
            },
            Self::Run { .. }
            | Self::Export { .. }
            | Self::Schema { .. }
            | Self::ProcessAgent
            | Self::ObserverAgent
            | Self::NushellAdapter
            | Self::Generator => {}
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Failure {
    exit: i32,
    code: &'static str,
    message: String,
    help: Option<String>,
    next_actions: Vec<crate::report::Action>,
}

impl Failure {
    fn io(message: impl Into<String>) -> Self {
        Self {
            exit: 1,
            code: "DESHELL_IO",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn limit(message: impl Into<String>) -> Self {
        Self {
            exit: 1,
            code: "DESHELL_LIMIT_EXCEEDED",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn usage(message: impl Into<String>) -> Self {
        Self {
            exit: 2,
            code: "DESHELL_USAGE",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            exit: 3,
            code: "DESHELL_INVALID_CONTRACT",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn policy(message: impl Into<String>) -> Self {
        Self {
            exit: 4,
            code: "DESHELL_POLICY",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn shell_reintroduced(message: impl Into<String>) -> Self {
        Self {
            exit: 4,
            code: "DESHELL_SHELL_REINTRODUCED",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn difference(message: impl Into<String>) -> Self {
        Self {
            exit: 5,
            code: "DESHELL_DIFFERENCE",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            exit: 6,
            code: "DESHELL_PROVIDER_UNAVAILABLE",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }
    fn internal(message: impl Into<String>) -> Self {
        Self {
            exit: 70,
            code: "DESHELL_INTERNAL",
            message: message.into(),
            help: None,
            next_actions: Vec::new(),
        }
    }

    fn with_next_actions(
        mut self,
        help: impl Into<String>,
        next_actions: Vec<crate::report::Action>,
    ) -> Self {
        self.help = Some(help.into());
        self.next_actions = next_actions;
        self
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
    let mut command = cli.command;
    if let Some(spec) = command.report_spec() {
        command.force_human_report_source();
        let mut captured_stdout = Vec::new();
        let mut captured_stderr = Vec::new();
        let outcome = dispatch(
            command,
            diagnostic_mode,
            &mut captured_stdout,
            &mut captured_stderr,
        );
        let (code, completed_failure) = match outcome {
            Ok(code) => (code, None),
            Err(failure) if completed_report_failure(&failure) => (failure.exit, Some(failure)),
            Err(failure) => {
                let exit = failure.exit;
                let diagnostic = failure_diagnostic(failure);
                return if crate::diagnostics::emit(stderr, diagnostic_mode, &diagnostic).is_err() {
                    70
                } else {
                    exit
                };
            }
        };
        let mut report = command_report(
            &spec,
            code,
            completed_failure.as_ref(),
            &captured_stdout,
            &captured_stderr,
        );
        if report.next_actions.is_empty() {
            report.next_actions = spec.next_actions;
        }
        let emitted = match spec.format {
            OutputFormat::Human => report.emit_human(stdout).map_err(|error| error.to_string()),
            OutputFormat::Json => report.emit_json(stdout),
        };
        if let Err(message) = emitted {
            let diagnostic = crate::diagnostics::Diagnostic::error(
                "DESHELL_IO",
                format!("cannot write report: {message}"),
            );
            let _ = crate::diagnostics::emit(stderr, diagnostic_mode, &diagnostic);
            return 1;
        }
        return code;
    }
    match dispatch(command, diagnostic_mode, stdout, stderr) {
        Ok(code) => code,
        Err(failure) => {
            let exit = failure.exit;
            let diagnostic = failure_diagnostic(failure);
            if crate::diagnostics::emit(stderr, diagnostic_mode, &diagnostic).is_err() {
                70
            } else {
                exit
            }
        }
    }
}

fn failure_diagnostic(failure: Failure) -> crate::diagnostics::Diagnostic {
    let mut diagnostic = crate::diagnostics::Diagnostic::error(failure.code, failure.message);
    if let Some(help) = failure.help {
        diagnostic.help = help;
    }
    if !failure.next_actions.is_empty() {
        diagnostic.next_actions = failure.next_actions;
    }
    diagnostic
}

fn completed_report_failure(failure: &Failure) -> bool {
    matches!(failure.exit, 4..=6)
        || failure.message.starts_with("migration verification failed")
        || failure
            .message
            .contains("current scenarios have not been observed")
}

fn command_report(
    spec: &ReportSpec,
    code: i32,
    failure: Option<&Failure>,
    stdout: &[u8],
    stderr: &[u8],
) -> crate::report::Report {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let not_ready = code == 0
        && match spec.command {
            "init" => stdout.contains("created ") || stdout.contains("initialized de-shell"),
            "scenario" | "matrix" => stdout.contains("\tdraft\t") || stdout.contains("\tstale\t"),
            "check" => stdout.contains("valid but not ready"),
            _ => false,
        };
    let status = if not_ready {
        crate::report::Status::NotReady
    } else {
        match code {
            0 => crate::report::Status::Ok,
            4 => crate::report::Status::Blocked,
            5 => crate::report::Status::Different,
            6 => crate::report::Status::Unavailable,
            _ => crate::report::Status::Failed,
        }
    };
    let summary = failure
        .map(|failure| failure.message.clone())
        .unwrap_or_else(|| match status {
            crate::report::Status::Ok => format!("{} completed", spec.command),
            crate::report::Status::NotReady => format!("{} is valid but not ready", spec.command),
            crate::report::Status::Blocked => format!("{} completed with blockers", spec.command),
            crate::report::Status::Different => format!("{} found a difference", spec.command),
            crate::report::Status::Unavailable => {
                format!("{} capability is unavailable", spec.command)
            }
            crate::report::Status::Failed => format!("{} failed", spec.command),
        });
    let mut report = crate::report::Report::new(spec.command, status, summary);
    if let Some(failure) = failure {
        report.details.items.push(crate::report::Item {
            kind: Some("failure".into()),
            name: Some(failure.code.into()),
            message: Some(failure.message.clone()),
            ..crate::report::Item::default()
        });
        report
            .details
            .output
            .push(format!("{}: {}", failure.code, failure.message));
        report.next_actions.extend(failure.next_actions.clone());
    }
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("next argv: ")
            && let Ok(argv) = serde_json::from_str::<Vec<String>>(value)
            && !argv.is_empty()
        {
            report
                .next_actions
                .push(crate::report::Action::Command { argv });
            continue;
        }
        report.details.output.push(line.to_owned());
        if spec.command == "scenario"
            && let Some(path) = line.strip_prefix("# ")
            && path.starts_with(".deshell/scenarios/")
        {
            report.next_actions.push(crate::report::Action::Review {
                paths: vec![path.into()],
            });
        }
        if let Some(path) = line.strip_prefix("created ") {
            report.details.paths.push(path.to_owned());
        } else if let Some(path) = line.strip_prefix("artifact ") {
            report.details.paths.push(path.to_owned());
        } else if let Some(entrypoint) = line.strip_prefix("entrypoint ") {
            report.details.items.push(crate::report::Item {
                kind: Some("entrypoint".into()),
                path: Some(entrypoint.into()),
                ..crate::report::Item::default()
            });
        } else if let Some(target) = line.strip_prefix("target ")
            && let Some((target, rest)) = target.split_once(" (module root ")
            && let Some((module_root, reason)) = rest.split_once("): ")
        {
            report.details.values.insert("target".into(), target.into());
            report
                .details
                .values
                .insert("module_root".into(), module_root.into());
            report.details.values.insert("reason".into(), reason.into());
        } else if let Some(cell) = line.strip_prefix("required cell ") {
            report.details.items.push(crate::report::Item {
                kind: Some("matrix_cell".into()),
                name: Some(cell.into()),
                ..crate::report::Item::default()
            });
        } else if let Some(reason) = line.strip_prefix("not-ready: ") {
            report.details.items.push(crate::report::Item {
                kind: Some("not_ready".into()),
                message: Some(reason.into()),
                ..crate::report::Item::default()
            });
        } else if let Some(blocker) = line.strip_prefix("blocker ") {
            let (name, message) = blocker.split_once(' ').unwrap_or((blocker, ""));
            report.details.items.push(crate::report::Item {
                kind: Some("blocker".into()),
                name: Some(name.into()),
                message: (!message.is_empty()).then(|| message.into()),
                ..crate::report::Item::default()
            });
        } else if spec.command == "scan" && line.contains('\t') {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() >= 2 {
                let item = match fields[0] {
                    "error" => crate::report::Item {
                        kind: Some("error".into()),
                        path: Some(fields[1].into()),
                        name: fields.get(2).map(|stage| (*stage).into()),
                        message: fields.get(3).map(|message| (*message).into()),
                        ..crate::report::Item::default()
                    },
                    "skipped" => crate::report::Item {
                        kind: Some("skipped".into()),
                        path: Some(fields[1].into()),
                        message: fields.get(2).map(|reason| (*reason).into()),
                        ..crate::report::Item::default()
                    },
                    kind => crate::report::Item {
                        kind: Some(kind.into()),
                        path: Some(fields[1].into()),
                        name: fields.get(2).map(|interpreter| (*interpreter).into()),
                        status: fields.get(3).map(|confidence| (*confidence).into()),
                        message: fields
                            .get(4)
                            .filter(|locator| **locator != "-")
                            .map(|locator| (*locator).into()),
                        ..crate::report::Item::default()
                    },
                };
                report.details.items.push(item);
            }
        } else if matches!(spec.command, "scenario" | "matrix") && line.contains('\t') {
            let fields = line.split('\t').collect::<Vec<_>>();
            if fields.len() >= 3 {
                report.details.items.push(crate::report::Item {
                    kind: Some(spec.command.into()),
                    name: Some(fields[0].into()),
                    status: Some(fields[1].into()),
                    digest: Some(fields[2].into()),
                    path: fields
                        .get(3)
                        .filter(|path| **path != "-")
                        .map(|path| (*path).into()),
                    ..crate::report::Item::default()
                });
            }
        }
        if let Some((name, value)) = line.split_once(": ")
            && !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-'))
        {
            report
                .details
                .values
                .insert(name.replace(' ', "_"), value.into());
        }
        for token in line.split_whitespace() {
            if let Some((name, value)) = token.split_once('=')
                && !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                let value = value.trim_end_matches([',', ';']);
                if let Ok(value) = value.parse::<u64>() {
                    report.details.counts.insert(name.into(), value);
                } else if !value.is_empty() {
                    report.details.values.insert(name.into(), value.into());
                }
            }
        }
        if spec.command == "scan"
            && let Some((findings, rest)) = line.split_once(" shell location(s) found; ")
            && let Ok(findings) = findings.parse()
        {
            report.details.counts.insert("findings".into(), findings);
            let fields = rest.split(';').map(str::trim).collect::<Vec<_>>();
            for (name, suffix) in [("skipped", " skipped"), ("errors", " error(s)")] {
                if let Some(field) = fields.iter().find(|field| field.ends_with(suffix))
                    && let Ok(value) = field.trim_end_matches(suffix).parse()
                {
                    report.details.counts.insert(name.into(), value);
                }
            }
        }
        if spec.command == "audit"
            && let Some((findings, rest)) = line.split_once(" audit finding(s); ")
            && let Ok(findings) = findings.parse()
        {
            report.details.counts.insert("findings".into(), findings);
            if let Some(failures) = rest.split_whitespace().next()
                && let Ok(failures) = failures.parse()
            {
                report.details.counts.insert("threshold".into(), failures);
            }
        }
    }
    report
        .details
        .output
        .extend(stderr.lines().map(|line| format!("diagnostic: {line}")));
    report
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
        Command::Init {
            root,
            entries,
            target,
            format,
        } => {
            let result = crate::project::init_cli(
                &root,
                &entries,
                match target {
                    InitTargetArg::Auto => crate::project::InitTarget::Auto,
                    InitTargetArg::Rust => crate::project::InitTarget::Rust,
                    InitTargetArg::Go => crate::project::InitTarget::Go,
                    InitTargetArg::Host => crate::project::InitTarget::Host,
                },
            )
            .map_err(|message| {
                if message.starts_with("cannot choose a unique migration target") {
                    let root = root.to_string_lossy().into_owned();
                    Failure::usage(message).with_next_actions(
                        "Select exactly one migration target after reviewing the detected project markers.",
                        ["rust", "go", "host"]
                            .into_iter()
                            .map(|target| crate::report::Action::Command {
                                argv: vec![
                                    "deshell".into(),
                                    "init".into(),
                                    "--root".into(),
                                    root.clone(),
                                    "--target".into(),
                                    target.into(),
                                ],
                            })
                            .collect(),
                    )
                } else if message.contains("cannot replace")
                    || message == "duplicate --entry value"
                {
                    Failure::usage(message)
                } else {
                    Failure::io(message)
                }
            })?;
            if format == OutputFormat::Json {
                let value = serde_json::json!({
                    "created": result.created,
                    "entrypoints": result.entrypoints,
                    "module_root": result.module_root,
                    "reason": result.reason,
                    "target": result.target.as_str(),
                });
                write_io(
                    stdout,
                    &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
                )?;
                return Ok(0);
            }
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
            writeln_io(
                stdout,
                format_args!(
                    "target {} (module root {}): {}",
                    result.target.as_str(),
                    result.module_root,
                    result.reason
                ),
            )?;
            Ok(0)
        }
        Command::Scan { root, format } => {
            let inventory = crate::project::scan(&root).map_err(Failure::io)?;
            let exit = if inventory.errors.is_empty() { 0 } else { 1 };
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
                        writeln_io(
                            stdout,
                            format_args!(
                                "{}\t{}\t{}\t{}\t{}",
                                finding_kind(&finding.kind),
                                finding.path,
                                finding.interpreter.as_deref().unwrap_or("unknown"),
                                interpreter_confidence(&finding.interpreter_confidence),
                                finding.locator.as_deref().unwrap_or("-"),
                            ),
                        )?;
                    }
                    for skipped in &inventory.skipped {
                        writeln_io(
                            stdout,
                            format_args!("skipped\t{}\t{}", skipped.path, skipped.reason),
                        )?;
                    }
                    for error in &inventory.errors {
                        writeln_io(
                            stdout,
                            format_args!(
                                "error\t{}\t{}\t{}",
                                error.path.as_deref().unwrap_or("<root>"),
                                error.stage,
                                error.message
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
            Ok(exit)
        }
        Command::Audit {
            root,
            format,
            persona,
        } => audit_command(&root, format, persona, stdout),
        Command::Scenario { command } => match command {
            ScenarioCommand::List { root, format } => {
                scenario_review_command(&root, None, format, stdout)
            }
            ScenarioCommand::Show { root, name, format } => {
                scenario_review_command(&root, Some(&name), format, stdout)
            }
            ScenarioCommand::Synthesize {
                root,
                apply,
                format,
            } => scenario_synthesize_command(&root, apply, format, stdout),
            ScenarioCommand::Approve {
                root,
                name,
                digest,
                format,
            } => scenario_approve_command(&root, &name, &digest, format, stdout),
        },
        Command::Matrix { command } => match command {
            MatrixCommand::List { root, format } => matrix_review_command(&root, format, stdout),
            MatrixCommand::Approve {
                root,
                cell,
                digest,
                format,
            } => matrix_approve_command(&root, &cell, &digest, format, stdout),
        },
        Command::Analyze { root, entry, .. } => {
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
        Command::Check { root, format } => {
            let readiness =
                crate::project::check_readiness(&root).map_err(classify_project_errors)?;
            match format {
                OutputFormat::Json => {
                    let value = serde_json::to_value(&readiness)
                        .map_err(|error| Failure::internal(error.to_string()))?;
                    write_io(
                        stdout,
                        &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
                    )?;
                }
                OutputFormat::Human if readiness.ready => writeln_io(
                    stdout,
                    format_args!("{}: project artifacts are ready", root.display()),
                )?,
                OutputFormat::Human => {
                    writeln_io(
                        stdout,
                        format_args!("{}: project is valid but not ready", root.display()),
                    )?;
                    for reason in readiness.reasons {
                        writeln_io(stdout, format_args!("not-ready: {reason}"))?;
                    }
                }
            }
            Ok(0)
        }
        Command::Verify {
            root,
            entry,
            require,
            ..
        } => {
            if require == Some(GuaranteeRequirement::ShellFree) {
                return shell_free_command(&root, stdout);
            }
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
            ..
        } => observe_command(&root, entry, &scenario, stdout),
        Command::Doctor {
            root,
            format,
            require,
        } => doctor_command(&root, format, require, stdout),
        Command::Explain { root, node_id, .. } => explain(&root, node_id.as_deref(), stdout),
        Command::Schema { name } => {
            write_io(stdout, schema(name))?;
            Ok(0)
        }
        Command::Rewrite {
            root,
            entry,
            equivalent,
            apply,
            ..
        } => rewrite_command(&root, entry, equivalent, apply, stdout),
        Command::Modernize {
            root,
            profile,
            apply,
            ..
        } => modernize_command(&root, &profile, apply, diagnostic_mode, stdout, stderr),
        Command::Harden { command } => harden_command(command, stdout),
        Command::Migrate { command } => match command {
            MigrateCommand::Plan { root, .. } => migrate_plan_command(&root, stdout),
            MigrateCommand::Verify {
                root,
                plan,
                cell,
                output,
                ..
            } => migrate_verify_command(&root, &plan, &cell, &output, stdout),
            MigrateCommand::Evidence { command } => match command {
                MigrateEvidenceCommand::Import {
                    root, plan, files, ..
                } => migrate_evidence_import_command(&root, &plan, &files, stdout),
            },
            MigrateCommand::Apply { root, plan, .. } => migrate_apply_command(&root, &plan, stdout),
            MigrateCommand::Status { root, format } => {
                migrate_status_command(&root, format, stdout)
            }
        },
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
        Command::Generator => {
            crate::protocol::serve_stdio(crate::protocol::AgentKind::Generator, stdout)
                .map_err(Failure::invalid)
        }
    }
}

fn schema(name: SchemaName) -> &'static [u8] {
    match name {
        SchemaName::Approval => {
            include_bytes!("../../../contracts/schema/approval-v1.schema.json")
        }
        SchemaName::InitReport => {
            include_bytes!("../../../contracts/schema/init-report-v1.schema.json")
        }
        SchemaName::ScanReport => {
            include_bytes!("../../../contracts/schema/scan-report-v1.schema.json")
        }
        SchemaName::ScenarioReport => {
            include_bytes!("../../../contracts/schema/scenario-report-v1.schema.json")
        }
        SchemaName::MatrixReport => {
            include_bytes!("../../../contracts/schema/matrix-report-v1.schema.json")
        }
        SchemaName::AuditReport => {
            include_bytes!("../../../contracts/schema/audit-report-v1.schema.json")
        }
        SchemaName::AnalyzeReport => {
            include_bytes!("../../../contracts/schema/analyze-report-v1.schema.json")
        }
        SchemaName::CheckReport => {
            include_bytes!("../../../contracts/schema/check-report-v1.schema.json")
        }
        SchemaName::VerifyReport => {
            include_bytes!("../../../contracts/schema/verify-report-v1.schema.json")
        }
        SchemaName::ObserveReport => {
            include_bytes!("../../../contracts/schema/observe-report-v1.schema.json")
        }
        SchemaName::DoctorReport => {
            include_bytes!("../../../contracts/schema/doctor-report-v1.schema.json")
        }
        SchemaName::ExplainReport => {
            include_bytes!("../../../contracts/schema/explain-report-v1.schema.json")
        }
        SchemaName::RewriteReport => {
            include_bytes!("../../../contracts/schema/rewrite-report-v1.schema.json")
        }
        SchemaName::ModernizeReport => {
            include_bytes!("../../../contracts/schema/modernize-report-v1.schema.json")
        }
        SchemaName::HardenReport => {
            include_bytes!("../../../contracts/schema/harden-report-v1.schema.json")
        }
        SchemaName::MigrateReport => {
            include_bytes!("../../../contracts/schema/migrate-report-v1.schema.json")
        }
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
        SchemaName::GeneratorProtocol => {
            include_bytes!("../../../contracts/schema/generator-protocol-v1.schema.json")
        }
        SchemaName::MigrationRequest => {
            include_bytes!("../../../contracts/schema/migration-request-v1.schema.json")
        }
        SchemaName::Proposal => include_bytes!("../../../contracts/schema/proposal-v1.schema.json"),
        SchemaName::MigrationPlan => {
            include_bytes!("../../../contracts/schema/migration-plan-v1.schema.json")
        }
        SchemaName::MigrationIndex => {
            include_bytes!("../../../contracts/schema/migration-index-v1.schema.json")
        }
        SchemaName::MigrationEvidence => {
            include_bytes!("../../../contracts/schema/migration-evidence-v1.schema.json")
        }
        SchemaName::ArchiveManifest => {
            include_bytes!("../../../contracts/schema/archive-manifest-v1.schema.json")
        }
        SchemaName::AuditFinding => {
            include_bytes!("../../../contracts/schema/audit-finding-v1.schema.json")
        }
        SchemaName::HardenPlan => {
            include_bytes!("../../../contracts/schema/harden-plan-v1.schema.json")
        }
        SchemaName::HardenApproval => {
            include_bytes!("../../../contracts/schema/harden-approval-v1.schema.json")
        }
        SchemaName::HardenEvidence => {
            include_bytes!("../../../contracts/schema/harden-evidence-v1.schema.json")
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
    if message.contains("rejected by policy") || message.contains("DESHELL_BLOCKER_") {
        Failure::policy(message)
    } else if errors.iter().all(|error| is_io_message(error)) {
        Failure::io(message)
    } else {
        Failure::invalid(message)
    }
}

fn classify_project_error(message: String) -> Failure {
    if message.contains("rejected by policy") || message.contains("DESHELL_BLOCKER_") {
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

fn interpreter_confidence(confidence: &crate::scanner::InterpreterConfidence) -> &'static str {
    match confidence {
        crate::scanner::InterpreterConfidence::High => "high",
        crate::scanner::InterpreterConfidence::Medium => "medium",
        crate::scanner::InterpreterConfidence::Low => "low",
    }
}

fn shell_free_command(root: &Path, stdout: &mut dyn Write) -> Result<i32, Failure> {
    let inventory = crate::project::scan(root).map_err(Failure::io)?;
    if !inventory.errors.is_empty() || !inventory.skipped.is_empty() {
        let blockers = inventory
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
        return Err(Failure::shell_reintroduced(format!(
            "shell-free scan is incomplete: {blockers}"
        )));
    }
    if !inventory.findings.is_empty() {
        let locations = inventory
            .findings
            .iter()
            .map(|finding| {
                format!(
                    "{}:{}@{}..{}",
                    finding_kind(&finding.kind),
                    finding.path,
                    finding.span.start_byte,
                    finding.span.end_byte
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Failure::shell_reintroduced(format!(
            "live tree is not shell-free ({} location(s)): {locations}",
            inventory.findings.len()
        )));
    }
    crate::migration::verify_integrity(root).map_err(Failure::policy)?;
    writeln_io(stdout, format_args!("shell-free: verified"))?;
    Ok(0)
}

fn scenario_synthesize_command(
    root: &Path,
    apply: bool,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    if format == OutputFormat::Json {
        let mut output = Vec::new();
        let code = scenario_synthesize_human(root, apply, &mut output)?;
        let value = serde_json::json!({
            "applied": apply,
            "output": String::from_utf8(output)
                .map_err(|_| Failure::internal("scenario output was not UTF-8"))?,
        });
        write_io(
            stdout,
            &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
        )?;
        return Ok(code);
    }
    scenario_synthesize_human(root, apply, stdout)
}

fn scenario_synthesize_human(
    root: &Path,
    apply: bool,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let config = crate::project::load_config(root).map_err(classify_project_errors)?;
    if config.entrypoints.is_empty() {
        return Err(Failure::invalid(
            "scenario synthesis requires at least one configured entrypoint",
        ));
    }
    let mut stem_counts = std::collections::BTreeMap::new();
    for entry in &config.entrypoints {
        *stem_counts.entry(scenario_stem(entry)).or_insert(0_usize) += 1;
    }
    for entry in &config.entrypoints {
        let (_, path) =
            crate::project::resolve_entry(root, entry).map_err(classify_project_error)?;
        let source = std::fs::read(&path)
            .map_err(|error| Failure::io(format!("cannot read {}: {error}", path.display())))?;
        let plan =
            crate::frontend::lower(entry, &source, config.policy.unknown_interpreter.clone())
                .map_err(classify_project_error)?;
        let task = plan
            .tasks
            .iter()
            .find(|task| task.name == plan.entrypoint)
            .ok_or_else(|| Failure::invalid("lowered plan entrypoint task is missing"))?;
        let stem = scenario_stem(entry);
        let suffix = if stem_counts.get(&stem).copied().unwrap_or(0) > 1 {
            let digest = crate::digest::sha256(entry.as_bytes());
            format!("-{}", &digest[..8])
        } else {
            String::new()
        };
        let name = format!("synthesized-{stem}{suffix}");
        let scenario = crate::config::Scenario {
            version: 1,
            name: name.clone(),
            approval: crate::config::ScenarioApproval::Draft,
            arguments: task
                .inputs
                .iter()
                .map(|input| crate::config::NamedValue {
                    name: input.name.clone(),
                    value: String::new(),
                })
                .collect(),
            argv: Vec::new(),
            environment: task
                .environment
                .iter()
                .map(|name| crate::config::NamedValue {
                    name: name.clone(),
                    value: String::new(),
                })
                .collect(),
            fixtures: Vec::new(),
            stdin: None,
            cwd: None,
            limits: config.limits,
            expect: crate::config::Expectation::default(),
        };
        let mut text = toml::to_string_pretty(&scenario)
            .map_err(|error| Failure::internal(format!("cannot encode scenario draft: {error}")))?;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        crate::config::Scenario::decode(&text).map_err(classify_project_errors)?;
        let relative = format!(".deshell/scenarios/{name}.toml");
        if apply {
            let scenario_directory =
                crate::project::project_directory_path(root, ".deshell/scenarios")
                    .map_err(Failure::invalid)?;
            let target = scenario_directory.join(format!("{name}.toml"));
            match target.symlink_metadata() {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                        return Err(Failure::policy(format!(
                            "scenario target is not a regular file: {relative}"
                        )));
                    }
                    let current = std::fs::read(&target).map_err(|error| {
                        Failure::io(format!("cannot read {}: {error}", target.display()))
                    })?;
                    if current != text.as_bytes() {
                        return Err(Failure::policy(format!(
                            "refusing to overwrite an existing scenario draft: {relative}"
                        )));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let proposal =
                        crate::patch::prepare_create(&target, text.as_bytes().to_vec(), 0o644)
                            .map_err(Failure::io)?;
                    crate::patch::apply_all(&[proposal]).map_err(Failure::io)?;
                }
                Err(error) => {
                    return Err(Failure::io(format!(
                        "cannot inspect {}: {error}",
                        target.display()
                    )));
                }
            }
            writeln_io(stdout, format_args!("wrote {relative} (approval=draft)"))?;
        } else {
            writeln_io(stdout, format_args!("# {relative}"))?;
            write_io(stdout, text.as_bytes())?;
        }
    }
    Ok(0)
}

fn scenario_review_command(
    root: &Path,
    selected: Option<&str>,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let mut reviews = crate::approval::scenario_reviews(root).map_err(classify_project_error)?;
    if let Some(name) = selected {
        reviews.retain(|review| review.name == name);
        if reviews.is_empty() {
            return Err(Failure::invalid(format!("scenario not found: {name}")));
        }
        if reviews.len() != 1 {
            return Err(Failure::invalid(format!(
                "scenario name is ambiguous: {name}"
            )));
        }
    }
    match format {
        OutputFormat::Json => {
            let value = serde_json::to_value(&reviews)
                .map_err(|error| Failure::internal(error.to_string()))?;
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        OutputFormat::Human => {
            for review in &reviews {
                writeln_io(
                    stdout,
                    format_args!(
                        "{}\t{}\t{}\t{}",
                        review.name,
                        review_status(review.status),
                        review.digest,
                        review.path.as_deref().unwrap_or("-")
                    ),
                )?;
                if review.status != crate::approval::ReviewStatus::Approved {
                    let argv = vec![
                        "deshell".to_owned(),
                        "scenario".to_owned(),
                        "approve".to_owned(),
                        "--root".to_owned(),
                        root.to_string_lossy().into_owned(),
                        "--name".to_owned(),
                        review.name.clone(),
                        "--digest".to_owned(),
                        review.digest.clone(),
                    ];
                    writeln_io(
                        stdout,
                        format_args!(
                            "next argv: {}",
                            serde_json::to_string(&argv)
                                .map_err(|error| Failure::internal(error.to_string()))?
                        ),
                    )?;
                }
            }
        }
    }
    Ok(0)
}

fn scenario_approve_command(
    root: &Path,
    name: &str,
    digest: &str,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let approval = crate::approval::approve_scenario(root, name, digest).map_err(|message| {
        if message.starts_with("review digest mismatch") {
            Failure::policy(message)
        } else {
            classify_project_error(message)
        }
    })?;
    match format {
        OutputFormat::Json => {
            let value = serde_json::to_value(&approval)
                .map_err(|error| Failure::internal(error.to_string()))?;
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        OutputFormat::Human => writeln_io(
            stdout,
            format_args!("approved scenario {name} as {}", approval.approval_digest),
        )?,
    }
    Ok(0)
}

fn matrix_review_command(
    root: &Path,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let reviews = crate::approval::matrix_reviews(root).map_err(classify_project_error)?;
    match format {
        OutputFormat::Json => {
            let value = serde_json::to_value(&reviews)
                .map_err(|error| Failure::internal(error.to_string()))?;
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        OutputFormat::Human => {
            for review in &reviews {
                writeln_io(
                    stdout,
                    format_args!(
                        "{}\t{}\t{}",
                        review.name,
                        review_status(review.status),
                        review.digest
                    ),
                )?;
                if review.status != crate::approval::ReviewStatus::Approved {
                    let argv = vec![
                        "deshell".to_owned(),
                        "matrix".to_owned(),
                        "approve".to_owned(),
                        "--root".to_owned(),
                        root.to_string_lossy().into_owned(),
                        "--cell".to_owned(),
                        review.name.clone(),
                        "--digest".to_owned(),
                        review.digest.clone(),
                    ];
                    writeln_io(
                        stdout,
                        format_args!(
                            "next argv: {}",
                            serde_json::to_string(&argv)
                                .map_err(|error| Failure::internal(error.to_string()))?
                        ),
                    )?;
                }
            }
        }
    }
    Ok(0)
}

fn matrix_approve_command(
    root: &Path,
    cell: &str,
    digest: &str,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let approval = crate::approval::approve_matrix(root, cell, digest).map_err(|message| {
        if message.starts_with("review digest mismatch") {
            Failure::policy(message)
        } else {
            classify_project_error(message)
        }
    })?;
    match format {
        OutputFormat::Json => {
            let value = serde_json::to_value(&approval)
                .map_err(|error| Failure::internal(error.to_string()))?;
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        OutputFormat::Human => writeln_io(
            stdout,
            format_args!(
                "approved matrix cell {cell} as {}",
                approval.approval_digest
            ),
        )?,
    }
    Ok(0)
}

fn review_status(status: crate::approval::ReviewStatus) -> &'static str {
    match status {
        crate::approval::ReviewStatus::Draft => "draft",
        crate::approval::ReviewStatus::Approved => "approved",
        crate::approval::ReviewStatus::Stale => "stale",
    }
}

fn scenario_stem(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    let mut output = String::new();
    for character in stem.chars() {
        if character.is_ascii_alphanumeric() {
            output.push(character.to_ascii_lowercase());
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-');
    if output.is_empty() {
        "entry".into()
    } else {
        output.into()
    }
}

fn audit_command(
    root: &Path,
    format: AuditOutputFormat,
    _persona: AuditPersona,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let inventory = crate::project::scan(root).map_err(Failure::io)?;
    let config_path = root.join(".deshell/project.toml");
    let config = match config_path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            crate::project::load_config(root).map_err(classify_project_errors)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::config::ProjectConfig::decode(&crate::config::ProjectConfig::default_text())
                .map_err(classify_project_errors)?
        }
        Ok(_) => {
            return Err(Failure::invalid(
                "audit project configuration is not a regular non-symlink file",
            ));
        }
        Err(error) => {
            return Err(Failure::io(format!(
                "cannot inspect {}: {error}",
                config_path.display()
            )));
        }
    };
    let findings = crate::audit::analyze(
        root,
        &inventory,
        &config.audit.acknowledgements,
        config.audit.acknowledgement_max_days,
    )
    .map_err(Failure::invalid)?;
    match format {
        AuditOutputFormat::Human => {
            for finding in &findings {
                writeln_io(
                    stdout,
                    format_args!(
                        "{}[{}]: {}:{}:{}: {} ({})",
                        audit_severity(finding.severity),
                        finding.rule_id,
                        finding.path,
                        finding.span.start_line,
                        finding.span.start_column + 1,
                        finding.message,
                        finding.url,
                    ),
                )?;
            }
        }
        AuditOutputFormat::Json => {
            let value = serde_json::json!({
                "findings": findings,
                "summary": {
                    "acknowledged": findings.iter().filter(|finding| finding.acknowledged).count(),
                    "findings": findings.len(),
                    "unacknowledged": findings.iter().filter(|finding| !finding.acknowledged).count(),
                }
            });
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        AuditOutputFormat::Jsonl => {
            for finding in &findings {
                let value = serde_json::to_value(finding)
                    .map_err(|error| Failure::internal(error.to_string()))?;
                let bytes =
                    crate::canonical_json::canonical_bytes(&value).map_err(Failure::internal)?;
                write_io(stdout, &bytes)?;
                write_io(stdout, b"\n")?;
            }
        }
        AuditOutputFormat::Sarif => {
            let mut rules = std::collections::BTreeMap::new();
            for finding in &findings {
                rules.entry(finding.rule_id.as_str()).or_insert_with(|| {
                    serde_json::json!({
                        "id": finding.rule_id,
                        "helpUri": finding.url,
                        "shortDescription": {"text": finding.message},
                        "defaultConfiguration": {"level": sarif_level(finding.severity)}
                    })
                });
            }
            let results = findings
                .iter()
                .map(|finding| {
                    serde_json::json!({
                        "ruleId": finding.rule_id,
                        "level": sarif_level(finding.severity),
                        "message": {"text": finding.message},
                        "locations": [{"physicalLocation": {
                            "artifactLocation": {"uri": finding.path},
                            "region": {
                                "startLine": finding.span.start_line,
                                "startColumn": finding.span.start_column + 1,
                                "endLine": finding.span.end_line,
                                "endColumn": finding.span.end_column + 1
                            }
                        }}],
                        "partialFingerprints": {"deshellLocationDigest": finding.location_digest}
                    })
                })
                .collect::<Vec<_>>();
            let value = serde_json::json!({
                "version": "2.1.0",
                "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
                "runs": [{
                    "tool": {"driver": {"name": "deshell", "rules": rules.into_values().collect::<Vec<_>>() }},
                    "results": results
                }]
            });
            write_io(
                stdout,
                &crate::canonical_json::pretty_bytes(&value).map_err(Failure::internal)?,
            )?;
        }
        AuditOutputFormat::Github => {
            for finding in &findings {
                let level = if finding.severity >= crate::config::AuditSeverity::High {
                    "error"
                } else if finding.severity >= crate::config::AuditSeverity::Medium {
                    "warning"
                } else {
                    "notice"
                };
                writeln_io(
                    stdout,
                    format_args!(
                        "::{level} file={},line={},col={},endLine={},endColumn={},title={}::{}",
                        github_escape(&finding.path),
                        finding.span.start_line,
                        finding.span.start_column + 1,
                        finding.span.end_line,
                        finding.span.end_column + 1,
                        github_escape(&finding.rule_id),
                        github_escape(&finding.message),
                    ),
                )?;
            }
        }
    }
    let failures = findings
        .iter()
        .filter(|finding| !finding.acknowledged && finding.severity >= config.audit.fail_on)
        .count();
    if format == AuditOutputFormat::Human {
        writeln_io(
            stdout,
            format_args!(
                "{} audit finding(s); {failures} met fail_on={}",
                findings.len(),
                audit_severity(config.audit.fail_on)
            ),
        )?;
    }
    Ok(if failures == 0 { 0 } else { 4 })
}

fn audit_severity(severity: crate::config::AuditSeverity) -> &'static str {
    match severity {
        crate::config::AuditSeverity::Note => "note",
        crate::config::AuditSeverity::Low => "low",
        crate::config::AuditSeverity::Medium => "medium",
        crate::config::AuditSeverity::High => "high",
        crate::config::AuditSeverity::Critical => "critical",
    }
}

fn sarif_level(severity: crate::config::AuditSeverity) -> &'static str {
    match severity {
        crate::config::AuditSeverity::Critical | crate::config::AuditSeverity::High => "error",
        crate::config::AuditSeverity::Medium => "warning",
        crate::config::AuditSeverity::Low | crate::config::AuditSeverity::Note => "note",
    }
}

fn github_escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn harden_command(command: HardenCommand, stdout: &mut dyn Write) -> Result<i32, Failure> {
    match command {
        HardenCommand::Plan { root, .. } => {
            let output = crate::harden::plan(&root).map_err(classify_harden_error)?;
            writeln_io(stdout, format_args!("harden plan {}", output.digest))?;
            for blocker in output.blockers {
                writeln_io(
                    stdout,
                    format_args!("blocker {}: {}", blocker.code, blocker.message),
                )?;
            }
            write_io(stdout, output.diff.as_bytes())?;
            writeln_io(
                stdout,
                format_args!("approval {}", output.approval_path.display()),
            )?;
            Ok(0)
        }
        HardenCommand::Verify { root, plan, .. } => {
            let evidence = crate::harden::verify(&root, &plan).map_err(classify_harden_error)?;
            writeln_io(
                stdout,
                format_args!(
                    "harden evidence {} {}",
                    evidence.evidence_digest,
                    match evidence.status {
                        crate::harden::HardenEvidenceStatus::Verified => "verified",
                        crate::harden::HardenEvidenceStatus::Failed => "failed",
                    }
                ),
            )?;
            Ok(match evidence.status {
                crate::harden::HardenEvidenceStatus::Verified => 0,
                crate::harden::HardenEvidenceStatus::Failed => 5,
            })
        }
        HardenCommand::Apply { root, plan, .. } => {
            crate::harden::apply(&root, &plan).map_err(classify_harden_error)?;
            writeln_io(stdout, format_args!("applied harden plan {plan}"))?;
            Ok(0)
        }
    }
}

fn classify_harden_error(message: String) -> Failure {
    if message.contains("APPROVAL_REQUIRED")
        || message.contains("HARDEN_BLOCKED")
        || message.contains("NO_CHANGES")
    {
        Failure::policy(message)
    } else if is_io_message(&message) {
        Failure::io(message)
    } else {
        Failure::invalid(message)
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
    require: Option<DoctorRequirement>,
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
    let planning_ready = binary_ok && config.is_ok() && lock.is_ok();
    let local_ready = planning_ready
        && config
            .as_ref()
            .ok()
            .is_some_and(|config| config.sandbox.allow_local);
    let dagger_ready = lock
        .as_ref()
        .ok()
        .is_some_and(|lock| crate::lab::digest_pinned(&lock.targets.dagger_image));
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
                "capabilities": {
                    "bundle": bundle_ready,
                    "dagger": dagger_ready,
                    "disposable": ready,
                    "local": local_ready,
                    "planning": planning_ready
                },
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
            writeln_io(
                stdout,
                format_args!(
                    "planning={} local={} disposable={} bundle={} dagger={}",
                    planning_ready, local_ready, ready, bundle_ready, dagger_ready
                ),
            )?;
        }
    }
    Ok(match require {
        None => 0,
        Some(DoctorRequirement::Planning) if !planning_ready => 3,
        Some(DoctorRequirement::Local) if !local_ready => 4,
        Some(DoctorRequirement::Disposable) if !ready => 6,
        Some(DoctorRequirement::Bundle) if !bundle_ready => 6,
        Some(DoctorRequirement::Dagger) if !dagger_ready => 6,
        Some(_) => 0,
    })
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

fn migrate_plan_command(root: &Path, stdout: &mut dyn Write) -> Result<i32, Failure> {
    let output = crate::migration::create_plan(root).map_err(classify_project_error)?;
    writeln_io(stdout, format_args!("plan {}", output.digest))?;
    writeln_io(stdout, format_args!("artifact {}", output.artifact_path))?;
    for cell in &output.required_cells {
        writeln_io(stdout, format_args!("required cell {cell}"))?;
    }
    writeln_io(
        stdout,
        format_args!(
            "required_cells={} remaining_evidence={}",
            output.required_cells.len(),
            output.remaining_evidence
        ),
    )?;
    for blocker in &output.blockers {
        let location = blocker.location.as_ref().map_or_else(
            || "<repository>".to_owned(),
            |location| {
                format!(
                    "{}@{}..{}",
                    location.path, location.start_byte, location.end_byte
                )
            },
        );
        writeln_io(
            stdout,
            format_args!("blocker {} {location}: {}", blocker.code, blocker.message),
        )?;
    }
    write_io(stdout, output.diff.as_bytes())?;
    let status = crate::migration::status(root).map_err(Failure::io)?;
    writeln_io(
        stdout,
        format_args!(
            "active={} state={}",
            status.active_plan.as_deref().unwrap_or("none"),
            status.active_state.as_str()
        ),
    )?;
    writeln_io(
        stdout,
        format_args!(
            "next argv: {}",
            serde_json::to_string(&status.next_argv)
                .map_err(|error| Failure::internal(error.to_string()))?
        ),
    )?;
    if output.blockers.is_empty() {
        Ok(0)
    } else {
        // A blocker is a completed planning outcome. The immutable plan and
        // active index are useful artifacts, so report it on stdout only.
        Ok(4)
    }
}

fn migrate_verify_command(
    root: &Path,
    plan: &str,
    cell: &str,
    output: &Path,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let evidence = crate::migration::verify(root, plan, cell).map_err(Failure::policy)?;
    let status = evidence.status;
    atomic_write(output, evidence.encode_pretty().map_err(Failure::invalid)?)?;
    writeln_io(
        stdout,
        format_args!("{} {}", status.as_str(), output.display()),
    )?;
    writeln_io(stdout, format_args!("artifact {}", output.display()))?;
    writeln_io(stdout, format_args!("cell {cell}"))?;
    let migration_status = crate::migration::status(root).map_err(Failure::io)?;
    writeln_io(
        stdout,
        format_args!(
            "active={} state={} required_cells={} remaining_evidence={}",
            migration_status.active_plan.as_deref().unwrap_or("none"),
            migration_status.active_state.as_str(),
            migration_status.required_cells.len(),
            migration_status.remaining_evidence
        ),
    )?;
    let next = vec![
        "deshell".to_owned(),
        "migrate".to_owned(),
        "evidence".to_owned(),
        "import".to_owned(),
        "--root".to_owned(),
        root.to_string_lossy().into_owned(),
        "--plan".to_owned(),
        plan.to_owned(),
        output.to_string_lossy().into_owned(),
    ];
    writeln_io(
        stdout,
        format_args!(
            "next argv: {}",
            serde_json::to_string(&next).map_err(|error| Failure::internal(error.to_string()))?
        ),
    )?;
    match status {
        crate::migration::EvidenceStatus::Verified => Ok(0),
        crate::migration::EvidenceStatus::Different
        | crate::migration::EvidenceStatus::Nondeterministic => Err(Failure::difference(format!(
            "migration verification was {} (Evidence: {})",
            status.as_str(),
            output.display()
        ))),
        crate::migration::EvidenceStatus::Unavailable => Err(Failure::unavailable(format!(
            "migration verification was unavailable (Evidence: {})",
            output.display()
        ))),
        crate::migration::EvidenceStatus::Failed => Err(Failure::io(format!(
            "migration verification failed (Evidence: {})",
            output.display()
        ))),
    }
}

fn migrate_evidence_import_command(
    root: &Path,
    plan: &str,
    files: &[PathBuf],
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let digests =
        crate::migration::import_evidence(root, plan, files).map_err(classify_project_error)?;
    for digest in digests {
        writeln_io(stdout, format_args!("imported {digest}"))?;
    }
    for file in files {
        writeln_io(stdout, format_args!("artifact {}", file.display()))?;
    }
    let status = crate::migration::status(root).map_err(Failure::io)?;
    writeln_io(
        stdout,
        format_args!(
            "active={} state={} verified={} planned={} required_cells={} remaining_evidence={}",
            status.active_plan.as_deref().unwrap_or("none"),
            status.active_state.as_str(),
            status.verified,
            status.planned,
            status.required_cells.len(),
            status.remaining_evidence
        ),
    )?;
    writeln_io(
        stdout,
        format_args!(
            "next argv: {}",
            serde_json::to_string(&status.next_argv)
                .map_err(|error| Failure::internal(error.to_string()))?
        ),
    )?;
    Ok(0)
}

fn migrate_apply_command(root: &Path, plan: &str, stdout: &mut dyn Write) -> Result<i32, Failure> {
    crate::migration::apply(root, plan).map_err(Failure::policy)?;
    writeln_io(stdout, format_args!("retired migration plan {plan}"))?;
    writeln_io(
        stdout,
        format_args!("artifact .deshell/archive/manifest.json"),
    )?;
    let status = crate::migration::status(root).map_err(Failure::io)?;
    writeln_io(
        stdout,
        format_args!(
            "active={} state={} required_cells={} remaining_evidence={}",
            status.active_plan.as_deref().unwrap_or("none"),
            status.active_state.as_str(),
            status.required_cells.len(),
            status.remaining_evidence
        ),
    )?;
    writeln_io(
        stdout,
        format_args!(
            "next argv: {}",
            serde_json::to_string(&status.next_argv)
                .map_err(|error| Failure::internal(error.to_string()))?
        ),
    )?;
    Ok(0)
}

fn migrate_status_command(
    root: &Path,
    format: OutputFormat,
    stdout: &mut dyn Write,
) -> Result<i32, Failure> {
    let status = crate::migration::status(root).map_err(Failure::io)?;
    match format {
        OutputFormat::Json => write_io(
            stdout,
            &crate::canonical_json::pretty_bytes(
                &serde_json::to_value(status)
                    .map_err(|error| Failure::internal(error.to_string()))?,
            )
            .map_err(Failure::internal)?,
        )?,
        OutputFormat::Human => {
            writeln_io(
                stdout,
                format_args!(
                    "live={} blocked={} planned={} verified={} retired={} archived={}",
                    status.live,
                    status.blocked,
                    status.planned,
                    status.verified,
                    status.retired,
                    status.archived
                ),
            )?;
            if let Some(active) = &status.active_plan {
                writeln_io(
                    stdout,
                    format_args!("artifact .deshell/migrations/sha256/{active}/plan.json"),
                )?;
            }
            writeln_io(
                stdout,
                format_args!(
                    "active={} state={} stale={} history={} superseded={}",
                    status.active_plan.as_deref().unwrap_or("none"),
                    status.active_state.as_str(),
                    status.stale,
                    status.history,
                    status.superseded
                ),
            )?;
            for cell in &status.required_cells {
                writeln_io(stdout, format_args!("required cell {cell}"))?;
            }
            writeln_io(
                stdout,
                format_args!(
                    "required_cells={} remaining_evidence={}",
                    status.required_cells.len(),
                    status.remaining_evidence
                ),
            )?;
            writeln_io(stdout, format_args!("next: {}", status.next))?;
            writeln_io(
                stdout,
                format_args!(
                    "next argv: {}",
                    serde_json::to_string(&status.next_argv)
                        .map_err(|error| Failure::internal(error.to_string()))?
                ),
            )?;
        }
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

fn atomic_write(path: &Path, contents: Vec<u8>) -> Result<(), Failure> {
    let normalized;
    let path = if path
        .parent()
        .is_some_and(|parent| parent.as_os_str().is_empty())
    {
        normalized = Path::new(".").join(path);
        normalized.as_path()
    } else {
        path
    };
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

    #[test]
    fn report_formats_share_status_counts_actions_and_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(directory.path().join("build.sh"), "#!/bin/sh\ntrue\n").unwrap();
        let initialized = invoke_owned(vec![
            "deshell".into(),
            "init".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(initialized.0, 0);
        assert!(initialized.2.is_empty());
        let init: serde_json::Value = crate::strict_json::parse(&initialized.1).unwrap();
        assert_eq!(init["schema_version"], 1);
        assert_eq!(init["command"], "init");
        assert_eq!(init["status"], "not_ready");
        assert_eq!(init["details"]["values"]["target"], "rust");
        assert_eq!(init["details"]["values"]["module_root"], "src/bin");
        assert_eq!(init["next_actions"].as_array().unwrap().len(), 2);

        let json = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        let human = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "human".into(),
        ]);
        assert_eq!(json.0, human.0);
        assert!(json.2.is_empty() && human.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&json.1).unwrap();
        assert_eq!(report["command"], "scan");
        assert_eq!(report["status"], "ok");
        let counts = &report["details"]["counts"];
        assert_eq!(counts["findings"], 1);
        let human = String::from_utf8(human.1).unwrap();
        assert!(human.starts_with(report["summary"].as_str().unwrap()));
        assert!(human.contains("1 shell location(s) found; 0 skipped; 0 error(s)"));
    }

    #[test]
    fn report_families_cover_review_approval_and_active_plan_states() {
        fn json_report(result: &(i32, Vec<u8>, Vec<u8>)) -> serde_json::Value {
            assert!(
                result.2.is_empty(),
                "{}",
                String::from_utf8_lossy(&result.2)
            );
            crate::strict_json::parse(&result.1).unwrap()
        }

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(
            directory.path().join("build.sh"),
            "#!/bin/sh\n/usr/bin/printf report\n",
        )
        .unwrap();
        let root = path(directory.path());
        let initialized = invoke_owned(vec![
            "deshell".into(),
            "init".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(initialized.0, 0);

        let scenarios = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "list".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(scenarios.0, 0);
        let scenarios = json_report(&scenarios);
        assert_eq!(scenarios["status"], "not_ready");
        let scenario_line = scenarios["details"]["output"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|line| line.contains("\tdraft\t"))
            .unwrap();
        let scenario_fields = scenario_line.split('\t').collect::<Vec<_>>();
        let scenario = scenario_fields[0].to_owned();
        let scenario_digest = scenario_fields[2].to_owned();

        let shown = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "show".into(),
            "--root".into(),
            root.clone(),
            "--name".into(),
            scenario.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(shown.0, 0);
        assert_eq!(json_report(&shown)["status"], "not_ready");

        let rejected = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "approve".into(),
            "--root".into(),
            root.clone(),
            "--name".into(),
            scenario.clone(),
            "--digest".into(),
            format!("sha256:{}", "0".repeat(64)),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(rejected.0, 4);
        assert_eq!(json_report(&rejected)["status"], "blocked");

        let approved = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "approve".into(),
            "--root".into(),
            root.clone(),
            "--name".into(),
            scenario,
            "--digest".into(),
            scenario_digest,
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(approved.0, 0);
        assert_eq!(json_report(&approved)["status"], "ok");

        let matrix = invoke_owned(vec![
            "deshell".into(),
            "matrix".into(),
            "list".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(matrix.0, 0);
        let matrix = json_report(&matrix);
        assert_eq!(matrix["status"], "not_ready");
        let matrix_line = matrix["details"]["output"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|line| line.contains("\tdraft\t"))
            .unwrap();
        let matrix_fields = matrix_line.split('\t').collect::<Vec<_>>();
        let cell = matrix_fields[0].to_owned();
        let cell_digest = matrix_fields[2].to_owned();
        let approved = invoke_owned(vec![
            "deshell".into(),
            "matrix".into(),
            "approve".into(),
            "--root".into(),
            root.clone(),
            "--cell".into(),
            cell,
            "--digest".into(),
            cell_digest,
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(approved.0, 0);
        assert_eq!(json_report(&approved)["status"], "ok");

        for arguments in [
            vec!["analyze", "--root", root.as_str(), "--format", "json"],
            vec!["check", "--root", root.as_str(), "--format", "json"],
            vec!["audit", "--root", root.as_str(), "--format", "json"],
            vec!["explain", "--root", root.as_str(), "--format", "json"],
            vec![
                "rewrite",
                "--root",
                root.as_str(),
                "--equivalent",
                "--format",
                "json",
            ],
            vec![
                "modernize",
                "--root",
                root.as_str(),
                "--profile",
                "secure",
                "--format",
                "json",
            ],
            vec!["doctor", "--root", root.as_str(), "--format", "json"],
            vec![
                "harden",
                "plan",
                "--root",
                root.as_str(),
                "--format",
                "json",
            ],
        ] {
            let mut argv = vec!["deshell".to_owned()];
            argv.extend(arguments.into_iter().map(str::to_owned));
            let result = invoke_owned(argv);
            assert_eq!(result.0, 0, "{}", String::from_utf8_lossy(&result.2));
            assert_eq!(json_report(&result)["schema_version"], 1);
        }

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let planned = json_report(&planned);
        assert_eq!(planned["status"], "ok");
        assert!(
            planned["details"]["paths"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path
                    .as_str()
                    .is_some_and(|path| path.ends_with("/plan.json")))
        );

        let status = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "status".into(),
            "--root".into(),
            root,
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(status.0, 0);
        let status = json_report(&status);
        assert_eq!(status["details"]["values"]["state"], "planned");
        assert!(!status["next_actions"].as_array().unwrap().is_empty());
    }

    #[test]
    fn ambiguous_init_returns_three_exact_argv_actions_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("build.sh"), "#!/bin/sh\ntrue\n").unwrap();
        let root = path(directory.path());
        let initialized = invoke_owned(vec![
            "deshell".into(),
            "init".into(),
            "--root".into(),
            root.clone(),
            "--diagnostics=jsonl".into(),
        ]);
        assert_eq!(initialized.0, 2);
        assert!(initialized.1.is_empty());
        let diagnostic: serde_json::Value = crate::strict_json::parse(&initialized.2).unwrap();
        assert_eq!(diagnostic["code"], "DESHELL_USAGE");
        let actions = diagnostic["next_actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3);
        for (action, target) in actions.iter().zip(["rust", "go", "host"]) {
            assert_eq!(action["action"], "command");
            assert_eq!(
                action["argv"],
                serde_json::json!([
                    "deshell",
                    "init",
                    "--root",
                    root.clone(),
                    "--target",
                    target
                ])
            );
        }
        assert!(!directory.path().join(".deshell").exists());
        assert!(!directory.path().join("deshell.lock").exists());
    }

    #[test]
    fn scan_read_errors_are_completed_reports_and_draft_check_is_not_ready() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        std::fs::write(
            directory.path().join(".github/workflows/broken.yml"),
            "jobs: [unterminated\n",
        )
        .unwrap();
        let scanned = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(scanned.0, 1);
        assert!(scanned.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&scanned.1).unwrap();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["details"]["counts"]["errors"], 1);

        std::fs::remove_file(directory.path().join(".github/workflows/broken.yml")).unwrap();
        let checked = invoke_owned(vec![
            "deshell".into(),
            "check".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(checked.0, 0);
        assert!(checked.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&checked.1).unwrap();
        assert_eq!(report["status"], "not_ready");
        assert!(
            report["details"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "not_ready")
        );
    }

    #[test]
    fn zero_audit_jsonl_is_empty_while_reports_keep_the_summary() {
        let directory = tempfile::tempdir().unwrap();
        let jsonl = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "jsonl".into(),
        ]);
        assert_eq!(jsonl.0, 0);
        assert!(jsonl.1.is_empty() && jsonl.2.is_empty());
        let json = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(json.0, 0);
        let report: serde_json::Value = crate::strict_json::parse(&json.1).unwrap();
        assert_eq!(report["details"]["counts"]["findings"], 0);
        assert!(
            report["details"]["output"]
                .as_array()
                .unwrap()
                .iter()
                .any(|line| line
                    .as_str()
                    .is_some_and(|line| line.contains("0 audit finding(s)")))
        );
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
        assert!(help.contains("migration oracle"), "{help}");
        assert!(!help.contains("behavioral compiler"), "{help}");
        for command in [
            "init",
            "audit",
            "scenario",
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
            "harden",
        ] {
            assert!(help.contains(command), "help omitted {command}");
        }
    }

    #[test]
    fn shell_free_gate_rejects_live_shell_and_accepts_an_empty_live_tree() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("live.sh"),
            b"#!/bin/sh\nprintf live\n",
        )
        .unwrap();
        let root = path(directory.path());
        let blocked = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(blocked.0, 4, "{}", String::from_utf8_lossy(&blocked.2));
        assert!(blocked.2.is_empty());
        assert!(String::from_utf8_lossy(&blocked.1).contains("DESHELL_SHELL_REINTRODUCED"));

        std::fs::remove_file(directory.path().join("live.sh")).unwrap();
        let clean = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(clean.0, 0, "{}", String::from_utf8_lossy(&clean.2));
    }

    #[test]
    fn scenario_synthesis_is_preview_first_and_persists_only_drafts() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\nprintf '%s' \"$MODE\"\n",
        );
        let root = path(directory.path());
        let preview = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "synthesize".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(preview.0, 0, "{}", String::from_utf8_lossy(&preview.2));
        assert!(String::from_utf8_lossy(&preview.1).contains("approval = \"draft\""));
        assert!(
            !directory
                .path()
                .join(".deshell/scenarios/synthesized-build.toml")
                .exists()
        );

        let applied = invoke_owned(vec![
            "deshell".into(),
            "scenario".into(),
            "synthesize".into(),
            "--root".into(),
            root,
            "--apply".into(),
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        let persisted = std::fs::read_to_string(
            directory
                .path()
                .join(".deshell/scenarios/synthesized-build.toml"),
        )
        .unwrap();
        assert!(persisted.contains("approval = \"draft\""));
    }

    #[test]
    fn audit_ignores_comments_and_heredoc_data_and_reports_exact_risk_spans() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = concat!(
            "#!/usr/bin/env bash\n",
            "# curl https://comment.invalid/x | sh\n",
            "cat <<'DATA'\n",
            "eval \"$HEREDOC\"\n",
            "DATA\n",
            "eval \"$DYNAMIC\"\n",
            "printf '%s\\n' \"$API_TOKEN\"\n",
            "rm -rf \"$TARGET\"\n",
        );
        configure(directory.path(), "audit.sh", source.as_bytes());
        let result = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "jsonl".into(),
        ]);
        assert_eq!(result.0, 4, "{}", String::from_utf8_lossy(&result.2));
        let findings = result
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| crate::strict_json::parse(line).unwrap())
            .collect::<Vec<_>>();
        let rules = findings
            .iter()
            .map(|finding| finding["rule_id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            rules,
            [
                "shell.dynamic-eval",
                "secret.argv-exposure",
                "filesystem.dangerous-delete"
            ]
        );
        assert!(!String::from_utf8_lossy(&result.1).contains("comment.invalid"));
        assert!(!String::from_utf8_lossy(&result.1).contains("HEREDOC"));
        for finding in &findings {
            assert_eq!(finding["schema_version"], 1);
            assert_eq!(finding["confidence"], "high");
            assert!(finding["url"].as_str().unwrap().starts_with("https://"));
            let start = finding["span"]["start_byte"].as_u64().unwrap() as usize;
            let end = finding["span"]["end_byte"].as_u64().unwrap() as usize;
            assert!(start < end);
            let selected = &source.as_bytes()[start..end];
            assert!(
                selected.starts_with(b"eval")
                    || selected.starts_with(b"$API_TOKEN")
                    || selected.starts_with(b"rm -rf"),
                "unexpected span: {}",
                String::from_utf8_lossy(selected)
            );
        }
    }

    #[test]
    fn audit_ignores_comments_for_every_supported_interpreter() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        for (path, source) in [
            ("comment.sh", "#!/bin/sh\n# eval \"$SH_COMMENT\"\n"),
            (
                "comment.bash",
                "#!/usr/bin/env bash\n# eval \"$BASH_COMMENT\"\n",
            ),
            (
                "comment.zsh",
                "#!/usr/bin/env zsh\n# eval \"$ZSH_COMMENT\"\n",
            ),
            (
                "comment.fish",
                "#!/usr/bin/env fish\n# eval $FISH_COMMENT\n",
            ),
            (
                "comment.ps1",
                concat!(
                    "# eval $POWERSHELL_COMMENT\n",
                    "<# eval $POWERSHELL_BLOCK_COMMENT #>\n",
                    "@'\n",
                    "eval $POWERSHELL_HERE_STRING\n",
                    "'@\n",
                ),
            ),
            (
                "comment.cmd",
                "@echo off\r\nREM eval %CMD_COMMENT%\r\n:: eval %CMD_LABEL_COMMENT%\r\n",
            ),
            ("comment.nu", "#!/usr/bin/env nu\n# eval $NU_COMMENT\n"),
        ] {
            std::fs::write(directory.path().join(path), source).unwrap();
        }

        let result = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "jsonl".into(),
        ]);
        assert_eq!(result.0, 0, "{}", String::from_utf8_lossy(&result.2));
        assert!(
            result.1.is_empty(),
            "{}",
            String::from_utf8_lossy(&result.1)
        );
    }

    #[test]
    fn audit_maps_embedded_shell_risks_to_host_bytes_and_ignores_comments() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let workflow = concat!(
            "jobs:\n",
            "  audit:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - run: |\n",
            "          # curl https://comment.invalid/tool | sh\n",
            "          eval \"$DYNAMIC\"\n",
        );
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        std::fs::write(
            directory.path().join(".github/workflows/audit.yml"),
            workflow,
        )
        .unwrap();

        let result = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "jsonl".into(),
        ]);
        assert_eq!(result.0, 4, "{}", String::from_utf8_lossy(&result.2));
        let findings = result
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| crate::strict_json::parse(line).unwrap())
            .collect::<Vec<serde_json::Value>>();
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0]["rule_id"], "shell.dynamic-eval");
        let start = findings[0]["span"]["start_byte"].as_u64().unwrap() as usize;
        let end = findings[0]["span"]["end_byte"].as_u64().unwrap() as usize;
        assert_eq!(&workflow[start..end], "eval");
    }

    #[test]
    fn audit_reports_splitting_glob_symlink_and_toctou_with_exact_spans() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = concat!(
            "#!/bin/sh\n",
            "rm -rf $TARGET/*.tmp\n",
            "ln -s \"$TARGET\" \"$LINK\"\n",
            "test -e \"$TARGET\"\n",
        );
        configure(directory.path(), "risk.sh", source.as_bytes());

        let result = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "jsonl".into(),
        ]);
        assert_eq!(result.0, 4, "{}", String::from_utf8_lossy(&result.2));
        let findings = result
            .1
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| crate::strict_json::parse(line).unwrap())
            .collect::<Vec<serde_json::Value>>();
        for (rule, selected) in [
            ("filesystem.unquoted-expansion", "$TARGET"),
            ("filesystem.unbounded-glob", "$TARGET/*.tmp"),
            ("filesystem.symlink-race", "ln -s"),
            ("filesystem.toctou-check", "test -e"),
        ] {
            let finding = findings
                .iter()
                .find(|finding| finding["rule_id"] == rule)
                .unwrap_or_else(|| panic!("missing {rule}: {findings:#?}"));
            let start = finding["span"]["start_byte"].as_u64().unwrap() as usize;
            let end = finding["span"]["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(&source[start..end], selected, "{rule}");
        }
    }

    #[test]
    fn audit_sarif_and_github_formats_are_derived_from_the_same_finding() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "audit.sh", b"#!/bin/sh\neval \"$X\"\n");
        let root = path(directory.path());
        let sarif = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "sarif".into(),
        ]);
        assert_eq!(sarif.0, 4);
        let document: serde_json::Value = crate::strict_json::parse(&sarif.1).unwrap();
        assert_eq!(document["version"], "2.1.0");
        assert_eq!(
            document["runs"][0]["results"][0]["ruleId"],
            "shell.dynamic-eval"
        );

        let github = invoke_owned(vec![
            "deshell".into(),
            "audit".into(),
            "--root".into(),
            root,
            "--format".into(),
            "github".into(),
        ]);
        assert_eq!(github.0, 4);
        assert!(String::from_utf8(github.1).unwrap().starts_with(
            "::error file=audit.sh,line=2,col=1,endLine=2,endColumn=5,title=shell.dynamic-eval::"
        ));
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
            "approval",
            "migration-index",
            "init-report",
            "scan-report",
            "scenario-report",
            "matrix-report",
            "audit-report",
            "analyze-report",
            "check-report",
            "verify-report",
            "observe-report",
            "doctor-report",
            "explain-report",
            "rewrite-report",
            "modernize-report",
            "harden-report",
            "migrate-report",
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
            "generator-protocol",
            "migration-request",
            "proposal",
            "migration-plan",
            "migration-evidence",
            "archive-manifest",
            "audit-finding",
            "harden-plan",
            "harden-approval",
            "harden-evidence",
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
    fn analyze_uses_the_same_explicit_interpreter_resolution_as_scan() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "automation",
            b"/usr/bin/printf configured\n",
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "interpreter_overrides = []",
            "interpreter_overrides = [{ path = \"automation\", interpreter = \"sh\" }]",
        );
        std::fs::write(config_path, config).unwrap();

        let analyzed = invoke_owned(vec![
            "deshell".into(),
            "analyze".into(),
            "--root".into(),
            path(directory.path()),
            "--entry".into(),
            "automation".into(),
        ]);
        assert_eq!(analyzed.0, 0, "{}", String::from_utf8_lossy(&analyzed.2));
        let manifest = crate::project::load_manifest(directory.path()).unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].entrypoint, "automation");
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
    fn scan_uses_an_explicit_interpreter_for_an_extensionless_source() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("automation"),
            b"/usr/bin/printf configured\n",
        )
        .unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("entrypoints = []", "entrypoints = [\"automation\"]")
            .replace(
                "interpreter_overrides = []",
                "interpreter_overrides = [{ path = \"automation\", interpreter = \"sh\" }]",
            );
        std::fs::write(config_path, config).unwrap();

        let scan = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(scan.0, 0, "{}", String::from_utf8_lossy(&scan.2));
        assert!(scan.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&scan.1).unwrap();
        assert_eq!(report["command"], "scan");
        assert_eq!(report["details"]["counts"]["errors"], 0);
        let findings = report["details"]["items"].as_array().unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["path"], "automation");
        assert_eq!(findings[0]["kind"], "shell_file");
        assert_eq!(findings[0]["name"], "sh");
        assert_eq!(findings[0]["status"], "high");
    }

    #[test]
    fn scan_blocks_an_explicit_interpreter_that_conflicts_with_the_source() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("automation.bash"),
            b"/usr/bin/printf configured\n",
        )
        .unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "interpreter_overrides = []",
            "interpreter_overrides = [{ path = \"automation.bash\", interpreter = \"sh\" }]",
        );
        std::fs::write(config_path, config).unwrap();

        let scan = invoke_owned(vec![
            "deshell".into(),
            "scan".into(),
            "--root".into(),
            path(directory.path()),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(scan.0, 1, "{}", String::from_utf8_lossy(&scan.2));
        assert!(scan.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&scan.1).unwrap();
        let errors = report["details"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["kind"] == "error")
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["path"], "automation.bash");
        assert_eq!(errors[0]["name"], "interpreter");
        assert!(
            errors[0]["message"]
                .as_str()
                .unwrap()
                .contains("DESHELL_BLOCKER_INTERPRETER_CONFLICT")
        );
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
        let report: serde_json::Value = crate::strict_json::parse(&scan.1).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "scan");
        assert_eq!(report["details"]["items"][0]["path"], "build.sh");
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
        assert_eq!(doctor.0, 0);
        assert!(doctor.2.is_empty());
        let report: serde_json::Value = crate::strict_json::parse(&doctor.1).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "doctor");
        assert_eq!(report["status"], "ok");
        assert!(
            report["details"]["output"]
                .as_array()
                .unwrap()
                .iter()
                .any(|line| line.as_str().is_some_and(|line| line.contains("not ready")))
        );
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

    fn approve_oracle_inputs(root: &Path) {
        let config_path = root.join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "platform_cells = []",
                &format!(
                    "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}]",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ),
            );
        std::fs::write(config_path, config).unwrap();
        let scenario_path = root.join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("approval = \"draft\"", "approval = \"approved\"");
        std::fs::write(scenario_path, scenario).unwrap();
        std::fs::create_dir_all(root.join("src/bin")).unwrap();
    }

    #[test]
    fn migration_plan_is_content_addressed_preview_only_and_generator_cannot_delete() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/usr/bin/env bash\n/usr/bin/printf '%s\\n' hello | /usr/bin/tr a-z A-Z\n",
        );
        approve_oracle_inputs(directory.path());
        let result = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(result.0, 0, "{}", String::from_utf8_lossy(&result.2));
        let output = String::from_utf8(result.1).unwrap();
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        assert!(crate::digest::valid_sha256(digest));
        assert!(output.contains("+++ b/src/bin/deshell_build.rs"));
        assert!(directory.path().join("build.sh").is_file());
        assert!(!directory.path().join("src/bin/deshell_build.rs").exists());
        assert!(!directory.path().join(".deshell/archive").exists());

        let plan_path = directory
            .path()
            .join(format!(".deshell/migrations/sha256/{digest}/plan.json"));
        let plan: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(plan_path).unwrap()).unwrap();
        assert_eq!(plan["plan_digest"], digest);
        assert!(plan["blockers"].as_array().unwrap().is_empty());
        assert_eq!(plan["coverage"]["delegated_bytes"], 0);
        assert_eq!(plan["coverage"]["residual_bytes"], 0);
        assert_eq!(plan["sources"].as_array().unwrap().len(), 1);
        let proposal_digest = plan["proposals"][0].as_str().unwrap();
        let proposal_path = directory.path().join(format!(
            ".deshell/migrations/sha256/{digest}/proposals/{proposal_digest}.json"
        ));
        let proposal: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&proposal_path).unwrap()).unwrap();
        assert_eq!(proposal["patches"][0]["operation"], "create");
        assert_eq!(proposal["patches"][0]["path"], "src/bin/deshell_build.rs");
        assert!(
            !std::fs::read_to_string(proposal_path)
                .unwrap()
                .contains("delete")
        );
    }

    #[test]
    fn migration_plan_allows_an_official_module_root_that_apply_will_create() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf ok\n",
        );
        approve_oracle_inputs(directory.path());
        std::fs::remove_dir(directory.path().join("src/bin")).unwrap();
        std::fs::remove_dir(directory.path().join("src")).unwrap();

        let plan = crate::migration::create_plan(directory.path()).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert!(!directory.path().join("src").exists());
        let artifact = crate::strict_json::parse(
            &std::fs::read(directory.path().join(&plan.artifact_path)).unwrap(),
        )
        .unwrap();
        let proposal_digest = artifact["sources"][0]["proposal_digest"].as_str().unwrap();
        let proposal = crate::strict_json::parse(
            &std::fs::read(
                directory
                    .path()
                    .join(".deshell/migrations/sha256")
                    .join(&plan.digest)
                    .join("proposals")
                    .join(format!("{proposal_digest}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(proposal["patches"].as_array().unwrap().iter().any(|patch| {
            patch["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("src/bin/"))
        }));
    }

    #[test]
    fn migration_status_ignores_superseded_blockers() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\neval \"$DYNAMIC\"\n",
        );
        approve_oracle_inputs(directory.path());
        let blocked = crate::migration::create_plan(directory.path()).unwrap();
        assert!(!blocked.blockers.is_empty());

        std::fs::write(
            directory.path().join("build.sh"),
            b"#!/bin/sh\n/usr/bin/printf ok\n",
        )
        .unwrap();
        let ready = crate::migration::create_plan(directory.path()).unwrap();
        assert!(ready.blockers.is_empty(), "{:?}", ready.blockers);
        assert_ne!(blocked.digest, ready.digest);

        let status = crate::migration::status(directory.path()).unwrap();
        assert_eq!(status.active_plan.as_deref(), Some(ready.digest.as_str()));
        assert_eq!(status.active_state, crate::migration::ActiveState::Planned);
        assert_eq!(status.blocked, 0);
        assert_eq!(status.planned, 1);
        assert_eq!(status.verified, 0);
        assert_eq!(status.history, 2);
        assert_eq!(status.superseded, 1);
        assert_eq!(
            status.next_argv.get(1..3),
            Some(["migrate".to_owned(), "verify".to_owned()].as_slice())
        );
    }

    #[test]
    fn every_scenario_and_matrix_draft_must_have_a_current_approval() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf ok\n",
        );
        approve_oracle_inputs(directory.path());

        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap();
        let approved_cell = format!(
            "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}]",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        let cells = format!(
            "platform_cells = [{{ id = \"host\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"approved\" }}, {{ id = \"pending\", operating_system = \"{}\", architecture = \"{}\", runtime = \"native\", approval = \"draft\" }}]",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert!(config.contains(&approved_cell));
        std::fs::write(&config_path, config.replace(&approved_cell, &cells)).unwrap();

        let default_path = directory.path().join(".deshell/scenarios/default.toml");
        let pending = std::fs::read_to_string(default_path)
            .unwrap()
            .replace("name = \"default\"", "name = \"pending\"")
            .replace("approval = \"approved\"", "approval = \"draft\"");
        std::fs::write(
            directory.path().join(".deshell/scenarios/pending.toml"),
            pending,
        )
        .unwrap();

        let readiness = crate::project::check_readiness(directory.path()).unwrap();
        assert!(!readiness.ready);
        assert!(
            readiness
                .reasons
                .iter()
                .any(|reason| reason.contains("1 of 2 scenario approval(s)"))
        );
        assert!(
            readiness
                .reasons
                .iter()
                .any(|reason| reason.contains("1 of 2 matrix approval(s)"))
        );

        let blocked = crate::migration::create_plan(directory.path()).unwrap();
        assert!(blocked.blockers.iter().any(|blocker| {
            blocker.code == "DESHELL_BLOCKER_UNAPPROVED_SCENARIO"
                && blocker.message.contains("scenario pending")
        }));
        let status = crate::migration::status(directory.path()).unwrap();
        assert_eq!(status.active_state, crate::migration::ActiveState::Blocked);
        assert_eq!(
            status.next_argv.get(1..3),
            Some(["scenario".to_owned(), "approve".to_owned()].as_slice())
        );

        let review = crate::approval::scenario_reviews(directory.path())
            .unwrap()
            .into_iter()
            .find(|review| review.name == "pending")
            .unwrap();
        crate::approval::approve_scenario(directory.path(), "pending", &review.digest).unwrap();
        let status = crate::migration::status(directory.path()).unwrap();
        assert_eq!(
            status.next_argv.get(1..3),
            Some(["matrix".to_owned(), "approve".to_owned()].as_slice())
        );
        let review = crate::approval::matrix_reviews(directory.path())
            .unwrap()
            .into_iter()
            .find(|review| review.name == "pending")
            .unwrap();
        crate::approval::approve_matrix(directory.path(), "pending", &review.digest).unwrap();
        let readiness = crate::project::check_readiness(directory.path()).unwrap();
        assert!(
            readiness
                .reasons
                .iter()
                .all(|reason| !reason.contains("scenario") && !reason.contains("matrix")),
            "{:?}",
            readiness.reasons
        );
        let ready = crate::migration::create_plan(directory.path()).unwrap();
        assert!(ready.blockers.is_empty(), "{:?}", ready.blockers);
    }

    #[test]
    fn migration_plan_blocks_when_approved_scenarios_omit_an_input_boundary() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "inputs.sh",
            b"#!/bin/sh\n/usr/bin/printf '%s:%s\\n' \"$1\" \"$TOKEN\"\n",
        );
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(
            output.contains("DESHELL_BLOCKER_SCENARIO_INPUT_COVERAGE"),
            "{output}"
        );
        assert!(output.contains("argument 1"), "{output}");
        assert!(output.contains("environment TOKEN"), "{output}");

        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace(
                "arguments = []",
                "arguments = [{ name = \"1\", value = \"value\" }]",
            )
            .replace(
                "environment = []",
                "environment = [{ name = \"TOKEN\", value = \"secret\" }]",
            );
        std::fs::write(scenario_path, scenario).unwrap();
        let covered = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(covered.0, 0, "{}", String::from_utf8_lossy(&covered.2));
        let covered_output = String::from_utf8(covered.1).unwrap();
        assert!(
            !covered_output.contains("DESHELL_BLOCKER_SCENARIO_INPUT_COVERAGE"),
            "{covered_output}"
        );
    }

    #[test]
    fn migration_plan_never_fabricates_empty_network_replay_evidence() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "download.sh",
            b"#!/bin/sh\n/usr/bin/curl https://example.invalid/artifact\n",
        );
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(
            output.contains("DESHELL_BLOCKER_NETWORK_REPLAY_UNAVAILABLE"),
            "{output}"
        );
        assert!(output.contains("download.sh"), "{output}");
    }

    #[cfg(unix)]
    #[test]
    fn migration_replay_proxy_records_and_compares_network_sequence_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "download.sh",
            b"#!/bin/sh\n/usr/bin/curl --silent http://artifact.test/data\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"network-replay-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        let response = b"replayed artifact\n";
        let replay = crate::replay::ReplayStore {
            schema_version: 1,
            entries: vec![crate::replay::ReplayEntry {
                method: "GET".into(),
                uri: "http://artifact.test/data".into(),
                request_body_sha256: crate::digest::sha256(b""),
                status: 200,
                headers: vec![crate::replay::Header {
                    name: "content-type".into(),
                    value: "application/octet-stream".into(),
                }],
                body: crate::ir::SourceBytes::from_bytes(response),
            }],
        };
        std::fs::write(
            directory.path().join(".deshell/replay.json"),
            replay.encode_pretty().unwrap(),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("processes = 512", "processes = 60000");
        std::fs::write(scenario_path, scenario).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        let plan_path = directory.path().join(format!(
            ".deshell/migrations/sha256/{}/plan.json",
            planned.digest
        ));
        let plan: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(plan_path).unwrap()).unwrap();
        assert_eq!(
            plan["network_replay_digest"],
            crate::digest::sha256(&replay.encode_pretty().unwrap())
        );

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(
            evidence.status,
            crate::migration::EvidenceStatus::Verified,
            "{evidence:#?}"
        );
        for comparison in &evidence.checks[0].comparisons {
            for observation in [
                &comparison.original,
                &comparison.ir,
                &comparison.replacement,
            ] {
                assert_eq!(observation.network.len(), 1);
                assert_eq!(observation.network[0].sequence, 0);
                assert_eq!(observation.network[0].method, "GET");
                assert_eq!(observation.network[0].uri, "http://artifact.test/data");
                assert_eq!(
                    observation.network[0].request_body_sha256,
                    crate::digest::sha256(b"")
                );
                assert_eq!(
                    observation.network[0].response_body_sha256,
                    crate::digest::sha256(response)
                );
            }
        }
        let mut invalid_sequence = evidence.clone();
        invalid_sequence.checks[0].comparisons[0].original.network[0].sequence = 1;
        assert!(invalid_sequence.encode_pretty().is_err());
        let evidence_path = directory.path().join("network-evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        let replay_path = directory.path().join(".deshell/replay.json");
        let canonical_replay = replay.encode_pretty().unwrap();
        let mut changed_replay = replay.clone();
        changed_replay.entries[0].body = crate::ir::SourceBytes::from_bytes(b"changed\n");
        std::fs::write(&replay_path, changed_replay.encode_pretty().unwrap()).unwrap();
        let stale = crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap_err();
        assert!(stale.contains("DESHELL_BLOCKER_STALE_NETWORK_REPLAY"));
        std::fs::write(replay_path, canonical_replay).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();
        assert!(!directory.path().join("download.sh").exists());
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn migration_plan_lowers_an_extensionless_source_with_its_configured_interpreter() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "automation",
            b"/usr/bin/printf configured\n",
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "interpreter_overrides = []",
            "interpreter_overrides = [{ path = \"automation\", interpreter = \"sh\" }]",
        );
        std::fs::write(config_path, config).unwrap();
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(!output.contains("blocker"), "{output}");
        assert!(
            output.contains("+++ b/src/bin/deshell_automation.rs"),
            "{output}"
        );
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("configured-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
            "--plan".into(),
            digest,
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let evidence: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(evidence_path).unwrap()).unwrap();
        assert_eq!(evidence["status"], "verified");
    }

    #[cfg(unix)]
    #[test]
    fn migration_plan_accepts_a_digest_pinned_external_json_rpc_generator() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "external.sh",
            b"#!/bin/sh\n/usr/bin/printf '%s' \"$TOKEN\"\n",
        );
        approve_oracle_inputs(directory.path());
        let scenario_path = directory.path().join(".deshell/scenarios/default.toml");
        let scenario = std::fs::read_to_string(&scenario_path).unwrap().replace(
            "environment = []",
            "environment = [{ name = \"TOKEN\", value = \"top-secret\" }]",
        );
        std::fs::write(scenario_path, scenario).unwrap();
        let generator_directory = directory.path().join(".deshell/generators");
        std::fs::create_dir(&generator_directory).unwrap();
        let generator_path = generator_directory.join("fixture");
        let generator = br##"#!/usr/bin/python3
import base64, hashlib, json, pathlib, sys

ZERO = "0" * 64
SELF_DIGEST = "sha256:" + hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()

def canonical(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()

def respond(identifier, result):
    print(json.dumps({"id": identifier, "jsonrpc": "2.0", "result": result}, separators=(",", ":"), sort_keys=True), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    if request["method"] == "deshell.handshake":
        respond(request["id"], {
            "generator": {"capabilities": ["agent"], "digest": SELF_DIGEST, "name": "fixture", "version": "1"},
            "max_frame_bytes": 4194304,
            "protocol": "deshell.generator.v1",
            "schema_version": 1,
        })
        continue
    params = request["params"]
    migration = params["request"]
    assert "source_bytes" not in migration["source"]
    assert migration["interface"]["secrets"] == ["TOKEN"]
    assert "top-secret" not in json.dumps(migration, sort_keys=True)
    target = params["target_path"]
    generated = b"generated by fixture\n"
    ids = []
    def walk(value):
        if isinstance(value, dict):
            if isinstance(value.get("id"), str) and "operation" in value:
                ids.append(value["id"])
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    walk(migration["effect_ir"])
    proposal = {
        "build_argv": ["/usr/bin/true"],
        "dependencies": [],
        "generator_digest": SELF_DIGEST,
        "patches": [{
            "content_base64": base64.b64encode(generated).decode(),
            "content_digest": hashlib.sha256(generated).hexdigest(),
            "expected_digest": params["expected_digest"],
            "operation": "update" if params["expected_digest"] else "create",
            "path": target,
            "permissions": 493,
        }],
        "proposal_digest": ZERO,
        "request_digest": migration["request_id"],
        "run_argv": ["/usr/bin/true"],
        "schema_version": 1,
        "source_map": [{"generated": {"end_byte": len(generated), "path": target, "start_byte": 0}, "ir_node": identifier} for identifier in ids],
        "validation": params["validation"],
    }
    proposal["proposal_digest"] = hashlib.sha256(canonical(proposal)).hexdigest()
    respond(request["id"], proposal)
"##;
        std::fs::write(&generator_path, generator).unwrap();
        std::fs::set_permissions(&generator_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let pinned_digest = format!("sha256:{}", crate::digest::sha256(generator));
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"external:fixture\"", 1)
            .replacen("target = \"rust\"", "target = \"agent\"", 1)
            .replace(
                "external_generators = []",
                &format!(
                    concat!(
                        "external_generators = [{{ name = \"fixture\", executable = \".deshell/generators/fixture\", digest = \"{}\", capabilities = [\"agent\"] }}]"
                    ),
                    pinned_digest
                ),
            );
        std::fs::write(config_path, config).unwrap();

        let blocked = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(blocked.0, 4, "{}", String::from_utf8_lossy(&blocked.2));
        let blocked_output = String::from_utf8(blocked.1).unwrap();
        assert!(
            blocked_output.contains("DESHELL_BLOCKER_GENERATOR_NETWORK_POLICY"),
            "{blocked_output}"
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("allow_agent_network = false", "allow_agent_network = true");
        std::fs::write(config_path, config).unwrap();

        let source_blocked = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(
            source_blocked.0,
            4,
            "{}",
            String::from_utf8_lossy(&source_blocked.2)
        );
        let source_output = String::from_utf8(source_blocked.1).unwrap();
        assert!(
            source_output.contains("DESHELL_BLOCKER_GENERATOR_SOURCE_POLICY"),
            "{source_output}"
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("allow_source_send = false", "allow_source_send = true");
        std::fs::write(config_path, config).unwrap();

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(!output.contains("blocker"), "{output}");
        assert!(output.contains("+++ b/src/bin/external"), "{output}");
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        let plan: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/migrations/sha256/{digest}/plan.json")),
            )
            .unwrap(),
        )
        .unwrap();
        let proposal_digest = plan["proposals"][0].as_str().unwrap();
        let proposal: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(directory.path().join(format!(
                ".deshell/migrations/sha256/{digest}/proposals/{proposal_digest}.json"
            )))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(proposal["generator_digest"], pinned_digest);
    }

    #[test]
    fn migration_apply_refuses_the_whole_plan_when_one_blocker_exists() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "blocked.sh",
            b"#!/bin/sh\neval \"$DYNAMIC\"\n",
        );
        approve_oracle_inputs(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let before = std::fs::read(directory.path().join("blocked.sh")).unwrap();
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            path(directory.path()),
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 4);
        assert!(applied.2.is_empty());
        assert!(String::from_utf8_lossy(&applied.1).contains("DESHELL_BLOCKER_DYNAMIC_EVAL"));
        assert_eq!(
            std::fs::read(directory.path().join("blocked.sh")).unwrap(),
            before
        );
        assert!(!directory.path().join(".deshell/archive").exists());
        assert!(!directory.path().join("src/bin/deshell_blocked.rs").exists());
    }

    #[test]
    fn migration_plan_uses_a_stable_parser_error_blocker() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "broken.sh",
            b"#!/bin/sh\nif /usr/bin/true; then\n/usr/bin/printf missing-fi\n",
        );
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(output.contains("DESHELL_BLOCKER_PARSE_ERROR"), "{output}");
        assert!(output.contains("broken.sh"), "{output}");
    }

    #[test]
    fn migration_plan_uses_pinned_fish_and_batch_parsers() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("broken.fish"),
            b"if /usr/bin/true\n/usr/bin/printf missing-end\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("broken.cmd"),
            b"@echo off\r\nif exist input (\r\n@tool.exe missing-close\r\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        for path in ["broken.cmd", "broken.fish"] {
            let line = output
                .lines()
                .find(|line| line.contains(path))
                .unwrap_or_else(|| panic!("missing blocker for {path}: {output}"));
            assert!(line.contains("DESHELL_BLOCKER_PARSE_ERROR"), "{line}");
        }
    }

    #[test]
    fn migration_plan_uses_powershell_and_nushell_parsers() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::write(
            directory.path().join("broken.ps1"),
            b"function Invoke-Build {\n& 'tool.exe'\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("broken.nu"),
            b"if true {\n^tool missing-close\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        for path in ["broken.nu", "broken.ps1"] {
            let line = output
                .lines()
                .find(|line| line.contains(path))
                .unwrap_or_else(|| panic!("missing blocker for {path}: {output}"));
            assert!(line.contains("DESHELL_BLOCKER_PARSE_ERROR"), "{line}");
        }
        assert!(!output.contains(".deshell.nu"), "{output}");
        assert!(!output.contains(".deshell.ps1"), "{output}");

        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        let plan: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/migrations/sha256/{digest}/plan.json")),
            )
            .unwrap(),
        )
        .unwrap();
        for blocker in plan["blockers"].as_array().unwrap() {
            if blocker["code"] == "DESHELL_BLOCKER_PARSE_ERROR" {
                let location = &blocker["location"];
                let source_len =
                    std::fs::read(directory.path().join(location["path"].as_str().unwrap()))
                        .unwrap()
                        .len() as u64;
                assert!(
                    location["start_byte"].as_u64().unwrap() > 0
                        || location["end_byte"].as_u64().unwrap() < source_len,
                    "parse blocker used a whole-file span: {blocker}"
                );
            }
        }
    }

    #[test]
    fn migration_plan_blocks_static_process_references_to_retired_scripts() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf referenced\n",
        );
        std::fs::write(
            directory.path().join("caller.py"),
            b"import subprocess\nsubprocess.run([\"sh\", \"build.sh\"], check=True)\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(
            output.contains("DESHELL_BLOCKER_UNRESOLVED_CALL_SITE"),
            "{output}"
        );
        assert!(output.contains("caller.py"), "{output}");

        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        let migration_root = directory
            .path()
            .join(format!(".deshell/migrations/sha256/{digest}"));
        let plan: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(migration_root.join("plan.json")).unwrap())
                .unwrap();
        let proposal_digest = plan["proposals"][0].as_str().unwrap();
        let proposal: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(migration_root.join(format!("proposals/{proposal_digest}.json")))
                .unwrap(),
        )
        .unwrap();
        let request_digest = proposal["request_digest"].as_str().unwrap();
        let request: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(migration_root.join(format!("requests/{request_digest}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            request["call_sites"],
            serde_json::json!([{
                "path": "caller.py",
                "start_byte": 18,
                "end_byte": 64
            }])
        );
    }

    #[test]
    fn migration_plan_blocks_untyped_json_toml_and_yaml_task_references() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("scripts")).unwrap();
        configure(
            directory.path(),
            "scripts/build.sh",
            b"#!/bin/sh\n/usr/bin/printf referenced\n",
        );
        std::fs::write(
            directory.path().join("tasks.json"),
            br#"{"tasks":{"build":{"command":"./scripts/build.sh"}}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("tasks.toml"),
            b"[tasks.build]\nrun = \"./scripts/build.sh\"\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("taskfile.yml"),
            b"tasks:\n  build:\n    command: ./scripts/build.sh\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        let unresolved = planned
            .blockers
            .iter()
            .filter(|blocker| blocker.code == "DESHELL_BLOCKER_UNRESOLVED_CALL_SITE")
            .filter_map(|blocker| {
                blocker
                    .location
                    .as_ref()
                    .map(|location| location.path.as_str())
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            unresolved,
            ["taskfile.yml", "tasks.json", "tasks.toml"].into()
        );
        let candidates = planned
            .blockers
            .iter()
            .filter(|blocker| blocker.code == "DESHELL_BLOCKER_DYNAMIC_CANDIDATE")
            .filter_map(|blocker| {
                blocker
                    .location
                    .as_ref()
                    .map(|location| location.path.as_str())
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(candidates, unresolved);
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_rewrites_a_static_python_call_site_and_retires_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf referenced\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"call-site-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        let caller = b"import subprocess\nsubprocess.run([\"sh\", \"build.sh\"], check=True)\n";
        std::fs::write(directory.path().join("caller.py"), caller).unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"caller\", kind = \"test\", argv = [\"python3\", \"caller.py\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(planned.diff.contains("+++ b/caller.py"), "{}", planned.diff);
        assert!(planned.diff.contains("cargo"), "{}", planned.diff);
        assert!(!planned.diff.contains("+subprocess.run([\"sh\""));

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(
            evidence.status,
            crate::migration::EvidenceStatus::Verified,
            "{evidence:#?}"
        );
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        assert!(!directory.path().join("build.sh").exists());
        let migrated = std::fs::read_to_string(directory.path().join("caller.py")).unwrap();
        assert!(migrated.contains("cargo"), "{migrated}");
        assert!(!migrated.contains("build.sh"), "{migrated}");
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_rewrites_a_static_javascript_call_site_and_retires_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf javascript\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"javascript-call-site-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        let caller = concat!(
            "const { spawnSync } = require('node:child_process');\n",
            "const outcome = spawnSync(\"sh\", [\"build.sh\"], { stdio: \"inherit\" });\n",
            "process.exit(outcome.status ?? 1);\n",
        );
        std::fs::write(directory.path().join("caller.cjs"), caller).unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"caller\", kind = \"test\", argv = [\"node\", \"caller.cjs\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("+++ b/caller.cjs"),
            "{}",
            planned.diff
        );
        assert!(
            planned.diff.contains("spawnSync(\"cargo\""),
            "{}",
            planned.diff
        );

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        assert!(!directory.path().join("build.sh").exists());
        let migrated = std::fs::read_to_string(directory.path().join("caller.cjs")).unwrap();
        assert!(migrated.contains("spawnSync(\"cargo\""), "{migrated}");
        assert!(!migrated.contains("build.sh"), "{migrated}");
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn go_generator_rewrites_a_static_python_call_site_and_retires_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf golang\n",
        );
        std::fs::write(
            directory.path().join("go.mod"),
            "module example.test/deshell-call-site\n\ngo 1.27.0\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("caller.py"),
            b"import subprocess\nsubprocess.run([\"sh\", \"build.sh\"], check=True)\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        std::fs::create_dir(directory.path().join("cmd")).unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"go\"", 1)
            .replacen("target = \"rust\"", "target = \"go\"", 1)
            .replacen("module_root = \"src/bin\"", "module_root = \"cmd\"", 1)
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"caller\", kind = \"test\", argv = [\"python3\", \"caller.py\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(planned.diff.contains("subprocess.run([\"go\""));
        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(
            evidence.status,
            crate::migration::EvidenceStatus::Verified,
            "{evidence:#?}"
        );
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();
        assert!(!directory.path().join("build.sh").exists());
        let migrated = std::fs::read_to_string(directory.path().join("caller.py")).unwrap();
        assert!(migrated.contains("go"), "{migrated}");
        assert!(!migrated.contains("build.sh"), "{migrated}");
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_retires_make_and_package_wrappers_with_the_script_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("scripts")).unwrap();
        configure(
            directory.path(),
            "scripts/build.sh",
            b"#!/bin/sh\n/usr/bin/printf interface\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"structured-call-site-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("Makefile"),
            b"# retain this comment\nbuild:\n\t/bin/sh scripts/build.sh\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("package.json"),
            br#"{"private":true,"scripts":{"build":"sh scripts/build.sh"}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("validate.py"),
            concat!(
                "import json, pathlib, subprocess, sys\n",
                "assert 'build.sh' not in pathlib.Path(sys.argv[1]).read_text()\n",
                "assert json.loads(pathlib.Path(sys.argv[2]).read_text())['scripts']['build'] == ''\n",
                "subprocess.run(['cargo', 'run', '--quiet', '--bin', 'deshell_build', '--'], check=True)\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"interfaces\", kind = \"test\", argv = [\"python3\", \"validate.py\", \"Makefile\", \"package.json\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(planned.diff.contains("--- a/Makefile"), "{}", planned.diff);
        assert!(
            planned.diff.contains("--- a/package.json"),
            "{}",
            planned.diff
        );

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(
            evidence.status,
            crate::migration::EvidenceStatus::Verified,
            "{evidence:#?}"
        );
        assert_eq!(evidence.checks.len(), 3);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        assert!(!directory.path().join("scripts/build.sh").exists());
        let makefile = std::fs::read_to_string(directory.path().join("Makefile")).unwrap();
        assert!(makefile.contains("# retain this comment"));
        assert!(!makefile.contains("build.sh"));
        let package: serde_json::Value = crate::strict_json::parse_host(
            &std::fs::read(directory.path().join("package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(package["private"], true);
        assert_eq!(package["scripts"]["build"], "");
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
        let archive: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(directory.path().join(".deshell/archive/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(archive["entries"].as_array().unwrap().len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn go_generator_retires_make_and_package_wrappers_with_the_script_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("scripts")).unwrap();
        std::fs::create_dir(directory.path().join("cmd")).unwrap();
        configure(
            directory.path(),
            "scripts/build.sh",
            b"#!/bin/sh\n/usr/bin/printf go-interface\n",
        );
        std::fs::write(
            directory.path().join("go.mod"),
            "module example.test/deshell-interface\n\ngo 1.27.0\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("Makefile"),
            b"build:\n\t/bin/sh scripts/build.sh\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("package.json"),
            br#"{"private":true,"scripts":{"build":"sh scripts/build.sh"}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("validate.py"),
            concat!(
                "import json, pathlib, subprocess, sys\n",
                "assert 'build.sh' not in pathlib.Path(sys.argv[1]).read_text()\n",
                "assert json.loads(pathlib.Path(sys.argv[2]).read_text())['scripts']['build'] == ''\n",
                "subprocess.run(['go', 'run', './cmd/build.go'], check=True)\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"go\"", 1)
            .replacen("target = \"rust\"", "target = \"go\"", 1)
            .replacen("module_root = \"src/bin\"", "module_root = \"cmd\"", 1)
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"interfaces\", kind = \"test\", argv = [\"python3\", \"validate.py\", \"Makefile\", \"package.json\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("+++ b/cmd/build.go"),
            "{}",
            planned.diff
        );
        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        assert_eq!(evidence.checks.len(), 3);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        assert!(!directory.path().join("scripts/build.sh").exists());
        assert!(directory.path().join("cmd/build.go").is_file());
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[test]
    fn migration_plan_persists_make_and_package_interface_blockers_instead_of_aborting() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir(directory.path().join("scripts")).unwrap();
        configure(
            directory.path(),
            "scripts/build.sh",
            b"#!/bin/sh\n/usr/bin/printf interface\n",
        );
        std::fs::write(
            directory.path().join("Makefile"),
            b"build:\n\t/bin/sh scripts/build.sh\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("package.json"),
            br#"{"scripts":{"build":"printf scripts/build.sh"}}"#,
        )
        .unwrap();
        approve_oracle_inputs(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(
            output.contains("DESHELL_BLOCKER_UNIMPLEMENTED_HOST_INTERFACE"),
            "{output}"
        );
        assert!(
            output.contains("DESHELL_BLOCKER_UNRESOLVED_CALL_SITE"),
            "{output}"
        );
        assert!(output.contains("Makefile"), "{output}");
        assert!(output.contains("package.json"), "{output}");
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        assert!(
            directory
                .path()
                .join(format!(".deshell/migrations/sha256/{digest}/plan.json"))
                .is_file()
        );
    }

    #[test]
    fn migration_plan_blocks_cross_script_exec_references_until_call_sites_are_updated() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "main.sh",
            b"#!/bin/sh\n/bin/sh child.sh fixed\n",
        );
        std::fs::write(
            directory.path().join("child.sh"),
            b"#!/bin/sh\n/usr/bin/printf child\n",
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 4, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(
            output.contains("DESHELL_BLOCKER_UNRESOLVED_CALL_SITE"),
            "{output}"
        );
        assert!(output.contains("main.sh"), "{output}");
        assert!(output.contains("child.sh"), "{output}");
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_retires_a_static_cross_script_wrapper_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "main.sh",
            b"#!/bin/sh\n/bin/sh child.sh\n",
        );
        std::fs::write(
            directory.path().join("child.sh"),
            b"#!/bin/sh\n/usr/bin/printf child\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"cross-script-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("deshell_child.rs"),
            "{}",
            planned.diff
        );
        assert!(
            !planned.diff.contains("deshell_main.rs"),
            "{}",
            planned.diff
        );

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        assert_eq!(evidence.checks.len(), 2);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        assert!(!directory.path().join("main.sh").exists());
        assert!(!directory.path().join("child.sh").exists());
        assert!(directory.path().join("src/bin/deshell_child.rs").is_file());
        assert!(!directory.path().join("src/bin/deshell_main.rs").exists());
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
        let archive: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(directory.path().join(".deshell/archive/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(archive["entries"].as_array().unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_rewrites_a_docker_exec_call_site_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf docker-exec\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"docker-exec-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("Dockerfile"),
            b"FROM rust:1.98\nRUN [\"/bin/sh\",\"build.sh\"]\nLABEL retained=true\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("validate.py"),
            concat!(
                "import pathlib, subprocess, sys\n",
                "dockerfile = pathlib.Path(sys.argv[1]).read_text()\n",
                "assert 'build.sh' not in dockerfile and 'LABEL retained=true' in dockerfile\n",
                "subprocess.run(['cargo', 'run', '--quiet', '--bin', 'deshell_build', '--'], check=True)\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"dockerfile\", kind = \"test\", argv = [\"python3\", \"validate.py\", \"Dockerfile\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("--- a/Dockerfile"),
            "{}",
            planned.diff
        );
        assert!(planned.diff.contains("[\"cargo\""), "{}", planned.diff);

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        let dockerfile = std::fs::read_to_string(directory.path().join("Dockerfile")).unwrap();
        assert!(dockerfile.contains("LABEL retained=true"));
        assert!(dockerfile.contains("[\"cargo\""), "{dockerfile}");
        assert!(!dockerfile.contains("build.sh"));
        assert!(!directory.path().join("build.sh").exists());
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rust_generator_rewrites_a_github_run_call_site_to_a_local_action_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf github-reference\n",
        );
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(
                directory.path().join("build.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"github-reference-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        std::fs::write(
            directory.path().join(".github/workflows/ci.yml"),
            concat!(
                "name: retained\n",
                "jobs:\n",
                "  build:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - run: ./build.sh\n",
            ),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("validate.py"),
            concat!(
                "import pathlib, subprocess, sys\n",
                "workflow = pathlib.Path(sys.argv[1]).read_text()\n",
                "assert 'uses: ./.github/actions/deshell-' in workflow and 'name: retained' in workflow\n",
                "actions = list(pathlib.Path('.github/actions').glob('deshell-*/index.js'))\n",
                "assert len(actions) == 1\n",
                "javascript = actions[0].read_text()\n",
                "assert 'spawnSync' in javascript and 'shell: false' in javascript\n",
                "subprocess.run(['cargo', 'run', '--quiet', '--bin', 'deshell_build', '--'], check=True)\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"workflow\", kind = \"test\", argv = [\"python3\", \"validate.py\", \".github/workflows/ci.yml\"] }]",
            )
            .replace("memory_bytes = 1073741824", "memory_bytes = 8589934592");
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("uses: ./.github/actions/deshell-"),
            "{}",
            planned.diff
        );
        assert!(planned.diff.contains("shell: false"), "{}", planned.diff);

        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        assert_eq!(evidence.checks.len(), 2);
        let evidence_path = directory.path().join("evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();
        crate::migration::apply(directory.path(), &planned.digest).unwrap();

        let workflow =
            std::fs::read_to_string(directory.path().join(".github/workflows/ci.yml")).unwrap();
        assert!(workflow.contains("name: retained"));
        assert!(workflow.contains("uses: ./.github/actions/deshell-"));
        assert!(!workflow.contains("run:"));
        assert!(!directory.path().join("build.sh").exists());
        assert!(
            crate::project::scan(directory.path())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    #[test]
    fn rust_generator_rewrites_a_github_block_run_without_damaging_siblings() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf github-block-reference\n",
        );
        std::fs::write(
            directory.path().join("Cargo.toml"),
            concat!(
                "[package]\n",
                "name = \"github-block-reference-fixture\"\n",
                "version = \"0.0.0\"\n",
                "edition = \"2024\"\n",
            ),
        )
        .unwrap();
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        std::fs::write(
            directory.path().join(".github/workflows/ci.yml"),
            concat!(
                "name: retained\n",
                "jobs:\n",
                "  build:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - name: retained step\n",
                "        run: |-\n",
                "          ./build.sh\n",
                "        env:\n",
                "          RETAINED: yes\n",
            ),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("validate.py"),
            concat!(
                "import pathlib, sys\n",
                "workflow = pathlib.Path(sys.argv[1]).read_text()\n",
                "assert 'name: retained' in workflow and 'RETAINED: yes' in workflow\n",
            ),
        )
        .unwrap();
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace(
                "validation_commands = []",
                "validation_commands = [{ name = \"workflow\", kind = \"test\", argv = [\"python3\", \"validate.py\", \".github/workflows/ci.yml\"] }]",
            );
        std::fs::write(config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        assert!(
            planned.diff.contains("      - name: retained step\n"),
            "{}",
            planned.diff
        );
        assert!(
            planned
                .diff
                .contains("        uses: ./.github/actions/deshell-"),
            "{}",
            planned.diff
        );
        assert!(
            planned.diff.contains("        env:\n")
                && planned.diff.contains("          RETAINED: yes\n"),
            "{}",
            planned.diff
        );
    }

    #[cfg(unix)]
    #[test]
    fn migration_verify_import_and_apply_retire_shell_atomically() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = b"#!/usr/bin/env bash\n/usr/bin/printf '%s\\n' hello\n";
        configure(directory.path(), "build.sh", source);
        approve_oracle_inputs(directory.path());
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("host-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let evidence: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&evidence_path).unwrap()).unwrap();
        assert_eq!(evidence["plan_digest"], digest);
        assert_eq!(evidence["cell"], "host");
        assert_eq!(evidence["status"], "verified");
        assert_eq!(evidence["repetitions"], 2);
        assert_eq!(evidence["checks"].as_array().unwrap().len(), 1);
        assert!(
            evidence["checks"][0]["comparisons"][0]["differences"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence_path),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));

        let status = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "status".into(),
            "--root".into(),
            root.clone(),
            "--format".into(),
            "json".into(),
        ]);
        assert_eq!(status.0, 0, "{}", String::from_utf8_lossy(&status.2));
        let status: serde_json::Value = crate::strict_json::parse(&status.1).unwrap();
        assert_eq!(status["details"]["counts"]["verified"], 1);
        assert_eq!(status["details"]["counts"]["retired"], 0);

        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        assert!(!directory.path().join("build.sh").exists());
        assert!(directory.path().join("src/bin/deshell_build.rs").is_file());
        let source_digest = crate::digest::sha256(source);
        assert_eq!(
            std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/archive/sha256/{source_digest}"))
            )
            .unwrap(),
            source
        );
        let archive: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(directory.path().join(".deshell/archive/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(archive["plan_digest"], digest);
        assert_eq!(archive["entries"].as_array().unwrap().len(), 1);
        let shell_free = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(
            shell_free.0,
            0,
            "{}",
            String::from_utf8_lossy(&shell_free.2)
        );

        let archived_path = directory
            .path()
            .join(format!(".deshell/archive/sha256/{source_digest}"));
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&archived_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&archived_path, b"tampered").unwrap();
        let tampered = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            path(directory.path()),
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(tampered.0, 4);
        assert!(tampered.2.is_empty());
        assert!(String::from_utf8_lossy(&tampered.1).contains("ARCHIVE_TAMPERED"));
    }

    #[cfg(unix)]
    #[test]
    fn migration_archive_deduplicates_identical_source_blobs_across_locations() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let source = b"#!/bin/sh\n/usr/bin/printf identical\n";
        configure(directory.path(), "alpha.sh", source);
        std::fs::write(directory.path().join("beta.sh"), source).unwrap();
        approve_oracle_inputs(directory.path());
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(!output.contains("blocker"), "{output}");
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("identical-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence_path),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        assert!(!directory.path().join("alpha.sh").exists());
        assert!(!directory.path().join("beta.sh").exists());
        let source_digest = crate::digest::sha256(source);
        assert!(
            directory
                .path()
                .join(format!(".deshell/archive/sha256/{source_digest}"))
                .is_file()
        );
        let manifest: serde_json::Value = crate::strict_json::parse(
            &std::fs::read(directory.path().join(".deshell/archive/manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["entries"].as_array().unwrap().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn migration_validation_runs_against_the_fully_retired_staged_tree() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf staged\n",
        );
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "validation_commands = []",
            "validation_commands = [{ name = \"retired-source-absent\", kind = \"test\", argv = [\"test\", \"!\", \"-e\", \"build.sh\"] }]",
        );
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("staged-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let evidence: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(evidence_path).unwrap()).unwrap();
        assert_eq!(evidence["status"], "verified");
        assert_eq!(evidence["validation"][0]["exit_code"], 0);
    }

    #[cfg(unix)]
    #[test]
    fn migration_apply_rejects_validation_policy_changed_after_evidence() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf stale\n",
        );
        approve_oracle_inputs(directory.path());
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("stale-policy-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence_path),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "validation_commands = []",
            "validation_commands = [{ name = \"new-policy\", kind = \"test\", argv = [\"true\"] }]",
        );
        std::fs::write(config_path, config).unwrap();

        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 4, "{}", String::from_utf8_lossy(&applied.2));
        assert!(applied.2.is_empty());
        assert!(
            String::from_utf8_lossy(&applied.1).contains("DESHELL_BLOCKER_STALE_VALIDATION_POLICY")
        );
        assert!(directory.path().join("build.sh").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn migration_apply_rejects_validation_limits_changed_after_evidence() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "limits.sh",
            b"#!/bin/sh\n/usr/bin/printf limits\n",
        );
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "validation_commands = []",
            "validation_commands = [{ name = \"true\", kind = \"test\", argv = [\"true\"] }]",
        );
        std::fs::write(&config_path, config).unwrap();

        let planned = crate::migration::create_plan(directory.path()).unwrap();
        assert!(planned.blockers.is_empty(), "{:#?}", planned.blockers);
        let evidence = crate::migration::verify(directory.path(), &planned.digest, "host").unwrap();
        assert_eq!(evidence.status, crate::migration::EvidenceStatus::Verified);
        let evidence_path = directory.path().join("limits-evidence.json");
        std::fs::write(&evidence_path, evidence.encode_pretty().unwrap()).unwrap();
        crate::migration::import_evidence(
            directory.path(),
            &planned.digest,
            std::slice::from_ref(&evidence_path),
        )
        .unwrap();

        let changed = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("timeout_ms = 30000", "timeout_ms = 30001");
        std::fs::write(config_path, changed).unwrap();
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            path(directory.path()),
            "--plan".into(),
            planned.digest,
        ]);
        assert_eq!(applied.0, 4, "{}", String::from_utf8_lossy(&applied.2));
        assert!(applied.2.is_empty());
        assert!(
            String::from_utf8_lossy(&applied.1).contains("DESHELL_BLOCKER_STALE_VALIDATION_LIMITS")
        );
        assert!(directory.path().join("limits.sh").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn migration_evidence_import_rejects_conflicting_verified_documents_for_one_cell() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf evidence\n",
        );
        approve_oracle_inputs(directory.path());
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("evidence-a.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence_path),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));

        let mut conflicting: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&evidence_path).unwrap()).unwrap();
        for comparison in conflicting["checks"][0]["comparisons"]
            .as_array_mut()
            .unwrap()
        {
            for subject in ["original", "ir", "replacement"] {
                comparison[subject]["stdout_base64"] = serde_json::json!("Zm9yZ2Vk");
            }
        }
        let conflicting_path = directory.path().join("evidence-b.json");
        std::fs::write(
            &conflicting_path,
            crate::canonical_json::pretty_bytes(&conflicting).unwrap(),
        )
        .unwrap();
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&conflicting_path),
        ]);
        assert_eq!(imported.0, 4, "{}", String::from_utf8_lossy(&imported.2));
        assert!(imported.2.is_empty());
        assert!(String::from_utf8_lossy(&imported.1).contains("DESHELL_BLOCKER_EVIDENCE_CONFLICT"));
        assert!(directory.path().join("build.sh").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn migration_verify_records_an_unavailable_foreign_platform_cell() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "foreign.sh",
            b"#!/bin/sh\n/usr/bin/printf foreign\n",
        );
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            &format!("operating_system = \"{}\"", std::env::consts::OS),
            "operating_system = \"foreign-os\"",
        );
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence_path = directory.path().join("foreign-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence_path),
        ]);
        assert_eq!(verified.0, 6, "{}", String::from_utf8_lossy(&verified.2));
        let evidence: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(evidence_path).unwrap()).unwrap();
        assert_eq!(evidence["status"], "unavailable");
        assert_eq!(evidence["checks"][0]["status"], "unavailable");
        assert!(
            evidence["checks"][0]["comparisons"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            evidence["checks"][0]["error"]
                .as_str()
                .unwrap()
                .contains("foreign platform")
        );
    }

    #[cfg(unix)]
    #[test]
    fn official_go_generator_uses_an_existing_module_root_and_retires_end_to_end() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/usr/bin/env bash\n/usr/bin/printf '%s\\n' hello | /usr/bin/tr a-z A-Z\n",
        );
        approve_oracle_inputs(directory.path());
        std::fs::create_dir(directory.path().join("cmd")).unwrap();
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"go\"", 1)
            .replacen("target = \"rust\"", "target = \"go\"", 1)
            .replacen("module_root = \"src/bin\"", "module_root = \"cmd\"", 1);
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(!String::from_utf8_lossy(&planned.1).contains("blocker"));
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("go-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        assert_eq!(
            invoke_owned(vec![
                "deshell".into(),
                "migrate".into(),
                "evidence".into(),
                "import".into(),
                "--root".into(),
                root.clone(),
                "--plan".into(),
                digest.clone(),
                path(&evidence),
            ])
            .0,
            0
        );
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        assert!(!directory.path().join("build.sh").exists());
        let generated = std::fs::read_to_string(directory.path().join("cmd/build.go")).unwrap();
        assert!(generated.contains("Code generated by de-shell"));
        assert!(!generated.contains("deshell runtime"));
    }

    #[cfg(unix)]
    #[test]
    fn migration_oracle_verifies_pipeline_status_and_both_condition_branches() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "branch.sh",
            b"#!/usr/bin/env bash\n/usr/bin/printf 'hello\\n' | /usr/bin/tr a-z A-Z\n/bin/test \"$1\" = yes && /usr/bin/printf 'branch\\n'\n",
        );
        approve_oracle_inputs(directory.path());
        let default_path = directory.path().join(".deshell/scenarios/default.toml");
        let default = std::fs::read_to_string(&default_path)
            .unwrap()
            .replace(
                "arguments = []",
                "arguments = [{ name = \"1\", value = \"no\" }]",
            )
            .replace("argv = []", "argv = [\"no\"]");
        std::fs::write(default_path, default).unwrap();
        let yes = crate::config::Scenario::default_text()
            .replace("name = \"default\"", "name = \"yes\"")
            .replace("approval = \"draft\"", "approval = \"approved\"")
            .replace(
                "arguments = []",
                "arguments = [{ name = \"1\", value = \"yes\" }]",
            )
            .replace("argv = []", "argv = [\"yes\"]");
        std::fs::write(directory.path().join(".deshell/scenarios/yes.toml"), yes).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(
            !String::from_utf8_lossy(&planned.1).contains("blocker"),
            "{}",
            String::from_utf8_lossy(&planned.1)
        );
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("branch-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let document: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&evidence).unwrap()).unwrap();
        assert_eq!(document["checks"].as_array().unwrap().len(), 2);
        assert_eq!(
            invoke_owned(vec![
                "deshell".into(),
                "migrate".into(),
                "evidence".into(),
                "import".into(),
                "--root".into(),
                root.clone(),
                "--plan".into(),
                digest.clone(),
                path(&evidence),
            ])
            .0,
            0
        );
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        assert!(!directory.path().join("branch.sh").exists());
        assert!(directory.path().join("src/bin/deshell_branch.rs").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn structured_host_generator_rewrites_docker_run_and_archives_only_the_snippet() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let snippet = b"/usr/bin/printf '%s\\n' docker";
        let dockerfile = b"FROM scratch\nRUN /usr/bin/printf '%s\\n' docker\n";
        configure(directory.path(), "Dockerfile", dockerfile);
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"host\"", 1)
            .replacen("target = \"rust\"", "target = \"host\"", 1);
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(
            !String::from_utf8_lossy(&planned.1).contains("blocker"),
            "{}",
            String::from_utf8_lossy(&planned.1)
        );
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("docker-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        assert_eq!(
            invoke_owned(vec![
                "deshell".into(),
                "migrate".into(),
                "evidence".into(),
                "import".into(),
                "--root".into(),
                root.clone(),
                "--plan".into(),
                digest.clone(),
                path(&evidence),
            ])
            .0,
            0
        );
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        let rewritten = std::fs::read_to_string(directory.path().join("Dockerfile")).unwrap();
        assert_eq!(
            rewritten,
            "FROM scratch\nRUN [\"/usr/bin/printf\",\"%s\\\\n\",\"docker\"]\n"
        );
        let snippet_digest = crate::digest::sha256(snippet);
        assert_eq!(
            std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/archive/sha256/{snippet_digest}"))
            )
            .unwrap(),
            snippet
        );
        let shell_free = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(
            shell_free.0,
            0,
            "{}",
            String::from_utf8_lossy(&shell_free.2)
        );
    }

    #[cfg(unix)]
    #[test]
    fn structured_host_generator_rewrites_python_subprocess_without_a_shell() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let snippet = b"/usr/bin/printf python";
        let host = b"import subprocess\nsubprocess.run(\"/usr/bin/printf python\", shell=True, check=False)\n";
        configure(directory.path(), "runner.py", host);
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"host\"", 1)
            .replacen("target = \"rust\"", "target = \"host\"", 1);
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(
            !String::from_utf8_lossy(&planned.1).contains("blocker"),
            "{}",
            String::from_utf8_lossy(&planned.1)
        );
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("python-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));

        let rewritten = std::fs::read_to_string(directory.path().join("runner.py")).unwrap();
        assert_eq!(
            rewritten,
            "import subprocess\nsubprocess.run([\"/usr/bin/printf\",\"python\"], shell=False, check=False)\n"
        );
        let snippet_digest = crate::digest::sha256(snippet);
        assert_eq!(
            std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/archive/sha256/{snippet_digest}"))
            )
            .unwrap(),
            snippet
        );
        let shell_free = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(
            shell_free.0,
            0,
            "{}",
            String::from_utf8_lossy(&shell_free.2)
        );
    }

    #[cfg(unix)]
    #[test]
    fn structured_host_generator_rewrites_javascript_exec_sync_without_a_shell() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        let snippet = b"/usr/bin/printf javascript";
        let host = concat!(
            "const child_process = require(\"node:child_process\");\n",
            "child_process.execSync(\"/usr/bin/printf javascript\", {stdio: \"inherit\"});\n",
        );
        configure(directory.path(), "runner.js", host.as_bytes());
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"host\"", 1)
            .replacen("target = \"rust\"", "target = \"host\"", 1);
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(
            !String::from_utf8_lossy(&planned.1).contains("blocker"),
            "{}",
            String::from_utf8_lossy(&planned.1)
        );
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("javascript-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));

        let rewritten = std::fs::read_to_string(directory.path().join("runner.js")).unwrap();
        assert_eq!(
            rewritten,
            concat!(
                "const child_process = require(\"node:child_process\");\n",
                "child_process.execFileSync(\"/usr/bin/printf\",[\"javascript\"], {stdio: \"inherit\"});\n",
            )
        );
        let snippet_digest = crate::digest::sha256(snippet);
        assert_eq!(
            std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/archive/sha256/{snippet_digest}"))
            )
            .unwrap(),
            snippet
        );
        let shell_free = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(
            shell_free.0,
            0,
            "{}",
            String::from_utf8_lossy(&shell_free.2)
        );
    }

    #[cfg(unix)]
    #[test]
    fn structured_host_generator_replaces_github_run_with_a_local_javascript_action() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        let snippet = b"/usr/bin/printf workflow";
        let workflow = concat!(
            "jobs:\n",
            "  test:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - run: /usr/bin/printf workflow\n",
        );
        configure(
            directory.path(),
            ".github/workflows/ci.yml",
            workflow.as_bytes(),
        );
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"host\"", 1)
            .replacen("target = \"rust\"", "target = \"host\"", 1);
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());
        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        assert!(
            !String::from_utf8_lossy(&planned.1).contains("blocker"),
            "{}",
            String::from_utf8_lossy(&planned.1)
        );
        let digest = String::from_utf8(planned.1)
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("plan ").map(str::to_owned))
            .unwrap();
        let evidence = directory.path().join("workflow-evidence.json");
        let verified = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            "--cell".into(),
            "host".into(),
            "--output".into(),
            path(&evidence),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let imported = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "evidence".into(),
            "import".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
            path(&evidence),
        ]);
        assert_eq!(imported.0, 0, "{}", String::from_utf8_lossy(&imported.2));
        let applied = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "apply".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));

        let rewritten =
            std::fs::read_to_string(directory.path().join(".github/workflows/ci.yml")).unwrap();
        assert!(rewritten.contains("- uses: ./.github/actions/deshell-"));
        assert!(!rewritten.contains("run:"));
        let actions = std::fs::read_dir(directory.path().join(".github/actions"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].path().join("action.yml").is_file());
        let action = std::fs::read_to_string(actions[0].path().join("index.js")).unwrap();
        assert!(action.contains("spawnSync"));
        assert!(!action.contains("shell: true"));
        let snippet_digest = crate::digest::sha256(snippet);
        assert_eq!(
            std::fs::read(
                directory
                    .path()
                    .join(format!(".deshell/archive/sha256/{snippet_digest}"))
            )
            .unwrap(),
            snippet
        );
        let shell_free = invoke_owned(vec![
            "deshell".into(),
            "verify".into(),
            "--root".into(),
            root,
            "--require".into(),
            "shell-free".into(),
        ]);
        assert_eq!(
            shell_free.0,
            0,
            "{}",
            String::from_utf8_lossy(&shell_free.2)
        );
    }

    #[test]
    fn structured_host_generator_preserves_github_block_scalar_siblings() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join(".github/workflows")).unwrap();
        let workflow = concat!(
            "jobs:\n",
            "  test:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - name: generated\n",
            "        run: |-\n",
            "          /usr/bin/printf workflow\n",
            "        env:\n",
            "          KEEP: yes\n",
        );
        configure(
            directory.path(),
            ".github/workflows/block.yml",
            workflow.as_bytes(),
        );
        approve_oracle_inputs(directory.path());
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replacen("generator = \"rust\"", "generator = \"host\"", 1)
            .replacen("target = \"rust\"", "target = \"host\"", 1);
        std::fs::write(config_path, config).unwrap();

        let planned = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "plan".into(),
            "--root".into(),
            path(directory.path()),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        assert!(!output.contains("blocker"), "{output}");
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("plan "))
            .unwrap();
        let migration_root = directory
            .path()
            .join(format!(".deshell/migrations/sha256/{digest}"));
        let plan: crate::migration::MigrationPlan =
            crate::strict_json::decode(&std::fs::read(migration_root.join("plan.json")).unwrap())
                .unwrap();
        let proposal: crate::migration::Proposal = crate::strict_json::decode(
            &std::fs::read(migration_root.join(format!("proposals/{}.json", plan.proposals[0])))
                .unwrap(),
        )
        .unwrap();
        let workflow_patch = proposal
            .patches
            .iter()
            .find(|patch| patch.path == ".github/workflows/block.yml")
            .unwrap();
        let rewritten = String::from_utf8(workflow_patch.contents().unwrap()).unwrap();
        assert!(
            rewritten.contains("      - name: generated\n"),
            "{rewritten}"
        );
        assert!(
            rewritten.contains("        uses: ./.github/actions/deshell-"),
            "{rewritten}"
        );
        assert!(
            rewritten.contains("        env:\n          KEEP: yes\n"),
            "{rewritten}"
        );
        assert!(!rewritten.contains("run:"), "{rewritten}");
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
        assert!(String::from_utf8_lossy(&preview.1).contains("--- a/build.sh\n+++ b/build.sh\n"));
        assert!(String::from_utf8_lossy(&preview.1).contains("DESHELL_MODERNIZE_FINDING"));
        assert!(preview.2.is_empty());
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
        assert!(String::from_utf8_lossy(&jsonl.1).contains("--- a/build.sh\n+++ b/build.sh\n"));
        assert!(String::from_utf8_lossy(&jsonl.1).contains("DESHELL_MODERNIZE_FINDING"));
        assert!(jsonl.2.is_empty());
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

    #[cfg(unix)]
    #[test]
    fn harden_uses_a_separate_approved_plan_evidence_and_atomic_apply_series() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(
            directory.path(),
            "build.sh",
            b"#!/bin/sh\n/usr/bin/printf hardened\n",
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path).unwrap().replace(
            "validation_commands = []",
            "validation_commands = [{ name = \"smoke\", kind = \"test\", argv = [\"/usr/bin/true\"] }]",
        );
        std::fs::write(config_path, config).unwrap();
        let root = path(directory.path());

        let planned = invoke_owned(vec![
            "deshell".into(),
            "harden".into(),
            "plan".into(),
            "--root".into(),
            root.clone(),
        ]);
        assert_eq!(planned.0, 0, "{}", String::from_utf8_lossy(&planned.2));
        let output = String::from_utf8(planned.1).unwrap();
        let digest = output
            .lines()
            .find_map(|line| line.strip_prefix("harden plan ").map(str::to_owned))
            .unwrap();
        assert!(crate::digest::valid_sha256(&digest));
        assert!(output.contains("+set -eu"), "{output}");
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"#!/bin/sh\n/usr/bin/printf hardened\n"
        );

        let unapproved = invoke_owned(vec![
            "deshell".into(),
            "harden".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
        ]);
        assert_eq!(unapproved.0, 4);
        assert!(unapproved.2.is_empty());
        assert!(
            String::from_utf8_lossy(&unapproved.1).contains("DESHELL_HARDEN_APPROVAL_REQUIRED")
        );

        let approval_path = directory
            .path()
            .join(format!(".deshell/hardening/approvals/{digest}.json"));
        let mut approval: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(&approval_path).unwrap()).unwrap();
        approval["approval"] = serde_json::json!("approved");
        approval["owner"] = serde_json::json!("release-engineering");
        approval["reason"] = serde_json::json!("reviewed strict failure semantics");
        std::fs::write(
            &approval_path,
            crate::canonical_json::pretty_bytes(&approval).unwrap(),
        )
        .unwrap();

        let verified = invoke_owned(vec![
            "deshell".into(),
            "harden".into(),
            "verify".into(),
            "--root".into(),
            root.clone(),
            "--plan".into(),
            digest.clone(),
        ]);
        assert_eq!(verified.0, 0, "{}", String::from_utf8_lossy(&verified.2));
        let evidence_path = directory
            .path()
            .join(format!(".deshell/hardening/sha256/{digest}/evidence.json"));
        let evidence: serde_json::Value =
            crate::strict_json::parse(&std::fs::read(evidence_path).unwrap()).unwrap();
        assert_eq!(evidence["plan_digest"], digest);
        assert_eq!(evidence["status"], "verified");

        let applied = invoke_owned(vec![
            "deshell".into(),
            "harden".into(),
            "apply".into(),
            "--root".into(),
            root,
            "--plan".into(),
            digest,
        ]);
        assert_eq!(applied.0, 0, "{}", String::from_utf8_lossy(&applied.2));
        assert_eq!(
            std::fs::read(directory.path().join("build.sh")).unwrap(),
            b"#!/bin/sh\nset -eu\n/usr/bin/printf hardened\n"
        );
        assert!(!directory.path().join(".deshell/migrations").exists());
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
        assert!(String::from_utf8_lossy(&verified.1).contains("native="));
        assert!(verified.2.is_empty());
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
    fn migrate_rejects_the_removed_legacy_export_surface() {
        let directory = tempfile::tempdir().unwrap();
        crate::project::init(directory.path()).unwrap();
        configure(directory.path(), "build.sh", b"/usr/bin/printf hello\n");
        let result = invoke_owned(vec![
            "deshell".into(),
            "migrate".into(),
            "--root".into(),
            path(directory.path()),
            "--target".into(),
            "cwl".into(),
        ]);
        assert_eq!(result.0, 2);
        assert!(result.1.is_empty());
        assert!(String::from_utf8_lossy(&result.2).contains("unexpected argument '--root'"));
        assert!(!directory.path().join(".deshell/export").exists());
    }
}
