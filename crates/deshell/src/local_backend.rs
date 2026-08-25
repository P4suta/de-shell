use crate::runner::{Backend, CapsuleRequest, ProcessRequest, ProcessResult};
use std::path::{Path, PathBuf};

pub(crate) struct LocalBackend {
    root: PathBuf,
    replay: Option<crate::replay::ReplayStore>,
}

impl LocalBackend {
    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|error| format!("cannot resolve project root {}: {error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!(
                "project root is not a directory: {}",
                root.display()
            ));
        }
        let replay_path = root.join(".deshell/replay.json");
        let replay = match replay_path.symlink_metadata() {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    return Err(format!(
                        "network replay store is not a regular non-symlink file: {}",
                        replay_path.display()
                    ));
                }
                let bytes = std::fs::read(&replay_path).map_err(|error| {
                    format!(
                        "cannot read network replay store {}: {error}",
                        replay_path.display()
                    )
                })?;
                Some(
                    crate::replay::ReplayStore::decode(&bytes)
                        .map_err(|errors| errors.join("; "))?,
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(format!(
                    "cannot inspect network replay store {}: {error}",
                    replay_path.display()
                ));
            }
        };
        Ok(Self { root, replay })
    }

    fn resolve_existing(&self, path: &str, expected_directory: bool) -> Result<PathBuf, String> {
        validate_path(path)?;
        let candidate = self.root.join(path);
        let metadata = candidate
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect project path {path}: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "filesystem effect may not target a symlink: {path}"
            ));
        }
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("cannot resolve project path {path}: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!("filesystem effect escapes project root: {path}"));
        }
        if expected_directory && !canonical.is_dir() {
            return Err(format!("working directory is not a directory: {path}"));
        }
        if !expected_directory && !canonical.is_file() {
            return Err(format!(
                "filesystem effect target is not a regular file: {path}"
            ));
        }
        Ok(canonical)
    }

    fn resolve_write(&self, path: &str) -> Result<PathBuf, String> {
        validate_path(path)?;
        let candidate = self.root.join(path);
        match candidate.symlink_metadata() {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    return Err(format!(
                        "file write target is not a regular non-symlink file: {path}"
                    ));
                }
                let canonical = candidate
                    .canonicalize()
                    .map_err(|error| format!("cannot resolve write target {path}: {error}"))?;
                if !canonical.starts_with(&self.root) {
                    return Err(format!("filesystem effect escapes project root: {path}"));
                }
                Ok(canonical)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = candidate
                    .parent()
                    .ok_or_else(|| format!("file write target has no parent: {path}"))?;
                let parent = parent
                    .canonicalize()
                    .map_err(|error| format!("cannot resolve write parent for {path}: {error}"))?;
                if !parent.starts_with(&self.root) {
                    return Err(format!("filesystem effect escapes project root: {path}"));
                }
                let filename = candidate
                    .file_name()
                    .ok_or_else(|| format!("file write target has no filename: {path}"))?;
                Ok(parent.join(filename))
            }
            Err(error) => Err(format!("cannot inspect write target {path}: {error}")),
        }
    }
}

impl Backend for LocalBackend {
    fn execute(&self, request: ProcessRequest) -> Result<ProcessResult, String> {
        let executable = request
            .argv
            .first()
            .filter(|value| !value.is_empty())
            .ok_or("cannot execute an empty argv")?;
        let directory = match &request.working_directory {
            Some(path) => self.resolve_existing(path, true)?,
            None => self.root.clone(),
        };
        let mut command = std::process::Command::new(executable);
        command
            .args(&request.argv[1..])
            .current_dir(directory)
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        add_essential_environment(&mut command);
        let mut seen = std::collections::BTreeSet::new();
        for (name, value) in &request.environment {
            if name.is_empty() || name.contains(['=', '\0']) {
                return Err(format!("invalid process environment name: {name:?}"));
            }
            let key = if cfg!(windows) {
                name.to_ascii_uppercase()
            } else {
                name.clone()
            };
            if !seen.insert(key) {
                return Err(format!("duplicate process environment variable: {name}"));
            }
            command.env(name, value);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("failed to start {executable}: {error}"))?;
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or("child stdin pipe is unavailable")?;
        let input = request.stdin;
        let writer = std::thread::spawn(move || -> Result<(), String> {
            use std::io::Write as _;
            child_stdin
                .write_all(&input)
                .map_err(|error| format!("failed to write child stdin: {error}"))
        });
        let output = child
            .wait_with_output()
            .map_err(|error| format!("failed to wait for {executable}: {error}"))?;
        writer
            .join()
            .map_err(|_| "child stdin writer panicked".to_owned())??;
        Ok(ProcessResult {
            exit_code: exit_code(&output.status),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn execute_capsule(&self, request: CapsuleRequest) -> Result<ProcessResult, String> {
        let suffix = match request.interpreter.to_ascii_lowercase().as_str() {
            "powershell" | "pwsh" => ".ps1",
            "cmd" => ".cmd",
            "nu" | "nushell" => ".nu",
            _ => ".sh",
        };
        let mut script = tempfile::Builder::new()
            .prefix(".deshell-capsule-")
            .suffix(suffix)
            .tempfile_in(&self.root)
            .map_err(|error| format!("cannot stage opaque capsule: {error}"))?;
        use std::io::Write as _;
        script
            .write_all(&request.source)
            .map_err(|error| format!("cannot write opaque capsule: {error}"))?;
        script
            .flush()
            .map_err(|error| format!("cannot flush opaque capsule: {error}"))?;
        script
            .as_file()
            .sync_all()
            .map_err(|error| format!("cannot sync opaque capsule: {error}"))?;
        let script = script.into_temp_path();
        let script_path = script.to_string_lossy().into_owned();
        let lower = request.interpreter.to_ascii_lowercase();
        let argv = match lower.as_str() {
            "powershell" => {
                let executable = if cfg!(windows) {
                    "powershell.exe"
                } else {
                    "pwsh"
                };
                [
                    executable,
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-File",
                    &script_path,
                ]
                .into_iter()
                .map(str::to_owned)
                .chain(request.arguments.clone())
                .collect()
            }
            "pwsh" => [
                "pwsh",
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-File",
                &script_path,
            ]
            .into_iter()
            .map(str::to_owned)
            .chain(request.arguments.clone())
            .collect(),
            "nu" | "nushell" => std::iter::once("nu".to_owned())
                .chain(std::iter::once(script_path.clone()))
                .chain(request.arguments.clone())
                .collect(),
            "cmd" => {
                if !cfg!(windows) {
                    return Err("cmd opaque capsules are unavailable on this platform".into());
                }
                let mut command_line = format!("call \"{}\"", script_path.replace('"', "\"\""));
                for argument in &request.arguments {
                    command_line.push(' ');
                    command_line.push_str(&quote_cmd(argument));
                }
                vec![
                    "cmd.exe".into(),
                    "/d".into(),
                    "/s".into(),
                    "/c".into(),
                    command_line,
                ]
            }
            "posix_sh" => std::iter::once("sh".to_owned())
                .chain(std::iter::once(script_path.clone()))
                .chain(request.arguments.clone())
                .collect(),
            other => std::iter::once(other.to_owned())
                .chain(std::iter::once(script_path.clone()))
                .chain(request.arguments.clone())
                .collect(),
        };
        let result = self.execute(ProcessRequest {
            argv,
            environment: request.environment,
            working_directory: None,
            stdin: request.stdin,
        });
        drop(script);
        result
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, String> {
        let path = self.resolve_existing(path, false)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let mut output = Vec::new();
        use std::io::Read as _;
        file.read_to_end(&mut output)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Ok(output)
    }

    fn write_file(&self, path: &str, contents: &[u8], append: bool) -> Result<(), String> {
        let path = self.resolve_write(path)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true);
        if append {
            options.append(true);
        } else {
            options.truncate(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        use std::io::Write as _;
        file.write_all(contents)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        file.flush()
            .map_err(|error| format!("cannot flush {}: {error}", path.display()))
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        let path = self.resolve_existing(path, false)?;
        std::fs::remove_file(&path)
            .map_err(|error| format!("cannot remove {}: {error}", path.display()))
    }

    fn network_request(&self, method: &str, uri: &str) -> Result<Vec<u8>, String> {
        match &self.replay {
            Some(replay) => replay.lookup(method, uri, b""),
            None => Err(format!(
                "network record/replay provider is unavailable for {uri}"
            )),
        }
    }
}

fn validate_path(path: &str) -> Result<(), String> {
    let normalized = crate::ir::normalize_path(path)?;
    if normalized != path {
        return Err(format!("filesystem path is not normalized: {path}"));
    }
    Ok(())
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

fn quote_cmd(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-./\\:".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn filesystem_effects_are_project_scoped() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(temporary.path()).unwrap();
        backend
            .write_file("nested/out.bin", b"one", false)
            .unwrap_err();
        fs::create_dir(temporary.path().join("nested")).unwrap();
        backend.write_file("nested/out.bin", b"one", false).unwrap();
        backend.write_file("nested/out.bin", b"-two", true).unwrap();
        assert_eq!(backend.read_file("nested/out.bin").unwrap(), b"one-two");
        backend.remove_file("nested/out.bin").unwrap();
        assert!(!temporary.path().join("nested/out.bin").exists());
        for path in ["../outside", "/tmp/outside", "a/../outside"] {
            assert!(backend.read_file(path).is_err(), "read accepted {path}");
            assert!(
                backend.write_file(path, b"bad", false).is_err(),
                "write accepted {path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_effects_reject_symlink_escape() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let backend = LocalBackend::new(root.path()).unwrap();
        assert!(
            backend
                .read_file("link/secret")
                .unwrap_err()
                .contains("escapes project root")
        );
        assert!(backend.write_file("link/new", b"bad", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn process_uses_exact_argv_raw_stdio_and_requested_project_cwd() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir(temporary.path().join("work")).unwrap();
        let backend = LocalBackend::new(temporary.path()).unwrap();
        let result = backend
            .execute(ProcessRequest {
                argv: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "pwd; cat".into(),
                    "ignored-$HOME".into(),
                ],
                environment: vec![],
                working_directory: Some("work".into()),
                stdin: vec![0, 1, 0xff],
            })
            .unwrap();
        assert_eq!(result.exit_code, 0);
        let newline = result
            .stdout
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap();
        assert_eq!(
            Path::new(std::str::from_utf8(&result.stdout[..newline]).unwrap())
                .canonicalize()
                .unwrap(),
            temporary.path().join("work").canonicalize().unwrap()
        );
        assert_eq!(&result.stdout[newline + 1..], &[0, 1, 0xff]);
    }

    #[cfg(unix)]
    #[test]
    fn capsule_executes_staged_source_and_cleans_it_up() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(temporary.path()).unwrap();
        let before: Vec<_> = fs::read_dir(temporary.path()).unwrap().collect();
        let result = backend
            .execute_capsule(CapsuleRequest {
                interpreter: "sh".into(),
                source: b"printf 'capsule:%s' \"$1\"".to_vec(),
                arguments: vec!["argument".into()],
                environment: vec![],
                stdin: vec![],
            })
            .unwrap();
        assert_eq!(result.stdout, b"capsule:argument");
        let after: Vec<_> = fs::read_dir(temporary.path()).unwrap().collect();
        assert_eq!(after.len(), before.len());
    }

    #[test]
    fn local_network_provider_is_explicitly_unavailable() {
        let temporary = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(temporary.path()).unwrap();
        assert!(
            backend
                .network_request("GET", "https://example.invalid")
                .unwrap_err()
                .contains("record/replay")
        );
    }

    #[test]
    fn pinned_replay_file_is_the_only_local_network_provider() {
        let temporary = tempfile::tempdir().unwrap();
        std::fs::create_dir(temporary.path().join(".deshell")).unwrap();
        let store = crate::replay::ReplayStore {
            schema_version: 1,
            entries: vec![crate::replay::ReplayEntry {
                method: "GET".into(),
                uri: "https://example.test/data".into(),
                request_body_sha256: crate::digest::sha256(b""),
                status: 200,
                headers: vec![],
                body: crate::ir::SourceBytes::from_bytes(&[0, 0xff]),
            }],
        };
        std::fs::write(
            temporary.path().join(".deshell/replay.json"),
            store.encode_pretty().unwrap(),
        )
        .unwrap();
        let backend = LocalBackend::new(temporary.path()).unwrap();
        assert_eq!(
            backend
                .network_request("GET", "https://example.test/data")
                .unwrap(),
            [0, 0xff]
        );
        assert!(
            backend
                .network_request("GET", "https://example.test/missing")
                .unwrap_err()
                .contains("replay miss")
        );
    }
}
