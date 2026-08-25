use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Request {
    pub argv: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub working_directory: Option<String>,
    pub stdin: Vec<u8>,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Outcome {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub signal: Option<i32>,
}

pub(crate) trait Clock: Sync {
    fn elapsed(&self) -> Duration;
    fn wait(&self, duration: Duration);
}

pub(crate) struct SystemClock {
    start: std::time::Instant,
}

impl SystemClock {
    pub(crate) fn start() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub(crate) fn execute(root: &Path, request: Request) -> Result<Outcome, String> {
    let clock = SystemClock::start();
    execute_with_clock(root, request, &clock)
}

pub(crate) fn execute_with_clock(
    root: &Path,
    request: Request,
    clock: &dyn Clock,
) -> Result<Outcome, String> {
    if request.argv.first().is_none_or(String::is_empty) {
        return Err("process agent argv must not be empty".into());
    }
    if request.argv.iter().any(|value| value.contains('\0')) {
        return Err("process agent argv must not contain NUL".into());
    }
    if request.timeout_ms == 0 || request.timeout_ms > 86_400_000 {
        return Err("process agent timeout_ms must be between 1 and 86400000".into());
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
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
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
    let stdout_reader = std::thread::spawn(move || read_pipe(child_stdout, "stdout"));
    let stderr_reader = std::thread::spawn(move || read_pipe(child_stderr, "stderr"));
    let input = request.stdin;
    let stdin_writer = std::thread::spawn(move || -> Result<(), String> {
        use std::io::Write as _;
        match child_stdin.write_all(&input) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(format!("failed to write process stdin: {error}")),
        }
    });
    let deadline = Duration::from_millis(request.timeout_ms);
    let mut timed_out = false;
    let status = loop {
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
        clock.wait(Duration::from_millis(5));
    };
    stdin_writer
        .join()
        .map_err(|_| "process stdin writer panicked".to_owned())??;
    let stdout = stdout_reader
        .join()
        .map_err(|_| "process stdout reader panicked".to_owned())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "process stderr reader panicked".to_owned())??;
    let signal = exit_signal(&status);
    Ok(Outcome {
        exit_code: if timed_out { 124 } else { exit_code(&status) },
        stdout,
        stderr,
        timed_out,
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
    let candidate = root.join(relative);
    let metadata = candidate
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect process working directory {relative}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "process working directory must not be a symlink: {relative}"
        ));
    }
    if !metadata.file_type().is_dir() {
        return Err(format!(
            "process working directory is not a directory: {relative}"
        ));
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
    for name in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "ComSpec",
        "COMSPEC",
        "PATHEXT",
        "WINDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn read_pipe(mut pipe: impl std::io::Read, label: &str) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)
        .map_err(|error| format!("failed to read process {label}: {error}"))?;
    Ok(output)
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The child starts in its own process group, so a negative PID kills
        // descendants as well as the direct child.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

fn exit_code(status: &std::process::ExitStatus) -> i32 {
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

fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                timeout_ms: 5_000,
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
            timeout_ms: 1,
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
                timeout_ms: 5,
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
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        assert!(
            _resolve_directory(root.path(), Some("link"))
                .unwrap_err()
                .contains("symlink")
        );
    }
}
