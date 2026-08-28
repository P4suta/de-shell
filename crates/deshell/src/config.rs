use base64::Engine as _;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectConfig {
    pub version: u32,
    pub entrypoints: Vec<String>,
    pub policy: Policy,
    pub sandbox: Sandbox,
    pub limits: ResourceLimits,
    pub export: ExportPolicy,
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
        config.limits.validate(&mut errors);
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
    }
}

impl ResourceLimits {
    pub(crate) const DEFAULT: Self = Self {
        timeout_ms: 30_000,
        memory_bytes: 1_073_741_824,
        processes: 512,
        stdout_bytes: 16_777_216,
        stderr_bytes: 16_777_216,
    };

    fn validate(self, errors: &mut Vec<String>) {
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

impl Lockfile {
    pub(crate) fn default_text() -> String {
        let powershell =
            crate::digest::sha256(include_bytes!("../../../adapters/powershell/adapter.ps1"));
        let nushell = crate::digest::sha256(b"deshell-internal-nushell-adapter-v1");
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
    pub(crate) fn default_text() -> String {
        concat!(
            "version = 1\n",
            "name = \"default\"\n",
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
    fn lockfile_is_a_fresh_v1_contract_without_migration() {
        let text = Lockfile::default_text();
        let lock = Lockfile::decode(&text).unwrap();
        assert_eq!(lock.version, 1);
        assert_eq!(lock.protocol.effect_ir, 1);
        assert_eq!(lock.protocol.json_rpc, 1);
        assert!(lock.adapters.powershell.starts_with("sha256:"));
        assert!(Lockfile::decode(&text.replacen("version = 1", "version = 2", 1)).is_err());
        assert!(Lockfile::decode(&(text.clone() + "migrated_from = 0\n")).is_err());
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
}
