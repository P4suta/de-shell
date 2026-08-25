use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
    pub locator: Option<String>,
    pub content_hash: String,
    #[serde(skip)]
    pub source: Vec<u8>,
}

pub(crate) fn scan(_root: &Path) -> Result<Vec<Finding>, String> {
    let root = _root
        .canonicalize()
        .map_err(|error| format!("cannot access scan root {}: {error}", _root.display()))?;
    if !root.is_dir() {
        return Err(format!("scan root is not a directory: {}", root.display()));
    }
    let files = inventory(&root)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .clamp(1, 8)
        .min(files.len());
    let files = Arc::new(files);
    let next = AtomicUsize::new(0);
    let findings = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let files = Arc::clone(&files);
            let next = &next;
            let findings = &findings;
            scope.spawn(move || {
                let mut local = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((relative, absolute)) = files.get(index) else {
                        break;
                    };
                    local.extend(findings_for_file(relative, absolute));
                }
                findings
                    .lock()
                    .expect("scanner result lock poisoned")
                    .extend(local);
            });
        }
    });
    let mut findings = findings
        .into_inner()
        .map_err(|_| "scanner result lock poisoned".to_owned())?;
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.locator.cmp(&right.locator))
            .then_with(|| kind_order(&left.kind).cmp(&kind_order(&right.kind)))
    });
    Ok(findings)
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

fn inventory(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    if valid_git_marker(root)
        && let Some(files) = git_inventory(root)
    {
        return Ok(files);
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !IGNORED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref())
        })
    {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative.to_str() else {
            continue;
        };
        files.push((relative.replace('\\', "/"), entry.path().to_path_buf()));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn valid_git_marker(root: &Path) -> bool {
    let marker = root.join(".git");
    marker.join("HEAD").is_file() || marker.is_file()
}

fn git_inventory(root: &Path) -> Option<Vec<(String, PathBuf)>> {
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
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut files = Vec::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        let relative = std::str::from_utf8(raw).ok()?.replace('\\', "/");
        if relative
            .split('/')
            .any(|part| IGNORED_DIRECTORIES.contains(&part))
        {
            continue;
        }
        let absolute = root.join(&relative);
        if absolute.symlink_metadata().ok()?.file_type().is_file() {
            files.push((relative, absolute));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Some(files)
}

fn findings_for_file(relative: &str, absolute: &Path) -> Vec<Finding> {
    let Ok(metadata) = absolute.symlink_metadata() else {
        return Vec::new();
    };
    if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Vec::new();
    }
    let Ok(source) = std::fs::read(absolute) else {
        return Vec::new();
    };
    let lower = relative.to_ascii_lowercase();
    let filename = lower.rsplit('/').next().unwrap_or(&lower);
    if [
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "pnpm-lock.yml",
    ]
    .contains(&filename)
    {
        return Vec::new();
    }
    if let Some(interpreter) = extension_interpreter(&lower) {
        return vec![finding(
            relative,
            FindingKind::ShellFile,
            Some(interpreter),
            None,
            source,
        )];
    }
    if !matches!(
        crate::frontend::detect(relative, &source),
        crate::frontend::Interpreter::Unknown(_)
    ) {
        let interpreter = crate::frontend::detect(relative, &source).name().to_owned();
        return vec![finding(
            relative,
            FindingKind::ShellFile,
            Some(interpreter),
            None,
            source,
        )];
    }
    let Ok(text) = std::str::from_utf8(&source) else {
        return Vec::new();
    };
    if filename == "package.json" {
        return package_findings(relative, text);
    }
    if filename == "makefile" || filename == "gnumakefile" || lower.ends_with(".mk") {
        return makefile_findings(relative, text);
    }
    if filename == "dockerfile"
        || filename.starts_with("dockerfile.")
        || lower.ends_with(".dockerfile")
    {
        return dockerfile_findings(relative, text);
    }
    if lower.ends_with(".yml") || lower.ends_with(".yaml") {
        return yaml_findings(relative, text);
    }
    if lower.ends_with(".json") {
        return json_candidate_findings(relative, text);
    }
    if lower.ends_with(".toml") {
        return toml_candidate_findings(relative, text);
    }
    host_findings(relative, text)
}

fn extension_interpreter(path: &str) -> Option<String> {
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
    Some(value.into())
}

fn finding(
    path: &str,
    kind: FindingKind,
    interpreter: Option<String>,
    locator: Option<String>,
    source: Vec<u8>,
) -> Finding {
    Finding {
        path: path.replace('\\', "/"),
        kind,
        interpreter,
        locator,
        content_hash: crate::digest::sha256(&source),
        source,
    }
}

fn package_findings(path: &str, source: &str) -> Vec<Finding> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(source) else {
        return Vec::new();
    };
    let Some(scripts) = value.get("scripts").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    scripts
        .iter()
        .filter_map(|(name, value)| {
            value.as_str().map(|script| {
                finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("package-shell".into()),
                    Some(format!("scripts.{name}")),
                    script.as_bytes().to_vec(),
                )
            })
        })
        .collect()
}

fn makefile_findings(path: &str, source: &str) -> Vec<Finding> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            line.strip_prefix('\t').map(|command| {
                finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("sh".into()),
                    Some(format!("recipe:{}", index + 1)),
                    command.as_bytes().to_vec(),
                )
            })
        })
        .collect()
}

fn dockerfile_findings(path: &str, source: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("RUN ") {
            let mut command = trimmed[4..].trim().to_owned();
            let line = index + 1;
            while command.ends_with('\\') && index + 1 < lines.len() {
                command.pop();
                command = format!("{} {}", command.trim_end(), lines[index + 1].trim());
                index += 1;
            }
            if !command.trim_start().starts_with('[') {
                findings.push(finding(
                    path,
                    FindingKind::EmbeddedShell,
                    Some("sh".into()),
                    Some(format!("RUN:{line}")),
                    command.into_bytes(),
                ));
            }
        }
        index += 1;
    }
    findings
}

fn yaml_findings(path: &str, source: &str) -> Vec<Finding> {
    let lower = path.to_ascii_lowercase();
    let known = lower.starts_with(".github/workflows/")
        || lower.starts_with(".github/actions/")
        || lower == ".gitlab-ci.yml"
        || lower == ".gitlab-ci.yaml"
        || lower == "azure-pipelines.yml"
        || lower == "azure-pipelines.yaml"
        || lower.starts_with(".circleci/");
    let mut findings = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let trimmed = raw.trim_start().trim_start_matches("- ");
        let Some((key, raw_value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
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
        .contains(&key.as_str());
        if !executable {
            index += 1;
            continue;
        }
        let interpreter = if key == "pwsh" || key == "powershell" {
            "powershell"
        } else if key == "bash" {
            "bash"
        } else {
            "sh"
        };
        let value = strip_quotes(raw_value.trim());
        let line = index + 1;
        if ["|", "|-", "|+", ">", ">-", ">+"].contains(&value.as_str()) {
            let indentation = raw.len() - raw.trim_start().len();
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
            let command = block.join("\n") + "\n";
            if !command.trim().is_empty() {
                findings.push(finding(
                    path,
                    if known {
                        FindingKind::EmbeddedShell
                    } else {
                        FindingKind::Candidate
                    },
                    Some(interpreter.into()),
                    Some(format!("{key}:{line}")),
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
                Some(interpreter.into()),
                Some(format!("{key}:{line}")),
                value.into_bytes(),
            ));
        }
        index += 1;
    }
    findings
}

fn json_candidate_findings(path: &str, source: &str) -> Vec<Finding> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(source) else {
        return Vec::new();
    };
    let mut output = Vec::new();
    collect_json_candidates(path, "$", false, &value, &mut output);
    output
}

fn collect_json_candidates(
    path: &str,
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
                Some(locator.into()),
                command.as_bytes().to_vec(),
            )),
        _ => {}
    }
}

fn toml_candidate_findings(path: &str, source: &str) -> Vec<Finding> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let (key, value) = line.split_once('=')?;
            let key = key
                .trim()
                .rsplit('.')
                .next()
                .unwrap_or("")
                .trim_matches(['\'', '"']);
            let value = strip_quotes(value.split('#').next().unwrap_or("").trim());
            if executable_field(key) && looks_like_shell(&value) {
                Some(finding(
                    path,
                    FindingKind::Candidate,
                    None,
                    Some(format!("line:{}", index + 1)),
                    value.into_bytes(),
                ))
            } else {
                None
            }
        })
        .collect()
}

fn host_findings(path: &str, source: &str) -> Vec<Finding> {
    let patterns = [
        (r#"os\.system\s*\(\s*([^\)]*)\)"#, "sh"),
        (r#"subprocess\.(?:run|call|Popen)\s*\(\s*([^,\)]*)"#, "sh"),
        (
            r#"(?:child_process\.)?(?:exec|execSync)\s*\(\s*([^\)]*)\)"#,
            "sh",
        ),
    ];
    let mut output = Vec::new();
    for (pattern, interpreter) in patterns {
        let regex = regex::Regex::new(pattern).expect("static scanner regex");
        for capture in regex.captures_iter(source) {
            let whole = capture.get(0).unwrap();
            let argument = capture.get(1).unwrap().as_str().trim();
            let (kind, command) = quoted_literal(argument)
                .map(|value| (FindingKind::EmbeddedShell, value))
                .unwrap_or_else(|| (FindingKind::Candidate, argument.to_owned()));
            let line = source[..whole.start()]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let column = source[..whole.start()]
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .chars()
                .count();
            output.push(finding(
                path,
                kind,
                Some(interpreter.into()),
                Some(format!("line:{line}:column:{column}")),
                command.into_bytes(),
            ));
        }
    }
    output
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
    let name = name.trim().to_ascii_lowercase();
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
    .contains(&name.as_str())
        || name.ends_with("command")
        || name.ends_with("script")
        || name.ends_with("hook")
}

fn looks_like_shell(value: &str) -> bool {
    let first = value
        .split_ascii_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "bash", "cat", "cd", "chmod", "cmd", "cp", "curl", "echo", "env", "exec", "fish", "git",
        "mkdir", "mv", "nu", "printf", "pwsh", "rm", "sh", "test", "wget", "zsh",
    ]
    .contains(&first.as_str())
        || ["&&", "||", "$(", "${", " | ", " > "]
            .iter()
            .any(|marker| value.contains(marker))
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
                .all(|finding| finding.content_hash.len() == 64)
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
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("visible.sh", temporary.path().join("linked.sh")).unwrap();
        }
        let findings = scan(temporary.path()).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path, "visible.sh");
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
            findings[0].content_hash,
            crate::digest::sha256(b"printf '\xff'\n")
        );
    }
}
