use crate::config::{Lockfile, ProjectConfig, Scenario};
use crate::evidence::Evidence;
use crate::ir::Plan;
use crate::scanner::Finding;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InitResult {
    pub created: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnalysisResult {
    pub plan: Plan,
    pub evidence: Evidence,
    pub plan_path: PathBuf,
    pub evidence_path: PathBuf,
}

pub(crate) fn init(root: &Path) -> Result<InitResult, String> {
    ensure_directory(root)?;
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect project root {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "project root is not a regular directory: {}",
            root.display()
        ));
    }
    let deshell = root.join(".deshell");
    let scenarios = deshell.join("scenarios");
    ensure_directory(&deshell)?;
    ensure_directory(&scenarios)?;
    let candidates = [
        (
            deshell.join("project.toml"),
            ProjectConfig::default_text().into_bytes(),
        ),
        (
            scenarios.join("default.toml"),
            Scenario::default_text().into_bytes(),
        ),
        (
            root.join("deshell.lock"),
            Lockfile::default_text().into_bytes(),
        ),
    ];
    let mut created = Vec::new();
    let mut proposals = Vec::new();
    for (path, contents) in candidates {
        match path.symlink_metadata() {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                continue;
            }
            Ok(_) => {
                return Err(format!(
                    "project artifact is not a regular non-symlink file: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                proposals.push(crate::patch::prepare_create(&path, contents, 0o644)?);
                created.push(path);
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect project artifact {}: {error}",
                    path.display()
                ));
            }
        }
    }
    crate::patch::apply_all(&proposals)?;
    Ok(InitResult { created })
}

pub(crate) fn load_config(root: &Path) -> Result<ProjectConfig, Vec<String>> {
    let path = root.join(".deshell/project.toml");
    let input = read_utf8(&path).map_err(|error| vec![error])?;
    ProjectConfig::decode(&input)
}

pub(crate) fn configured_entry(root: &Path) -> Result<String, String> {
    let config = load_config(root).map_err(|errors| errors.join("; "))?;
    match config.entrypoints.as_slice() {
        [entry] => Ok(entry.clone()),
        [] => Err("no entrypoint was supplied and project.toml entrypoints is empty".into()),
        _ => Err("project.toml contains multiple entrypoints; select one with --entry".into()),
    }
}

pub(crate) fn resolve_entry(root: &Path, entry: &str) -> Result<(PathBuf, PathBuf), String> {
    let normalized = crate::ir::normalize_path(entry)?;
    if normalized != entry.replace('\\', "/") || normalized != entry {
        return Err(format!("entrypoint path is not normalized: {entry}"));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project root {}: {error}", root.display()))?;
    let root_metadata = canonical_root.symlink_metadata().map_err(|error| {
        format!(
            "cannot inspect project root {}: {error}",
            canonical_root.display()
        )
    })?;
    if !root_metadata.file_type().is_dir() {
        return Err(format!(
            "project root is not a directory: {}",
            canonical_root.display()
        ));
    }
    let candidate = canonical_root.join(&normalized);
    let metadata = candidate
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect entrypoint {entry}: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("entrypoint must not be a symlink: {entry}"));
    }
    if !metadata.file_type().is_file() {
        return Err(format!("entrypoint is not a regular file: {entry}"));
    }
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(format!(
            "entrypoint exceeds the 4 MiB analysis limit: {entry}"
        ));
    }
    let canonical_entry = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve entrypoint {entry}: {error}"))?;
    if !canonical_entry.starts_with(&canonical_root) {
        return Err(format!("entrypoint escapes the project root: {entry}"));
    }
    Ok((canonical_root, canonical_entry))
}

pub(crate) fn scan(root: &Path) -> Result<Vec<Finding>, String> {
    crate::scanner::scan(root)
}

pub(crate) fn analyze(root: &Path, entry: &str) -> Result<AnalysisResult, String> {
    let config = load_config(root).map_err(|errors| errors.join("; "))?;
    load_lock(root).map_err(|errors| errors.join("; "))?;
    let (canonical_root, entry_path) = resolve_entry(root, entry)?;
    let source = std::fs::read(&entry_path)
        .map_err(|error| format!("cannot read entrypoint {entry}: {error}"))?;
    let plan = crate::frontend::lower(entry, &source, config.policy.unknown_interpreter)?;
    let evidence = Evidence::from_plan(&plan, entry, &source)?;
    let directory = canonical_root.join(".deshell");
    ensure_existing_directory(&directory)?;
    let plan_path = directory.join("plan.json");
    let evidence_path = directory.join("evidence.json");
    let proposals = vec![
        prepare_write(&plan_path, plan.encode_pretty()?)?,
        prepare_write(&evidence_path, evidence.encode_pretty()?)?,
    ];
    crate::patch::apply_all(&proposals)?;
    Ok(AnalysisResult {
        plan,
        evidence,
        plan_path,
        evidence_path,
    })
}

pub(crate) fn load_artifacts(root: &Path) -> Result<(Plan, Evidence), Vec<String>> {
    let plan_path = root.join(".deshell/plan.json");
    let evidence_path = root.join(".deshell/evidence.json");
    let plan_bytes = std::fs::read(&plan_path)
        .map_err(|error| vec![format!("cannot read {}: {error}", plan_path.display())])?;
    let evidence_bytes = std::fs::read(&evidence_path)
        .map_err(|error| vec![format!("cannot read {}: {error}", evidence_path.display())])?;
    let plan = Plan::decode(&plan_bytes)?;
    let evidence = Evidence::decode(&evidence_bytes)?;
    Ok((plan, evidence))
}

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if let Err(mut values) = load_config(root) {
        errors.append(&mut values);
    }
    if let Err(mut values) = load_lock(root) {
        errors.append(&mut values);
    }
    if let Err(mut values) = validate_scenarios(root) {
        errors.append(&mut values);
    }
    if let Err(mut values) = validate_replay(root) {
        errors.append(&mut values);
    }
    let artifacts = load_artifacts(root);
    match artifacts {
        Err(mut values) => errors.append(&mut values),
        Ok((plan, evidence)) => match resolve_entry(root, &evidence.source.path) {
            Err(error) => errors.push(error),
            Ok((_, source_path)) => match std::fs::read(&source_path) {
                Err(error) => errors.push(format!(
                    "cannot read evidence source {}: {error}",
                    evidence.source.path
                )),
                Ok(source) => {
                    if let Err(mut values) = evidence.validate_against(&plan, &source) {
                        errors.append(&mut values);
                    }
                }
            },
        },
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub(crate) fn save_evidence(root: &Path, evidence: &Evidence) -> Result<PathBuf, String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project root {}: {error}", root.display()))?;
    let plan_path = canonical_root.join(".deshell/plan.json");
    let plan = Plan::decode(
        &std::fs::read(&plan_path)
            .map_err(|error| format!("cannot read {}: {error}", plan_path.display()))?,
    )
    .map_err(|errors| errors.join("; "))?;
    let (_, source_path) = resolve_entry(&canonical_root, &evidence.source.path)?;
    let source = std::fs::read(&source_path).map_err(|error| {
        format!(
            "cannot read evidence source {}: {error}",
            evidence.source.path
        )
    })?;
    evidence
        .validate_against(&plan, &source)
        .map_err(|errors| errors.join("; "))?;
    let path = canonical_root.join(".deshell/evidence.json");
    let proposal = prepare_write(&path, evidence.encode_pretty()?)?;
    crate::patch::apply_all(&[proposal])?;
    Ok(path)
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(format!(
            "path is not a regular directory: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(path)
            .map_err(|error| format!("cannot create directory {}: {error}", path.display())),
        Err(error) => Err(format!(
            "cannot inspect directory {}: {error}",
            path.display()
        )),
    }
}

fn ensure_existing_directory(path: &Path) -> Result<(), String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect directory {}: {error}", path.display()))?;
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(format!(
            "path is not a regular directory: {}",
            path.display()
        ))
    }
}

fn read_utf8(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

fn load_lock(root: &Path) -> Result<Lockfile, Vec<String>> {
    let path = root.join("deshell.lock");
    let input = read_utf8(&path).map_err(|error| vec![error])?;
    Lockfile::decode(&input)
}

fn validate_scenarios(root: &Path) -> Result<(), Vec<String>> {
    let directory = root.join(".deshell/scenarios");
    let entries = std::fs::read_dir(&directory).map_err(|error| {
        vec![format!(
            "cannot read scenario directory {}: {error}",
            directory.display()
        )]
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| vec![format!("cannot read scenario entry: {error}")])?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(vec![
            "project must contain at least one scenario TOML file".into(),
        ]);
    }
    let mut errors = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for path in paths {
        match read_utf8(&path)
            .map_err(|error| vec![error])
            .and_then(|input| Scenario::decode(&input))
        {
            Err(mut values) => errors.append(&mut values),
            Ok(scenario) if !names.insert(scenario.name.clone()) => {
                errors.push(format!("duplicate scenario name: {}", scenario.name))
            }
            Ok(_) => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_replay(root: &Path) -> Result<(), Vec<String>> {
    let path = root.join(".deshell/replay.json");
    let metadata = match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(vec![format!("cannot inspect {}: {error}", path.display())]),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(vec![format!(
            "network replay store is not a regular non-symlink file: {}",
            path.display()
        )]);
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| vec![format!("cannot read {}: {error}", path.display())])?;
    let store = crate::replay::ReplayStore::decode(&bytes)?;
    let canonical = store.encode_pretty().map_err(|error| vec![error])?;
    if canonical != bytes {
        Err(vec![format!(
            "network replay store does not use canonical persisted bytes: {}",
            path.display()
        )])
    } else {
        Ok(())
    }
}

fn prepare_write(path: &Path, contents: Vec<u8>) -> Result<crate::patch::Proposal, String> {
    match path.symlink_metadata() {
        Ok(_) => crate::patch::prepare(path, contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::patch::prepare_create(path, contents, 0o644)
        }
        Err(error) => Err(format!(
            "cannot inspect artifact {}: {error}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn configured_project(source: &[u8]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        init(directory.path()).unwrap();
        write(directory.path(), "build.sh", source);
        let config_path = directory.path().join(".deshell/project.toml");
        let config = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("entrypoints = []", "entrypoints = [\"build.sh\"]");
        std::fs::write(config_path, config).unwrap();
        directory
    }

    #[test]
    fn init_is_additive_and_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let first = init(directory.path()).unwrap();
        assert_eq!(first.created.len(), 3);
        assert_eq!(
            std::fs::read_to_string(directory.path().join(".deshell/project.toml")).unwrap(),
            ProjectConfig::default_text()
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join(".deshell/scenarios/default.toml"))
                .unwrap(),
            crate::config::Scenario::default_text()
        );
        crate::config::Lockfile::decode(
            &std::fs::read_to_string(directory.path().join("deshell.lock")).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.path().join(".deshell/project.toml"),
            "user-owned\n",
        )
        .unwrap();
        assert!(init(directory.path()).unwrap().created.is_empty());
        assert_eq!(
            std::fs::read_to_string(directory.path().join(".deshell/project.toml")).unwrap(),
            "user-owned\n"
        );
    }

    #[test]
    fn configured_entry_requires_exactly_one_safe_path() {
        let directory = tempfile::tempdir().unwrap();
        init(directory.path()).unwrap();
        assert!(
            configured_entry(directory.path())
                .unwrap_err()
                .contains("empty")
        );
        let path = directory.path().join(".deshell/project.toml");
        let two = ProjectConfig::default_text()
            .replace("entrypoints = []", "entrypoints = [\"a.sh\", \"b.sh\"]");
        std::fs::write(path, two).unwrap();
        assert!(
            configured_entry(directory.path())
                .unwrap_err()
                .contains("multiple")
        );
    }

    #[test]
    fn entry_resolution_rejects_traversal_symlinks_and_large_files() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "ok.sh", b"true\n");
        assert!(resolve_entry(directory.path(), "ok.sh").is_ok());
        assert!(resolve_entry(directory.path(), "../outside.sh").is_err());
        write(
            directory.path(),
            "large.sh",
            &vec![b'x'; 4 * 1024 * 1024 + 1],
        );
        assert!(
            resolve_entry(directory.path(), "large.sh")
                .unwrap_err()
                .contains("4 MiB")
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("ok.sh", directory.path().join("link.sh")).unwrap();
            assert!(
                resolve_entry(directory.path(), "link.sh")
                    .unwrap_err()
                    .contains("symlink")
            );
        }
    }

    #[test]
    fn analyze_persists_canonical_bound_artifacts_and_check_validates_them() {
        let directory = configured_project(b"#!/bin/sh\nprintf '%s' \"$NAME\"\n");
        let result = analyze(directory.path(), "build.sh").unwrap();
        assert_eq!(
            result.plan,
            Plan::decode(&std::fs::read(&result.plan_path).unwrap()).unwrap()
        );
        assert_eq!(
            result.evidence,
            Evidence::decode(&std::fs::read(&result.evidence_path).unwrap()).unwrap()
        );
        result
            .evidence
            .validate_against(
                &result.plan,
                &std::fs::read(directory.path().join("build.sh")).unwrap(),
            )
            .unwrap();
        check(directory.path()).unwrap();
    }

    #[test]
    fn check_detects_source_and_artifact_tampering() {
        let directory = configured_project(b"true\n");
        analyze(directory.path(), "build.sh").unwrap();
        write(directory.path(), "build.sh", b"false\n");
        assert!(
            check(directory.path())
                .unwrap_err()
                .join("; ")
                .contains("source digest mismatch")
        );

        analyze(directory.path(), "build.sh").unwrap();
        let plan_path = directory.path().join(".deshell/plan.json");
        let mut plan = std::fs::read_to_string(&plan_path).unwrap();
        plan.push_str("{}\n");
        std::fs::write(plan_path, plan).unwrap();
        assert!(check(directory.path()).is_err());
    }

    #[test]
    fn scan_uses_project_boundary_and_stable_inventory() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "z.sh", b"true\n");
        write(directory.path(), "a.fish", b"echo hello\n");
        let findings = scan(directory.path()).unwrap();
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.fish", "z.sh"]
        );
    }

    #[test]
    fn evidence_updates_are_validated_and_atomic() {
        let directory = configured_project(b"true\n");
        let analysis = analyze(directory.path(), "build.sh").unwrap();
        let original = std::fs::read(&analysis.evidence_path).unwrap();
        let mut evidence = analysis.evidence;
        evidence
            .append_observation(crate::evidence::ObservationEvidence {
                scenarios: vec!["default".into()],
                status: crate::evidence::ObservationStatus::Unavailable,
                provider: Some("test".into()),
                reason: Some("not configured".into()),
                digest: None,
            })
            .unwrap();
        assert_eq!(
            save_evidence(directory.path(), &evidence).unwrap(),
            analysis.evidence_path
        );
        assert_eq!(
            Evidence::decode(&std::fs::read(&analysis.evidence_path).unwrap()).unwrap(),
            evidence
        );
        let mut tampered = evidence;
        tampered.plan_digest = "0".repeat(64);
        assert!(save_evidence(directory.path(), &tampered).is_err());
        assert_ne!(std::fs::read(&analysis.evidence_path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn evidence_updates_return_the_canonical_path_for_a_root_alias() {
        let directory = configured_project(b"true\n");
        let analysis = analyze(directory.path(), "build.sh").unwrap();
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("project");
        std::os::unix::fs::symlink(directory.path(), &alias).unwrap();

        assert_eq!(
            save_evidence(&alias, &analysis.evidence).unwrap(),
            analysis.evidence_path
        );
    }

    #[test]
    fn optional_replay_store_must_use_canonical_persisted_bytes() {
        let directory = configured_project(b"true\n");
        analyze(directory.path(), "build.sh").unwrap();
        std::fs::write(
            directory.path().join(".deshell/replay.json"),
            b"{\"entries\":[],\"schema_version\":1}\n",
        )
        .unwrap();
        assert!(
            check(directory.path())
                .unwrap_err()
                .join("; ")
                .contains("canonical persisted bytes")
        );
        let store = crate::replay::ReplayStore {
            schema_version: 1,
            entries: vec![],
        };
        std::fs::write(
            directory.path().join(".deshell/replay.json"),
            store.encode_pretty().unwrap(),
        )
        .unwrap();
        check(directory.path()).unwrap();
    }
}
