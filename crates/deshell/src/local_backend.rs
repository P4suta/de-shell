use crate::runner::{Backend, InterpreterRequest, ProcessRequest, ProcessResult};
use std::path::{Path, PathBuf};

pub(crate) struct LocalBackend {
    root: PathBuf,
    replay: Option<crate::replay::ReplayStore>,
    limits: crate::config::ResourceLimits,
    interpreter_pins: Option<crate::config::InterpreterPins>,
}

impl LocalBackend {
    pub(crate) fn for_validated_project(project: &crate::project::ValidatedProject) -> Self {
        Self {
            root: project.canonical_root.clone(),
            replay: project.replay.clone(),
            limits: project.config.limits,
            interpreter_pins: None,
        }
    }

    pub(crate) fn new(root: &Path) -> Result<Self, String> {
        let metadata = root
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect project root {}: {error}", root.display()))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(format!(
                "project root is not a regular non-symlink directory: {}",
                root.display()
            ));
        }
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
        let deshell_path = root.join(".deshell");
        if let Ok(metadata) = deshell_path.symlink_metadata()
            && (metadata.file_type().is_symlink() || !metadata.file_type().is_dir())
        {
            return Err(format!(
                "project metadata root is not a regular non-symlink directory: {}",
                deshell_path.display()
            ));
        }
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
        Ok(Self {
            root,
            replay,
            limits: crate::config::ResourceLimits::DEFAULT,
            interpreter_pins: None,
        })
    }

    pub(crate) fn with_limits(
        root: &Path,
        limits: crate::config::ResourceLimits,
    ) -> Result<Self, String> {
        let mut backend = Self::new(root)?;
        backend.limits = limits;
        Ok(backend)
    }

    pub(crate) fn with_pinned_interpreters(
        root: &Path,
        limits: crate::config::ResourceLimits,
        pins: crate::config::InterpreterPins,
    ) -> Result<Self, String> {
        let mut backend = Self::with_limits(root, limits)?;
        backend.interpreter_pins = Some(pins);
        Ok(backend)
    }

    fn resolve_existing(&self, path: &str, expected_directory: bool) -> Result<PathBuf, String> {
        validate_path(path)?;
        let mut candidate = self.root.clone();
        let components = path.split('/').collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            candidate.push(component);
            let metadata = candidate
                .symlink_metadata()
                .map_err(|error| format!("cannot inspect project path {path}: {error}"))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "filesystem effect path must not contain a symlink: {path}"
                ));
            }
            let final_component = index + 1 == components.len();
            if !final_component && !metadata.file_type().is_dir() {
                return Err(format!(
                    "filesystem effect parent is not a directory: {path}"
                ));
            }
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
        let (filename, parents) = path
            .rsplit_once('/')
            .map_or((path, None), |(parents, filename)| {
                (filename, Some(parents))
            });
        let mut parent = self.root.clone();
        if let Some(parents) = parents {
            for component in parents.split('/') {
                parent.push(component);
                let metadata = parent
                    .symlink_metadata()
                    .map_err(|error| format!("cannot inspect write parent for {path}: {error}"))?;
                if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                    return Err(format!(
                        "filesystem write parent must contain only regular directories: {path}"
                    ));
                }
            }
        }
        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| format!("cannot resolve write parent for {path}: {error}"))?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(format!("filesystem effect escapes project root: {path}"));
        }
        let candidate = canonical_parent.join(filename);
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(candidate),
            Err(error) => Err(format!("cannot inspect write target {path}: {error}")),
        }
    }
}

impl Backend for LocalBackend {
    fn execute(&self, request: ProcessRequest) -> Result<ProcessResult, String> {
        let outcome = crate::agent_process::execute(
            &self.root,
            crate::agent_process::Request {
                argv: request.argv,
                environment: request.environment,
                working_directory: request.working_directory,
                stdin: request.stdin,
                limits: self.limits.into(),
            },
        )?;
        if let Some(limit) = outcome.limit_exceeded {
            return Err(format!("limit_exceeded: {limit}"));
        }
        Ok(ProcessResult {
            exit_code: outcome.exit_code,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
        })
    }

    fn execute_pipeline(
        &self,
        requests: Vec<ProcessRequest>,
    ) -> Result<Vec<ProcessResult>, String> {
        let requests = requests
            .into_iter()
            .map(|request| crate::agent_process::Request {
                argv: request.argv,
                environment: request.environment,
                working_directory: request.working_directory,
                stdin: request.stdin,
                limits: self.limits.into(),
            })
            .collect();
        let outcomes = crate::agent_process::execute_pipeline(&self.root, requests)?;
        if let Some(limit) = outcomes
            .iter()
            .find_map(|outcome| outcome.limit_exceeded.as_deref())
        {
            return Err(format!("limit_exceeded: {limit}"));
        }
        Ok(outcomes
            .into_iter()
            .map(|outcome| ProcessResult {
                exit_code: outcome.exit_code,
                stdout: outcome.stdout,
                stderr: outcome.stderr,
            })
            .collect())
    }

    fn execute_interpreter(&self, request: InterpreterRequest) -> Result<ProcessResult, String> {
        let Some(pins) = &self.interpreter_pins else {
            return Err(format!(
                "pinned interpreter delegation for {} ({}) is unavailable in the local backend",
                request.interpreter, request.interpreter_pin
            ));
        };
        let interpreter = request.interpreter.to_ascii_lowercase();
        let expected_pin = match interpreter.as_str() {
            "sh" | "posix_sh" => &pins.posix_sh,
            "bash" => &pins.bash,
            "zsh" => &pins.zsh,
            "fish" => &pins.fish,
            "powershell" | "pwsh" => &pins.powershell,
            "cmd" | "cmd.exe" => &pins.cmd,
            "nu" | "nushell" => &pins.nushell,
            _ => {
                return Err(format!(
                    "unknown pinned interpreter: {}",
                    request.interpreter
                ));
            }
        };
        if expected_pin != &request.interpreter_pin {
            return Err(format!(
                "interpreter pin mismatch for {} (expected {}, found {})",
                request.interpreter, expected_pin, request.interpreter_pin
            ));
        }
        let directory = self.root.join(".deshell/runtime");
        match directory.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => return Err("delegation runtime path is not a regular directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&directory).map_err(|error| {
                    format!("cannot create delegation runtime directory: {error}")
                })?;
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect delegation runtime directory: {error}"
                ));
            }
        }
        let suffix = match interpreter.as_str() {
            "powershell" | "pwsh" => ".ps1",
            "cmd" | "cmd.exe" => ".cmd",
            "fish" => ".fish",
            "nu" | "nushell" => ".nu",
            _ => ".sh",
        };
        let mut source = tempfile::Builder::new()
            .prefix("delegated-")
            .suffix(suffix)
            .tempfile_in(&directory)
            .map_err(|error| format!("cannot stage delegated source: {error}"))?;
        use std::io::Write as _;
        source
            .write_all(&request.source)
            .and_then(|()| source.flush())
            .map_err(|error| format!("cannot stage delegated source: {error}"))?;
        let path = source
            .path()
            .to_str()
            .ok_or("delegated source path is not valid UTF-8")?
            .to_owned();
        let mut argv = match interpreter.as_str() {
            "sh" | "posix_sh" => vec!["sh".into(), path],
            "bash" | "zsh" | "fish" => vec![interpreter, path],
            "nu" | "nushell" => vec!["nu".into(), path],
            "powershell" | "pwsh" => vec![
                "pwsh".into(),
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                path,
            ],
            "cmd" | "cmd.exe" => vec![
                "cmd.exe".into(),
                "/d".into(),
                "/s".into(),
                "/c".into(),
                path,
            ],
            _ => unreachable!(),
        };
        argv.extend(request.arguments);
        let outcome = crate::agent_process::execute(
            &self.root,
            crate::agent_process::Request {
                argv,
                environment: request.environment,
                working_directory: None,
                stdin: request.stdin,
                limits: self.limits.into(),
            },
        )?;
        if let Some(limit) = outcome.limit_exceeded {
            return Err(format!("limit_exceeded: {limit}"));
        }
        Ok(ProcessResult {
            exit_code: outcome.exit_code,
            stdout: outcome.stdout,
            stderr: outcome.stderr,
        })
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
            Some(replay) => replay.lookup_prevalidated(method, uri, b""),
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
        fs::create_dir(root.path().join("inside")).unwrap();
        fs::write(root.path().join("inside/value"), b"inside").unwrap();
        fs::write(outside.path().join("secret"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink("inside", root.path().join("inside-link")).unwrap();
        let backend = LocalBackend::new(root.path()).unwrap();
        assert!(
            backend
                .read_file("link/secret")
                .unwrap_err()
                .contains("symlink")
        );
        assert!(backend.write_file("link/new", b"bad", false).is_err());
        assert!(backend.read_file("inside-link/value").is_err());
        assert!(
            backend
                .write_file("inside-link/new", b"bad", false)
                .is_err()
        );
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
