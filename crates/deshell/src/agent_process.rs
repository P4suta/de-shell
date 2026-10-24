use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    pub argv: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub working_directory: Option<String>,
    pub stdin: Vec<u8>,
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Limits {
    pub timeout_ms: u64,
    pub memory_bytes: u64,
    pub processes: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        let limits = crate::config::ResourceLimits::DEFAULT;
        Self::from(limits)
    }
}

impl From<crate::config::ResourceLimits> for Limits {
    fn from(value: crate::config::ResourceLimits) -> Self {
        Self {
            timeout_ms: value.timeout_ms,
            memory_bytes: value.memory_bytes,
            processes: value.processes,
            stdout_bytes: value.stdout_bytes,
            stderr_bytes: value.stderr_bytes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Outcome {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub limit_exceeded: Option<String>,
    pub signal: Option<i32>,
}

pub(crate) trait Clock: Sync {
    fn elapsed(&self) -> Duration;
    fn yield_now(&self);
    fn wait(&self, duration: Duration);
}

pub(crate) struct SystemClock {
    start: crate::host::Stopwatch,
}

impl SystemClock {
    pub(crate) fn start() -> Self {
        Self {
            start: crate::host::Stopwatch::start(),
        }
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn yield_now(&self) {
        std::thread::yield_now();
    }
    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

const POLL_YIELDS: u8 = 4;
const POLL_INITIAL_WAIT: Duration = Duration::from_micros(250);
const POLL_MAX_WAIT: Duration = Duration::from_millis(5);

#[derive(Default)]
struct PollBackoff {
    polls: u8,
    wait: Duration,
}

impl PollBackoff {
    fn pause<C: Clock>(&mut self, clock: &C, deadline: Duration) {
        if self.polls < POLL_YIELDS {
            self.polls += 1;
            clock.yield_now();
            return;
        }
        self.wait = if self.wait.is_zero() {
            POLL_INITIAL_WAIT
        } else {
            self.wait.saturating_mul(2).min(POLL_MAX_WAIT)
        };
        let remaining = deadline.saturating_sub(clock.elapsed());
        if !remaining.is_zero() {
            clock.wait(self.wait.min(remaining));
        }
    }
}

pub(crate) fn execute(root: &Path, request: Request) -> Result<Outcome, String> {
    let clock = SystemClock::start();
    execute_with_clock(root, request, &clock)
}

pub(crate) fn execute_pipeline(
    root: &Path,
    requests: Vec<Request>,
) -> Result<Vec<Outcome>, String> {
    if requests.is_empty() {
        return Err("process pipeline must contain at least one stage".into());
    }
    let limits = requests[0].limits;
    if requests.iter().any(|request| request.limits != limits) {
        return Err("process pipeline stages must use identical resource limits".into());
    }
    if limits.timeout_ms == 0
        || limits.timeout_ms > 86_400_000
        || limits.memory_bytes < 16 * 1024 * 1024
        || limits.processes == 0
        || limits.stdout_bytes == 0
        || limits.stderr_bytes == 0
    {
        return Err("process pipeline resource limits are invalid".into());
    }

    let clock = SystemClock::start();
    let count = requests.len();
    let exceeded = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let stderr_total = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut children: Vec<std::process::Child> = Vec::with_capacity(count);
    let mut previous_stdout: Option<std::process::ChildStdout> = None;
    let mut stderr_readers = Vec::with_capacity(count);
    let mut stdout_reader = None;
    let mut stdin_writer = None;

    for (index, request) in requests.into_iter().enumerate() {
        let executable = request
            .argv
            .first()
            .filter(|value| !value.is_empty())
            .ok_or("process pipeline argv must not be empty")?;
        if request.argv.iter().any(|value| value.contains('\0')) {
            terminate_children(&mut children);
            return Err("process pipeline argv must not contain NUL".into());
        }
        let directory = match _resolve_directory(root, request.working_directory.as_deref()) {
            Ok(directory) => directory,
            Err(error) => {
                terminate_children(&mut children);
                return Err(error);
            }
        };
        let mut seen = std::collections::BTreeSet::new();
        for (name, value) in &request.environment {
            if !valid_environment_name(name) || value.contains('\0') {
                terminate_children(&mut children);
                return Err(format!(
                    "invalid process pipeline environment entry: {name}"
                ));
            }
            let key = if cfg!(windows) {
                name.to_ascii_uppercase()
            } else {
                name.clone()
            };
            if !seen.insert(key) {
                terminate_children(&mut children);
                return Err(format!(
                    "duplicate process pipeline environment variable: {name}"
                ));
            }
        }

        let mut command = std::process::Command::new(executable);
        command
            .args(&request.argv[1..])
            .current_dir(directory)
            .env_clear()
            .stderr(std::process::Stdio::piped());
        if index == 0 {
            command.stdin(std::process::Stdio::piped());
        } else {
            let input = previous_stdout
                .take()
                .ok_or("previous pipeline stdout is unavailable")?;
            command.stdin(std::process::Stdio::from(input));
        }
        command.stdout(std::process::Stdio::piped());
        add_essential_environment(&mut command);
        for (name, value) in &request.environment {
            command.env(name, value);
        }
        #[cfg(unix)]
        if let Err(error) = configure_unix_limits(&mut command, limits) {
            terminate_children(&mut children);
            return Err(format!(
                "failed to prepare pipeline stage {executable} resource limits: {error}"
            ));
        }

        let mut child = match crate::host::spawn(&mut command) {
            Ok(child) => child,
            Err(error) => {
                terminate_children(&mut children);
                return Err(format!(
                    "failed to start pipeline stage {executable}: {error}"
                ));
            }
        };
        if index == 0 {
            let mut input = child
                .stdin
                .take()
                .ok_or("pipeline stdin pipe is unavailable")?;
            stdin_writer = Some(std::thread::spawn(move || -> Result<(), String> {
                use std::io::Write as _;
                match input.write_all(&request.stdin) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
                    Err(error) => Err(format!("failed to write pipeline stdin: {error}")),
                }
            }));
        }
        let output = child
            .stdout
            .take()
            .ok_or("pipeline stdout pipe is unavailable")?;
        if index + 1 == count {
            let exceeded = std::sync::Arc::clone(&exceeded);
            stdout_reader = Some(std::thread::spawn(move || {
                read_pipe(ReadPipeArgs {
                    pipe: output,
                    label: "stdout",
                    limit: limits.stdout_bytes,
                    exceeded: &exceeded,
                    code: 1,
                })
            }));
        } else {
            previous_stdout = Some(output);
        }
        let error = child
            .stderr
            .take()
            .ok_or("pipeline stderr pipe is unavailable")?;
        let exceeded_reader = std::sync::Arc::clone(&exceeded);
        let stderr_total_reader = std::sync::Arc::clone(&stderr_total);
        stderr_readers.push(std::thread::spawn(move || {
            read_pipe_shared(ReadPipeSharedArgs {
                pipe: error,
                label: "stderr",
                limit: limits.stderr_bytes,
                total: &stderr_total_reader,
                exceeded: &exceeded_reader,
                code: 2,
            })
        }));
        children.push(child);
    }

    let deadline = Duration::from_millis(limits.timeout_ms);
    let mut statuses = vec![None; count];
    let mut timed_out = false;
    let mut backoff = PollBackoff::default();
    loop {
        let mut complete = true;
        for (index, child) in children.iter_mut().enumerate() {
            if statuses[index].is_none() {
                statuses[index] = child
                    .try_wait()
                    .map_err(|error| format!("failed to poll pipeline stage: {error}"))?;
            }
            complete &= statuses[index].is_some();
        }
        if complete {
            break;
        }
        if exceeded.load(std::sync::atomic::Ordering::Acquire) != 0 {
            terminate_children(&mut children);
            break;
        }
        if clock.elapsed() >= deadline {
            timed_out = true;
            terminate_children(&mut children);
            break;
        }
        backoff.pause(&clock, deadline);
    }
    for (index, child) in children.iter_mut().enumerate() {
        if statuses[index].is_none() {
            statuses[index] = Some(
                child
                    .wait()
                    .map_err(|error| format!("failed to reap pipeline stage: {error}"))?,
            );
        }
    }
    if let Some(writer) = stdin_writer {
        writer
            .join()
            .map_err(|_error| "pipeline stdin writer panicked".to_owned())??;
    }
    let mut stdout = stdout_reader
        .ok_or("pipeline final stdout reader is unavailable")?
        .join()
        .map_err(|_error| "pipeline stdout reader panicked".to_owned())??;
    let mut stderrs = Vec::with_capacity(count);
    for reader in stderr_readers {
        stderrs.push(
            reader
                .join()
                .map_err(|_error| "pipeline stderr reader panicked".to_owned())??,
        );
    }
    let limit_exceeded = match exceeded.load(std::sync::atomic::Ordering::Acquire) {
        1 => Some("stdout".into()),
        2 => Some("stderr".into()),
        _ if timed_out => Some("timeout".into()),
        _ => None,
    };
    statuses
        .into_iter()
        .zip(stderrs)
        .enumerate()
        .map(|(index, (status, stderr))| {
            let status = status.ok_or_else(|| {
                format!("pipeline stage {index} was reaped without recording its status")
            })?;
            Ok(Outcome {
                exit_code: if timed_out {
                    124
                } else if limit_exceeded.is_some() {
                    1
                } else {
                    exit_code(status)
                },
                stdout: if index + 1 == count {
                    std::mem::take(&mut stdout)
                } else {
                    Vec::new()
                },
                stderr,
                timed_out,
                limit_exceeded: limit_exceeded.clone(),
                signal: exit_signal(status),
            })
        })
        .collect()
}

pub(crate) fn execute_with_clock<C: Clock>(
    root: &Path,
    request: Request,
    clock: &C,
) -> Result<Outcome, String> {
    if request.argv.first().is_none_or(String::is_empty) {
        return Err("process agent argv must not be empty".into());
    }
    if request.argv.iter().any(|value| value.contains('\0')) {
        return Err("process agent argv must not contain NUL".into());
    }
    if request.limits.timeout_ms == 0 || request.limits.timeout_ms > 86_400_000 {
        return Err("process agent timeout_ms must be between 1 and 86400000".into());
    }
    if request.limits.memory_bytes < 16 * 1024 * 1024
        || request.limits.processes == 0
        || request.limits.stdout_bytes == 0
        || request.limits.stderr_bytes == 0
    {
        return Err(
            "process agent resource limits must be positive and memory at least 16 MiB".into(),
        );
    }
    let directory = _resolve_directory(root, request.working_directory.as_deref())?;
    let mut seen = std::collections::BTreeSet::new();
    for (name, value) in &request.environment {
        if !valid_environment_name(name) {
            return Err(format!("invalid process environment name: {name}"));
        }
        if value.contains('\0') {
            return Err(format!("process environment value contains NUL: {name}"));
        }
        let key = if cfg!(windows) {
            name.to_ascii_uppercase()
        } else {
            name.clone()
        };
        if !seen.insert(key) {
            return Err(format!("duplicate process environment variable: {name}"));
        }
    }
    let executable = &request.argv[0];
    let mut command = std::process::Command::new(executable);
    command
        .args(&request.argv[1..])
        .current_dir(directory)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    add_essential_environment(&mut command);
    for (name, value) in &request.environment {
        command.env(name, value);
    }
    #[cfg(unix)]
    configure_unix_limits(&mut command, request.limits)
        .map_err(|error| format!("failed to prepare resource limits for {executable}: {error}"))?;
    let mut child = crate::host::spawn(&mut command)
        .map_err(|error| format!("failed to start {executable}: {error}"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or("process stdout pipe is unavailable")?;
    let child_stderr = child
        .stderr
        .take()
        .ok_or("process stderr pipe is unavailable")?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or("process stdin pipe is unavailable")?;
    let exceeded = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let stdout_exceeded = std::sync::Arc::clone(&exceeded);
    let stderr_exceeded = std::sync::Arc::clone(&exceeded);
    let stdout_limit = request.limits.stdout_bytes;
    let stderr_limit = request.limits.stderr_bytes;
    let stdout_reader = std::thread::spawn(move || {
        read_pipe(ReadPipeArgs {
            pipe: child_stdout,
            label: "stdout",
            limit: stdout_limit,
            exceeded: &stdout_exceeded,
            code: 1,
        })
    });
    let stderr_reader = std::thread::spawn(move || {
        read_pipe(ReadPipeArgs {
            pipe: child_stderr,
            label: "stderr",
            limit: stderr_limit,
            exceeded: &stderr_exceeded,
            code: 2,
        })
    });
    let input = request.stdin;
    let stdin_writer = std::thread::spawn(move || -> Result<(), String> {
        use std::io::Write as _;
        match child_stdin.write_all(&input) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(format!("failed to write process stdin: {error}")),
        }
    });
    let deadline = Duration::from_millis(request.limits.timeout_ms);
    let mut timed_out = false;
    let mut output_limited = false;
    let mut backoff = PollBackoff::default();
    let status = loop {
        if exceeded.load(std::sync::atomic::Ordering::Acquire) != 0 {
            output_limited = true;
            kill_process_tree(&mut child);
            break child
                .wait()
                .map_err(|error| format!("failed to reap output-limited {executable}: {error}"))?;
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed to poll {executable}: {error}"))?
        {
            break status;
        }
        if clock.elapsed() >= deadline {
            timed_out = true;
            kill_process_tree(&mut child);
            break child
                .wait()
                .map_err(|error| format!("failed to reap timed-out {executable}: {error}"))?;
        }
        backoff.pause(clock, deadline);
    };
    stdin_writer
        .join()
        .map_err(|_error| "process stdin writer panicked".to_owned())??;
    let stdout = stdout_reader
        .join()
        .map_err(|_error| "process stdout reader panicked".to_owned())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_error| "process stderr reader panicked".to_owned())??;
    let signal = exit_signal(status);
    let limit_exceeded = match exceeded.load(std::sync::atomic::Ordering::Acquire) {
        1 => Some("stdout".into()),
        2 => Some("stderr".into()),
        _ if timed_out => Some("timeout".into()),
        _ => None,
    };
    Ok(Outcome {
        exit_code: if timed_out {
            124
        } else if output_limited || limit_exceeded.is_some() {
            1
        } else {
            exit_code(status)
        },
        stdout,
        stderr,
        timed_out,
        limit_exceeded,
        signal,
    })
}

fn _resolve_directory(root: &Path, relative: Option<&str>) -> Result<PathBuf, String> {
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect process root {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "process root is not a regular directory: {}",
            root.display()
        ));
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve process root {}: {error}", root.display()))?;
    let Some(relative) = relative else {
        return Ok(root);
    };
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!(
            "process working directory is not normalized: {relative}"
        ));
    }
    let mut candidate = root.clone();
    for component in relative.split('/') {
        candidate.push(component);
        let metadata = candidate.symlink_metadata().map_err(|error| {
            format!("cannot inspect process working directory {relative}: {error}")
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "process working directory path must not contain a symlink: {relative}"
            ));
        }
        if !metadata.file_type().is_dir() {
            return Err(format!(
                "process working directory is not a directory: {relative}"
            ));
        }
    }
    let candidate = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve process working directory {relative}: {error}"))?;
    if !candidate.starts_with(&root) {
        return Err(format!(
            "process working directory escapes root: {relative}"
        ));
    }
    Ok(candidate)
}

fn valid_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn add_essential_environment(command: &mut std::process::Command) {
    let toolchain = msvc_toolchain_environment();
    add_essential_environment_with(command, crate::host::variable, toolchain);
}

fn add_essential_environment_with(
    command: &mut std::process::Command,
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
    msvc_toolchain: &[(std::ffi::OsString, std::ffi::OsString)],
) {
    for name in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "ComSpec",
        "COMSPEC",
        "PATHEXT",
        // rustc's pinned find-msvc-tools fallback resolves vswhere.exe from
        // these roots after env_clear; neither value carries user credentials.
        "ProgramFiles",
        "ProgramFiles(x86)",
        "WINDIR",
    ] {
        if let Some(value) = lookup(name) {
            command.env(name, value);
        }
    }
    // `find-msvc-tools` derives only these toolchain search paths. They are
    // applied after the ambient allowlist so an unrelated Git `link.exe`
    // cannot win PATH resolution after env_clear.
    for expected in ["PATH", "LIB", "INCLUDE"] {
        if let Some((_, value)) = msvc_toolchain
            .iter()
            .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(expected))
        {
            command.env(expected, value);
        }
    }
}

#[cfg(windows)]
fn msvc_toolchain_environment() -> &'static [(std::ffi::OsString, std::ffi::OsString)] {
    // COM-based Visual Studio discovery is process-global and the toolchain is
    // immutable for one deshell invocation. Resolve it once even when a plan
    // launches many commands or pipeline stages.
    static ENVIRONMENT: std::sync::OnceLock<Vec<(std::ffi::OsString, std::ffi::OsString)>> =
        std::sync::OnceLock::new();
    ENVIRONMENT.get_or_init(discover_msvc_toolchain_environment)
}

#[cfg(windows)]
fn discover_msvc_toolchain_environment() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let Some(tool) = find_msvc_tools::find_tool(std::env::consts::ARCH, "link.exe") else {
        return Vec::new();
    };
    let mut entries = tool
        .env()
        .into_iter()
        .filter(|(name, _)| {
            matches!(
                name.to_string_lossy().to_ascii_uppercase().as_str(),
                "PATH" | "LIB" | "INCLUDE"
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    // A developer-command-prompt discovery may return no explicit PATH
    // override. Always bind the exact discovered linker directory first.
    let inherited_path = entries
        .iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value.clone())
        .or_else(|| crate::host::variable("PATH"))
        .unwrap_or_default();
    let mut paths = tool
        .path()
        .parent()
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    paths.extend(std::env::split_paths(&inherited_path));
    if let Ok(path) = std::env::join_paths(paths) {
        entries.retain(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case("PATH"));
        entries.push(("PATH".into(), path));
    }
    entries
}

#[cfg(not(windows))]
fn msvc_toolchain_environment() -> &'static [(std::ffi::OsString, std::ffi::OsString)] {
    &[]
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(coverage),
    not(deshell_sanitizer_address),
    not(deshell_sanitizer_undefined)
))]
fn inherited_hard_limit(resource: libc::c_int) -> std::io::Result<libc::rlim_t> {
    let mut inherited = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: `getrlimit` writes one complete `rlimit` when it returns success.
    let result = unsafe { libc::getrlimit(resource, inherited.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the successful `getrlimit` call initialized the complete value.
    let inherited = unsafe { inherited.assume_init() };
    Ok(inherited.rlim_max)
}

#[cfg(any(
    all(
        unix,
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ),
    test,
    kani
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HardCeiling {
    Finite(u64),
    Infinite,
}

#[cfg(any(
    all(
        unix,
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ),
    test,
    kani
))]
fn cap_limit(requested: u64, inherited_hard: HardCeiling) -> u64 {
    match inherited_hard {
        HardCeiling::Finite(inherited) => requested.min(inherited),
        HardCeiling::Infinite => requested,
    }
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(coverage),
    not(deshell_sanitizer_address),
    not(deshell_sanitizer_undefined)
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CappedLimitError {
    NotRepresentable,
    CollidesWithInfinity,
    InheritedHardCeilingNotRepresentable,
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(coverage),
    not(deshell_sanitizer_address),
    not(deshell_sanitizer_undefined)
))]
impl CappedLimitError {
    fn description(self) -> &'static str {
        match self {
            Self::NotRepresentable => "the requested cap is not representable on this platform",
            Self::CollidesWithInfinity => "the requested cap collides with RLIM_INFINITY",
            Self::InheritedHardCeilingNotRepresentable => {
                "the inherited hard ceiling is not representable by the cap core"
            }
        }
    }
}

#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(coverage),
    not(deshell_sanitizer_address),
    not(deshell_sanitizer_undefined)
))]
fn rlimit_for_cap(
    requested: u64,
    inherited_hard: libc::rlim_t,
) -> Result<libc::rlimit, CappedLimitError> {
    let requested_limit =
        libc::rlim_t::try_from(requested).map_err(|_| CappedLimitError::NotRepresentable)?;
    if requested_limit == libc::RLIM_INFINITY {
        return Err(CappedLimitError::CollidesWithInfinity);
    }
    let inherited_hard = if inherited_hard == libc::RLIM_INFINITY {
        HardCeiling::Infinite
    } else {
        HardCeiling::Finite(
            u64::try_from(inherited_hard)
                .map_err(|_| CappedLimitError::InheritedHardCeilingNotRepresentable)?,
        )
    };
    let effective = libc::rlim_t::try_from(cap_limit(requested, inherited_hard))
        .map_err(|_| CappedLimitError::NotRepresentable)?;
    if effective == libc::RLIM_INFINITY {
        return Err(CappedLimitError::CollidesWithInfinity);
    }
    Ok(libc::rlimit {
        rlim_cur: effective,
        rlim_max: effective,
    })
}

#[cfg(unix)]
fn configure_unix_limits(
    command: &mut std::process::Command,
    _limits: Limits,
) -> Result<(), String> {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);

    // Keep Darwin on Rust's posix_spawn path. Coverage and AddressSanitizer
    // instrumentation also expand the harness address space and create runtime
    // threads that are unrelated to the command under test; applying per-UID
    // RLIMIT_NPROC or a production-sized RLIMIT_AS there produces false
    // `cannot fork` failures. Ordinary builds still exercise these limits, and
    // disposable providers enforce their own memory and PID boundaries.
    #[cfg(all(
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ))]
    {
        // Snapshot inherited ceilings before `fork`; `getrlimit` is not part of the POSIX async-signal-safe set required for a `pre_exec` closure.
        // If another thread lowers a ceiling before `fork`, applying this stale snapshot can fail, and the spawn must remain fail-closed.
        let memory_hard = inherited_hard_limit(libc::RLIMIT_AS)
            .map_err(|error| format!("failed to read inherited RLIMIT_AS: {error}"))?;
        let memory_limit = rlimit_for_cap(_limits.memory_bytes, memory_hard)
            .map_err(|error| format!("invalid RLIMIT_AS cap: {}", error.description()))?;
        let process_hard = inherited_hard_limit(libc::RLIMIT_NPROC)
            .map_err(|error| format!("failed to read inherited RLIMIT_NPROC: {error}"))?;
        let process_limit = rlimit_for_cap(_limits.processes, process_hard)
            .map_err(|error| format!("invalid RLIMIT_NPROC cap: {}", error.description()))?;
        let apply_limits = move || {
            // SAFETY: the pointer names a fully initialized `rlimit` that remains
            // alive for the duration of this system call.
            let memory_result =
                unsafe { libc::setrlimit(libc::RLIMIT_AS, &raw const memory_limit) };
            if memory_result != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: the pointer names a fully initialized `rlimit` that remains
            // alive for the duration of this system call.
            let process_result =
                unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &raw const process_limit) };
            if process_result != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        };
        // SAFETY: `pre_exec` applies only the already prepared limits through
        // the existing setrlimit boundary and allocates no memory after `fork`.
        unsafe {
            command.pre_exec(apply_limits);
        }
    }
    Ok(())
}

fn terminate_children(children: &mut [std::process::Child]) {
    for child in children.iter_mut() {
        kill_process_tree(child);
    }
}

#[cfg(kani)]
mod rlimit_proofs {
    use super::{HardCeiling, cap_limit};

    #[kani::proof]
    fn cap_limit_stays_within_requested_and_inherited_hard_ceilings() {
        let requested: u64 = kani::any();
        let inherited_value: u64 = kani::any();
        let inherited_is_infinite: bool = kani::any();
        let inherited_hard = if inherited_is_infinite {
            HardCeiling::Infinite
        } else {
            HardCeiling::Finite(inherited_value)
        };
        let effective = cap_limit(requested, inherited_hard);

        assert!(effective <= requested);
        match inherited_hard {
            HardCeiling::Finite(inherited) => {
                assert_eq!(effective, requested.min(inherited));
                assert!(effective <= inherited);
                kani::cover!(inherited < requested);
                kani::cover!(inherited >= requested);
            }
            HardCeiling::Infinite => {
                assert_eq!(effective, requested);
                kani::cover!(inherited_is_infinite);
            }
        }
    }
}

/// The inputs of [`read_pipe`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`read_pipe`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ReadPipeArgs<'a, R: std::io::Read> {
    pipe: R,
    label: &'a str,
    limit: u64,
    exceeded: &'a std::sync::atomic::AtomicU8,
    code: u8,
}

fn read_pipe<R: std::io::Read>(parts: ReadPipeArgs<'_, R>) -> Result<Vec<u8>, String> {
    // Destructured without `..`: see `ReadPipeArgs`.
    let ReadPipeArgs {
        mut pipe,
        label,
        limit,
        exceeded,
        code,
    } = parts;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = pipe
            .read(&mut buffer)
            .map_err(|error| format!("failed to read process {label}: {error}"))?;
        if count == 0 {
            break;
        }
        let written = u64::try_from(output.len()).unwrap_or(u64::MAX);
        let remaining = usize::try_from(limit.saturating_sub(written)).unwrap_or(usize::MAX);
        output.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            let _first_exceeded_stream = exceeded.compare_exchange(
                0,
                code,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            );
            break;
        }
    }
    Ok(output)
}

/// The inputs of [`read_pipe_shared`].
///
/// An argument list admits no exhaustive destructuring, so a parameter added to a
/// many-argument function stays invisible to every call site that already
/// compiles. [`read_pipe_shared`] takes this apart without `..`, so a field added here fails
/// to compile until somebody gives it a destination.
struct ReadPipeSharedArgs<'a, R: std::io::Read> {
    pipe: R,
    label: &'a str,
    limit: u64,
    total: &'a std::sync::atomic::AtomicU64,
    exceeded: &'a std::sync::atomic::AtomicU8,
    code: u8,
}

fn read_pipe_shared<R: std::io::Read>(parts: ReadPipeSharedArgs<'_, R>) -> Result<Vec<u8>, String> {
    // Destructured without `..`: see `ReadPipeSharedArgs`.
    let ReadPipeSharedArgs {
        mut pipe,
        label,
        limit,
        total,
        exceeded,
        code,
    } = parts;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = pipe
            .read(&mut buffer)
            .map_err(|error| format!("failed to read process {label}: {error}"))?;
        if count == 0 {
            break;
        }
        let count_u64 = u64::try_from(count).unwrap_or(u64::MAX);
        let previous = total.fetch_add(count_u64, std::sync::atomic::Ordering::AcqRel);
        let remaining = usize::try_from(limit.saturating_sub(previous)).unwrap_or(usize::MAX);
        output.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            let _first_exceeded_stream = exceeded.compare_exchange(
                0,
                code,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            );
            break;
        }
    }
    Ok(output)
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The child starts in its own process group, so a negative PID kills
        // descendants as well as the direct child.
        if let Ok(group_leader) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `group_leader` came from a live OS child, is representable
            // as `pid_t`, and `kill` does not retain pointers.
            unsafe {
                libc::kill(-group_leader, libc::SIGKILL);
            }
        }
    }
    let _direct_kill_result = child.kill();
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        128 + status.signal().unwrap_or(0)
    }
    #[cfg(not(unix))]
    {
        1
    }
}

fn exit_signal(status: std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _status = status;
        None
    }
}

/// Why a request to a long-lived agent did not produce an answer.
///
/// `TimedOut` is separate from the rest for the same reason `LoweringFailure`
/// separates `Unmeasured` from `Delegate`: a budget that ran out is not an
/// answer about the input, and treating it as one makes a guarantee depend on
/// how busy the machine was.
#[derive(Debug)]
pub(crate) enum AgentError {
    TimedOut,
    Ended(String),
}

/// A process that stays alive across requests, one framed line each way.
///
/// `execute` starts a process, sends it one thing and waits for it to die. That
/// is the right shape for running a script and the wrong one for a parser: the
/// PowerShell adapter has always been a loop over framed requests on stdin — a
/// long-lived agent — and de-shell started one, sent one request and let it die,
/// so every parse paid a process start.
///
/// Measured on this machine: one `pwsh` start is 0.26 s, and sixteen concurrent
/// starts are 4.6 s each, because `pwsh` here resolves through a version-manager
/// shim that serialises. Under the test suite's parallelism that reached the
/// parser's ten-second budget. It had always been reaching it; before budget
/// failures stopped being delegated silently, the answer was a block marked
/// `delegated` instead of a visible failure.
pub(crate) struct Agent {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    stderr: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl Agent {
    /// Start `argv` in `directory`, with the same cleared environment every
    /// other process here gets.
    pub(crate) fn start(directory: &Path, argv: &[String]) -> Result<Self, String> {
        let executable = argv
            .first()
            .ok_or_else(|| "agent argv must name an executable".to_owned())?;
        let mut command = std::process::Command::new(executable);
        command
            .args(&argv[1..])
            .current_dir(directory)
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        add_essential_environment(&mut command);
        let mut child = crate::host::spawn(&mut command)
            .map_err(|error| format!("cannot start {executable}: {error}"))?;
        let stdin = child.stdin.take().ok_or("agent stdin is unavailable")?;
        let stdout = child.stdout.take().ok_or("agent stdout is unavailable")?;
        let child_stderr = child.stderr.take().ok_or("agent stderr is unavailable")?;

        // Read on a thread so a request can wait with a budget, and drain stderr
        // on another so a talkative agent cannot fill its pipe and block.
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        while matches!(line.last(), Some(b'\n' | b'\r')) {
                            line.pop();
                        }
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _send_result = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        let stderr = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let drain = std::sync::Arc::clone(&stderr);
        std::thread::spawn(move || {
            use std::io::Read;
            let mut reader = child_stderr;
            let mut buffer = [0_u8; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                if let Ok(mut held) = drain.lock() {
                    held.extend_from_slice(&buffer[..read]);
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            stderr,
        })
    }

    /// Send one line and wait for one back, for at most `budget`.
    pub(crate) fn request(&mut self, line: &[u8], budget: Duration) -> Result<Vec<u8>, AgentError> {
        use std::io::Write;
        self.stdin
            .write_all(line)
            .and_then(|()| self.stdin.write_all(b"\n"))
            .and_then(|()| self.stdin.flush())
            .map_err(|error| AgentError::Ended(format!("cannot write to the agent: {error}")))?;
        match self.lines.recv_timeout(budget) {
            Ok(Ok(line)) => Ok(line),
            Ok(Err(error)) => Err(AgentError::Ended(format!(
                "cannot read from the agent: {error}"
            ))),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(AgentError::TimedOut),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(AgentError::Ended(self.ending()))
            }
        }
    }

    /// What the agent said on its way out, for a message a reader can act on.
    fn ending(&self) -> String {
        let stderr = self
            .stderr
            .lock()
            .map(|held| String::from_utf8_lossy(&held).trim().to_owned())
            .unwrap_or_default();
        if stderr.is_empty() {
            "the agent ended without answering".into()
        } else {
            format!("the agent ended without answering: {stderr}")
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // Closing stdin is how the adapter's read loop ends. Killing follows in
        // case it does not, so a dropped agent never outlives the run.
        let _kill_result = self.child.kill();
        let _wait_result = self.child.wait();
    }
}

#[cfg(test)]
// Tests reach for the raw APIs on purpose: they stage corrupt trees, race two
// writers against one path, and assert on what the transactional layer does with
// the result. Constructing those situations is precisely what the production ban
// exists to prevent, so the ban is lifted here and nowhere else.
#[expect(
    clippy::disallowed_methods,
    reason = "tests construct the races and corrupt trees the production ban prevents"
)]
mod tests {
    use super::*;

    #[test]
    fn cap_limit_uses_the_finite_minimum_and_preserves_infinite_ceilings() {
        for (requested, inherited, expected) in [
            (0, HardCeiling::Finite(0), 0),
            (100, HardCeiling::Finite(50), 50),
            (50, HardCeiling::Finite(100), 50),
            (75, HardCeiling::Infinite, 75),
        ] {
            assert_eq!(cap_limit(requested, inherited), expected);
        }
    }

    #[cfg(all(
        unix,
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ))]
    #[test]
    fn rlimit_adapter_refines_finite_and_infinite_ceilings() {
        let finite_50 = libc::rlim_t::try_from(50_u64).unwrap();
        let finite_75 = libc::rlim_t::try_from(75_u64).unwrap();
        for (requested, inherited_hard, expected) in [
            (100_u64, finite_50, finite_50),
            (50, libc::rlim_t::try_from(100_u64).unwrap(), finite_50),
            (75, libc::RLIM_INFINITY, finite_75),
        ] {
            let limit = rlimit_for_cap(requested, inherited_hard).unwrap();
            assert_eq!(limit.rlim_cur, expected);
            assert_eq!(limit.rlim_max, expected);
        }
    }

    #[cfg(all(
        unix,
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ))]
    #[test]
    fn rlimit_adapter_rejects_infinity_requests() {
        assert!(matches!(
            rlimit_for_cap(u64::MAX, libc::RLIM_INFINITY),
            Err(CappedLimitError::NotRepresentable | CappedLimitError::CollidesWithInfinity)
        ));
        if let Ok(infinity_as_u64) = u64::try_from(libc::RLIM_INFINITY) {
            assert!(matches!(
                rlimit_for_cap(infinity_as_u64, libc::RLIM_INFINITY),
                Err(CappedLimitError::CollidesWithInfinity)
            ));
        }
    }

    #[cfg(all(
        unix,
        not(target_os = "macos"),
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ))]
    #[test]
    fn inherited_hard_limit_reports_invalid_resource_queries() {
        assert!(inherited_hard_limit(-1).is_err());
    }

    #[cfg(all(
        target_os = "linux",
        target_pointer_width = "64",
        not(coverage),
        not(deshell_sanitizer_address),
        not(deshell_sanitizer_undefined)
    ))]
    #[test]
    fn inherited_lower_memory_hard_limit_is_clamped_in_an_isolated_test_process() {
        const CHILD_MARKER: &str = "DESHELL_RLIMIT_REGRESSION_CHILD";
        const CHILD_HARD_LIMIT: u64 = 16 * 1024 * 1024 * 1024;
        const REQUESTED_LIMIT: u64 = 32 * 1024 * 1024 * 1024;
        const CHILD_TEST: &str = "agent_process::tests::inherited_lower_memory_hard_limit_is_clamped_in_an_isolated_test_process";
        const CAP_SYS_RESOURCE: u32 = 24;

        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let effective_capabilities = status
            .lines()
            .find_map(|line| line.strip_prefix("CapEff:\t"))
            .and_then(|value| u64::from_str_radix(value, 16).ok())
            .expect("Linux process status must expose effective capabilities");
        assert_eq!(
            effective_capabilities & (1_u64 << CAP_SYS_RESOURCE),
            0,
            "this hard-limit regression requires an unprivileged process without CAP_SYS_RESOURCE"
        );

        if crate::host::variable(CHILD_MARKER).is_none() {
            let executable = std::env::current_exe().unwrap();
            let output = std::process::Command::new(executable)
                .args(["--exact", CHILD_TEST, "--nocapture"])
                .env(CHILD_MARKER, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "isolated regression process failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let mut inherited = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        // SAFETY: `getrlimit` writes one complete `rlimit` when it returns success.
        let read_result = unsafe { libc::getrlimit(libc::RLIMIT_AS, inherited.as_mut_ptr()) };
        assert_eq!(read_result, 0, "{}", std::io::Error::last_os_error());
        // SAFETY: the successful `getrlimit` call initialized the complete value.
        let inherited = unsafe { inherited.assume_init() };
        let child_hard = libc::rlim_t::try_from(CHILD_HARD_LIMIT).unwrap();
        if inherited.rlim_max == libc::RLIM_INFINITY || inherited.rlim_max > child_hard {
            let child_limit = libc::rlimit {
                rlim_cur: if inherited.rlim_cur == libc::RLIM_INFINITY {
                    child_hard
                } else {
                    inherited.rlim_cur.min(child_hard)
                },
                rlim_max: child_hard,
            };
            // SAFETY: this regression runs in a dedicated child test process and
            // only lowers its own hard address-space limit.
            let limit_result = unsafe { libc::setrlimit(libc::RLIMIT_AS, &raw const child_limit) };
            assert_eq!(limit_result, 0, "{}", std::io::Error::last_os_error());
        }

        let outcome = execute(
            Path::new("/"),
            Request {
                argv: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "test \"$(ulimit -H -v)\" = \"$(ulimit -S -v)\" && ! ulimit -S -v 33554433 2>/dev/null && test \"$(ulimit -H -u)\" = \"$(ulimit -S -u)\" && ! ulimit -S -u 65536 2>/dev/null".into(),
                ],
                environment: Vec::new(),
                working_directory: None,
                stdin: Vec::new(),
                limits: Limits {
                    timeout_ms: 5_000,
                    memory_bytes: REQUESTED_LIMIT,
                    processes: 65_535,
                    ..Limits::default()
                },
            },
        )
        .unwrap();
        assert_eq!(
            outcome.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        assert!(!outcome.timed_out);
    }

    #[test]
    fn essential_environment_supports_isolated_msvc_discovery_without_ambient_secrets() {
        let mut command = std::process::Command::new("unused");
        let toolchain = vec![
            ("PATH".into(), "msvc:path".into()),
            ("LIB".into(), "msvc:lib".into()),
            ("INCLUDE".into(), "msvc:include".into()),
            ("GITHUB_TOKEN".into(), "must-not-leak".into()),
        ];
        add_essential_environment_with(
            &mut command,
            |name| Some(format!("value:{name}").into()),
            &toolchain,
        );
        let environment = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            environment.get("ProgramFiles").map(String::as_str),
            Some("value:ProgramFiles")
        );
        assert_eq!(
            environment.get("ProgramFiles(x86)").map(String::as_str),
            Some("value:ProgramFiles(x86)")
        );
        for (name, expected) in [
            ("PATH", "msvc:path"),
            ("LIB", "msvc:lib"),
            ("INCLUDE", "msvc:include"),
        ] {
            assert_eq!(
                environment.get(name).map(String::as_str),
                Some(expected),
                "missing derived MSVC environment entry {name}"
            );
        }
        assert!(!environment.contains_key("USERPROFILE"));
        assert!(!environment.contains_key("GITHUB_TOKEN"));
    }

    #[cfg(windows)]
    #[test]
    fn isolated_rustc_discovers_the_msvc_linker_after_environment_clear() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("main.rs"), b"fn main() {}\n").unwrap();
        let outcome = execute(
            directory.path(),
            Request {
                argv: vec![
                    "rustc".into(),
                    "main.rs".into(),
                    "-o".into(),
                    "isolated.exe".into(),
                ],
                environment: Vec::new(),
                working_directory: None,
                stdin: Vec::new(),
                limits: Limits {
                    timeout_ms: 60_000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    processes: 60_000,
                    ..Limits::default()
                },
            },
        )
        .unwrap();
        assert_eq!(
            outcome.exit_code,
            0,
            "{}",
            String::from_utf8_lossy(&outcome.stderr)
        );
        assert!(directory.path().join("isolated.exe").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn exact_argv_environment_cwd_and_raw_stdio_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("work")).unwrap();
        let outcome = execute(
            directory.path(),
            Request {
                argv: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "printf '%s\\n' \"$1\" \"$VALUE\"; pwd; cat".into(),
                    "ignored".into(),
                    "literal-$HOME".into(),
                ],
                environment: vec![("VALUE".into(), "from-env".into())],
                working_directory: Some("work".into()),
                stdin: vec![0, 0xff],
                limits: Limits {
                    timeout_ms: 5_000,
                    ..Limits::default()
                },
            },
        )
        .unwrap();
        assert_eq!(outcome.exit_code, 0);
        assert!(!outcome.timed_out);
        assert!(outcome.stdout.starts_with(b"literal-$HOME\nfrom-env\n"));
        assert!(outcome.stdout.ends_with(&[0, 0xff]));
        let text = String::from_utf8_lossy(&outcome.stdout[..outcome.stdout.len() - 2]);
        assert!(
            text.contains(
                directory
                    .path()
                    .join("work")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn invalid_requests_and_cwd_escape_are_rejected_before_spawn() {
        let directory = tempfile::tempdir().unwrap();
        let base = Request {
            argv: vec![],
            environment: vec![],
            working_directory: None,
            stdin: vec![],
            limits: Limits {
                timeout_ms: 1,
                ..Limits::default()
            },
        };
        assert!(
            execute(directory.path(), base.clone())
                .unwrap_err()
                .contains("argv")
        );
        let mut duplicate = base.clone();
        duplicate.argv = vec!["missing".into()];
        duplicate.environment = vec![("A".into(), "1".into()), ("A".into(), "2".into())];
        assert!(
            execute(directory.path(), duplicate)
                .unwrap_err()
                .contains("duplicate")
        );
        let mut escape = base;
        escape.argv = vec!["missing".into()];
        escape.working_directory = Some("../outside".into());
        assert!(execute(directory.path(), escape).is_err());

        let mut excessive_pipeline_limit = Request {
            argv: vec!["missing".into()],
            environment: vec![],
            working_directory: None,
            stdin: vec![],
            limits: Limits::default(),
        };
        excessive_pipeline_limit.limits.timeout_ms = 86_400_001;
        assert!(
            execute_pipeline(directory.path(), vec![excessive_pipeline_limit])
                .unwrap_err()
                .contains("limits")
        );
    }

    #[test]
    fn poll_backoff_yields_then_doubles_caps_and_honors_the_deadline() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        enum Event {
            Yield,
            Wait(Duration),
        }
        struct RecordingClock {
            elapsed: std::sync::Mutex<Duration>,
            events: std::sync::Mutex<Vec<Event>>,
        }
        impl RecordingClock {
            fn at(elapsed: Duration) -> Self {
                Self {
                    elapsed: std::sync::Mutex::new(elapsed),
                    events: std::sync::Mutex::new(Vec::new()),
                }
            }
        }
        impl Clock for RecordingClock {
            fn elapsed(&self) -> Duration {
                *self.elapsed.lock().unwrap()
            }
            fn yield_now(&self) {
                self.events.lock().unwrap().push(Event::Yield);
            }
            fn wait(&self, duration: Duration) {
                self.events.lock().unwrap().push(Event::Wait(duration));
                *self.elapsed.lock().unwrap() += duration;
            }
        }

        let clock = RecordingClock::at(Duration::ZERO);
        let mut backoff = PollBackoff::default();
        for _ in 0..11 {
            backoff.pause(&clock, Duration::from_secs(1));
        }
        assert_eq!(
            *clock.events.lock().unwrap(),
            [
                Event::Yield,
                Event::Yield,
                Event::Yield,
                Event::Yield,
                Event::Wait(Duration::from_micros(250)),
                Event::Wait(Duration::from_micros(500)),
                Event::Wait(Duration::from_millis(1)),
                Event::Wait(Duration::from_millis(2)),
                Event::Wait(Duration::from_millis(4)),
                Event::Wait(Duration::from_millis(5)),
                Event::Wait(Duration::from_millis(5)),
            ]
        );

        let clock = RecordingClock::at(Duration::from_micros(9_900));
        let mut backoff = PollBackoff::default();
        for _ in 0..5 {
            backoff.pause(&clock, Duration::from_millis(10));
        }
        assert_eq!(
            *clock.events.lock().unwrap(),
            [
                Event::Yield,
                Event::Yield,
                Event::Yield,
                Event::Yield,
                Event::Wait(Duration::from_micros(100)),
            ]
        );
        assert_eq!(clock.elapsed(), Duration::from_millis(10));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_uses_the_injected_clock_and_returns_124() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct FakeClock(AtomicU64);
        impl Clock for FakeClock {
            fn elapsed(&self) -> Duration {
                Duration::from_millis(self.0.fetch_add(10, Ordering::Relaxed))
            }
            fn yield_now(&self) {}
            fn wait(&self, _duration: Duration) {}
        }
        let directory = tempfile::tempdir().unwrap();
        let outcome = execute_with_clock(
            directory.path(),
            Request {
                argv: vec!["/bin/sh".into(), "-c".into(), "while :; do :; done".into()],
                environment: vec![],
                working_directory: None,
                stdin: vec![],
                limits: Limits {
                    timeout_ms: 5,
                    ..Limits::default()
                },
            },
            &FakeClock(AtomicU64::new(0)),
        )
        .unwrap();
        assert!(outcome.timed_out);
        assert_eq!(outcome.exit_code, 124);
        assert!(outcome.signal.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn cwd_symlinks_are_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("inside")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink("inside", root.path().join("inside-link")).unwrap();
        assert!(
            _resolve_directory(root.path(), Some("link"))
                .unwrap_err()
                .contains("symlink")
        );
        assert!(
            _resolve_directory(root.path(), Some("inside-link"))
                .unwrap_err()
                .contains("symlink")
        );
    }

    #[cfg(unix)]
    #[test]
    fn output_limit_is_explicit_and_bounded() {
        let directory = tempfile::tempdir().unwrap();
        let outcome = execute(
            directory.path(),
            Request {
                argv: vec!["/bin/sh".into(), "-c".into(), "yes x".into()],
                environment: vec![],
                working_directory: None,
                stdin: vec![],
                limits: Limits {
                    stdout_bytes: 1024,
                    ..Limits::default()
                },
            },
        )
        .unwrap();
        assert_eq!(outcome.limit_exceeded.as_deref(), Some("stdout"));
        assert_eq!(outcome.exit_code, 1);
        assert_eq!(outcome.stdout.len(), 1024);
    }

    #[cfg(unix)]
    #[test]
    fn pipeline_processes_run_concurrently_and_broken_pipe_completes() {
        let directory = tempfile::tempdir().unwrap();
        let limits = Limits {
            timeout_ms: 2_000,
            ..Limits::default()
        };
        let outcomes = execute_pipeline(
            directory.path(),
            vec![
                Request {
                    argv: vec!["yes".into(), "value".into()],
                    environment: vec![],
                    working_directory: None,
                    stdin: vec![],
                    limits,
                },
                Request {
                    argv: vec!["head".into(), "-n".into(), "1".into()],
                    environment: vec![],
                    working_directory: None,
                    stdin: vec![],
                    limits,
                },
            ],
        )
        .unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[1].exit_code, 0);
        assert_eq!(outcomes[1].stdout, b"value\n");
        assert!(!outcomes[1].timed_out);
    }

    #[cfg(unix)]
    #[test]
    fn pipeline_stderr_limit_is_aggregate_across_stages() {
        let directory = tempfile::tempdir().unwrap();
        let limits = Limits {
            stderr_bytes: 1024,
            timeout_ms: 2_000,
            ..Limits::default()
        };
        let outcomes = execute_pipeline(
            directory.path(),
            vec![
                Request {
                    argv: vec!["sh".into(), "-c".into(), "yes a >&2".into()],
                    environment: vec![],
                    working_directory: None,
                    stdin: vec![],
                    limits,
                },
                Request {
                    argv: vec!["sh".into(), "-c".into(), "yes b >&2".into()],
                    environment: vec![],
                    working_directory: None,
                    stdin: vec![],
                    limits,
                },
            ],
        )
        .unwrap();
        assert!(
            outcomes
                .iter()
                .all(|outcome| { outcome.limit_exceeded.as_deref() == Some("stderr") })
        );
        assert!(
            outcomes
                .iter()
                .map(|outcome| outcome.stderr.len())
                .sum::<usize>()
                <= 1024
        );
    }
}
