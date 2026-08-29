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
pub(crate) enum Target {
    Original {
        interpreter: String,
        script: String,
    },
    Plan {
        entrypoint: String,
        node_id: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    pub workspace: String,
    pub result_path: String,
    pub target: Target,
    pub arguments: Vec<String>,
    pub named_inputs: Vec<(String, String)>,
    pub environment: Vec<(String, String)>,
    pub stdin: Vec<u8>,
    pub working_directory: Option<String>,
    pub fixtures: Vec<crate::config::Fixture>,
    pub expected_files: Vec<crate::config::ExpectedFile>,
    pub limits: crate::config::ResourceLimits,
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

pub(crate) fn execution_connected(provider: Provider) -> bool {
    matches!(provider, Provider::Podman | Provider::DockerRootless)
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
        Provider::HyperV | Provider::VirtualizationFramework => guest_agent_spec(provider, request),
    }
}

pub(crate) fn platform_of_host() -> Platform {
    match std::env::consts::OS {
        "windows" => Platform::Windows,
        "macos" => Platform::Macos,
        _ => Platform::Linux,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExecutionFailureKind {
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionFailure {
    pub kind: ExecutionFailureKind,
    pub message: String,
}

pub(crate) fn execute(
    provider: Provider,
    request: &Request,
) -> Result<crate::runner::RunResult, ExecutionFailure> {
    execute_with(provider, request, crate::agent_process::execute)
}

fn execute_with(
    provider: Provider,
    request: &Request,
    transport: impl FnOnce(
        &Path,
        crate::agent_process::Request,
    ) -> Result<crate::agent_process::Outcome, String>,
) -> Result<crate::runner::RunResult, ExecutionFailure> {
    if !execution_connected(provider) {
        return Err(ExecutionFailure {
            kind: ExecutionFailureKind::Unavailable,
            message: format!(
                "{} has a validated launch contract, but its signed helper transport is not connected in this build",
                provider_name(provider)
            ),
        });
    }
    let specification = launch_spec(provider, request).map_err(|message| ExecutionFailure {
        kind: ExecutionFailureKind::Unavailable,
        message,
    })?;
    let LaunchSpec::Process(specification) = specification else {
        unreachable!("connected providers always use the supervised process transport")
    };
    let root = Path::new(&request.workspace);
    let outer_stdout = request
        .limits
        .stdout_bytes
        .saturating_mul(2)
        .saturating_add(1024 * 1024);
    let outcome = transport(
        root,
        crate::agent_process::Request {
            argv: std::iter::once(specification.program)
                .chain(specification.arguments)
                .collect(),
            environment: specification.environment,
            working_directory: None,
            stdin: specification.stdin,
            limits: crate::agent_process::Limits {
                timeout_ms: request
                    .limits
                    .timeout_ms
                    .saturating_add(10_000)
                    .min(86_400_000),
                memory_bytes: request
                    .limits
                    .memory_bytes
                    .saturating_add(512 * 1024 * 1024),
                processes: request.limits.processes.saturating_add(32),
                stdout_bytes: outer_stdout,
                stderr_bytes: request.limits.stderr_bytes.saturating_add(1024 * 1024),
            },
        },
    )
    .map_err(|message| ExecutionFailure {
        kind: ExecutionFailureKind::Failed,
        message,
    })?;
    if let Some(limit) = outcome.limit_exceeded {
        return Err(ExecutionFailure {
            kind: ExecutionFailureKind::Failed,
            message: format!("disposable provider limit_exceeded: {limit}"),
        });
    }
    if outcome.exit_code != 0 {
        return Err(ExecutionFailure {
            kind: ExecutionFailureKind::Failed,
            message: format!(
                "{} exited with {}: {}",
                provider_name(provider),
                outcome.exit_code,
                String::from_utf8_lossy(&outcome.stderr)
            ),
        });
    }
    decode_run_result(&outcome.stdout, request.limits).map_err(|message| ExecutionFailure {
        kind: ExecutionFailureKind::Failed,
        message,
    })
}

fn decode_run_result(
    response: &[u8],
    limits: crate::config::ResourceLimits,
) -> Result<crate::runner::RunResult, String> {
    let result = crate::protocol::decode_streamed_response(
        response,
        &serde_json::json!(1),
        limits.stdout_bytes,
        limits.stderr_bytes,
    )?;
    let fields = result
        .as_object()
        .ok_or("observer RPC result must be an object")?;
    let exit_code = fields
        .get("exit_code")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or("observer exit_code must be a 32-bit integer")?;
    let decode_stream = |name: &str| -> Result<Vec<u8>, String> {
        let encoded = fields
            .get(name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("observer result is missing {name}"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| format!("observer {name} is invalid base64: {error}"))?;
        if base64::engine::general_purpose::STANDARD.encode(&bytes) != encoded {
            return Err(format!("observer {name} is not canonical base64"));
        }
        Ok(bytes)
    };
    let mut trace = Vec::new();
    if let Some(files) = fields.get("files") {
        let files = files.as_array().ok_or("observer files must be an array")?;
        for file in files {
            let file = file
                .as_object()
                .ok_or("observer file change must be an object")?;
            let path = file
                .get("path")
                .and_then(serde_json::Value::as_str)
                .ok_or("observer file change path must be a string")?;
            match file.get("kind").and_then(serde_json::Value::as_str) {
                Some("created" | "modified") => {
                    trace.push(crate::runner::TraceEvent::FileWrite { path: path.into() })
                }
                Some("removed") => {
                    trace.push(crate::runner::TraceEvent::FileRemove { path: path.into() })
                }
                _ => return Err("observer file change kind is invalid".into()),
            }
        }
    }
    Ok(crate::runner::RunResult {
        exit_code,
        stdout: decode_stream("stdout_base64")?,
        stderr: decode_stream("stderr_base64")?,
        trace,
    })
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
    if request.limits.timeout_ms == 0 || request.limits.timeout_ms > 86_400_000 {
        return Err("lab timeout must be between 1 and 86400000 milliseconds".into());
    }
    match &request.target {
        Target::Original {
            interpreter,
            script,
        } => {
            let normalized = crate::ir::normalize_path(script)
                .map_err(|error| format!("lab script is invalid: {error}"))?;
            if normalized != *script {
                return Err("lab script must be a normalized workspace-relative path".into());
            }
            if interpreter.trim().is_empty() || interpreter.contains('\0') {
                return Err("lab interpreter must not be empty or contain NUL".into());
            }
        }
        Target::Plan {
            entrypoint,
            node_id,
        } => {
            let normalized = crate::ir::normalize_path(entrypoint)
                .map_err(|error| format!("lab entrypoint is invalid: {error}"))?;
            if normalized != *entrypoint {
                return Err("lab entrypoint must be a normalized workspace-relative path".into());
            }
            if node_id
                .as_ref()
                .is_some_and(|value| value.trim().is_empty() || value.contains('\0'))
            {
                return Err("lab node id must be non-empty and NUL-free".into());
            }
        }
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
    if let Some(directory) = &request.working_directory {
        let normalized = crate::ir::normalize_path(directory)
            .map_err(|error| format!("lab working directory is invalid: {error}"))?;
        if normalized != *directory {
            return Err("lab working directory must be normalized".into());
        }
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
    let mut inputs = BTreeSet::new();
    for (name, value) in &request.named_inputs {
        if name.trim().is_empty() || name.contains('\0') || value.contains('\0') {
            return Err(format!("invalid lab named input: {name}"));
        }
        if !inputs.insert(name) {
            return Err(format!("duplicate lab named input: {name}"));
        }
    }
    for fixture in &request.fixtures {
        let normalized = crate::ir::normalize_path(&fixture.path)
            .map_err(|error| format!("lab fixture path is invalid: {error}"))?;
        if normalized != fixture.path {
            return Err(format!(
                "lab fixture path is not normalized: {}",
                fixture.path
            ));
        }
        fixture.contents.bytes()?;
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
        format!("--pids-limit={}", request.limits.processes),
        format!("--memory={}", request.limits.memory_bytes),
        "--workdir=/workspace".into(),
    ];
    if program == "podman" {
        arguments.push("--userns=keep-id".into());
    }
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
    // The mounted tree is already a private snapshot. It may be mutated inside
    // the disposable boundary without exposing the live project.
    arguments.push(format!("{}:/workspace:rw", request.workspace));
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
        stdin: observer_request_bytes(request)?,
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
    let encoded_request =
        base64::engine::general_purpose::STANDARD.encode(observer_request_bytes(request)?);
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

fn guest_agent_spec(provider: Provider, request: &Request) -> Result<LaunchSpec, String> {
    let network = network_name(&request.network);
    let mut payload: Value = crate::strict_json::parse(&observer_request_bytes(request)?)?;
    let parameters = payload["params"]
        .as_object_mut()
        .ok_or_else(|| "observer params are not an object".to_owned())?;
    parameters.insert("image".into(), Value::String(request.image.clone()));
    parameters.insert(
        "result_path".into(),
        Value::String(request.result_path.clone()),
    );
    parameters.insert("workspace".into(), Value::String(request.workspace.clone()));
    parameters.insert("network".into(), Value::String(network.into()));
    Ok(LaunchSpec::AgentRequest(AgentRequest {
        provider: provider_name(provider).into(),
        host_write: "deny".into(),
        network: network.into(),
        payload,
    }))
}

fn observer_request_bytes(request: &Request) -> Result<Vec<u8>, String> {
    let environment = request
        .environment
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect::<Vec<_>>();
    let named_inputs = request
        .named_inputs
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect::<Vec<_>>();
    let fixtures = request.fixtures.iter().map(|fixture| {
        Ok(serde_json::json!({
            "contents_base64": base64::engine::general_purpose::STANDARD.encode(fixture.contents.bytes()?),
            "executable": fixture.executable,
            "path": fixture.path
        }))
    }).collect::<Result<Vec<_>, String>>()?;
    let expected_files = request
        .expected_files
        .iter()
        .map(|file| serde_json::json!({"path": file.path, "sha256": file.sha256}))
        .collect::<Vec<_>>();
    let (method, mut parameters) = match &request.target {
        Target::Original {
            interpreter,
            script,
        } => (
            "observer.observe",
            serde_json::json!({
                "argv": interpreter_argv(interpreter, script, &request.arguments)?
            }),
        ),
        Target::Plan {
            entrypoint,
            node_id,
        } => (
            "observer.run_plan",
            serde_json::json!({
                "arguments": named_inputs,
                "argv": request.arguments,
                "entrypoint": entrypoint,
                "node_id": node_id
            }),
        ),
    };
    let fields = parameters
        .as_object_mut()
        .ok_or_else(|| "lab parameters are not an object".to_owned())?;
    fields.insert("environment".into(), serde_json::Value::Array(environment));
    fields.insert(
        "expected_files".into(),
        serde_json::Value::Array(expected_files),
    );
    fields.insert("fixtures".into(), serde_json::Value::Array(fixtures));
    fields.insert(
        "stdin_base64".into(),
        serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(&request.stdin)),
    );
    fields.insert(
        "working_directory".into(),
        request
            .working_directory
            .clone()
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    for (name, value) in [
        ("timeout_ms", request.limits.timeout_ms),
        ("memory_bytes", request.limits.memory_bytes),
        ("processes", request.limits.processes),
        ("stdout_bytes", request.limits.stdout_bytes),
        ("stderr_bytes", request.limits.stderr_bytes),
    ] {
        fields.insert(name.into(), serde_json::Value::from(value));
    }
    let value = serde_json::json!({
        "id": 1,
        "jsonrpc": "2.0",
        "method": method,
        "params": parameters
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
            target: Target::Original {
                interpreter: "sh".into(),
                script: "build.sh".into(),
            },
            arguments: vec!["--check".into()],
            named_inputs: vec![],
            environment: vec![("MODE".into(), "test".into())],
            stdin: vec![],
            working_directory: None,
            fixtures: vec![],
            expected_files: vec![],
            limits: crate::config::ResourceLimits {
                timeout_ms: 5_000,
                ..crate::config::ResourceLimits::DEFAULT
            },
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
        assert!(execution_connected(Provider::Podman));
        assert!(execution_connected(Provider::DockerRootless));
        assert!(!execution_connected(Provider::WindowsSandbox));
        assert!(!execution_connected(Provider::HyperV));
        assert!(!execution_connected(Provider::VirtualizationFramework));
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

        let LaunchSpec::Process(docker) =
            launch_spec(Provider::DockerRootless, &request(Network::Deny)).unwrap()
        else {
            panic!("rootless Docker must produce a process specification");
        };
        assert_eq!(docker.program, "docker");
        assert!(
            !docker
                .arguments
                .iter()
                .any(|argument| argument == "--userns=keep-id")
        );
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
        invalid.target = Target::Original {
            interpreter: "sh".into(),
            script: "../escape.sh".into(),
        };
        assert!(
            launch_spec(Provider::Podman, &invalid)
                .unwrap_err()
                .contains("script")
        );
        invalid.target = Target::Original {
            interpreter: "sh".into(),
            script: "build.sh".into(),
        };
        invalid.limits.timeout_ms = 0;
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

    #[test]
    fn observer_results_decode_raw_streams_and_file_changes_strictly() {
        let response = |result: serde_json::Value| {
            crate::canonical_json::canonical_bytes(&serde_json::json!({
                "id": 1,
                "jsonrpc": "2.0",
                "result": result
            }))
            .unwrap()
        };
        let limits = crate::config::ResourceLimits::DEFAULT;
        let valid = response(serde_json::json!({
            "exit_code": 7,
            "files": [
                {"kind": "created", "path": "created"},
                {"kind": "modified", "path": "modified"},
                {"kind": "removed", "path": "removed"}
            ],
            "stderr_base64": "ZXJy",
            "stdout_base64": "AP8="
        }));
        let result = decode_run_result(&valid, limits).unwrap();
        assert_eq!(result.exit_code, 7);
        assert_eq!(result.stdout, [0, 0xff]);
        assert_eq!(result.stderr, b"err");
        assert!(matches!(
            result.trace[0],
            crate::runner::TraceEvent::FileWrite { .. }
        ));
        assert!(matches!(
            result.trace[1],
            crate::runner::TraceEvent::FileWrite { .. }
        ));
        assert!(matches!(
            result.trace[2],
            crate::runner::TraceEvent::FileRemove { .. }
        ));

        for (label, result) in [
            (
                "exit_code",
                serde_json::json!({"stderr_base64": "", "stdout_base64": ""}),
            ),
            (
                "stdout_base64",
                serde_json::json!({"exit_code": 0, "stderr_base64": ""}),
            ),
            (
                "invalid base64",
                serde_json::json!({"exit_code": 0, "stderr_base64": "", "stdout_base64": "?"}),
            ),
            (
                "files array",
                serde_json::json!({"exit_code": 0, "files": {}, "stderr_base64": "", "stdout_base64": ""}),
            ),
            (
                "file object",
                serde_json::json!({"exit_code": 0, "files": [1], "stderr_base64": "", "stdout_base64": ""}),
            ),
            (
                "file path",
                serde_json::json!({"exit_code": 0, "files": [{"kind": "created"}], "stderr_base64": "", "stdout_base64": ""}),
            ),
            (
                "file kind",
                serde_json::json!({"exit_code": 0, "files": [{"kind": "future", "path": "x"}], "stderr_base64": "", "stdout_base64": ""}),
            ),
        ] {
            assert!(
                decode_run_result(&response(result), limits).is_err(),
                "{label}"
            );
        }
    }

    #[test]
    fn connected_provider_execution_classifies_every_transport_outcome() {
        let rpc_response = |result: serde_json::Value| {
            crate::canonical_json::canonical_bytes(&serde_json::json!({
                "id": 1,
                "jsonrpc": "2.0",
                "result": result
            }))
            .unwrap()
        };
        let success = rpc_response(serde_json::json!({
            "exit_code": 0,
            "stderr_base64": "",
            "stdout_base64": "b2s="
        }));
        let observed = execute_with(
            Provider::Podman,
            &request(Network::Deny),
            |root, process| {
                assert_eq!(root, Path::new("/tmp/staged/workspace"));
                assert_eq!(process.argv[0], "podman");
                assert!(process.limits.timeout_ms > 5_000);
                assert!(process.limits.memory_bytes > request(Network::Deny).limits.memory_bytes);
                Ok(crate::agent_process::Outcome {
                    exit_code: 0,
                    stdout: success.clone(),
                    stderr: vec![],
                    timed_out: false,
                    limit_exceeded: None,
                    signal: None,
                })
            },
        )
        .unwrap();
        assert_eq!(observed.stdout, b"ok");

        let transport = execute_with(Provider::DockerRootless, &request(Network::Deny), |_, _| {
            Err("spawn denied".into())
        })
        .unwrap_err();
        assert_eq!(transport.kind, ExecutionFailureKind::Failed);
        assert!(transport.message.contains("spawn denied"));

        for (outcome, expected) in [
            (
                crate::agent_process::Outcome {
                    exit_code: 0,
                    stdout: vec![],
                    stderr: vec![],
                    timed_out: false,
                    limit_exceeded: Some("stdout".into()),
                    signal: None,
                },
                "limit_exceeded",
            ),
            (
                crate::agent_process::Outcome {
                    exit_code: 9,
                    stdout: vec![],
                    stderr: b"provider failed".to_vec(),
                    timed_out: false,
                    limit_exceeded: None,
                    signal: None,
                },
                "provider failed",
            ),
            (
                crate::agent_process::Outcome {
                    exit_code: 0,
                    stdout: b"not json".to_vec(),
                    stderr: vec![],
                    timed_out: false,
                    limit_exceeded: None,
                    signal: None,
                },
                "JSON",
            ),
        ] {
            let error = execute_with(Provider::Podman, &request(Network::Deny), |_, _| {
                Ok(outcome.clone())
            })
            .unwrap_err();
            assert_eq!(error.kind, ExecutionFailureKind::Failed);
            assert!(error.message.contains(expected), "{}", error.message);
        }
    }

    #[test]
    fn request_validation_rejects_every_unbounded_or_ambiguous_input() {
        let mut valid_plan = request(Network::Deny);
        valid_plan.target = Target::Plan {
            entrypoint: "build.sh".into(),
            node_id: Some("node".into()),
        };
        valid_plan.named_inputs = vec![("name".into(), "value".into())];
        valid_plan.working_directory = Some("work".into());
        assert!(validate_request(&valid_plan).is_ok());

        let mut cases = Vec::new();
        let mut value = valid_plan.clone();
        value.target = Target::Plan {
            entrypoint: "../escape".into(),
            node_id: None,
        };
        cases.push(value);
        let mut value = valid_plan.clone();
        value.target = Target::Plan {
            entrypoint: "build.sh".into(),
            node_id: Some("".into()),
        };
        cases.push(value);
        let mut value = valid_plan.clone();
        value.target = Target::Original {
            interpreter: "".into(),
            script: "build.sh".into(),
        };
        cases.push(value);
        let mut value = valid_plan.clone();
        value.workspace.clear();
        cases.push(value);
        let mut value = valid_plan.clone();
        value.arguments.push("nul\0value".into());
        cases.push(value);
        let mut value = valid_plan.clone();
        value.working_directory = Some("../escape".into());
        cases.push(value);
        let mut value = valid_plan.clone();
        value.environment = vec![("1BAD".into(), "x".into())];
        cases.push(value);
        let mut value = valid_plan.clone();
        value.environment = vec![("MODE".into(), "a".into()), ("MODE".into(), "b".into())];
        cases.push(value);
        let mut value = valid_plan.clone();
        value.named_inputs = vec![("".into(), "x".into())];
        cases.push(value);
        let mut value = valid_plan.clone();
        value.named_inputs = vec![("name".into(), "a".into()), ("name".into(), "b".into())];
        cases.push(value);
        let mut value = valid_plan.clone();
        value.fixtures = vec![crate::config::Fixture {
            path: "../escape".into(),
            contents: crate::config::BinaryData {
                utf8: Some("value".into()),
                base64: None,
            },
            executable: false,
        }];
        cases.push(value);
        let mut value = valid_plan.clone();
        value.network = Network::Replay {
            proxy: "".into(),
            tape: "tape".into(),
        };
        cases.push(value);
        for (index, invalid) in cases.into_iter().enumerate() {
            assert!(validate_request(&invalid).is_err(), "case {index}");
        }
    }

    #[test]
    fn provider_validation_and_launch_variants_are_exhaustive() {
        for (platform, provider, probe, expected) in [
            (
                Platform::Linux,
                Provider::Podman,
                probe(&["podman"], &[], false),
                true,
            ),
            (
                Platform::Linux,
                Provider::Podman,
                probe(&[], &[], false),
                false,
            ),
            (
                Platform::Linux,
                Provider::DockerRootless,
                probe(&[], &[], false),
                false,
            ),
            (
                Platform::Linux,
                Provider::DockerRootless,
                probe(&["docker"], &[], true),
                true,
            ),
            (
                Platform::Windows,
                Provider::WindowsSandbox,
                probe(&[], &["Containers-DisposableClientVM"], false),
                true,
            ),
            (
                Platform::Windows,
                Provider::HyperV,
                probe(&[], &["Microsoft-Hyper-V-All"], false),
                true,
            ),
            (
                Platform::Macos,
                Provider::VirtualizationFramework,
                probe(&["deshell-vz-agent"], &[], false),
                true,
            ),
            (
                Platform::Macos,
                Provider::WindowsSandbox,
                probe(&[], &[], false),
                false,
            ),
        ] {
            assert_eq!(
                validate_provider(platform, &probe, provider).is_ok(),
                expected
            );
        }
        assert!(select(Platform::Windows, &probe(&[], &[], false)).is_err());
        assert!(select(Platform::Macos, &probe(&[], &[], false)).is_err());
        assert!(matches!(
            platform_of_host(),
            Platform::Linux | Platform::Macos | Platform::Windows
        ));

        let mut replay = request(Network::Replay {
            proxy: "http://proxy".into(),
            tape: "tape".into(),
        });
        assert!(launch_spec(Provider::WindowsSandbox, &replay).is_err());
        let LaunchSpec::AgentRequest(vz) =
            launch_spec(Provider::VirtualizationFramework, &replay).unwrap()
        else {
            panic!("Virtualization.framework must use the agent request");
        };
        assert_eq!(vz.provider, "virtualization-framework");
        assert_eq!(vz.network, "record-replay");
        assert_eq!(vz.payload["params"]["network"], "record-replay");

        replay.image = "latest".into();
        let failure = execute(Provider::Podman, &replay).unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::Unavailable);
        let failure = execute(Provider::HyperV, &request(Network::Deny)).unwrap_err();
        assert_eq!(failure.kind, ExecutionFailureKind::Unavailable);
        assert!(failure.message.contains("not connected"));
    }

    #[test]
    fn interpreter_argv_pins_noninteractive_forms_and_rejects_command_injection() {
        for (interpreter, prefix) in [
            ("sh", "sh"),
            ("posix_sh", "sh"),
            ("bash", "bash"),
            ("zsh", "zsh"),
            ("fish", "fish"),
            ("nu", "nu"),
            ("nushell", "nu"),
            ("powershell", "pwsh"),
            ("pwsh", "pwsh"),
            ("cmd", "cmd.exe"),
            ("deshell", "deshell"),
            ("custom-runtime", "custom-runtime"),
        ] {
            let argv = interpreter_argv(interpreter, "build.script", &["arg".into()]).unwrap();
            assert_eq!(argv[0], prefix);
            assert_eq!(argv.last().unwrap(), "arg");
        }
        assert!(interpreter_argv("bad/value", "build", &[]).is_err());
        assert_eq!(network_name(&Network::Deny), "deny");
        assert_eq!(
            network_name(&Network::Replay {
                proxy: "proxy".into(),
                tape: "tape".into(),
            }),
            "record-replay"
        );
        assert!(digest_pinned(&format!(
            "registry/image@sha256:{}",
            "a".repeat(64)
        )));
        for invalid in [
            "",
            "image:latest",
            "@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "image@tag@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(!digest_pinned(invalid), "{invalid}");
        }
        assert!(command_path("").is_none());
        assert!(command_path("bad/path").is_none());
        assert!(command_path("sh").is_some());
        assert_eq!(xml_escape("<&>\"'"), "&lt;&amp;&gt;&quot;&apos;");
    }
}
