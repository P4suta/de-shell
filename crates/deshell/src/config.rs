use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectConfig {
    pub version: u32,
    pub entrypoints: Vec<String>,
    pub policy: Policy,
    pub sandbox: Sandbox,
    pub export: ExportPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub host_write: HostWrite,
    pub network: NetworkPolicy,
    pub unknown_interpreter: UnknownInterpreter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum HostWrite {
    Deny,
    Project,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NetworkPolicy {
    Deny,
    RecordReplay,
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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SandboxMode {
    Disposable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportPolicy {
    pub strict: bool,
    pub bridge: bool,
}

impl ProjectConfig {
    pub(crate) fn default_text() -> String {
        concat!(
            "version = 1\n",
            "entrypoints = []\n",
            "\n",
            "[policy]\n",
            "host_write = \"deny\"\n",
            "network = \"deny\"\n",
            "unknown_interpreter = \"trace-only\"\n",
            "\n",
            "[sandbox]\n",
            "mode = \"disposable\"\n",
            "\n",
            "[export]\n",
            "strict = true\n",
            "bridge = false\n",
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
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(errors)
        }
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
pub(crate) struct LabPin {
    pub image: String,
}

impl Lockfile {
    pub(crate) fn default_text() -> String {
        let powershell =
            crate::digest::sha256(include_bytes!("../../../adapters/powershell/adapter.ps1"));
        let nushell = crate::digest::sha256(b"deshell-internal-nushell-adapter-v1");
        let command_model =
            crate::digest::sha256(b"deshell-command-model-v1:posix-exit-status:exec");
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
                "posix_sh = \"provided-by-lab-image\"\n",
                "bash = \"provided-by-lab-image\"\n",
                "zsh = \"provided-by-lab-image\"\n",
                "fish = \"provided-by-lab-image\"\n",
                "powershell = \"provided-by-lab-image\"\n",
                "cmd = \"provided-by-lab-image\"\n",
                "nushell = \"provided-by-lab-image\"\n",
                "\n",
                "[lab]\n",
                "image = \"unconfigured\"\n",
            ),
            command_model, powershell, nushell,
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
            if value.trim().is_empty() {
                errors.push(format!("interpreters.{name} must not be empty"));
            }
        }
        if lock.lab.image != "unconfigured" && !crate::lab::digest_pinned(&lock.lab.image) {
            errors.push("lab.image must be unconfigured or pinned by @sha256 digest".into());
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
    pub timeout_ms: u64,
    pub expect: Expectation,
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
    pub contents: String,
    #[serde(default)]
    pub executable: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expectation {
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
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
            "timeout_ms = 30000\n",
            "\n",
            "[expect]\n",
            "exit_code = 0\n",
            "stdout = \"\"\n",
            "stderr = \"\"\n",
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
        if scenario.timeout_ms == 0 || scenario.timeout_ms > 86_400_000 {
            errors.push("scenario timeout_ms must be between 1 and 86400000".into());
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
            UnknownInterpreter::TraceOnly
        );
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
            + "\n[[fixtures]]\npath = \"../outside\"\ncontents = \"bad\"\n";
        assert!(Scenario::decode(&traversal).is_err());
        let digest =
            Scenario::default_text() + "\n[[expect.files]]\npath = \"out.txt\"\nsha256 = \"abc\"\n";
        assert!(Scenario::decode(&digest).is_err());
    }
}
