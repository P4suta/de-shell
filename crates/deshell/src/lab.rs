use base64::Engine as _;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Platform {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Provider {
    Podman,
    DockerRootless,
    WindowsSandbox,
    HyperV,
    VirtualizationFramework,
}

pub(crate) trait Probe {
    fn command_exists(&self, command: &str) -> bool;
    fn feature_enabled(&self, feature: &str) -> bool;
    fn docker_rootless(&self) -> bool;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Network {
    Deny,
    Replay { proxy: String, tape: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    pub workspace: String,
    pub result_path: String,
    pub interpreter: String,
    pub script: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub timeout_ms: u64,
    pub network: Network,
    pub image: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessSpec {
    pub program: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub stdin: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentRequest {
    pub provider: String,
    pub host_write: String,
    pub network: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LaunchSpec {
    Process(ProcessSpec),
    WindowsConfig(String),
    AgentRequest(AgentRequest),
}

pub(crate) fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Podman => "podman",
        Provider::DockerRootless => "docker-rootless",
        Provider::WindowsSandbox => "windows-sandbox",
        Provider::HyperV => "hyper-v",
        Provider::VirtualizationFramework => "virtualization-framework",
    }
}

pub(crate) fn select(platform: Platform, probe: &dyn Probe) -> Result<Provider, String> {
    match platform {
        Platform::Linux if probe.command_exists("podman") => Ok(Provider::Podman),
        Platform::Linux if probe.command_exists("docker") && probe.docker_rootless() => {
            Ok(Provider::DockerRootless)
        }
        Platform::Linux => Err(
            "no supported rootless OCI runtime is available (install Podman or enable rootless Docker)"
                .into(),
        ),
        Platform::Windows if probe.feature_enabled("Containers-DisposableClientVM") => {
            Ok(Provider::WindowsSandbox)
        }
        Platform::Windows if probe.feature_enabled("Microsoft-Hyper-V-All") => {
            Ok(Provider::HyperV)
        }
        Platform::Windows => {
            Err("Windows Sandbox or Hyper-V is required for disposable observation".into())
        }
        Platform::Macos if probe.command_exists("deshell-vz-agent") => {
            Ok(Provider::VirtualizationFramework)
        }
        Platform::Macos => Err(
            "the signed deshell-vz-agent is required for Virtualization.framework observation"
                .into(),
        ),
    }
}

pub(crate) fn validate_provider(
    platform: Platform,
    probe: &dyn Probe,
    provider: Provider,
) -> Result<(), String> {
    match (platform, provider) {
        (Platform::Linux, Provider::Podman) if probe.command_exists("podman") => Ok(()),
        (Platform::Linux, Provider::Podman) => {
            Err("the requested Podman executable is unavailable".into())
        }
        (Platform::Linux, Provider::DockerRootless) if !probe.command_exists("docker") => {
            Err("the requested Docker executable is unavailable".into())
        }
        (Platform::Linux, Provider::DockerRootless) if !probe.docker_rootless() => {
            Err("the requested Docker daemon is not running in rootless mode".into())
        }
        (Platform::Linux, Provider::DockerRootless) => Ok(()),
        (Platform::Windows, Provider::WindowsSandbox)
            if probe.feature_enabled("Containers-DisposableClientVM") =>
        {
            Ok(())
        }
        (Platform::Windows, Provider::WindowsSandbox) => {
            Err("the requested Windows Sandbox feature is unavailable".into())
        }
        (Platform::Windows, Provider::HyperV) if probe.feature_enabled("Microsoft-Hyper-V-All") => {
            Ok(())
        }
        (Platform::Windows, Provider::HyperV) => {
            Err("the requested Hyper-V feature is unavailable".into())
        }
        (Platform::Macos, Provider::VirtualizationFramework)
            if probe.command_exists("deshell-vz-agent") =>
        {
            Ok(())
        }
        (Platform::Macos, Provider::VirtualizationFramework) => {
            Err("the signed deshell-vz-agent is unavailable".into())
        }
        (_, Provider::Podman | Provider::DockerRootless) => {
            Err("the requested OCI provider is supported only on Linux".into())
        }
        (_, Provider::WindowsSandbox | Provider::HyperV) => {
            Err("the requested provider is supported only on Windows".into())
        }
        (_, Provider::VirtualizationFramework) => {
            Err("the requested Virtualization.framework provider is supported only on macOS".into())
        }
    }
}

pub(crate) fn launch_spec(provider: Provider, request: &Request) -> Result<LaunchSpec, String> {
    validate_request(request)?;
    match provider {
        Provider::Podman => oci_spec("podman", request),
        Provider::DockerRootless => oci_spec("docker", request),
        Provider::WindowsSandbox => windows_sandbox_spec(request),
        Provider::HyperV | Provider::VirtualizationFramework => {
            Ok(guest_agent_spec(provider, request))
        }
    }
}

pub(crate) fn platform_of_host() -> Platform {
    match std::env::consts::OS {
        "windows" => Platform::Windows,
        "macos" => Platform::Macos,
        _ => Platform::Linux,
    }
}

pub(crate) struct SystemProbe;

impl Probe for SystemProbe {
    fn command_exists(&self, command: &str) -> bool {
        command_path(command).is_some()
    }

    fn feature_enabled(&self, feature: &str) -> bool {
        if !cfg!(windows) {
            return false;
        }
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        match feature {
            "Containers-DisposableClientVM" => root.join("System32/WindowsSandbox.exe").is_file(),
            "Microsoft-Hyper-V-All" => {
                root.join("System32/vmcompute.exe").is_file()
                    && root.join("System32/vmconnect.exe").is_file()
            }
            _ => false,
        }
    }

    fn docker_rootless(&self) -> bool {
        std::process::Command::new("docker")
            .args(["info", "--format", "{{json .SecurityOptions}}"])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .to_ascii_lowercase()
                        .contains("rootless")
            })
    }
}

fn validate_request(request: &Request) -> Result<(), String> {
    if request.timeout_ms == 0 || request.timeout_ms > 86_400_000 {
        return Err("lab timeout must be between 1 and 86400000 milliseconds".into());
    }
    let script = crate::ir::normalize_path(&request.script)
        .map_err(|error| format!("lab script is invalid: {error}"))?;
    if script != request.script {
        return Err("lab script must be a normalized workspace-relative path".into());
    }
    if request.interpreter.trim().is_empty() || request.interpreter.contains('\0') {
        return Err("lab interpreter must not be empty or contain NUL".into());
    }
    if request.workspace.trim().is_empty()
        || request.result_path.trim().is_empty()
        || request.workspace.contains('\0')
        || request.result_path.contains('\0')
    {
        return Err("lab workspace and result path must be non-empty and NUL-free".into());
    }
    if request.arguments.iter().any(|value| value.contains('\0')) {
        return Err("lab arguments must not contain NUL".into());
    }
    let mut names = BTreeSet::new();
    for (name, value) in &request.environment {
        if !valid_environment_name(name) || value.contains('\0') {
            return Err(format!("invalid lab environment entry: {name}"));
        }
        let key = if cfg!(windows) {
            name.to_ascii_uppercase()
        } else {
            name.clone()
        };
        if !names.insert(key) {
            return Err(format!("duplicate lab environment variable: {name}"));
        }
    }
    if let Network::Replay { proxy, tape } = &request.network
        && (proxy.trim().is_empty()
            || tape.trim().is_empty()
            || proxy.contains('\0')
            || tape.contains('\0'))
    {
        return Err("lab replay proxy and tape must be non-empty and NUL-free".into());
    }
    Ok(())
}

fn oci_spec(program: &str, request: &Request) -> Result<LaunchSpec, String> {
    if !digest_pinned(&request.image) {
        return Err("OCI lab image must be pinned by sha256 digest".into());
    }
    let mut arguments = vec![
        "run".into(),
        "--rm".into(),
        "--interactive".into(),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--security-opt".into(),
        "no-new-privileges".into(),
        "--userns=keep-id".into(),
        "--pids-limit=512".into(),
        "--memory=1g".into(),
        "--workdir=/workspace".into(),
    ];
    match &request.network {
        Network::Deny => arguments.push("--network=none".into()),
        Network::Replay { proxy, tape } => {
            arguments.push("--network=deshell-replay".into());
            for value in [
                format!("HTTP_PROXY={proxy}"),
                format!("HTTPS_PROXY={proxy}"),
                "NO_PROXY=".into(),
                format!("DESHELL_REPLAY_TAPE={tape}"),
            ] {
                arguments.push("--env".into());
                arguments.push(value);
            }
        }
    }
    arguments.push("--volume".into());
    arguments.push(format!("{}:/workspace:ro", request.workspace));
    let output = Path::new(&request.result_path)
        .parent()
        .ok_or("lab result path must have a parent directory")?;
    arguments.push("--volume".into());
    arguments.push(format!("{}:/deshell-output:rw", output.display()));
    for (name, value) in &request.environment {
        arguments.push("--env".into());
        arguments.push(format!("{name}={value}"));
    }
    arguments.extend([
        request.image.clone(),
        "deshell".into(),
        "__observer-agent".into(),
    ]);
    Ok(LaunchSpec::Process(ProcessSpec {
        program: program.into(),
        arguments,
        environment: vec![],
        stdin: observer_request_bytes(request, &request.script)?,
    }))
}

fn windows_sandbox_spec(request: &Request) -> Result<LaunchSpec, String> {
    if matches!(request.network, Network::Replay { .. }) {
        return Err(
            "Windows Sandbox replay networking requires Hyper-V with an isolated proxy switch"
                .into(),
        );
    }
    let result_name = Path::new(&request.result_path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or("lab result path must have a UTF-8 filename")?;
    let output_directory = Path::new(&request.result_path)
        .parent()
        .ok_or("lab result path must have a parent directory")?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot resolve the embedded observer executable: {error}"))?;
    let executable_directory = executable
        .parent()
        .ok_or("embedded observer executable has no parent directory")?;
    let encoded_request = base64::engine::general_purpose::STANDARD
        .encode(observer_request_bytes(request, &request.script)?);
    let powershell = format!(
        "$r=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_request}'));$r|&'C:\\deshell\\deshell.exe' '__observer-agent'|Set-Content -LiteralPath 'C:\\output\\{result_name}' -Encoding utf8NoBOM;shutdown.exe /s /t 0 /f"
    );
    let mut utf16 = Vec::with_capacity(powershell.len() * 2);
    for value in powershell.encode_utf16() {
        utf16.extend_from_slice(&value.to_le_bytes());
    }
    let encoded_command = base64::engine::general_purpose::STANDARD.encode(utf16);
    let xml = format!(
        concat!(
            "<Configuration>\n",
            "  <vGPU>Disable</vGPU>\n",
            "  <Networking>Disable</Networking>\n",
            "  <AudioInput>Disable</AudioInput>\n",
            "  <VideoInput>Disable</VideoInput>\n",
            "  <ProtectedClient>Enable</ProtectedClient>\n",
            "  <PrinterRedirection>Disable</PrinterRedirection>\n",
            "  <ClipboardRedirection>Disable</ClipboardRedirection>\n",
            "  <MappedFolders>\n",
            "    <MappedFolder><HostFolder>{}</HostFolder><SandboxFolder>C:\\workspace</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>\n",
            "    <MappedFolder><HostFolder>{}</HostFolder><SandboxFolder>C:\\output</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>\n",
            "    <MappedFolder><HostFolder>{}</HostFolder><SandboxFolder>C:\\deshell</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>\n",
            "  </MappedFolders>\n",
            "  <LogonCommand><Command>powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}</Command></LogonCommand>\n",
            "</Configuration>\n"
        ),
        xml_escape(&request.workspace),
        xml_escape(&output_directory.to_string_lossy()),
        xml_escape(&executable_directory.to_string_lossy()),
        encoded_command
    );
    Ok(LaunchSpec::WindowsConfig(xml))
}

fn guest_agent_spec(provider: Provider, request: &Request) -> LaunchSpec {
    let network = network_name(&request.network);
    let mut payload: Value = crate::strict_json::parse(
        &observer_request_bytes(request, &request.script)
            .expect("validated observer request must serialize"),
    )
    .expect("validated observer request must be JSON");
    let parameters = payload["params"]
        .as_object_mut()
        .expect("observer params are an object");
    parameters.insert("image".into(), Value::String(request.image.clone()));
    parameters.insert(
        "result_path".into(),
        Value::String(request.result_path.clone()),
    );
    parameters.insert("workspace".into(), Value::String(request.workspace.clone()));
    parameters.insert("network".into(), Value::String(network.into()));
    LaunchSpec::AgentRequest(AgentRequest {
        provider: provider_name(provider).into(),
        host_write: "deny".into(),
        network: network.into(),
        payload,
    })
}

fn observer_request_bytes(request: &Request, script: &str) -> Result<Vec<u8>, String> {
    let argv = interpreter_argv(&request.interpreter, script, &request.arguments)?;
    let environment = request
        .environment
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect::<Vec<_>>();
    let value = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": "observer.observe",
        "params": {
            "argv": argv,
            "environment": environment,
            "stdin_base64": "",
            "timeout_ms": request.timeout_ms,
            "working_directory": null
        }
    });
    let mut bytes = crate::canonical_json::canonical_bytes(&value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn interpreter_argv(
    interpreter: &str,
    script: &str,
    arguments: &[String],
) -> Result<Vec<String>, String> {
    let lower = interpreter.to_ascii_lowercase();
    let mut argv: Vec<String> = match lower.as_str() {
        "sh" | "posix_sh" => vec!["sh".into(), script.into()],
        "bash" | "zsh" | "fish" | "nu" => vec![lower, script.into()],
        "nushell" => vec!["nu".into(), script.into()],
        "powershell" | "pwsh" => vec![
            "pwsh".into(),
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-File".into(),
            script.into(),
        ],
        "cmd" => vec![
            "cmd.exe".into(),
            "/d".into(),
            "/s".into(),
            "/c".into(),
            script.into(),
        ],
        "deshell" => vec!["deshell".into(), script.into()],
        _ if interpreter
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) =>
        {
            vec![interpreter.into(), script.into()]
        }
        _ => return Err(format!("lab interpreter name is invalid: {interpreter}")),
    };
    argv.extend_from_slice(arguments);
    Ok(argv)
}

fn network_name(network: &Network) -> &'static str {
    match network {
        Network::Deny => "deny",
        Network::Replay { .. } => "record-replay",
    }
}

pub(crate) fn digest_pinned(value: &str) -> bool {
    value
        .rsplit_once("@sha256:")
        .is_some_and(|(image, digest)| {
            !image.is_empty()
                && image
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && image.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
                })
                && !image.contains('@')
                && crate::digest::valid_sha256(digest)
        })
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn command_path(command: &str) -> Option<PathBuf> {
    if command.is_empty() || command.contains(['/', '\\', '\0']) {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(str::to_owned)
            .collect()
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let candidate = if Path::new(command).extension().is_some() {
                directory.join(command)
            } else {
                directory.join(format!("{command}{extension}"))
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProbe {
        commands: Vec<&'static str>,
        features: Vec<&'static str>,
        rootless: bool,
    }

    impl Probe for FakeProbe {
        fn command_exists(&self, command: &str) -> bool {
            self.commands.contains(&command)
        }

        fn feature_enabled(&self, feature: &str) -> bool {
            self.features.contains(&feature)
        }

        fn docker_rootless(&self) -> bool {
            self.rootless
        }
    }

    fn probe(commands: &[&'static str], features: &[&'static str], rootless: bool) -> FakeProbe {
        FakeProbe {
            commands: commands.to_vec(),
            features: features.to_vec(),
            rootless,
        }
    }

    fn request(network: Network) -> Request {
        Request {
            workspace: "/tmp/staged/workspace".into(),
            result_path: "/tmp/staged/output/observation.json".into(),
            interpreter: "sh".into(),
            script: "build.sh".into(),
            arguments: vec!["--check".into()],
            environment: vec![("MODE".into(), "test".into())],
            timeout_ms: 5_000,
            network,
            image: concat!(
                "ghcr.io/deshell-lang/lab@sha256:",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            )
            .into(),
        }
    }

    #[test]
    fn provider_selection_requires_the_platform_security_boundary() {
        assert_eq!(
            select(Platform::Linux, &probe(&["podman"], &[], false)).unwrap(),
            Provider::Podman
        );
        assert_eq!(
            select(Platform::Linux, &probe(&["docker"], &[], true)).unwrap(),
            Provider::DockerRootless
        );
        assert!(
            select(Platform::Linux, &probe(&["docker"], &[], false))
                .unwrap_err()
                .contains("rootless")
        );
        assert_eq!(
            select(
                Platform::Windows,
                &probe(&[], &["Containers-DisposableClientVM"], false)
            )
            .unwrap(),
            Provider::WindowsSandbox
        );
        assert_eq!(
            select(
                Platform::Windows,
                &probe(&[], &["Microsoft-Hyper-V-All"], false)
            )
            .unwrap(),
            Provider::HyperV
        );
        assert_eq!(
            select(Platform::Macos, &probe(&["deshell-vz-agent"], &[], false)).unwrap(),
            Provider::VirtualizationFramework
        );
    }

    #[test]
    fn a_forced_provider_is_still_verified() {
        assert!(
            validate_provider(
                Platform::Linux,
                &probe(&["docker"], &[], false),
                Provider::DockerRootless
            )
            .unwrap_err()
            .contains("rootless")
        );
        assert!(
            validate_provider(
                Platform::Windows,
                &probe(&[], &["Microsoft-Hyper-V-All"], false),
                Provider::Podman
            )
            .unwrap_err()
            .contains("Linux")
        );
    }

    #[test]
    fn oci_launch_is_read_only_rootless_and_digest_pinned() {
        let LaunchSpec::Process(spec) =
            launch_spec(Provider::Podman, &request(Network::Deny)).unwrap()
        else {
            panic!("Podman must produce a process specification");
        };
        assert_eq!(spec.program, "podman");
        for required in [
            "--rm",
            "--read-only",
            "--network=none",
            "--cap-drop=ALL",
            "no-new-privileges",
            "--userns=keep-id",
            "--workdir=/workspace",
        ] {
            assert!(
                spec.arguments.iter().any(|value| value == required),
                "{required}"
            );
        }
        assert!(
            spec.arguments
                .iter()
                .any(|value| value.starts_with("ghcr.io/deshell-lang/lab@sha256:"))
        );
        let rpc = crate::strict_json::parse(&spec.stdin).unwrap();
        assert_eq!(rpc["method"], "observer.observe");
    }

    #[test]
    fn unpinned_images_and_unsafe_requests_are_rejected() {
        let mut invalid = request(Network::Deny);
        invalid.image = "ghcr.io/deshell-lang/lab:latest".into();
        assert!(
            launch_spec(Provider::Podman, &invalid)
                .unwrap_err()
                .contains("sha256")
        );
        invalid.image = format!("--privileged@sha256:{}", "a".repeat(64));
        assert!(
            launch_spec(Provider::Podman, &invalid)
                .unwrap_err()
                .contains("sha256")
        );
        invalid.image = request(Network::Deny).image;
        invalid.script = "../escape.sh".into();
        assert!(
            launch_spec(Provider::Podman, &invalid)
                .unwrap_err()
                .contains("script")
        );
        invalid.script = "build.sh".into();
        invalid.timeout_ms = 0;
        assert!(
            launch_spec(Provider::Podman, &invalid)
                .unwrap_err()
                .contains("timeout")
        );
    }

    #[test]
    fn replay_network_is_explicit_and_proxy_scoped() {
        let replay = Network::Replay {
            proxy: "http://10.0.0.2:8080".into(),
            tape: "tape.json".into(),
        };
        let LaunchSpec::Process(spec) =
            launch_spec(Provider::DockerRootless, &request(replay)).unwrap()
        else {
            panic!("Docker must produce a process specification");
        };
        for expected in [
            "--network=deshell-replay",
            "HTTP_PROXY=http://10.0.0.2:8080",
            "HTTPS_PROXY=http://10.0.0.2:8080",
            "NO_PROXY=",
            "DESHELL_REPLAY_TAPE=tape.json",
        ] {
            assert!(
                spec.arguments.iter().any(|value| value == expected),
                "{expected}"
            );
        }
    }

    #[test]
    fn windows_sandbox_disables_host_integrations_and_encodes_hostile_values() {
        let mut request = request(Network::Deny);
        request.arguments = vec!["hello & whoami".into(), "%PATH%".into()];
        request.environment = vec![("TOKEN".into(), "secret & echo leaked".into())];
        let LaunchSpec::WindowsConfig(xml) =
            launch_spec(Provider::WindowsSandbox, &request).unwrap()
        else {
            panic!("Windows Sandbox must produce a configuration");
        };
        for required in [
            "<Networking>Disable</Networking>",
            "<ClipboardRedirection>Disable</ClipboardRedirection>",
            "<PrinterRedirection>Disable</PrinterRedirection>",
            "<vGPU>Disable</vGPU>",
            "<ReadOnly>true</ReadOnly>",
            "-EncodedCommand",
        ] {
            assert!(xml.contains(required), "{required}");
        }
        for plaintext in ["hello & whoami", "%PATH%", "secret & echo leaked"] {
            assert!(!xml.contains(plaintext), "interpolated {plaintext}");
        }
    }

    #[test]
    fn guest_agents_receive_deny_host_write_policy() {
        let LaunchSpec::AgentRequest(spec) =
            launch_spec(Provider::HyperV, &request(Network::Deny)).unwrap()
        else {
            panic!("Hyper-V must use the guest-agent contract");
        };
        assert_eq!(spec.provider, "hyper-v");
        assert_eq!(spec.host_write, "deny");
        assert_eq!(spec.network, "deny");
        assert_eq!(spec.payload["method"], "observer.observe");
    }
}
