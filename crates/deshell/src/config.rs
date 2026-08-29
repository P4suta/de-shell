use base64::Engine as _;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectConfig {
    pub version: u32,
    pub entrypoints: Vec<String>,
    pub location_overrides: Vec<LocationOverride>,
    pub interpreter_overrides: Vec<InterpreterOverride>,
    pub platform_cells: Vec<PlatformCell>,
    pub validation_commands: Vec<ValidationCommand>,
    pub migration: MigrationPolicy,
    pub integration: IntegrationTargets,
    pub audit: AuditPolicy,
    pub policy: Policy,
    pub sandbox: Sandbox,
    pub limits: ResourceLimits,
    pub export: ExportPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MigrationPolicy {
    pub generator: String,
    pub target: MigrationTarget,
    pub module_root: String,
    pub agent_context: AgentContextPolicy,
    pub allow_agent_network: bool,
    pub allow_source_send: bool,
    pub external_generators: Vec<ExternalGenerator>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MigrationTarget {
    Rust,
    Go,
    Host,
    Agent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalGenerator {
    pub name: String,
    pub executable: String,
    pub digest: String,
    pub capabilities: Vec<MigrationTarget>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentContextPolicy {
    Minimal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntegrationTargets {
    pub rust: LanguageIntegration,
    pub go: LanguageIntegration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LanguageIntegration {
    pub module_root: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationOverride {
    pub path: String,
    pub start_byte: u64,
    pub end_byte: u64,
    pub generator: String,
    pub target: MigrationTarget,
    pub module_root: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InterpreterOverride {
    pub path: String,
    pub interpreter: ConfiguredInterpreter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfiguredInterpreter {
    Sh,
    Bash,
    Zsh,
    Fish,
    Powershell,
    Cmd,
    Nu,
}

impl ConfiguredInterpreter {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
            Self::Cmd => "cmd",
            Self::Nu => "nu",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlatformCell {
    pub id: String,
    pub operating_system: String,
    pub architecture: String,
    pub runtime: String,
    pub approval: Approval,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Approval {
    Draft,
    Approved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ValidationCommand {
    pub name: String,
    pub kind: ValidationKind,
    pub argv: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidationKind {
    Build,
    Test,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditPolicy {
    pub persona: AuditPersonaPolicy,
    pub fail_on: AuditSeverity,
    pub acknowledgement_max_days: u32,
    pub acknowledgements: Vec<AuditAcknowledgement>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditSeverity {
    Note,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuditPersonaPolicy {
    Pedantic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditAcknowledgement {
    pub rule: String,
    pub location_digest: String,
    pub reason: String,
    pub owner: String,
    pub expires: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub file_read: FileReadPolicy,
    pub file_write: FileWritePolicy,
    pub host_materialization: DenyPolicy,
    pub host_execution: DenyPolicy,
    pub network: NetworkPolicy,
    pub delegation: DelegationPolicy,
    pub unknown_interpreter: UnknownInterpreter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FileReadPolicy {
    Deny,
    Project,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FileWritePolicy {
    Deny,
    Sandbox,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DenyPolicy {
    Deny,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NetworkPolicy {
    Deny,
    RecordReplay,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DelegationPolicy {
    Deny,
    Pinned,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UnknownInterpreter {
    TraceOnly,
    Reject,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sandbox {
    pub mode: SandboxMode,
    pub allow_local: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxMode {
    Disposable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceLimits {
    pub timeout_ms: u64,
    pub memory_bytes: u64,
    pub processes: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportPolicy {
    pub mode: ExportMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ExportMode {
    Strict,
    Bundle,
}

impl ProjectConfig {
    pub(crate) fn default_text() -> String {
        concat!(
            "version = 1\n",
            "entrypoints = []\n",
            "location_overrides = []\n",
            "interpreter_overrides = []\n",
            "platform_cells = []\n",
            "validation_commands = []\n",
            "\n",
            "[migration]\n",
            "generator = \"rust\"\n",
            "target = \"rust\"\n",
            "module_root = \"src/bin\"\n",
            "agent_context = \"minimal\"\n",
            "allow_agent_network = false\n",
            "allow_source_send = false\n",
            "external_generators = []\n",
            "\n",
            "[integration.rust]\n",
            "module_root = \"src/bin\"\n",
            "\n",
            "[integration.go]\n",
            "module_root = \"cmd\"\n",
            "\n",
            "[policy]\n",
            "file_read = \"project\"\n",
            "file_write = \"sandbox\"\n",
            "host_materialization = \"deny\"\n",
            "host_execution = \"deny\"\n",
            "network = \"deny\"\n",
            "delegation = \"pinned\"\n",
            "unknown_interpreter = \"reject\"\n",
            "\n",
            "[sandbox]\n",
            "mode = \"disposable\"\n",
            "allow_local = false\n",
            "\n",
            "[limits]\n",
            "timeout_ms = 30000\n",
            "memory_bytes = 1073741824\n",
            "processes = 512\n",
            "stdout_bytes = 16777216\n",
            "stderr_bytes = 16777216\n",
            "\n",
            "[export]\n",
            "mode = \"strict\"\n",
            "\n",
            "[audit]\n",
            "persona = \"pedantic\"\n",
            "fail_on = \"high\"\n",
            "acknowledgement_max_days = 30\n",
            "acknowledgements = []\n",
        )
        .to_owned()
    }

    pub(crate) fn decode(input: &str) -> Result<Self, Vec<String>> {
        let config: Self = toml::from_str(input)
            .map_err(|error| vec![format!("invalid project.toml: {error}")])?;
        let mut errors = Vec::new();
        if config.version != 1 {
            errors.push(format!(
                "project.toml version must be 1 (found {})",
                config.version
            ));
        }
        validate_unique_paths("entrypoint", &config.entrypoints, &mut errors);
        validate_migration_config(&config, &mut errors);
        config.limits.validate(&mut errors);
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }

    pub(crate) fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        let text = toml::to_string_pretty(self)
            .map_err(|error| format!("cannot encode project.toml: {error}"))?;
        Self::decode(&text).map_err(|errors| errors.join("; "))?;
        Ok(text.into_bytes())
    }
}

fn validate_migration_config(config: &ProjectConfig, errors: &mut Vec<String>) {
    if config.migration.generator.trim().is_empty()
        || config.migration.generator.chars().any(char::is_whitespace)
    {
        errors.push("migration.generator must be a non-empty token".into());
    }
    for (label, path) in [
        (
            "migration.module_root",
            config.migration.module_root.as_str(),
        ),
        (
            "integration.rust.module_root",
            config.integration.rust.module_root.as_str(),
        ),
        (
            "integration.go.module_root",
            config.integration.go.module_root.as_str(),
        ),
    ] {
        validate_contract_path(label, path, errors);
    }
    let mut external_names = std::collections::BTreeSet::new();
    for generator in &config.migration.external_generators {
        if !portable_name(&generator.name) || generator.name.starts_with("external:") {
            errors.push(format!(
                "external generator name must be a portable unprefixed token: {}",
                generator.name
            ));
        }
        if !external_names.insert(generator.name.as_str()) {
            errors.push(format!(
                "duplicate external generator name: {}",
                generator.name
            ));
        }
        validate_contract_path(
            "external generator executable",
            &generator.executable,
            errors,
        );
        if !crate::digest::valid_pinned_sha256(&generator.digest) {
            errors.push(format!(
                "external generator {} digest must be pinned with sha256",
                generator.name
            ));
        }
        let capabilities = generator
            .capabilities
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if capabilities.is_empty() || capabilities.len() != generator.capabilities.len() {
            errors.push(format!(
                "external generator {} capabilities must be non-empty and unique",
                generator.name
            ));
        }
    }
    validate_generator_selection(
        "migration",
        &config.migration.generator,
        config.migration.target,
        &config.migration.external_generators,
        errors,
    );
    let mut overrides = std::collections::BTreeSet::new();
    for location in &config.location_overrides {
        validate_contract_path("location override", &location.path, errors);
        validate_contract_path(
            "location override module_root",
            &location.module_root,
            errors,
        );
        if location.start_byte >= location.end_byte {
            errors.push(format!(
                "location override span must be non-empty for {}",
                location.path
            ));
        }
        if location.generator.trim().is_empty()
            || location.generator.chars().any(char::is_whitespace)
        {
            errors.push(format!(
                "location override generator must be a non-empty token for {}",
                location.path
            ));
        }
        validate_generator_selection(
            "location override",
            &location.generator,
            location.target,
            &config.migration.external_generators,
            errors,
        );
        if !overrides.insert((
            location.path.as_str(),
            location.start_byte,
            location.end_byte,
        )) {
            errors.push(format!(
                "duplicate exact location override: {}@{}..{}",
                location.path, location.start_byte, location.end_byte
            ));
        }
    }
    let mut interpreter_paths = std::collections::BTreeSet::new();
    for interpreter in &config.interpreter_overrides {
        validate_contract_path("interpreter override", &interpreter.path, errors);
        if !interpreter_paths.insert(interpreter.path.as_str()) {
            errors.push(format!(
                "duplicate exact interpreter override: {}",
                interpreter.path
            ));
        }
    }
    let mut cell_ids = std::collections::BTreeSet::new();
    for cell in &config.platform_cells {
        if !cfg!(test) && cell.approval != Approval::Draft {
            errors.push(format!(
                "platform cell {} must remain draft; use deshell matrix approve",
                cell.id
            ));
        }
        if cell.id.is_empty()
            || cell.id.len() > 128
            || !cell.id.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphanumeric() || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
            })
        {
            errors.push(format!(
                "platform cell id must be a portable filename component: {}",
                cell.id
            ));
        }
        if cell.operating_system.trim().is_empty()
            || cell.architecture.trim().is_empty()
            || cell.runtime.trim().is_empty()
        {
            errors.push("platform cell OS, architecture, and runtime must not be empty".into());
        }
        if !cell_ids.insert(cell.id.as_str()) {
            errors.push(format!("duplicate platform cell id: {}", cell.id));
        }
    }
    let mut command_names = std::collections::BTreeSet::new();
    for command in &config.validation_commands {
        if command.name.trim().is_empty() || !command_names.insert(command.name.as_str()) {
            errors.push(format!(
                "validation command name must be non-empty and unique: {}",
                command.name
            ));
        }
        if command.argv.is_empty()
            || command.argv[0].trim().is_empty()
            || command.argv[0].starts_with('-')
        {
            errors.push(format!(
                "validation command {} argv[0] must be an exact non-option program",
                command.name
            ));
        }
        if command.argv.iter().any(|argument| argument.contains('\0')) {
            errors.push(format!(
                "validation command {} argv must not contain NUL",
                command.name
            ));
        }
    }
    if config.audit.acknowledgement_max_days == 0 || config.audit.acknowledgement_max_days > 366 {
        errors.push("audit.acknowledgement_max_days must be between 1 and 366".into());
    }
    let mut acknowledgements = std::collections::BTreeSet::new();
    for acknowledgement in &config.audit.acknowledgements {
        if acknowledgement.rule.trim().is_empty()
            || acknowledgement.reason.trim().is_empty()
            || acknowledgement.owner.trim().is_empty()
        {
            errors.push("audit acknowledgement rule, reason, and owner must not be empty".into());
        }
        if !crate::digest::valid_sha256(&acknowledgement.location_digest) {
            errors.push(format!(
                "audit acknowledgement location_digest is invalid for {}",
                acknowledgement.rule
            ));
        }
        if !valid_iso_date(&acknowledgement.expires) {
            errors.push(format!(
                "audit acknowledgement expiry must be YYYY-MM-DD for {}",
                acknowledgement.rule
            ));
        }
        if !acknowledgements.insert((
            acknowledgement.rule.as_str(),
            acknowledgement.location_digest.as_str(),
        )) {
            errors.push(format!(
                "duplicate audit acknowledgement for {}",
                acknowledgement.rule
            ));
        }
    }
}

fn validate_generator_selection(
    label: &str,
    generator: &str,
    target: MigrationTarget,
    external: &[ExternalGenerator],
    errors: &mut Vec<String>,
) {
    if let Some(name) = generator.strip_prefix("external:") {
        let Some(registration) = external.iter().find(|entry| entry.name == name) else {
            errors.push(format!(
                "{label} generator references an unregistered external generator: {generator}"
            ));
            return;
        };
        if !registration.capabilities.contains(&target) {
            errors.push(format!(
                "external generator {name} does not declare the selected {target:?} capability"
            ));
        }
        return;
    }
    let expected = match target {
        MigrationTarget::Rust => "rust",
        MigrationTarget::Go => "go",
        MigrationTarget::Host => "host",
        MigrationTarget::Agent => {
            errors.push(format!(
                "{label} agent target requires a registered external generator"
            ));
            return;
        }
    };
    if generator != expected {
        errors.push(format!(
            "{label} generator {generator} does not match target {expected}"
        ));
    }
}

fn portable_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn valid_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<usize>().unwrap_or(0);
    let day = value[8..10].parse::<u8>().unwrap_or(0);
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap_year { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    month
        .checked_sub(1)
        .and_then(|index| days.get(index))
        .is_some_and(|maximum| (1..=*maximum).contains(&day))
}

impl ResourceLimits {
    pub(crate) const DEFAULT: Self = Self {
        timeout_ms: 30_000,
        memory_bytes: 1_073_741_824,
        processes: 512,
        stdout_bytes: 16_777_216,
        stderr_bytes: 16_777_216,
    };

    pub(crate) fn validate(self, errors: &mut Vec<String>) {
        if self.timeout_ms == 0 || self.timeout_ms > 86_400_000 {
            errors.push("limits.timeout_ms must be between 1 and 86400000".into());
        }
        if self.memory_bytes < 16 * 1024 * 1024 || self.memory_bytes > 1024_u64.pow(4) {
            errors.push("limits.memory_bytes must be between 16 MiB and 1 TiB".into());
        }
        if self.processes == 0 || self.processes > 65_535 {
            errors.push("limits.processes must be between 1 and 65535".into());
        }
        for (name, value) in [
            ("stdout_bytes", self.stdout_bytes),
            ("stderr_bytes", self.stderr_bytes),
        ] {
            if value == 0 || value > 1024 * 1024 * 1024 {
                errors.push(format!("limits.{name} must be between 1 byte and 1 GiB"));
            }
        }
    }

    pub(crate) fn narrows(self, project: Self) -> bool {
        self.timeout_ms <= project.timeout_ms
            && self.memory_bytes <= project.memory_bytes
            && self.processes <= project.processes
            && self.stdout_bytes <= project.stdout_bytes
            && self.stderr_bytes <= project.stderr_bytes
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Lockfile {
    pub version: u32,
    pub toolchain: Toolchain,
    pub protocol: ProtocolPins,
    pub artifacts: ArtifactPins,
    pub adapters: AdapterPins,
    pub parsers: ParserPins,
    pub interpreters: InterpreterPins,
    pub targets: TargetPins,
    pub lab: LabPin,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Toolchain {
    pub rust: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolPins {
    pub json_rpc: u32,
    pub effect_ir: u32,
    pub evidence: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactPins {
    pub command_model: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdapterPins {
    pub powershell: String,
    pub nushell: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParserPins {
    pub bash: String,
    pub fish: String,
    pub cmd: String,
    pub powershell: String,
    pub nushell: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InterpreterPins {
    pub posix_sh: String,
    pub bash: String,
    pub zsh: String,
    pub fish: String,
    pub powershell: String,
    pub cmd: String,
    pub nushell: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TargetPins {
    pub dagger_image: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabPin {
    pub image: String,
    pub assets: Vec<LabAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LabAsset {
    pub name: String,
    pub role: LabAssetRole,
    pub operating_system: String,
    pub architecture: String,
    pub path: String,
    pub sha256: String,
    pub executable: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LabAssetRole {
    Runtime,
    Helper,
}

fn parser_digest(identity: &str, contract: &[u8]) -> String {
    let mut basis = Vec::with_capacity(identity.len() + 1 + contract.len());
    basis.extend_from_slice(identity.as_bytes());
    basis.push(0);
    basis.extend_from_slice(contract);
    crate::digest::sha256(&basis)
}

impl Lockfile {
    pub(crate) fn default_text() -> String {
        let powershell =
            crate::digest::sha256(include_bytes!("../../../adapters/powershell/adapter.ps1"));
        let nushell =
            crate::digest::sha256(b"deshell-internal-nushell-adapter-v1:nu-parser=0.115.1");
        let bash_parser = parser_digest(
            "tree-sitter-bash/0.25.1",
            tree_sitter_bash::NODE_TYPES.as_bytes(),
        );
        let fish_parser = parser_digest(
            "tree-sitter-fish/3.6.0",
            tree_sitter_fish::NODE_TYPES.as_bytes(),
        );
        let cmd_parser = parser_digest(
            "tree-sitter-batch/0.11.1",
            tree_sitter_batch::NODE_TYPES.as_bytes(),
        );
        let powershell_parser = parser_digest(
            "System.Management.Automation.Language.Parser:PowerShell/7.6.5",
            include_bytes!("../../../adapters/powershell/adapter.ps1"),
        );
        let nushell_parser = parser_digest("nu-parser/0.115.1", b"nu-parser/0.115.1");
        let command_model =
            crate::digest::sha256(b"deshell-command-model-v1:posix-exit-status:exec");
        let interpreter_pins = [
            crate::frontend::default_interpreter_pin("sh"),
            crate::frontend::default_interpreter_pin("bash"),
            crate::frontend::default_interpreter_pin("zsh"),
            crate::frontend::default_interpreter_pin("fish"),
            crate::frontend::default_interpreter_pin("powershell"),
            crate::frontend::default_interpreter_pin("cmd"),
            crate::frontend::default_interpreter_pin("nu"),
        ];
        format!(
            concat!(
                "version = 1\n",
                "\n",
                "[toolchain]\n",
                "rust = \"1.98.0\"\n",
                "\n",
                "[protocol]\n",
                "json_rpc = 1\n",
                "effect_ir = 1\n",
                "evidence = 1\n",
                "\n",
                "[artifacts]\n",
                "command_model = \"sha256:{}\"\n",
                "\n",
                "[adapters]\n",
                "powershell = \"sha256:{}\"\n",
                "nushell = \"sha256:{}\"\n",
                "\n",
                "[parsers]\n",
                "bash = \"sha256:{}\"\n",
                "fish = \"sha256:{}\"\n",
                "cmd = \"sha256:{}\"\n",
                "powershell = \"sha256:{}\"\n",
                "nushell = \"sha256:{}\"\n",
                "\n",
                "[interpreters]\n",
                "posix_sh = \"{}\"\n",
                "bash = \"{}\"\n",
                "zsh = \"{}\"\n",
                "fish = \"{}\"\n",
                "powershell = \"{}\"\n",
                "cmd = \"{}\"\n",
                "nushell = \"{}\"\n",
                "\n",
                "[targets]\n",
                "dagger_image = \"unconfigured\"\n",
                "\n",
                "[lab]\n",
                "image = \"unconfigured\"\n",
                "assets = []\n",
            ),
            command_model,
            powershell,
            nushell,
            bash_parser,
            fish_parser,
            cmd_parser,
            powershell_parser,
            nushell_parser,
            interpreter_pins[0],
            interpreter_pins[1],
            interpreter_pins[2],
            interpreter_pins[3],
            interpreter_pins[4],
            interpreter_pins[5],
            interpreter_pins[6],
        )
    }

    pub(crate) fn decode(input: &str) -> Result<Self, Vec<String>> {
        let lock: Self = toml::from_str(input)
            .map_err(|error| vec![format!("invalid deshell.lock: {error}")])?;
        let mut errors = Vec::new();
        if lock.version != 1 {
            errors.push(format!(
                "deshell.lock version must be 1 (found {})",
                lock.version
            ));
        }
        if lock.toolchain.rust != "1.98.0" {
            errors.push("toolchain.rust must be 1.98.0".into());
        }
        if lock.protocol.json_rpc != 1 {
            errors.push("protocol.json_rpc must be 1".into());
        }
        if lock.protocol.effect_ir != 1 {
            errors.push("protocol.effect_ir must be 1".into());
        }
        if lock.protocol.evidence != 1 {
            errors.push("protocol.evidence must be 1".into());
        }
        for (name, value) in [
            (
                "artifacts.command_model",
                lock.artifacts.command_model.as_str(),
            ),
            ("adapters.powershell", lock.adapters.powershell.as_str()),
            ("adapters.nushell", lock.adapters.nushell.as_str()),
            ("parsers.bash", lock.parsers.bash.as_str()),
            ("parsers.fish", lock.parsers.fish.as_str()),
            ("parsers.cmd", lock.parsers.cmd.as_str()),
            ("parsers.powershell", lock.parsers.powershell.as_str()),
            ("parsers.nushell", lock.parsers.nushell.as_str()),
        ] {
            if !crate::digest::valid_pinned_sha256(value) {
                errors.push(format!("{name} must be sha256:<64 lowercase hex>"));
            }
        }
        let expected = Self::default_text();
        if let Ok(expected) = toml::from_str::<Self>(&expected) {
            if lock.artifacts != expected.artifacts {
                errors.push("command model digest does not match this deshell binary".into());
            }
            if lock.adapters != expected.adapters {
                errors.push("adapter digest does not match this deshell binary".into());
            }
            if lock.parsers != expected.parsers {
                errors.push("parser digest does not match this deshell binary".into());
            }
        }
        for (name, value) in [
            ("posix_sh", &lock.interpreters.posix_sh),
            ("bash", &lock.interpreters.bash),
            ("zsh", &lock.interpreters.zsh),
            ("fish", &lock.interpreters.fish),
            ("powershell", &lock.interpreters.powershell),
            ("cmd", &lock.interpreters.cmd),
            ("nushell", &lock.interpreters.nushell),
        ] {
            if !crate::digest::valid_pinned_sha256(value) {
                errors.push(format!(
                    "interpreters.{name} must be sha256:<64 lowercase hex>"
                ));
            }
        }
        if lock.targets.dagger_image != "unconfigured"
            && !crate::lab::digest_pinned(&lock.targets.dagger_image)
        {
            errors.push(
                "targets.dagger_image must be unconfigured or pinned by @sha256 digest".into(),
            );
        }
        if lock.lab.image != "unconfigured" && !crate::lab::digest_pinned(&lock.lab.image) {
            errors.push("lab.image must be unconfigured or pinned by @sha256 digest".into());
        }
        let mut asset_names = std::collections::BTreeSet::new();
        let mut asset_paths = std::collections::BTreeSet::new();
        for asset in &lock.lab.assets {
            if asset.name.is_empty()
                || !asset
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            {
                errors.push(format!(
                    "lab asset name must be a portable filename: {}",
                    asset.name
                ));
            }
            if !asset_names.insert(asset.name.as_str()) {
                errors.push(format!("duplicate lab asset name: {}", asset.name));
            }
            if !matches!(
                asset.operating_system.as_str(),
                "linux" | "macos" | "windows"
            ) {
                errors.push(format!(
                    "lab asset {} has unsupported operating_system {}",
                    asset.name, asset.operating_system
                ));
            }
            if !matches!(asset.architecture.as_str(), "x86_64" | "aarch64") {
                errors.push(format!(
                    "lab asset {} has unsupported architecture {}",
                    asset.name, asset.architecture
                ));
            }
            match crate::ir::normalize_path(&asset.path) {
                Ok(path) if path == asset.path => {
                    if !asset_paths.insert(asset.path.as_str()) {
                        errors.push(format!("duplicate lab asset path: {}", asset.path));
                    }
                }
                Ok(_) => errors.push(format!(
                    "lab asset {} path is not normalized: {}",
                    asset.name, asset.path
                )),
                Err(error) => {
                    errors.push(format!("lab asset {} path is invalid: {error}", asset.name))
                }
            }
            if !crate::digest::valid_pinned_sha256(&asset.sha256) {
                errors.push(format!(
                    "lab asset {} sha256 must be sha256:<64 lowercase hex>",
                    asset.name
                ));
            }
        }
        if !lock.lab.assets.is_empty() && !crate::lab::digest_pinned(&lock.lab.image) {
            errors.push("lab assets require a digest-pinned lab.image".into());
        }
        if errors.is_empty() {
            Ok(lock)
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scenario {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub approval: ScenarioApproval,
    #[serde(default)]
    pub arguments: Vec<NamedValue>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub environment: Vec<NamedValue>,
    #[serde(default)]
    pub fixtures: Vec<Fixture>,
    #[serde(default)]
    pub stdin: Option<BinaryData>,
    #[serde(default)]
    pub cwd: Option<String>,
    pub limits: ResourceLimits,
    pub expect: Expectation,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScenarioApproval {
    #[default]
    Draft,
    Approved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BinaryData {
    #[serde(default)]
    pub utf8: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
}

impl BinaryData {
    pub(crate) fn from_utf8(value: impl Into<String>) -> Self {
        Self {
            utf8: Some(value.into()),
            base64: None,
        }
    }

    pub(crate) fn bytes(&self) -> Result<Vec<u8>, String> {
        match (&self.utf8, &self.base64) {
            (Some(value), None) => Ok(value.as_bytes().to_vec()),
            (None, Some(value)) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(value)
                    .map_err(|error| format!("invalid canonical base64: {error}"))?;
                if base64::engine::general_purpose::STANDARD.encode(&bytes) != *value {
                    return Err("base64 must use canonical padded encoding".into());
                }
                Ok(bytes)
            }
            _ => Err("binary data requires exactly one of utf8 or base64".into()),
        }
    }
}

impl From<&str> for BinaryData {
    fn from(value: &str) -> Self {
        Self::from_utf8(value)
    }
}

impl From<String> for BinaryData {
    fn from(value: String) -> Self {
        Self::from_utf8(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedValue {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fixture {
    pub path: String,
    pub contents: BinaryData,
    #[serde(default)]
    pub executable: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expectation {
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<BinaryData>,
    #[serde(default)]
    pub stderr: Option<BinaryData>,
    #[serde(default)]
    pub files: Vec<ExpectedFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExpectedFile {
    pub path: String,
    pub sha256: String,
}

impl Scenario {
    #[cfg(test)]
    pub(crate) fn default_text() -> String {
        concat!(
            "version = 1\n",
            "name = \"default\"\n",
            "approval = \"draft\"\n",
            "arguments = []\n",
            "argv = []\n",
            "environment = []\n",
            "\n",
            "[limits]\n",
            "timeout_ms = 30000\n",
            "memory_bytes = 1073741824\n",
            "processes = 512\n",
            "stdout_bytes = 16777216\n",
            "stderr_bytes = 16777216\n",
            "\n",
            "[expect]\n",
        )
        .to_owned()
    }

    pub(crate) fn decode(input: &str) -> Result<Self, Vec<String>> {
        let scenario: Self = toml::from_str(input)
            .map_err(|error| vec![format!("invalid scenario TOML: {error}")])?;
        let mut errors = Vec::new();
        if scenario.version != 1 {
            errors.push(format!(
                "scenario version must be 1 (found {})",
                scenario.version
            ));
        }
        if scenario.name.trim().is_empty() {
            errors.push("scenario name must not be empty".into());
        }
        if !cfg!(test) && scenario.approval != ScenarioApproval::Draft {
            errors.push("scenario must remain draft; use deshell scenario approve".into());
        }
        scenario.limits.validate(&mut errors);
        if let Some(cwd) = &scenario.cwd {
            validate_contract_path("scenario cwd", cwd, &mut errors);
        }
        if let Some(stdin) = &scenario.stdin
            && let Err(error) = stdin.bytes()
        {
            errors.push(format!("invalid scenario stdin: {error}"));
        }
        validate_named_values("scenario argument", &scenario.arguments, false, &mut errors);
        validate_named_values(
            "scenario environment",
            &scenario.environment,
            true,
            &mut errors,
        );
        let mut fixture_paths = std::collections::BTreeSet::new();
        for fixture in &scenario.fixtures {
            validate_contract_path("fixture", &fixture.path, &mut errors);
            if !fixture_paths.insert(fixture.path.clone()) {
                errors.push(format!("duplicate fixture path: {}", fixture.path));
            }
            if let Err(error) = fixture.contents.bytes() {
                errors.push(format!(
                    "invalid fixture contents for {}: {error}",
                    fixture.path
                ));
            }
        }
        for (name, value) in [
            ("stdout", scenario.expect.stdout.as_ref()),
            ("stderr", scenario.expect.stderr.as_ref()),
        ] {
            if let Some(value) = value
                && let Err(error) = value.bytes()
            {
                errors.push(format!("invalid expected {name}: {error}"));
            }
        }
        let mut expected_paths = std::collections::BTreeSet::new();
        for file in &scenario.expect.files {
            validate_contract_path("expected file", &file.path, &mut errors);
            if !expected_paths.insert(file.path.clone()) {
                errors.push(format!("duplicate expected file path: {}", file.path));
            }
            if !crate::digest::valid_sha256(&file.sha256) {
                errors.push(format!("expected file sha256 is invalid: {}", file.path));
            }
        }
        if errors.is_empty() {
            Ok(scenario)
        } else {
            Err(errors)
        }
    }

    pub(crate) fn digest(&self) -> Result<String, String> {
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        Ok(crate::digest::sha256(
            &crate::canonical_json::canonical_bytes(&value)?,
        ))
    }
}

fn validate_unique_paths(label: &str, paths: &[String], errors: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    for path in paths {
        validate_contract_path(label, path, errors);
        if !seen.insert(path.clone()) {
            errors.push(format!("duplicate {label}: {path}"));
        }
    }
}

fn validate_contract_path(label: &str, path: &str, errors: &mut Vec<String>) {
    match crate::ir::normalize_path(path) {
        Ok(normalized) if normalized == path => {}
        Ok(_) => errors.push(format!("{label} path is not normalized: {path}")),
        Err(error) => errors.push(format!("invalid {label} path {path}: {error}")),
    }
}

fn validate_named_values(
    label: &str,
    values: &[NamedValue],
    environment: bool,
    errors: &mut Vec<String>,
) {
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        if value.name.trim().is_empty() {
            errors.push(format!("{label} name must not be empty"));
        }
        if environment && !valid_environment_name(&value.name) {
            errors.push(format!("invalid scenario environment name: {}", value.name));
        }
        if !seen.insert(value.name.clone()) {
            errors.push(format!("duplicate {label}: {}", value.name));
        }
    }
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_v1_round_trips_and_rejects_extensions() {
        let text = ProjectConfig::default_text();
        let config = ProjectConfig::decode(&text).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(
            config.policy.unknown_interpreter,
            UnknownInterpreter::Reject
        );
        assert_eq!(config.limits, ResourceLimits::DEFAULT);
        assert!(!config.sandbox.allow_local);
        assert!(ProjectConfig::decode(&(text + "future = true\n")).is_err());
    }

    #[test]
    fn project_config_rejects_duplicate_and_unsafe_entrypoints() {
        let duplicate = ProjectConfig::default_text().replace(
            "entrypoints = []",
            "entrypoints = [\"build.sh\", \"build.sh\"]",
        );
        assert!(
            ProjectConfig::decode(&duplicate)
                .unwrap_err()
                .join("; ")
                .contains("duplicate")
        );
        let unsafe_path = ProjectConfig::default_text()
            .replace("entrypoints = []", "entrypoints = [\"../build.sh\"]");
        assert!(ProjectConfig::decode(&unsafe_path).is_err());
    }

    #[test]
    fn project_v1_declares_the_complete_migration_oracle_policy() {
        let config = ProjectConfig::decode(&ProjectConfig::default_text()).unwrap();
        let value = serde_json::to_value(config).unwrap();
        assert_eq!(value["migration"]["generator"], "rust");
        assert_eq!(value["migration"]["target"], "rust");
        assert_eq!(value["migration"]["module_root"], "src/bin");
        assert_eq!(value["migration"]["agent_context"], "minimal");
        assert_eq!(value["migration"]["allow_agent_network"], false);
        assert!(
            value["migration"]["external_generators"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(value["integration"]["rust"]["module_root"], "src/bin");
        assert_eq!(value["integration"]["go"]["module_root"], "cmd");
        assert_eq!(value["audit"]["persona"], "pedantic");
        assert_eq!(value["audit"]["fail_on"], "high");
        assert!(value["location_overrides"].as_array().unwrap().is_empty());
        assert!(value["platform_cells"].as_array().unwrap().is_empty());
        assert!(value["validation_commands"].as_array().unwrap().is_empty());
    }

    #[test]
    fn project_v1_rejects_non_exact_overrides_and_option_like_validation_programs() {
        let override_without_extent = ProjectConfig::default_text().replace(
            "location_overrides = []",
            "location_overrides = [{ path = \"build.sh\", start_byte = 4, end_byte = 4, generator = \"go\", target = \"go\", module_root = \"cmd\" }]",
        );
        let error = ProjectConfig::decode(&override_without_extent)
            .unwrap_err()
            .join("; ");
        assert!(error.contains("location override span"), "{error}");

        let option_program = ProjectConfig::default_text().replace(
            "validation_commands = []",
            "validation_commands = [{ name = \"test\", kind = \"test\", argv = [\"--dangerous\"] }]",
        );
        let error = ProjectConfig::decode(&option_program)
            .unwrap_err()
            .join("; ");
        assert!(error.contains("argv[0]"), "{error}");
    }

    #[test]
    fn external_generators_require_a_registration_digest_and_matching_capability() {
        let unregistered = ProjectConfig::default_text()
            .replacen(
                "generator = \"rust\"",
                "generator = \"external:missing\"",
                1,
            )
            .replacen("target = \"rust\"", "target = \"agent\"", 1);
        let error = ProjectConfig::decode(&unregistered).unwrap_err().join("; ");
        assert!(error.contains("unregistered external generator"), "{error}");

        let wrong_capability = unregistered
            .replace("external:missing", "external:fixture")
            .replace(
                "external_generators = []",
                &format!(
                    "external_generators = [{{ name = \"fixture\", executable = \"tools/fixture\", digest = \"sha256:{}\", capabilities = [\"rust\"] }}]",
                    "a".repeat(64)
                ),
            );
        let error = ProjectConfig::decode(&wrong_capability)
            .unwrap_err()
            .join("; ");
        assert!(error.contains("does not declare"), "{error}");
    }

    #[test]
    fn platform_cell_ids_are_safe_evidence_filename_components() {
        for id in ["../escape", "linux/x86", "two words", ""] {
            let text = ProjectConfig::default_text().replace(
                "platform_cells = []",
                &format!(
                    "platform_cells = [{{ id = {id:?}, operating_system = \"linux\", architecture = \"x86_64\", runtime = \"native\", approval = \"approved\" }}]"
                ),
            );
            let errors = ProjectConfig::decode(&text).unwrap_err().join("; ");
            assert!(errors.contains("portable filename"), "{id:?}: {errors}");
        }
    }

    #[test]
    fn lockfile_is_a_fresh_v1_contract_without_migration() {
        let text = Lockfile::default_text();
        let lock = Lockfile::decode(&text).unwrap();
        assert_eq!(lock.version, 1);
        assert_eq!(lock.protocol.effect_ir, 1);
        assert_eq!(lock.protocol.json_rpc, 1);
        assert!(lock.adapters.powershell.starts_with("sha256:"));
        for parser in [
            &lock.parsers.bash,
            &lock.parsers.fish,
            &lock.parsers.cmd,
            &lock.parsers.powershell,
            &lock.parsers.nushell,
        ] {
            assert!(parser.starts_with("sha256:"));
        }
        assert!(Lockfile::decode(&text.replacen("version = 1", "version = 2", 1)).is_err());
        assert!(Lockfile::decode(&(text.clone() + "migrated_from = 0\n")).is_err());
        let stale_parser =
            text.replacen(&lock.parsers.bash, &format!("sha256:{}", "a".repeat(64)), 1);
        assert!(
            Lockfile::decode(&stale_parser)
                .unwrap_err()
                .join("; ")
                .contains("parser")
        );
        let option_like_image = text.replace(
            "image = \"unconfigured\"",
            &format!("image = \"--privileged@sha256:{}\"", "a".repeat(64)),
        );
        assert!(Lockfile::decode(&option_like_image).is_err());

        let assets = text
            .replace(
                "image = \"unconfigured\"",
                &format!("image = \"registry.invalid/lab@sha256:{}\"", "a".repeat(64)),
            )
            .replace(
                "assets = []",
                &format!(
                    concat!(
                        "[[lab.assets]]\n",
                        "name = \"runtime-one\"\n",
                        "role = \"runtime\"\n",
                        "operating_system = \"linux\"\n",
                        "architecture = \"x86_64\"\n",
                        "path = \".deshell/runtime/shared.bin\"\n",
                        "sha256 = \"sha256:{}\"\n",
                        "executable = false\n",
                        "\n",
                        "[[lab.assets]]\n",
                        "name = \"runtime-two\"\n",
                        "role = \"runtime\"\n",
                        "operating_system = \"linux\"\n",
                        "architecture = \"x86_64\"\n",
                        "path = \".deshell/runtime/shared.bin\"\n",
                        "sha256 = \"sha256:{}\"\n",
                        "executable = false\n",
                    ),
                    "b".repeat(64),
                    "b".repeat(64)
                ),
            );
        assert!(
            Lockfile::decode(&assets)
                .unwrap_err()
                .join("; ")
                .contains("duplicate lab asset path")
        );
    }

    #[test]
    fn scenario_uses_name_value_arrays_and_rejects_duplicates() {
        let text = Scenario::default_text().replace(
            "environment = []",
            "environment = [{ name = \"TOKEN\", value = \"one\" }, { name = \"TOKEN\", value = \"two\" }]",
        );
        let error = Scenario::decode(&text).unwrap_err().join("; ");
        assert!(
            error.contains("duplicate scenario environment: TOKEN"),
            "{error}"
        );
    }

    #[test]
    fn scenario_rejects_fixture_traversal_and_bad_digest() {
        let traversal = Scenario::default_text()
            + "\n[[fixtures]]\npath = \"../outside\"\ncontents = { utf8 = \"bad\" }\n";
        assert!(Scenario::decode(&traversal).is_err());
        let digest =
            Scenario::default_text() + "\n[[expect.files]]\npath = \"out.txt\"\nsha256 = \"abc\"\n";
        assert!(Scenario::decode(&digest).is_err());
    }

    #[test]
    fn scenario_binary_fields_are_canonical_and_expectations_are_optional() {
        let text = Scenario::default_text().replace(
            "environment = []",
            "environment = []\nstdin = { base64 = \"AP8=\" }",
        );
        let scenario = Scenario::decode(&text).unwrap();
        assert_eq!(scenario.stdin.unwrap().bytes().unwrap(), [0, 0xff]);
        assert_eq!(scenario.expect, Expectation::default());
        assert!(Scenario::decode(&text.replace("AP8=", "AP8")).is_err());
    }

    #[test]
    fn synthesized_scenarios_are_drafts_until_a_human_approves_them() {
        let draft = Scenario::default_text();
        assert!(draft.contains("approval = \"draft\""));
        let scenario = Scenario::decode(&draft).unwrap();
        let encoded = serde_json::to_value(scenario).unwrap();
        assert_eq!(encoded["approval"], "draft");

        let approved = draft.replace("approval = \"draft\"", "approval = \"approved\"");
        let scenario = Scenario::decode(&approved).unwrap();
        let encoded = serde_json::to_value(scenario).unwrap();
        assert_eq!(encoded["approval"], "approved");
    }

    #[test]
    fn interpreter_names_dates_and_resource_limits_cover_contract_boundaries() {
        assert_eq!(
            [
                ConfiguredInterpreter::Sh,
                ConfiguredInterpreter::Bash,
                ConfiguredInterpreter::Zsh,
                ConfiguredInterpreter::Fish,
                ConfiguredInterpreter::Powershell,
                ConfiguredInterpreter::Cmd,
                ConfiguredInterpreter::Nu,
            ]
            .map(ConfiguredInterpreter::name),
            ["sh", "bash", "zsh", "fish", "powershell", "cmd", "nu"]
        );

        for valid in ["2000-02-29", "2026-08-29", "9999-12-31"] {
            assert!(valid_iso_date(valid), "rejected {valid}");
        }
        for invalid in [
            "2026-2-01",
            "2026/02/01",
            "year-01-01",
            "2025-02-29",
            "2024-02-30",
            "2026-04-31",
            "2026-00-01",
            "2026-01-00",
        ] {
            assert!(!valid_iso_date(invalid), "accepted {invalid}");
        }

        let mut limits = ResourceLimits::DEFAULT;
        limits.timeout_ms = 0;
        limits.memory_bytes = 1;
        limits.processes = 0;
        limits.stdout_bytes = 0;
        limits.stderr_bytes = 1024 * 1024 * 1024 + 1;
        let mut errors = Vec::new();
        limits.validate(&mut errors);
        assert_eq!(errors.len(), 5, "{errors:?}");

        let baseline = ResourceLimits::DEFAULT;
        assert!(baseline.narrows(baseline));
        for wider in [
            ResourceLimits {
                timeout_ms: baseline.timeout_ms + 1,
                ..baseline
            },
            ResourceLimits {
                memory_bytes: baseline.memory_bytes + 1,
                ..baseline
            },
            ResourceLimits {
                processes: baseline.processes + 1,
                ..baseline
            },
            ResourceLimits {
                stdout_bytes: baseline.stdout_bytes + 1,
                ..baseline
            },
            ResourceLimits {
                stderr_bytes: baseline.stderr_bytes + 1,
                ..baseline
            },
        ] {
            assert!(!wider.narrows(baseline));
        }
    }

    #[test]
    fn migration_policy_validation_reports_every_invalid_collection_member() {
        let mut config = ProjectConfig::decode(&ProjectConfig::default_text()).unwrap();
        config.migration.generator = "two words".into();
        config.migration.module_root = "../outside".into();
        config.integration.rust.module_root = "double//slash".into();
        config.integration.go.module_root.clear();
        config.migration.external_generators = vec![
            ExternalGenerator {
                name: "external:bad".into(),
                executable: "../generator".into(),
                digest: "bad".into(),
                capabilities: vec![],
            },
            ExternalGenerator {
                name: "external:bad".into(),
                executable: "generator".into(),
                digest: format!("sha256:{}", "a".repeat(64)),
                capabilities: vec![MigrationTarget::Rust, MigrationTarget::Rust],
            },
        ];
        let invalid_override = LocationOverride {
            path: "../script".into(),
            start_byte: 9,
            end_byte: 9,
            generator: " ".into(),
            target: MigrationTarget::Agent,
            module_root: "../module".into(),
        };
        config.location_overrides = vec![invalid_override.clone(), invalid_override];
        config.interpreter_overrides = vec![
            InterpreterOverride {
                path: "../script".into(),
                interpreter: ConfiguredInterpreter::Sh,
            },
            InterpreterOverride {
                path: "../script".into(),
                interpreter: ConfiguredInterpreter::Bash,
            },
        ];
        let invalid_cell = PlatformCell {
            id: "-bad/id".into(),
            operating_system: " ".into(),
            architecture: "".into(),
            runtime: "".into(),
            approval: Approval::Draft,
        };
        config.platform_cells = vec![invalid_cell.clone(), invalid_cell];
        config.validation_commands = vec![
            ValidationCommand {
                name: "".into(),
                kind: ValidationKind::Build,
                argv: vec![],
            },
            ValidationCommand {
                name: "".into(),
                kind: ValidationKind::Test,
                argv: vec!["program\0argument".into()],
            },
        ];
        config.audit.acknowledgement_max_days = 0;
        let invalid_ack = AuditAcknowledgement {
            rule: "".into(),
            location_digest: "bad".into(),
            reason: "".into(),
            owner: "".into(),
            expires: "2026-02-30".into(),
        };
        config.audit.acknowledgements = vec![invalid_ack.clone(), invalid_ack];

        let mut errors = Vec::new();
        validate_migration_config(&config, &mut errors);
        let errors = errors.join("; ");
        for expected in [
            "migration.generator",
            "migration.module_root",
            "external generator name",
            "duplicate external generator",
            "digest must be pinned",
            "capabilities must be non-empty and unique",
            "location override span",
            "duplicate exact location override",
            "duplicate exact interpreter override",
            "platform cell id",
            "duplicate platform cell id",
            "validation command name",
            "argv[0]",
            "argv must not contain NUL",
            "acknowledgement_max_days",
            "location_digest is invalid",
            "expiry must be YYYY-MM-DD",
            "duplicate audit acknowledgement",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }

        for (generator, target, expected) in [
            ("go", MigrationTarget::Rust, "does not match target rust"),
            ("rust", MigrationTarget::Go, "does not match target go"),
            ("rust", MigrationTarget::Host, "does not match target host"),
            (
                "rust",
                MigrationTarget::Agent,
                "requires a registered external",
            ),
            (
                "external:missing",
                MigrationTarget::Agent,
                "unregistered external",
            ),
        ] {
            let mut errors = Vec::new();
            validate_generator_selection("test", generator, target, &[], &mut errors);
            assert!(errors.join("; ").contains(expected));
        }
    }

    #[test]
    fn lockfile_validation_aggregates_stale_pins_and_unsafe_lab_assets() {
        let mut lock = Lockfile::decode(&Lockfile::default_text()).unwrap();
        lock.version = 2;
        lock.toolchain.rust = "nightly".into();
        lock.protocol.json_rpc = 2;
        lock.protocol.effect_ir = 2;
        lock.protocol.evidence = 2;
        lock.artifacts.command_model = "bad".into();
        lock.adapters.powershell = "bad".into();
        lock.adapters.nushell = "bad".into();
        lock.parsers.bash = "bad".into();
        lock.parsers.fish = "bad".into();
        lock.parsers.cmd = "bad".into();
        lock.parsers.powershell = "bad".into();
        lock.parsers.nushell = "bad".into();
        lock.interpreters.posix_sh = "bad".into();
        lock.interpreters.bash = "bad".into();
        lock.interpreters.zsh = "bad".into();
        lock.interpreters.fish = "bad".into();
        lock.interpreters.powershell = "bad".into();
        lock.interpreters.cmd = "bad".into();
        lock.interpreters.nushell = "bad".into();
        lock.targets.dagger_image = "latest".into();
        lock.lab.image = "latest".into();
        let invalid_asset = LabAsset {
            name: "bad/name".into(),
            role: LabAssetRole::Helper,
            operating_system: "plan9".into(),
            architecture: "mips".into(),
            path: "../outside".into(),
            sha256: "bad".into(),
            executable: false,
        };
        lock.lab.assets = vec![invalid_asset.clone(), invalid_asset];
        let text = toml::to_string(&lock).unwrap();
        let errors = Lockfile::decode(&text).unwrap_err().join("; ");
        for expected in [
            "version must be 1",
            "toolchain.rust",
            "protocol.json_rpc",
            "protocol.effect_ir",
            "protocol.evidence",
            "command model digest",
            "adapter digest",
            "parser digest",
            "interpreters.posix_sh",
            "dagger_image",
            "lab.image",
            "portable filename",
            "duplicate lab asset name",
            "unsupported operating_system",
            "unsupported architecture",
            "path is invalid",
            "sha256 must be",
            "assets require",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }
    }

    #[test]
    fn scenario_validation_aggregates_all_typed_input_errors() {
        let mut scenario = Scenario::decode(&Scenario::default_text()).unwrap();
        scenario.version = 2;
        scenario.name = " ".into();
        scenario.limits.timeout_ms = 0;
        scenario.cwd = Some("../outside".into());
        scenario.stdin = Some(BinaryData {
            utf8: None,
            base64: None,
        });
        scenario.arguments = vec![
            NamedValue {
                name: "".into(),
                value: "one".into(),
            },
            NamedValue {
                name: "".into(),
                value: "two".into(),
            },
        ];
        scenario.environment = vec![NamedValue {
            name: "1INVALID".into(),
            value: "value".into(),
        }];
        let invalid_fixture = Fixture {
            path: "../fixture".into(),
            contents: BinaryData {
                utf8: Some("one".into()),
                base64: Some("b25l".into()),
            },
            executable: false,
        };
        scenario.fixtures = vec![invalid_fixture.clone(), invalid_fixture];
        scenario.expect.stdout = Some(BinaryData {
            utf8: None,
            base64: None,
        });
        scenario.expect.stderr = Some(BinaryData {
            utf8: None,
            base64: Some("not canonical".into()),
        });
        let invalid_file = ExpectedFile {
            path: "../out".into(),
            sha256: "bad".into(),
        };
        scenario.expect.files = vec![invalid_file.clone(), invalid_file];

        let text = toml::to_string(&scenario).unwrap();
        let errors = Scenario::decode(&text).unwrap_err().join("; ");
        for expected in [
            "version must be 1",
            "name must not be empty",
            "limits.timeout_ms",
            "scenario cwd",
            "invalid scenario stdin",
            "scenario argument name",
            "duplicate scenario argument",
            "invalid scenario environment name",
            "invalid fixture",
            "duplicate fixture path",
            "invalid fixture contents",
            "invalid expected stdout",
            "invalid expected stderr",
            "invalid expected file",
            "duplicate expected file",
            "sha256 is invalid",
        ] {
            assert!(
                errors.contains(expected),
                "missing {expected:?} in {errors}"
            );
        }

        let approved = Scenario::decode(&Scenario::default_text()).unwrap();
        assert_eq!(
            BinaryData::from(String::from("bytes")).bytes().unwrap(),
            b"bytes"
        );
        assert_eq!(approved.digest().unwrap().len(), 64);
    }
}
