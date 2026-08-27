use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

static PYTHON_OS_SYSTEM: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"os\.system\s*\(\s*([^\)]*)\)"#).expect("static scanner regex")
});
static PYTHON_SUBPROCESS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"subprocess\.(?:run|call|Popen)\s*\(\s*([^,\)]*)"#)
        .expect("static scanner regex")
});
static JAVASCRIPT_EXEC: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(?:child_process\.)?(?:exec|execSync)\s*\(\s*([^\)]*)\)"#)
        .expect("static scanner regex")
});

pub(crate) const INVENTORY_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Inventory {
    pub schema_version: u32,
    pub findings: Vec<Finding>,
    pub skipped: Vec<Skipped>,
    pub errors: Vec<ScanError>,
}

impl std::ops::Deref for Inventory {
    type Target = [Finding];

    fn deref(&self) -> &Self::Target {
        &self.findings
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ByteSpan {
    pub start_byte: u64,
    pub end_byte: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InterpreterConfidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Skipped {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScanError {
    pub path: Option<String>,
    pub stage: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FindingKind {
    ShellFile,
    EmbeddedShell,
    Candidate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Finding {
    pub path: String,
    pub kind: FindingKind,
    pub interpreter: Option<String>,
    pub interpreter_confidence: InterpreterConfidence,
    pub locator: Option<String>,
    pub span: ByteSpan,
    pub content_digest: String,
    #[serde(skip)]
    pub source: Vec<u8>,
}

#[derive(Default)]
struct FileScan {
    findings: Vec<Finding>,
    skipped: Vec<Skipped>,
    errors: Vec<ScanError>,
}

pub(crate) fn scan(_root: &Path) -> Result<Inventory, String> {
    let metadata = _root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect scan root {}: {error}", _root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "scan root is not a regular non-symlink directory: {}",
            _root.display()
        ));
    }
    let root = _root
        .canonicalize()
        .map_err(|error| format!("cannot access scan root {}: {error}", _root.display()))?;
    if !root.is_dir() {
        return Err(format!("scan root is not a directory: {}", root.display()));
    }
    let InventoryWalk {
        files,
        errors: mut inventory_errors,
    } = inventory(&root)?;
    if files.is_empty() {
        inventory_errors.sort_by(scan_error_order);
        return Ok(Inventory {
            schema_version: INVENTORY_SCHEMA_VERSION,
            findings: Vec::new(),
            skipped: Vec::new(),
            errors: inventory_errors,
        });
    }

    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 8)
        .min(files.len());
    let results = if worker_count == 1 {
        vec![scan_files_sequential(&files)]
    } else {
        let next = AtomicUsize::new(0);
        let results = std::sync::Mutex::new(Vec::with_capacity(worker_count));
        std::thread::scope(|scope| {
            let files = &files;
            for _ in 0..worker_count {
                let next = &next;
                let results = &results;
                scope.spawn(move || {
                    let local = scan_files(files, next);
                    match results.lock() {
                        Ok(mut results) => results.push(local),
                        Err(poisoned) => poisoned.into_inner().push(local),
                    }
                });
            }
        });
        results
            .into_inner()
            .map_err(|_| "scanner result lock poisoned".to_owned())?
    };
    let mut findings = Vec::new();
    let mut skipped = Vec::new();
    let mut errors = inventory_errors;
    for result in results {
        findings.extend(result.findings);
        skipped.extend(result.skipped);
        errors.extend(result.errors);
    }
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.span.start_byte.cmp(&right.span.start_byte))
            .then_with(|| left.span.end_byte.cmp(&right.span.end_byte))
            .then_with(|| kind_order(&left.kind).cmp(&kind_order(&right.kind)))
            .then_with(|| left.locator.cmp(&right.locator))
    });
    skipped.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    errors.sort_by(scan_error_order);
    Ok(Inventory {
        schema_version: INVENTORY_SCHEMA_VERSION,
        findings,
        skipped,
        errors,
    })
}

fn scan_files(files: &[(String, PathBuf)], next: &AtomicUsize) -> FileScan {
    let mut local = FileScan::default();
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some((relative, absolute)) = files.get(index) else {
            break;
        };
        scan_file(&mut local, relative, absolute);
    }
    local
}

fn scan_files_sequential(files: &[(String, PathBuf)]) -> FileScan {
    let mut local = FileScan::default();
    for (relative, absolute) in files {
        scan_file(&mut local, relative, absolute);
    }
    local
}

fn scan_file(local: &mut FileScan, relative: &str, absolute: &Path) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        findings_for_file(relative, absolute)
    })) {
        Ok(result) => local.extend(result),
        Err(_) => local.errors.push(ScanError {
            path: Some(relative.into()),
            stage: "worker".into(),
            message: "scanner worker panicked while inspecting file".into(),
        }),
    }
}

impl FileScan {
    fn extend(&mut self, mut other: Self) {
        self.findings.append(&mut other.findings);
        self.skipped.append(&mut other.skipped);
        self.errors.append(&mut other.errors);
    }
}

fn scan_error_order(left: &ScanError, right: &ScanError) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.stage.cmp(&right.stage))
        .then_with(|| left.message.cmp(&right.message))
}

const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".deshell",
    "_build",
    "_opam",
    "target",
    "node_modules",
    "vendor",
];

// A plain filesystem walk has no ignore metadata. Conventional generated
// build trees are excluded there, while a Git-backed inventory still honors a
// deliberately tracked `build/` directory.
const WALK_IGNORED_DIRECTORIES: &[&str] = &["build"];

struct InventoryWalk {
    files: Vec<(String, PathBuf)>,
    errors: Vec<ScanError>,
}

fn inventory(root: &Path) -> Result<InventoryWalk, String> {
    let mut errors = Vec::new();
    if valid_git_marker(root) {
        match git_inventory(root) {
            Ok(files) => return Ok(InventoryWalk { files, errors }),
            Err(message) => errors.push(ScanError {
                path: None,
                stage: "inventory".into(),
                message,
            }),
        }
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || (!IGNORED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref())
                    && !WALK_IGNORED_DIRECTORIES
                        .contains(&entry.file_name().to_string_lossy().as_ref()))
        })
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(ScanError {
                    path: error
                        .path()
                        .and_then(|path| path.strip_prefix(root).ok())
                        .map(normalized_lossy_path),
                    stage: "walk".into(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            errors.push(ScanError {
                path: Some(normalized_lossy_path(entry.path())),
                stage: "walk".into(),
                message: "walked path is outside the scan root".into(),
            });
            continue;
        };
        let Some(relative) = relative.to_str() else {
            errors.push(ScanError {
                path: Some(normalized_lossy_path(relative)),
                stage: "path_encoding".into(),
                message: "path is not valid UTF-8 and cannot be represented by Inventory v1".into(),
            });
            continue;
        };
        files.push((relative.replace('\\', "/"), entry.path().to_path_buf()));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(InventoryWalk { files, errors })
}

fn valid_git_marker(root: &Path) -> bool {
    let marker = root.join(".git");
    marker.join("HEAD").is_file() || marker.is_file()
}

fn git_inventory(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
        ])
        .output()
        .map_err(|error| format!("cannot run git inventory: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git inventory failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut files = Vec::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        let relative = std::str::from_utf8(raw)
            .map_err(|_| "git inventory returned a non-UTF-8 path".to_owned())?
            .replace('\\', "/");
        if relative
            .split('/')
            .any(|part| IGNORED_DIRECTORIES.contains(&part))
        {
            continue;
        }
        let absolute = root.join(&relative);
        let metadata = absolute
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect git inventory path {relative}: {error}"))?;
        if metadata.file_type().is_file() {
            files.push((relative, absolute));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn findings_for_file(relative: &str, absolute: &Path) -> FileScan {
    let metadata = match absolute.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            return FileScan::error(relative, "metadata", error.to_string());
        }
    };
    if !metadata.file_type().is_file() {
        return FileScan::skipped(relative, "not_regular_file");
    }
    let lower = relative.to_ascii_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    if ignored_structured_host(filename) {
        return FileScan::default();
    }
    let known_structured_host = is_known_structured_host(&lower, filename);
    let potential_structured_host = is_structured_extension(&lower);
    let extension_interpreter = extension_interpreter(&lower);
    let path_is_relevant = extension_interpreter.is_some()
        || is_makefile(&lower, filename)
        || is_dockerfile(&lower, filename)
        || filename == "package.json"
        || known_structured_host
        || is_language_host(&lower);
    if metadata.len() > 4 * 1024 * 1024 {
        return if path_is_relevant {
            FileScan::skipped(relative, "size_limit_exceeded")
        } else {
            FileScan::default()
        };
    }
    let source = match std::fs::read(absolute) {
        Ok(source) => source,
        Err(error) => return FileScan::error(relative, "read", error.to_string()),
    };
    if let Some(interpreter) = extension_interpreter {
        return FileScan::findings(vec![finding(
            relative,
            FindingKind::ShellFile,
            Some(interpreter.into()),
            InterpreterConfidence::High,
            None,
            ByteSpan::whole(&source),
            source,
        )]);
    }
    let detected = crate::frontend::detect(relative, &source);
    if !matches!(detected, crate::frontend::Interpreter::Unknown(_)) {
        let interpreter = detected.name().to_owned();
        return FileScan::findings(vec![finding(
            relative,
            FindingKind::ShellFile,
            Some(interpreter),
            InterpreterConfidence::High,
            None,
            ByteSpan::whole(&source),
            source,
        )]);
    }
    if !path_is_relevant && !potential_structured_host {
        return FileScan::default();
    }
    let text = match std::str::from_utf8(&source) {
        Ok(text) => text,
        Err(_) if path_is_relevant => {
            return FileScan::skipped(relative, "unsupported_encoding");
        }
        Err(_) => return FileScan::default(),
    };
    if filename == "package.json" {
        return match package_findings(relative, text) {
            Ok(findings) => FileScan::findings(findings),
            Err(message) => FileScan::error(relative, "parse_json", message),
        };
    }
    if is_makefile(&lower, filename) {
        return FileScan::findings(makefile_findings(relative, text));
    }
    if is_dockerfile(&lower, filename) {
        return match dockerfile_findings(relative, text) {
            Ok(findings) => FileScan::findings(findings),
            Err(message) => FileScan::error(relative, "parse_dockerfile", message),
        };
    }
    if lower.ends_with(".yml") || lower.ends_with(".yaml") {
        if !known_structured_host && !yaml_shell_hint(text) {
            return FileScan::default();
        }
        return match yaml_findings(relative, text, &lower) {
            Ok(findings) => FileScan::findings(findings),
            Err(message) => FileScan::error(relative, "parse_yaml", message),
        };
    }
    if lower.ends_with(".json") || lower.ends_with(".jsonc") {
        if !known_structured_host && !json_shell_hint(text) {
            return FileScan::default();
        }
        return match json_candidate_findings(relative, text) {
            Ok(findings) => FileScan::findings(findings),
            Err(message) => FileScan::error(relative, "parse_json", message),
        };
    }
    if lower.ends_with(".toml") {
        if !known_structured_host && !toml_shell_hint(text) {
            return FileScan::default();
        }
        return match toml_candidate_findings(relative, text) {
            Ok(findings) => FileScan::findings(findings),
            Err(message) => FileScan::error(relative, "parse_toml", message),
        };
    }
    FileScan::findings(host_findings(relative, text, &lower))
}

fn ignored_structured_host(filename: &str) -> bool {
    [
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "pnpm-lock.yml",
    ]
    .contains(&filename)
}

fn is_makefile(lower: &str, filename: &str) -> bool {
    filename == "makefile" || filename == "gnumakefile" || lower.ends_with(".mk")
}

fn is_dockerfile(lower: &str, filename: &str) -> bool {
    filename == "dockerfile"
        || filename.starts_with("dockerfile.")
        || lower.ends_with(".dockerfile")
}

fn is_structured_extension(lower: &str) -> bool {
    lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".json")
        || lower.ends_with(".jsonc")
        || lower.ends_with(".toml")
}

fn is_known_structured_host(lower: &str, filename: &str) -> bool {
    if !is_structured_extension(lower) {
        return false;
    }
    if lower.starts_with(".github/workflows/")
        || lower.starts_with(".github/actions/")
        || lower.starts_with(".circleci/")
        || lower.starts_with(".devcontainer/")
        || lower.starts_with(".vscode/tasks.")
        || lower.starts_with(".vscode/launch.")
    {
        return true;
    }
    [
        ".gitlab-ci.yml",
        ".gitlab-ci.yaml",
        "azure-pipelines.yml",
        "azure-pipelines.yaml",
        "compose.yml",
        "compose.yaml",
        "docker-compose.yml",
        "docker-compose.yaml",
        "makefile.toml",
        "mise.toml",
        ".mise.toml",
        "taskfile.yml",
        "taskfile.yaml",
        "tasks.json",
        "tasks.toml",
        "deno.json",
        "deno.jsonc",
        "turbo.json",
    ]
    .contains(&filename)
        || filename.contains("workflow")
        || filename.contains("pipeline")
        || filename.contains("lefthook")
}

fn yaml_shell_hint(source: &str) -> bool {
    source.lines().any(|line| {
        let key = line
            .trim_start()
            .trim_start_matches("- ")
            .split_once(':')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        executable_field(key)
    })
}

fn json_shell_hint(source: &str) -> bool {
    [
        "\"automation\"",
        "\"cmd\"",
        "\"command\"",
        "\"commands\"",
        "\"exec\"",
        "\"execute\"",
        "\"hook\"",
        "\"hooks\"",
        "\"run\"",
        "\"script\"",
        "\"scripts\"",
        "\"shell\"",
    ]
    .iter()
    .any(|name| source.contains(name))
}

fn toml_shell_hint(source: &str) -> bool {
    source.lines().any(|line| {
        line.split_once('=')
            .map(|(key, _)| {
                executable_field(
                    key.trim()
                        .trim_matches(['\'', '"'])
                        .rsplit('.')
                        .next()
                        .unwrap_or(""),
                )
            })
            .unwrap_or(false)
    })
}

fn is_language_host(lower: &str) -> bool {
    [".py", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

impl FileScan {
    fn findings(findings: Vec<Finding>) -> Self {
        Self {
            findings,
            ..Self::default()
        }
    }

    fn skipped(path: &str, reason: &str) -> Self {
        Self {
            skipped: vec![Skipped {
                path: path.into(),
                reason: reason.into(),
            }],
            ..Self::default()
        }
    }

    fn error(path: &str, stage: &str, message: String) -> Self {
        Self {
            errors: vec![ScanError {
                path: Some(path.into()),
                stage: stage.into(),
                message,
            }],
            ..Self::default()
        }
    }
}

impl ByteSpan {
    fn whole(source: &[u8]) -> Self {
        Self {
            start_byte: 0,
            end_byte: source.len() as u64,
        }
    }
}

fn normalized_lossy_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn extension_interpreter(path: &str) -> Option<&'static str> {
    let value = if path.ends_with(".sh") {
        "sh"
    } else if path.ends_with(".bash") {
        "bash"
    } else if path.ends_with(".zsh") {
        "zsh"
    } else if path.ends_with(".fish") {
        "fish"
    } else if path.ends_with(".ps1") || path.ends_with(".psm1") {
        "powershell"
    } else if path.ends_with(".cmd") || path.ends_with(".bat") {
        "cmd"
    } else if path.ends_with(".nu") {
        "nu"
    } else {
        return None;
    };
    Some(value)
}

fn finding(
    path: &str,
    kind: FindingKind,
    interpreter: Option<String>,
    interpreter_confidence: InterpreterConfidence,
    locator: Option<String>,
    span: ByteSpan,
    source: Vec<u8>,
) -> Finding {
    Finding {
        path: path.to_owned(),
        kind,
        interpreter,
        interpreter_confidence,
        locator,
        span,
        content_digest: crate::digest::sha256(&source),
        source,
    }
}

fn span_of(source: &str, value: &str) -> ByteSpan {
    source.find(value).map_or_else(
        || ByteSpan::whole(source.as_bytes()),
        |start| ByteSpan {
            start_byte: start as u64,
            end_byte: (start + value.len()) as u64,
        },
    )
}

fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

fn package_findings(path: &str, source: &str) -> Result<Vec<Finding>, String> {
    let value = crate::strict_json::parse_host(source.as_bytes())
        .map_err(|error| format!("malformed package.json: {error}"))?;
    let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
        return Ok(Vec::new());
    };
    Ok(scripts
        .iter()
        .filter_map(|(name, value)| {
            value.as_str().map(|script| {
                finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("package-shell".into()),
                    InterpreterConfidence::Medium,
                    Some(format!("scripts.{name}")),
                    span_of(source, script),
                    script.as_bytes().to_vec(),
                )
            })
        })
        .collect())
}

fn makefile_findings(path: &str, source: &str) -> Vec<Finding> {
    let offsets = line_offsets(source);
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            line.strip_prefix('\t').map(|command| {
                let start = offsets[index] + 1;
                finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("sh".into()),
                    InterpreterConfidence::High,
                    Some(format!("recipe:{}", index + 1)),
                    ByteSpan {
                        start_byte: start as u64,
                        end_byte: (start + command.len()) as u64,
                    },
                    command.as_bytes().to_vec(),
                )
            })
        })
        .collect()
}

fn dockerfile_findings(path: &str, source: &str) -> Result<Vec<Finding>, String> {
    let mut findings = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let offsets = line_offsets(source);
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed
            .as_bytes()
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"RUN "))
        {
            let first_index = index;
            let mut command = trimmed[4..].trim().to_owned();
            let line = index + 1;
            while command.ends_with('\\') && index + 1 < lines.len() {
                command.pop();
                command = format!("{} {}", command.trim_end(), lines[index + 1].trim());
                index += 1;
            }
            if command.trim_start().starts_with('[') {
                let argv = serde_json::from_str::<Vec<String>>(&command).map_err(|error| {
                    format!("invalid JSON-form RUN instruction on line {line}: {error}")
                })?;
                if argv.is_empty() {
                    return Err(format!(
                        "JSON-form RUN instruction on line {line} must not be empty"
                    ));
                }
            } else {
                findings.push(finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("sh".into()),
                    InterpreterConfidence::High,
                    Some(format!("RUN:{line}")),
                    ByteSpan {
                        start_byte: offsets[first_index] as u64,
                        end_byte: (offsets[index] + lines[index].len()) as u64,
                    },
                    command.into_bytes(),
                ));
            }
        }
        index += 1;
    }
    Ok(findings)
}

fn yaml_findings(path: &str, source: &str, lower: &str) -> Result<Vec<Finding>, String> {
    for document in serde_yaml_ng::Deserializer::from_str(source) {
        serde_yaml_ng::Value::deserialize(document)
            .map_err(|error| format!("malformed YAML: {error}"))?;
    }
    let known = lower.starts_with(".github/workflows/")
        || lower.starts_with(".github/actions/")
        || lower == ".gitlab-ci.yml"
        || lower == ".gitlab-ci.yaml"
        || lower == "azure-pipelines.yml"
        || lower == "azure-pipelines.yaml"
        || lower.starts_with(".circleci/");
    let mut findings = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let offsets = line_offsets(source);
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let trimmed = raw.trim_start().trim_start_matches("- ");
        let Some((key, raw_value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let raw_key = key.trim();
        let executable = [
            "run",
            "script",
            "before_script",
            "after_script",
            "bash",
            "pwsh",
            "powershell",
            "command",
        ]
        .iter()
        .any(|key| raw_key.eq_ignore_ascii_case(key));
        if !executable {
            index += 1;
            continue;
        }
        let key = raw_key.to_ascii_lowercase();
        let interpreter = if key == "pwsh" || key == "powershell" {
            "powershell"
        } else if key == "bash" {
            "bash"
        } else if key == "run" && known {
            yaml_run_interpreter(&lines, index)
        } else {
            "sh"
        };
        let value = strip_quotes(raw_value.trim());
        let line = index + 1;
        if ["|", "|-", "|+", ">", ">-", ">+"].contains(&value.as_str()) {
            let indentation = raw.len() - raw.trim_start().len();
            let style = value.clone();
            let mut block = Vec::new();
            index += 1;
            while index < lines.len() {
                let current = lines[index];
                let current_indent = current.len() - current.trim_start().len();
                if !current.trim().is_empty() && current_indent <= indentation {
                    break;
                }
                block.push(current.trim_start());
                index += 1;
            }
            let command = yaml_scalar(&block, &style);
            if !command.trim().is_empty() {
                findings.push(finding(
                    path,
                    if known {
                        FindingKind::EmbeddedShell
                    } else {
                        FindingKind::Candidate
                    },
                    Some(interpreter.into()),
                    if known {
                        InterpreterConfidence::High
                    } else {
                        InterpreterConfidence::Low
                    },
                    Some(format!("{key}:{line}")),
                    ByteSpan {
                        start_byte: offsets[line - 1] as u64,
                        end_byte: if index < offsets.len() {
                            offsets[index].saturating_sub(1) as u64
                        } else {
                            source.len() as u64
                        },
                    },
                    command.into_bytes(),
                ));
            }
            continue;
        }
        if !value.is_empty() && (known || looks_like_shell(&value)) {
            findings.push(finding(
                path,
                if known {
                    FindingKind::EmbeddedShell
                } else {
                    FindingKind::Candidate
                },
                Some((*interpreter).into()),
                if known {
                    InterpreterConfidence::High
                } else {
                    InterpreterConfidence::Low
                },
                Some(format!("{key}:{line}")),
                span_of(source, &value),
                value.into_bytes(),
            ));
        }
        index += 1;
    }
    Ok(findings)
}

fn yaml_scalar(lines: &[&str], style: &str) -> String {
    let folded = style.starts_with('>');
    let strip = style.ends_with('-');
    let keep = style.ends_with('+');
    let mut output = if folded {
        let mut output = String::new();
        let mut previous_blank = false;
        for line in lines {
            let blank = line.is_empty();
            if !output.is_empty() {
                if blank || previous_blank {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
            }
            output.push_str(line);
            previous_blank = blank;
        }
        output
    } else {
        lines.join("\n")
    };
    if !strip && (!output.is_empty() || keep) {
        output.push('\n');
    }
    output
}

fn yaml_run_interpreter(lines: &[&str], run_index: usize) -> &'static str {
    if let Some(shell) = yaml_step_shell(lines, run_index) {
        return yaml_shell_name(shell);
    }
    let (job_start, job_end) = yaml_job_range(lines, run_index);
    if let Some(shell) = yaml_defaults_shell(lines, job_start, job_end) {
        return yaml_shell_name(shell);
    }
    if let Some(shell) = yaml_defaults_shell(lines, 0, lines.len()) {
        return yaml_shell_name(shell);
    }
    if lines[job_start..job_end].iter().any(|line| {
        let value = line.trim().to_ascii_lowercase();
        value.starts_with("runs-on:") && value.contains("windows")
    }) {
        "powershell"
    } else {
        "bash"
    }
}

fn yaml_shell_name(value: &str) -> &'static str {
    let executable = value
        .trim()
        .trim_matches(['\'', '"'])
        .split_ascii_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches("{0}");
    if executable.eq_ignore_ascii_case("pwsh") || executable.eq_ignore_ascii_case("powershell") {
        "powershell"
    } else if executable.eq_ignore_ascii_case("cmd") || executable.eq_ignore_ascii_case("cmd.exe") {
        "cmd"
    } else if executable.eq_ignore_ascii_case("fish") {
        "fish"
    } else if executable.eq_ignore_ascii_case("nu") || executable.eq_ignore_ascii_case("nushell") {
        "nu"
    } else if executable.eq_ignore_ascii_case("zsh") {
        "zsh"
    } else if executable.eq_ignore_ascii_case("bash") {
        "bash"
    } else {
        "sh"
    }
}

fn yaml_step_shell<'a>(lines: &'a [&str], run_index: usize) -> Option<&'a str> {
    let run_indent = indentation(lines[run_index]);
    let mut start = run_index;
    while start > 0 {
        let candidate = lines[start].trim_start();
        if indentation(lines[start]) == run_indent && candidate.starts_with("- ") {
            break;
        }
        if indentation(lines[start]) < run_indent {
            break;
        }
        start -= 1;
    }
    let step_indent = indentation(lines[start]);
    let mut end = run_index + 1;
    while end < lines.len() {
        let candidate = lines[end].trim_start();
        if indentation(lines[end]) == step_indent && candidate.starts_with("- ") {
            break;
        }
        if !candidate.is_empty() && indentation(lines[end]) < step_indent {
            break;
        }
        end += 1;
    }
    lines[start..end].iter().find_map(|line| {
        let trimmed = line.trim_start().trim_start_matches("- ");
        trimmed
            .split_once(':')
            .filter(|(key, _)| key.trim().eq_ignore_ascii_case("shell"))
            .map(|(_, value)| value.trim())
    })
}

fn yaml_job_range(lines: &[&str], run_index: usize) -> (usize, usize) {
    let mut jobs_indent = None;
    for (index, line) in lines.iter().take(run_index + 1).enumerate() {
        if line.trim() == "jobs:" {
            jobs_indent = Some((index, indentation(line)));
        }
    }
    let Some((jobs_index, jobs_indent)) = jobs_indent else {
        return (0, lines.len());
    };
    let job_indent = jobs_indent + 2;
    let mut start = jobs_index + 1;
    for (index, line) in lines
        .iter()
        .enumerate()
        .take(run_index + 1)
        .skip(jobs_index + 1)
    {
        let trimmed = line.trim();
        if indentation(line) == job_indent && trimmed.ends_with(':') {
            start = index;
        }
    }
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate().skip(run_index + 1) {
        if !line.trim().is_empty() && indentation(line) <= job_indent {
            end = index;
            break;
        }
    }
    (start, end)
}

fn yaml_defaults_shell<'a>(lines: &'a [&str], start: usize, end: usize) -> Option<&'a str> {
    let mut defaults_indent = None;
    let mut run_indent = None;
    for line in &lines[start..end] {
        let trimmed = line.trim();
        let indent = indentation(line);
        if trimmed == "defaults:" && (start != 0 || indent == 0) {
            defaults_indent = Some(indent);
            run_indent = None;
            continue;
        }
        let Some(defaults) = defaults_indent else {
            continue;
        };
        if !trimmed.is_empty() && indent <= defaults {
            defaults_indent = None;
            run_indent = None;
            continue;
        }
        if trimmed == "run:" && indent > defaults {
            run_indent = Some(indent);
            continue;
        }
        if let Some(run) = run_indent
            && indent > run
            && let Some((key, value)) = trimmed.split_once(':')
            && key.trim().eq_ignore_ascii_case("shell")
        {
            return Some(value.trim());
        }
    }
    None
}

fn indentation(line: &str) -> usize {
    line.as_bytes()
        .iter()
        .take_while(|byte| **byte == b' ' || **byte == b'\t')
        .count()
}

fn json_candidate_findings(path: &str, source: &str) -> Result<Vec<Finding>, String> {
    let normalized = normalize_jsonc(source)?;
    let value = crate::strict_json::parse_host(&normalized)
        .map_err(|error| format!("malformed JSON: {error}"))?;
    let mut output = Vec::new();
    collect_json_candidates(path, source, "$", false, &value, &mut output);
    Ok(output)
}

fn normalize_jsonc(source: &str) -> Result<Vec<u8>, String> {
    let input = source.as_bytes();
    let mut output = input.to_vec();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < input.len() {
        let byte = input[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            index += 1;
            continue;
        }
        if byte == b'/' && input.get(index + 1) == Some(&b'/') {
            output[index] = b' ';
            output[index + 1] = b' ';
            index += 2;
            while index < input.len() && input[index] != b'\n' && input[index] != b'\r' {
                output[index] = b' ';
                index += 1;
            }
            continue;
        }
        if byte == b'/' && input.get(index + 1) == Some(&b'*') {
            let start = index;
            output[index] = b' ';
            output[index + 1] = b' ';
            index += 2;
            let mut closed = false;
            while index < input.len() {
                if input[index] == b'*' && input.get(index + 1) == Some(&b'/') {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    index += 2;
                    closed = true;
                    break;
                }
                if input[index] != b'\n' && input[index] != b'\r' {
                    output[index] = b' ';
                }
                index += 1;
            }
            if !closed {
                return Err(format!(
                    "malformed JSONC: unterminated block comment at byte {start}"
                ));
            }
            continue;
        }
        index += 1;
    }

    index = 0;
    in_string = false;
    escaped = false;
    while index < output.len() {
        let byte = output[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b',' {
            let mut next = index + 1;
            while output
                .get(next)
                .is_some_and(|value| value.is_ascii_whitespace())
            {
                next += 1;
            }
            if matches!(output.get(next), Some(b']' | b'}')) {
                output[index] = b' ';
            }
        }
        index += 1;
    }
    Ok(output)
}

fn collect_json_candidates(
    path: &str,
    source: &str,
    locator: &str,
    executable: bool,
    value: &serde_json::Value,
    output: &mut Vec<Finding>,
) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                collect_json_candidates(
                    path,
                    source,
                    &format!("{locator}.{name}"),
                    executable || executable_field(name),
                    value,
                    output,
                );
            }
        }
        serde_json::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_json_candidates(
                    path,
                    source,
                    &format!("{locator}[{index}]"),
                    executable,
                    value,
                    output,
                );
            }
        }
        serde_json::Value::String(command) if executable && looks_like_shell(command) => output
            .push(finding(
                path,
                FindingKind::Candidate,
                None,
                InterpreterConfidence::Low,
                Some(locator.into()),
                span_of(source, command),
                command.as_bytes().to_vec(),
            )),
        _ => {}
    }
}

fn toml_candidate_findings(path: &str, source: &str) -> Result<Vec<Finding>, String> {
    let value = toml::from_str::<toml::Value>(source)
        .map_err(|error| format!("malformed TOML: {error}"))?;
    let mut output = Vec::new();
    collect_toml_candidates(path, source, "$", false, &value, &mut output);
    Ok(output)
}

fn collect_toml_candidates(
    path: &str,
    source: &str,
    locator: &str,
    executable: bool,
    value: &toml::Value,
    output: &mut Vec<Finding>,
) {
    match value {
        toml::Value::Table(fields) => {
            for (name, value) in fields {
                collect_toml_candidates(
                    path,
                    source,
                    &format!("{locator}.{name}"),
                    executable || executable_field(name),
                    value,
                    output,
                );
            }
        }
        toml::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_toml_candidates(
                    path,
                    source,
                    &format!("{locator}[{index}]"),
                    executable,
                    value,
                    output,
                );
            }
        }
        toml::Value::String(command) if executable && looks_like_shell(command) => {
            output.push(finding(
                path,
                FindingKind::Candidate,
                None,
                InterpreterConfidence::Low,
                Some(locator.into()),
                span_of(source, command),
                command.as_bytes().to_vec(),
            ));
        }
        _ => {}
    }
}

fn host_findings(path: &str, source: &str, lower: &str) -> Vec<Finding> {
    let offsets = line_offsets(source);
    let mut output = Vec::new();
    if lower.ends_with(".py") {
        append_host_findings(&mut output, path, source, &offsets, &PYTHON_OS_SYSTEM, "sh");
        append_host_findings(
            &mut output,
            path,
            source,
            &offsets,
            &PYTHON_SUBPROCESS,
            "sh",
        );
    } else if [".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    {
        append_host_findings(&mut output, path, source, &offsets, &JAVASCRIPT_EXEC, "sh");
    }
    output
}

fn append_host_findings(
    output: &mut Vec<Finding>,
    path: &str,
    source: &str,
    line_offsets: &[usize],
    regex: &regex::Regex,
    interpreter: &str,
) {
    for capture in regex.captures_iter(source) {
        let whole = capture.get(0).expect("host regex has a whole match");
        let argument = capture
            .get(1)
            .expect("host regex has an argument capture")
            .as_str()
            .trim();
        let quoted = quoted_literal(argument);
        let confidence = if quoted.is_some() {
            InterpreterConfidence::High
        } else {
            InterpreterConfidence::Low
        };
        let (kind, command) = quoted
            .map(|value| (FindingKind::EmbeddedShell, value))
            .unwrap_or_else(|| (FindingKind::Candidate, argument.to_owned()));
        let line_index = line_offsets
            .partition_point(|offset| *offset <= whole.start())
            .saturating_sub(1);
        let line_start = line_offsets[line_index];
        let line = line_index + 1;
        let column = source[line_start..whole.start()].chars().count();
        output.push(finding(
            path,
            kind,
            Some(interpreter.into()),
            confidence,
            Some(format!("line:{line}:column:{column}")),
            ByteSpan {
                start_byte: whole.start() as u64,
                end_byte: whole.end() as u64,
            },
            command.into_bytes(),
        ));
    }
}

fn quoted_literal(value: &str) -> Option<String> {
    if value.len() < 2 {
        return None;
    }
    let first = value.as_bytes()[0];
    let last = *value.as_bytes().last()?;
    if (first == b'\'' || first == b'"') && first == last {
        let inner = &value[1..value.len() - 1];
        if inner.contains(first as char) {
            None
        } else {
            Some(inner.to_owned())
        }
    } else {
        None
    }
}

fn strip_quotes(value: &str) -> String {
    quoted_literal(value).unwrap_or_else(|| value.trim().to_owned())
}

fn executable_field(name: &str) -> bool {
    let name = name.trim();
    [
        "automation",
        "cmd",
        "command",
        "commands",
        "exec",
        "execute",
        "hook",
        "hooks",
        "run",
        "script",
        "scripts",
        "shell",
    ]
    .iter()
    .any(|field| name.eq_ignore_ascii_case(field))
        || ["command", "script", "hook"]
            .iter()
            .any(|suffix| ends_with_ignore_ascii_case(name, suffix))
}

fn looks_like_shell(value: &str) -> bool {
    let first = value.split_ascii_whitespace().next().unwrap_or("");
    [
        "bash", "cat", "cd", "chmod", "cmd", "cp", "curl", "echo", "env", "exec", "fish", "git",
        "mkdir", "mv", "nu", "printf", "pwsh", "rm", "sh", "test", "wget", "zsh",
    ]
    .iter()
    .any(|command| first.eq_ignore_ascii_case(command))
        || ["&&", "||", "$(", "${", " | ", " > "]
            .iter()
            .any(|marker| value.contains(marker))
}

fn ends_with_ignore_ascii_case(value: &str, suffix: &str) -> bool {
    value
        .len()
        .checked_sub(suffix.len())
        .and_then(|start| value.get(start..))
        .is_some_and(|ending| ending.eq_ignore_ascii_case(suffix))
}

fn kind_order(kind: &FindingKind) -> u8 {
    match kind {
        FindingKind::ShellFile => 0,
        FindingKind::EmbeddedShell => 1,
        FindingKind::Candidate => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn inventories_shell_extensions_and_shebangs_in_stable_order() {
        let temporary = tempfile::tempdir().unwrap();
        write(temporary.path(), "z.ps1", b"& 'tool.exe'\n");
        write(temporary.path(), "a", b"#!/usr/bin/env bash\nprintf ok\n");
        write(temporary.path(), "nested/b.fish", b"command printf ok\n");
        let findings = scan(temporary.path()).unwrap();
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            ["a", "nested/b.fish", "z.ps1"]
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.kind == FindingKind::ShellFile)
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.content_digest.len() == 64)
        );
    }

    #[test]
    fn ignores_build_vendor_and_symlink_boundaries() {
        let temporary = tempfile::tempdir().unwrap();
        write(temporary.path(), "visible.sh", b"printf visible\n");
        write(temporary.path(), ".git/hidden.sh", b"printf hidden\n");
        write(
            temporary.path(),
            "node_modules/hidden.sh",
            b"printf hidden\n",
        );
        write(temporary.path(), "build/hidden.sh", b"printf hidden\n");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("visible.sh", temporary.path().join("linked.sh")).unwrap();
        }
        let findings = scan(temporary.path()).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "visible.sh");
    }

    #[cfg(unix)]
    #[test]
    fn scan_root_itself_must_not_be_a_symlink() {
        let parent = tempfile::tempdir().unwrap();
        let project = parent.path().join("project");
        std::fs::create_dir(&project).unwrap();
        write(&project, "build.sh", b"true\n");
        let alias = parent.path().join("alias");
        std::os::unix::fs::symlink(&project, &alias).unwrap();

        assert!(scan(&alias).unwrap_err().contains("non-symlink"));
    }

    #[test]
    fn discovers_structured_and_build_file_shell_locations() {
        let temporary = tempfile::tempdir().unwrap();
        write(temporary.path(), "Makefile", b"build:\n\tprintf make\n");
        write(
            temporary.path(),
            "Dockerfile",
            b"FROM scratch\nRUN printf docker\n",
        );
        write(
            temporary.path(),
            "package.json",
            br#"{"scripts":{"test":"printf package"}}"#,
        );
        write(
            temporary.path(),
            ".github/workflows/ci.yml",
            b"jobs:\n  test:\n    steps:\n      - run: printf workflow\n",
        );
        let findings = scan(temporary.path()).unwrap();
        let sources: Vec<String> = findings
            .iter()
            .map(|finding| String::from_utf8(finding.source.clone()).unwrap())
            .collect();
        assert!(sources.iter().any(|source| source == "printf make"));
        assert!(sources.iter().any(|source| source == "printf docker"));
        assert!(sources.iter().any(|source| source == "printf package"));
        assert!(sources.iter().any(|source| source == "printf workflow"));
        assert!(
            findings
                .iter()
                .all(|finding| finding.kind == FindingKind::EmbeddedShell)
        );
    }

    #[test]
    fn reports_dynamic_host_calls_as_candidates_without_claiming_static_shell() {
        let temporary = tempfile::tempdir().unwrap();
        write(
            temporary.path(),
            "build.py",
            b"import os\nos.system(command)\nos.system('printf static')\n",
        );
        let findings = scan(temporary.path()).unwrap();
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == FindingKind::Candidate)
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == FindingKind::EmbeddedShell
                    && finding.source == b"printf static")
        );
    }

    #[test]
    fn retains_non_utf8_shell_bytes_for_later_residual_lowering() {
        let temporary = tempfile::tempdir().unwrap();
        write(temporary.path(), "bad.sh", b"printf '\xff'\n");
        let findings = scan(temporary.path()).unwrap();
        assert_eq!(findings[0].source, b"printf '\xff'\n");
        assert_eq!(
            findings[0].content_digest,
            crate::digest::sha256(b"printf '\xff'\n")
        );
    }

    #[test]
    fn unicode_dockerfile_prefixes_never_panic_and_spans_sort_numerically() {
        let temporary = tempfile::tempdir().unwrap();
        write(
            temporary.path(),
            "Dockerfile",
            "日本語\nRUN printf first\nRUN printf second\n".as_bytes(),
        );
        let inventory = scan(temporary.path()).unwrap();
        assert!(inventory.errors.is_empty(), "{:?}", inventory.errors);
        assert_eq!(inventory.findings.len(), 2);
        assert!(inventory.findings[0].span.start_byte < inventory.findings[1].span.start_byte);
        assert_eq!(inventory.findings[0].source, b"printf first");
    }

    #[test]
    fn host_extractors_do_not_apply_javascript_regexes_to_rust() {
        let temporary = tempfile::tempdir().unwrap();
        write(
            temporary.path(),
            "src/main.rs",
            br#"fn main() { runtime.exec(\"not shell\"); }"#,
        );
        let inventory = scan(temporary.path()).unwrap();
        assert!(inventory.findings.is_empty());
    }

    #[test]
    fn host_findings_keep_order_spans_unicode_columns_and_digests_across_files() {
        let temporary = tempfile::tempdir().unwrap();
        let python_a = "接頭辞 os.system('printf py-a')\nsubprocess.run(dynamic_a)\n";
        let python_b =
            "# second python\nprefix = 1; subprocess.call(\"printf py-b\")\nos.system(dynamic_b)\n";
        let javascript_c = "前置き exec('printf js-c');\nexecSync(dynamic_c);\n";
        let javascript_d = concat!(
            "// second javascript\n",
            "child_process.execSync(\"printf js-d\"); child_process.exec(dynamic_d);\n",
        );
        write(temporary.path(), "a.py", python_a.as_bytes());
        write(temporary.path(), "b.py", python_b.as_bytes());
        write(temporary.path(), "c.js", javascript_c.as_bytes());
        write(temporary.path(), "d.js", javascript_d.as_bytes());

        let inventory = scan(temporary.path()).unwrap();
        assert!(inventory.errors.is_empty(), "{:#?}", inventory.errors);
        let expected = [
            (
                "a.py",
                python_a,
                "os.system('printf py-a')",
                "printf py-a",
                FindingKind::EmbeddedShell,
                InterpreterConfidence::High,
            ),
            (
                "a.py",
                python_a,
                "subprocess.run(dynamic_a",
                "dynamic_a",
                FindingKind::Candidate,
                InterpreterConfidence::Low,
            ),
            (
                "b.py",
                python_b,
                "subprocess.call(\"printf py-b\"",
                "printf py-b",
                FindingKind::EmbeddedShell,
                InterpreterConfidence::High,
            ),
            (
                "b.py",
                python_b,
                "os.system(dynamic_b)",
                "dynamic_b",
                FindingKind::Candidate,
                InterpreterConfidence::Low,
            ),
            (
                "c.js",
                javascript_c,
                "exec('printf js-c')",
                "printf js-c",
                FindingKind::EmbeddedShell,
                InterpreterConfidence::High,
            ),
            (
                "c.js",
                javascript_c,
                "execSync(dynamic_c)",
                "dynamic_c",
                FindingKind::Candidate,
                InterpreterConfidence::Low,
            ),
            (
                "d.js",
                javascript_d,
                "child_process.execSync(\"printf js-d\")",
                "printf js-d",
                FindingKind::EmbeddedShell,
                InterpreterConfidence::High,
            ),
            (
                "d.js",
                javascript_d,
                "child_process.exec(dynamic_d)",
                "dynamic_d",
                FindingKind::Candidate,
                InterpreterConfidence::Low,
            ),
        ];
        assert_eq!(inventory.findings.len(), expected.len());
        for (finding, (path, source, whole, command, kind, confidence)) in
            inventory.findings.iter().zip(expected)
        {
            let start = source.find(whole).unwrap();
            let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
            let line = source[..start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let column = source[line_start..start].chars().count();
            let locator = format!("line:{line}:column:{column}");
            assert_eq!(finding.path, path);
            assert_eq!(finding.kind, kind);
            assert_eq!(finding.interpreter.as_deref(), Some("sh"));
            assert_eq!(finding.interpreter_confidence, confidence);
            assert_eq!(finding.locator.as_deref(), Some(locator.as_str()));
            assert_eq!(finding.span.start_byte, start as u64);
            assert_eq!(finding.span.end_byte, (start + whole.len()) as u64);
            assert_eq!(finding.source, command.as_bytes());
            assert_eq!(
                finding.content_digest,
                crate::digest::sha256(command.as_bytes())
            );
        }
    }

    #[test]
    fn github_shell_defaults_and_step_overrides_are_reported() {
        let temporary = tempfile::tempdir().unwrap();
        write(
            temporary.path(),
            ".github/workflows/ci.yml",
            b"defaults:\n  run:\n    shell: pwsh\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: Write-Output default\n      - run: echo override\n        shell: fish\n",
        );
        let inventory = scan(temporary.path()).unwrap();
        assert_eq!(inventory.findings.len(), 2);
        assert_eq!(
            inventory.findings[0].interpreter.as_deref(),
            Some("powershell")
        );
        assert_eq!(inventory.findings[1].interpreter.as_deref(), Some("fish"));
    }

    #[test]
    fn malformed_structured_hosts_are_inventory_errors_not_silent_omissions() {
        let temporary = tempfile::tempdir().unwrap();
        write(
            temporary.path(),
            "package.json",
            br#"{"scripts":{"x":"echo x"}"#,
        );
        write(temporary.path(), "tasks.toml", b"command = [\"echo\"\n");
        write(temporary.path(), "Dockerfile", b"RUN [\"echo\",]\n");
        write(temporary.path(), "workflow.yml", b"jobs:\n  test: [\n");
        let inventory = scan(temporary.path()).unwrap();
        assert_eq!(inventory.errors.len(), 4, "{:#?}", inventory.errors);
        assert_eq!(
            inventory
                .errors
                .iter()
                .map(|error| error.stage.as_str())
                .collect::<Vec<_>>(),
            ["parse_dockerfile", "parse_json", "parse_toml", "parse_yaml"]
        );
        assert!(inventory.findings.is_empty());
    }

    #[test]
    fn json_hosts_reject_duplicate_keys_without_rejecting_valid_floats() {
        let duplicate = tempfile::tempdir().unwrap();
        write(
            duplicate.path(),
            "package.json",
            br#"{"scripts":{"build":"echo one","build":"echo two"}}"#,
        );
        let inventory = scan(duplicate.path()).unwrap();
        assert!(inventory.findings.is_empty());
        assert_eq!(inventory.errors.len(), 1);
        assert!(inventory.errors[0].message.contains("duplicate JSON key"));

        let valid = tempfile::tempdir().unwrap();
        write(
            valid.path(),
            "package.json",
            br#"{"private":true,"scripts":{"build":"echo ok"},"version_number":1.5}"#,
        );
        let inventory = scan(valid.path()).unwrap();
        assert!(inventory.errors.is_empty(), "{:?}", inventory.errors);
        assert_eq!(inventory.findings.len(), 1);
        assert_eq!(inventory.findings[0].source, b"echo ok");
    }

    #[test]
    fn yaml_hosts_reject_duplicate_mapping_keys() {
        let directory = tempfile::tempdir().unwrap();
        write(
            directory.path(),
            ".github/workflows/duplicate.yml",
            b"jobs:\n  build:\n    steps:\n      - run: echo one\n        run: echo two\n",
        );

        let inventory = scan(directory.path()).unwrap();
        assert!(inventory.findings.is_empty());
        assert_eq!(inventory.errors.len(), 1, "{:#?}", inventory.errors);
        assert_eq!(inventory.errors[0].stage, "parse_yaml");
        assert!(inventory.errors[0].message.contains("duplicate"));
    }

    #[test]
    fn irrelevant_binary_files_are_out_of_scope_but_binary_hosts_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "image.png", &[0, 0xff, 1]);
        write(directory.path(), "data/frame.json", &[0, 0xff, 1]);
        write(directory.path(), "data/invalid.json", b"not json");
        write(directory.path(), "build.py", &[0, 0xff, 1]);

        let inventory = scan(directory.path()).unwrap();
        assert!(inventory.findings.is_empty());
        assert!(inventory.errors.is_empty());
        assert_eq!(inventory.skipped.len(), 1);
        assert_eq!(inventory.skipped[0].path, "build.py");
        assert_eq!(inventory.skipped[0].reason, "unsupported_encoding");
    }

    #[test]
    fn jsonc_hosts_accept_comments_and_trailing_commas_without_moving_spans() {
        let directory = tempfile::tempdir().unwrap();
        let source = br#"{
  // A URL inside a string is not a comment.
  "url": "https://example.invalid/path",
  "tasks": [{"command": "printf jsonc",}],
}"#;
        write(directory.path(), ".vscode/tasks.json", source);

        let inventory = scan(directory.path()).unwrap();
        assert!(inventory.errors.is_empty(), "{:#?}", inventory.errors);
        assert!(inventory.skipped.is_empty());
        assert_eq!(inventory.findings.len(), 1);
        assert_eq!(inventory.findings[0].source, b"printf jsonc");
        let start = inventory.findings[0].span.start_byte as usize;
        let end = inventory.findings[0].span.end_byte as usize;
        assert_eq!(&source[start..end], b"printf jsonc");
    }
}
