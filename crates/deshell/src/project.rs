use crate::config::{Lockfile, ProjectConfig, Scenario};
use crate::evidence::Evidence;
use crate::ir::Plan;
use crate::scanner::Inventory;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InitResult {
    pub created: Vec<PathBuf>,
    pub entrypoints: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnalysisResult {
    pub plan: Plan,
    pub evidence: Evidence,
    pub plan_path: PathBuf,
    pub evidence_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedProject {
    pub canonical_root: PathBuf,
    pub config: ProjectConfig,
    pub lock: Lockfile,
    pub manifest: Manifest,
    pub entries: Vec<ValidatedEntry>,
    pub scenarios: Vec<ValidatedScenario>,
    pub replay: Option<crate::replay::ReplayStore>,
    pub runtime_lock_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedEntry {
    pub manifest: ManifestEntry,
    pub plan: Plan,
    pub evidence: Evidence,
    pub source: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedScenario {
    pub scenario: Scenario,
    pub digest: String,
}

struct ProjectReader {
    root: PathBuf,
}

impl ProjectReader {
    fn new(root: &Path) -> Result<Self, String> {
        Ok(Self {
            root: canonical_project_root(root)?,
        })
    }

    fn read_file(&self, relative: &str) -> Result<Vec<u8>, String> {
        let path = project_file_path_from_root(&self.root, relative)?;
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
    }

    fn read_utf8(&self, relative: &str) -> Result<String, String> {
        String::from_utf8(self.read_file(relative)?)
            .map_err(|_| format!("project file is not valid UTF-8: {relative}"))
    }

    fn file_path(&self, relative: &str) -> Result<PathBuf, String> {
        project_file_path_from_root(&self.root, relative)
    }

    fn directory_path(&self, relative: &str) -> Result<PathBuf, String> {
        project_directory_path_from_root(&self.root, relative)
    }

    fn entry_path(&self, entry: &str) -> Result<PathBuf, String> {
        resolve_entry_from_root(&self.root, entry)
    }

    fn load_config(&self) -> Result<ProjectConfig, Vec<String>> {
        let input = self
            .read_utf8(".deshell/project.toml")
            .map_err(|error| vec![error])?;
        ProjectConfig::decode(&input)
    }

    fn load_manifest(&self) -> Result<Manifest, Vec<String>> {
        let bytes = self
            .read_file(".deshell/manifest.json")
            .map_err(|error| vec![error])?;
        Manifest::decode(&bytes)
    }

    fn load_lock_snapshot(&self) -> Result<(Lockfile, String), Vec<String>> {
        let bytes = self
            .read_file("deshell.lock")
            .map_err(|error| vec![error])?;
        let input = std::str::from_utf8(&bytes)
            .map_err(|_| vec!["project file is not valid UTF-8: deshell.lock".into()])?;
        let lock = Lockfile::decode(input)?;
        Ok((lock, crate::digest::sha256(&bytes)))
    }
}

impl ValidatedProject {
    pub(crate) fn load(root: &Path) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let reader = ProjectReader::new(root).ok();
        let config_result = if let Some(reader) = &reader {
            reader.load_config()
        } else {
            load_config(root)
        };
        let config = match config_result {
            Ok(config) => Some(config),
            Err(values) => {
                errors.extend(values);
                None
            }
        };
        let lock_result = if let Some(reader) = &reader {
            reader.load_lock_snapshot()
        } else {
            load_lock_snapshot(root)
        };
        let lock_snapshot = match lock_result {
            Ok(snapshot) => Some(snapshot),
            Err(values) => {
                errors.extend(values);
                None
            }
        };
        if let Some((lock, _)) = &lock_snapshot {
            for asset in &lock.lab.assets {
                let path = if let Some(reader) = &reader {
                    reader.file_path(&asset.path)
                } else {
                    project_file_path(root, &asset.path)
                };
                match path.and_then(|path| crate::digest::file_sha256(&path)) {
                    Ok((_, digest)) if asset.sha256 == format!("sha256:{digest}") => {}
                    Ok((_, digest)) => errors.push(format!(
                        "lab asset digest mismatch for {} (expected {}, found sha256:{digest})",
                        asset.path, asset.sha256
                    )),
                    Err(error) => {
                        errors.push(format!("cannot validate lab asset {}: {error}", asset.path))
                    }
                }
            }
        }
        let scenarios = match load_validated_scenarios(
            root,
            reader.as_ref(),
            config.as_ref().map(|config| config.limits),
        ) {
            Ok(scenarios) => Some(scenarios),
            Err(values) => {
                errors.extend(values);
                None
            }
        };
        let replay = match load_validated_replay(root) {
            Ok(replay) => Some(replay),
            Err(values) => {
                errors.extend(values);
                None
            }
        };
        let manifest_result = if let Some(reader) = &reader {
            reader.load_manifest()
        } else {
            load_manifest(root)
        };
        let manifest = match manifest_result {
            Ok(manifest) => Some(manifest),
            Err(values) => {
                errors.extend(values);
                None
            }
        };
        let mut validated_entries = None;
        if let Some(manifest) = &manifest {
            let mut entries = Vec::with_capacity(manifest.entries.len());
            let mut artifact_errors = Vec::new();
            for entry in &manifest.entries {
                let loaded = if let Some(reader) = &reader {
                    load_validated_entry_with_reader(reader, entry)
                } else {
                    load_validated_entry(root, entry)
                };
                match loaded {
                    Ok(entry) => entries.push(entry),
                    Err(mut values) => artifact_errors.append(&mut values),
                }
            }
            if artifact_errors.is_empty() {
                if entries.is_empty() {
                    errors.push("manifest contains no analyzed entrypoint".into());
                } else {
                    if let Some(config) = &config {
                        let configured = config
                            .entrypoints
                            .iter()
                            .map(String::as_str)
                            .collect::<std::collections::BTreeSet<_>>();
                        let manifested = entries
                            .iter()
                            .map(|entry| entry.manifest.entrypoint.as_str())
                            .collect::<std::collections::BTreeSet<_>>();
                        if configured != manifested {
                            errors.push(
                                "manifest entrypoints must exactly match project.toml entrypoints"
                                    .into(),
                            );
                        }
                    }
                    if let Some((lock, _)) = &lock_snapshot {
                        for entry in &entries {
                            let mut rebound = entry.plan.clone();
                            match crate::frontend::bind_interpreter_pins(
                                &mut rebound,
                                &lock.interpreters,
                            ) {
                                Err(error) => errors.push(error),
                                Ok(()) if rebound != entry.plan => errors.push(format!(
                                    "plan delegated interpreter pins do not match deshell.lock for {}",
                                    entry.evidence.source.path
                                )),
                                Ok(()) => {}
                            }
                        }
                    }
                }
                validated_entries = Some(entries);
            } else {
                errors.append(&mut artifact_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let (lock, runtime_lock_digest) = lock_snapshot.expect("valid project has a lock");
        Ok(Self {
            canonical_root: reader.expect("valid project has a canonical root").root,
            config: config.expect("valid project has a config"),
            lock,
            manifest: manifest.expect("valid project has a manifest"),
            entries: validated_entries.expect("valid project has validated entries"),
            scenarios: scenarios.expect("valid project has validated scenarios"),
            replay: replay.expect("valid project has a replay snapshot"),
            runtime_lock_digest,
        })
    }

    pub(crate) fn entry(&self, entrypoint: &str) -> Result<&ValidatedEntry, Vec<String>> {
        self.entries
            .iter()
            .find(|entry| entry.manifest.entrypoint == entrypoint)
            .ok_or_else(|| vec![format!("entrypoint is not analyzed: {entrypoint}")])
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub schema_version: u32,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestEntry {
    pub entrypoint: String,
    pub source_digest: String,
    pub plan_digest: String,
    pub plan_path: String,
    pub evidence_path: String,
}

impl Manifest {
    fn empty() -> Self {
        Self {
            schema_version: 1,
            entries: Vec::new(),
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self, Vec<String>> {
        let manifest: Self = crate::strict_json::decode(bytes).map_err(|error| vec![error])?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        self.validate().map_err(|errors| errors.join("; "))?;
        crate::canonical_json::pretty_bytes(
            &serde_json::to_value(self).map_err(|error| error.to_string())?,
        )
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != 1 {
            errors.push("manifest schema_version must be 1".into());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut previous = None;
        for entry in &self.entries {
            match crate::ir::normalize_path(&entry.entrypoint) {
                Ok(path) if path == entry.entrypoint => {}
                Ok(_) => errors.push(format!(
                    "manifest entrypoint is not normalized: {}",
                    entry.entrypoint
                )),
                Err(error) => errors.push(format!(
                    "invalid manifest entrypoint {}: {error}",
                    entry.entrypoint
                )),
            }
            if !names.insert(entry.entrypoint.as_str()) {
                errors.push(format!(
                    "duplicate manifest entrypoint: {}",
                    entry.entrypoint
                ));
            }
            if previous.is_some_and(|value: &str| value >= entry.entrypoint.as_str()) {
                errors.push("manifest entries must be sorted by entrypoint".into());
            }
            previous = Some(entry.entrypoint.as_str());
            if !crate::digest::valid_sha256(&entry.source_digest)
                || !crate::digest::valid_sha256(&entry.plan_digest)
            {
                errors.push(format!(
                    "manifest digests are invalid for {}",
                    entry.entrypoint
                ));
            }
            let prefix = format!(
                ".deshell/artifacts/{}/{}",
                entry.source_digest, entry.plan_digest
            );
            if entry.plan_path != format!("{prefix}/plan.json")
                || entry.evidence_path != format!("{prefix}/evidence.json")
            {
                errors.push(format!(
                    "manifest artifact paths are not content-addressed for {}",
                    entry.entrypoint
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

pub(crate) fn init(root: &Path) -> Result<InitResult, String> {
    init_with_entries(root, &[])
}

pub(crate) fn init_with_entries(root: &Path, requested: &[String]) -> Result<InitResult, String> {
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
    let existing_config = deshell.join("project.toml").is_file();
    let entrypoints = if existing_config {
        let configured = load_config(root)
            .map_err(|errors| errors.join("; "))?
            .entrypoints;
        if !requested.is_empty() {
            let mut requested = requested.to_vec();
            requested.sort();
            requested.dedup();
            if requested != configured {
                return Err(
                    "project is already initialized; --entry cannot replace existing entrypoints"
                        .into(),
                );
            }
        }
        configured
    } else if requested.is_empty() {
        discover_entrypoints(root)?
    } else {
        let mut entries = requested.to_vec();
        entries.sort();
        entries.dedup();
        if entries.len() != requested.len() {
            return Err("duplicate --entry value".into());
        }
        for entry in &entries {
            resolve_entry(root, entry)?;
        }
        entries
    };
    let config_text = ProjectConfig::default_text().replace(
        "entrypoints = []",
        &format!(
            "entrypoints = [{}]",
            entrypoints
                .iter()
                .map(|entry| format!("\"{}\"", entry.replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    let candidates = [
        (deshell.join("project.toml"), config_text.into_bytes()),
        (
            scenarios.join("default.toml"),
            Scenario::default_text().into_bytes(),
        ),
        (
            root.join("deshell.lock"),
            Lockfile::default_text().into_bytes(),
        ),
        (
            deshell.join("manifest.json"),
            Manifest::empty().encode_pretty()?,
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
    Ok(InitResult {
        created,
        entrypoints,
    })
}

fn discover_entrypoints(root: &Path) -> Result<Vec<String>, String> {
    let inventory = crate::scanner::scan(root)?;
    if !inventory.errors.is_empty() {
        return Err(format!(
            "entrypoint discovery failed: {}",
            inventory
                .errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let mut entries = inventory
        .findings
        .into_iter()
        .filter(|finding| {
            finding.kind == crate::scanner::FindingKind::ShellFile
                && finding.interpreter_confidence == crate::scanner::InterpreterConfidence::High
        })
        .map(|finding| finding.path)
        .collect::<Vec<_>>();
    entries.sort();
    entries.dedup();
    Ok(entries)
}

pub(crate) fn load_config(root: &Path) -> Result<ProjectConfig, Vec<String>> {
    let input = read_project_utf8(root, ".deshell/project.toml").map_err(|error| vec![error])?;
    ProjectConfig::decode(&input)
}

pub(crate) fn load_manifest(root: &Path) -> Result<Manifest, Vec<String>> {
    let bytes = read_project_file(root, ".deshell/manifest.json").map_err(|error| vec![error])?;
    Manifest::decode(&bytes)
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
    let normalized = normalized_entry(entry)?;
    let canonical_root = canonical_project_root(root)?;
    let canonical_entry = resolve_normalized_entry_from_root(&canonical_root, entry, &normalized)?;
    Ok((canonical_root, canonical_entry))
}

fn resolve_entry_from_root(canonical_root: &Path, entry: &str) -> Result<PathBuf, String> {
    let normalized = normalized_entry(entry)?;
    resolve_normalized_entry_from_root(canonical_root, entry, &normalized)
}

fn normalized_entry(entry: &str) -> Result<String, String> {
    let normalized = crate::ir::normalize_path(entry)?;
    if normalized != entry.replace('\\', "/") || normalized != entry {
        return Err(format!("entrypoint path is not normalized: {entry}"));
    }
    Ok(normalized)
}

fn resolve_normalized_entry_from_root(
    canonical_root: &Path,
    entry: &str,
    normalized: &str,
) -> Result<PathBuf, String> {
    let mut candidate = canonical_root.to_path_buf();
    let components = normalized.split('/').collect::<Vec<_>>();
    let mut metadata = None;
    for (index, component) in components.iter().enumerate() {
        candidate.push(component);
        let current = candidate
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect entrypoint {entry}: {error}"))?;
        if current.file_type().is_symlink() {
            return Err(format!(
                "entrypoint path must not contain a symlink: {entry}"
            ));
        }
        if index + 1 < components.len() && !current.file_type().is_dir() {
            return Err(format!("entrypoint parent is not a directory: {entry}"));
        }
        metadata = Some(current);
    }
    let metadata = metadata.ok_or_else(|| "entrypoint path must not be empty".to_owned())?;
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
    if !canonical_entry.starts_with(canonical_root) {
        return Err(format!("entrypoint escapes the project root: {entry}"));
    }
    Ok(canonical_entry)
}

pub(crate) fn scan(root: &Path) -> Result<Inventory, String> {
    crate::scanner::scan(root)
}

pub(crate) fn analyze(root: &Path, entry: &str) -> Result<AnalysisResult, String> {
    let config = load_config(root).map_err(|errors| errors.join("; "))?;
    if !config
        .entrypoints
        .iter()
        .any(|configured| configured == entry)
    {
        return Err(format!(
            "entrypoint is not declared in project.toml: {entry}"
        ));
    }
    let lock = load_lock(root).map_err(|errors| errors.join("; "))?;
    let (canonical_root, entry_path) = resolve_entry(root, entry)?;
    let source = std::fs::read(&entry_path)
        .map_err(|error| format!("cannot read entrypoint {entry}: {error}"))?;
    let mut plan = crate::frontend::lower(entry, &source, config.policy.unknown_interpreter)?;
    crate::frontend::bind_interpreter_pins(&mut plan, &lock.interpreters)?;
    let mut evidence = Evidence::from_plan(&plan, entry, &source)?;
    let directory = canonical_root.join(".deshell");
    ensure_existing_directory(&directory)?;
    let source_digest = crate::digest::sha256(&source);
    let plan_digest = evidence.plan_digest.clone();
    let relative_directory = format!(".deshell/artifacts/{source_digest}/{plan_digest}");
    let artifact_root = canonical_root.join(&relative_directory);
    ensure_directory(&directory.join("artifacts"))?;
    ensure_directory(&directory.join("artifacts").join(&source_digest))?;
    ensure_directory(&artifact_root)?;
    let plan_path = artifact_root.join("plan.json");
    let evidence_path = artifact_root.join("evidence.json");
    let plan_bytes = plan.encode_pretty()?;
    let mut proposals = Vec::new();
    match plan_path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            proposals.push(crate::patch::prepare_create(
                &plan_path,
                plan_bytes.clone(),
                0o644,
            )?);
        }
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            if std::fs::read(&plan_path).map_err(|error| error.to_string())? != plan_bytes {
                return Err(format!(
                    "content-addressed plan bytes do not match {}",
                    plan_path.display()
                ));
            }
        }
        Ok(_) => {
            return Err(format!(
                "artifact plan is not a regular file: {}",
                plan_path.display()
            ));
        }
        Err(error) => return Err(format!("cannot inspect {}: {error}", plan_path.display())),
    }
    match evidence_path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            proposals.push(crate::patch::prepare_create(
                &evidence_path,
                evidence.encode_pretty()?,
                0o644,
            )?);
        }
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            evidence = Evidence::decode(
                &std::fs::read(&evidence_path).map_err(|error| error.to_string())?,
            )
            .map_err(|errors| errors.join("; "))?;
            evidence
                .validate_against(&plan, &source)
                .map_err(|errors| errors.join("; "))?;
        }
        Ok(_) => {
            return Err(format!(
                "artifact evidence is not a regular file: {}",
                evidence_path.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "cannot inspect {}: {error}",
                evidence_path.display()
            ));
        }
    }
    let mut manifest = load_manifest(&canonical_root).map_err(|errors| errors.join("; "))?;
    manifest.entries.retain(|value| value.entrypoint != entry);
    manifest.entries.push(ManifestEntry {
        entrypoint: entry.into(),
        source_digest,
        plan_digest,
        plan_path: format!("{relative_directory}/plan.json"),
        evidence_path: format!("{relative_directory}/evidence.json"),
    });
    manifest
        .entries
        .sort_by(|left, right| left.entrypoint.cmp(&right.entrypoint));
    proposals.push(prepare_write(
        &directory.join("manifest.json"),
        manifest.encode_pretty()?,
    )?);
    crate::patch::apply_all(&proposals)?;
    Ok(AnalysisResult {
        plan,
        evidence,
        plan_path,
        evidence_path,
    })
}

pub(crate) fn load_artifacts(root: &Path) -> Result<(Plan, Evidence), Vec<String>> {
    let manifest = load_manifest(root)?;
    match manifest.entries.as_slice() {
        [entry] => load_manifest_entry(root, entry),
        [] => Err(vec!["manifest contains no analyzed entrypoint".into()]),
        _ => Err(vec![
            "manifest contains multiple entrypoints; select one with --entry".into(),
        ]),
    }
}

pub(crate) fn load_entry_artifacts(
    root: &Path,
    entrypoint: &str,
) -> Result<(Plan, Evidence), Vec<String>> {
    let manifest = load_manifest(root)?;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| entry.entrypoint == entrypoint)
        .ok_or_else(|| vec![format!("entrypoint is not analyzed: {entrypoint}")])?;
    load_manifest_entry(root, entry)
}

fn load_manifest_entry(
    root: &Path,
    entry: &ManifestEntry,
) -> Result<(Plan, Evidence), Vec<String>> {
    load_validated_entry(root, entry).map(|entry| (entry.plan, entry.evidence))
}

fn load_validated_entry(root: &Path, entry: &ManifestEntry) -> Result<ValidatedEntry, Vec<String>> {
    let reader = ProjectReader::new(root).map_err(|error| vec![error])?;
    load_validated_entry_with_reader(&reader, entry)
}

fn load_validated_entry_with_reader(
    reader: &ProjectReader,
    entry: &ManifestEntry,
) -> Result<ValidatedEntry, Vec<String>> {
    let plan_bytes = reader
        .read_file(&entry.plan_path)
        .map_err(|error| vec![error])?;
    let evidence_bytes = reader
        .read_file(&entry.evidence_path)
        .map_err(|error| vec![error])?;
    let plan = Plan::decode(&plan_bytes)?;
    let evidence = Evidence::decode(&evidence_bytes)?;
    if evidence.plan_digest != entry.plan_digest {
        return Err(vec![format!(
            "manifest plan digest mismatch for {}",
            entry.entrypoint
        )]);
    }
    if evidence.source.content_hash != entry.source_digest
        || evidence.source.path != entry.entrypoint
    {
        return Err(vec![format!(
            "manifest source binding mismatch for {}",
            entry.entrypoint
        )]);
    }
    let source_path = reader
        .entry_path(&entry.entrypoint)
        .map_err(|error| vec![error])?;
    let source = std::fs::read(&source_path).map_err(|error| {
        vec![format!(
            "cannot read manifest source {}: {error}",
            entry.entrypoint
        )]
    })?;
    evidence.validate_against(&plan, &source)?;
    Ok(ValidatedEntry {
        manifest: entry.clone(),
        plan,
        evidence,
        source,
    })
}

pub(crate) fn check(root: &Path) -> Result<(), Vec<String>> {
    ValidatedProject::load(root).map(|_| ())
}

pub(crate) fn save_evidence(root: &Path, evidence: &Evidence) -> Result<PathBuf, String> {
    let canonical_root = canonical_project_root(root)?;
    let manifest = load_manifest(&canonical_root).map_err(|errors| errors.join("; "))?;
    let entry = manifest
        .entries
        .iter()
        .find(|entry| {
            entry.entrypoint == evidence.source.path && entry.plan_digest == evidence.plan_digest
        })
        .ok_or("evidence does not belong to an active manifest entry")?;
    let plan_path = project_file_path(&canonical_root, &entry.plan_path)?;
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
    let path = project_file_path(&canonical_root, &entry.evidence_path)?;
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
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "path is not a regular non-symlink file: {}",
            path.display()
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

pub(crate) fn load_lock(root: &Path) -> Result<Lockfile, Vec<String>> {
    let input = read_project_utf8(root, "deshell.lock").map_err(|error| vec![error])?;
    Lockfile::decode(&input)
}

fn load_lock_snapshot(root: &Path) -> Result<(Lockfile, String), Vec<String>> {
    let bytes = read_project_file(root, "deshell.lock").map_err(|error| vec![error])?;
    let input = std::str::from_utf8(&bytes)
        .map_err(|_| vec!["project file is not valid UTF-8: deshell.lock".into()])?;
    let lock = Lockfile::decode(input)?;
    let digest = crate::digest::sha256(&bytes);
    Ok((lock, digest))
}

fn read_project_utf8(root: &Path, relative: &str) -> Result<String, String> {
    let bytes = read_project_file(root, relative)?;
    String::from_utf8(bytes).map_err(|_| format!("project file is not valid UTF-8: {relative}"))
}

fn read_project_file(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = project_file_path(root, relative)?;
    std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

pub(crate) fn project_file_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!("project file path is not normalized: {relative}"));
    }
    let root = canonical_project_root(root)?;
    project_file_path_from_root(&root, relative)
}

fn project_file_path_from_root(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!("project file path is not normalized: {relative}"));
    }
    let mut current = root.to_path_buf();
    let mut final_metadata = None;
    for component in relative.split('/') {
        current.push(component);
        let metadata = current
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "project file path must not contain a symlink: {relative}"
            ));
        }
        final_metadata = Some(metadata);
    }
    let metadata =
        final_metadata.ok_or_else(|| "project file path must not be empty".to_owned())?;
    if !metadata.file_type().is_file() {
        return Err(format!("project path is not a regular file: {relative}"));
    }
    let canonical = current
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", current.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!("project file path escapes root: {relative}"));
    }
    Ok(canonical)
}

pub(crate) fn project_directory_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!(
            "project directory path is not normalized: {relative}"
        ));
    }
    let root = canonical_project_root(root)?;
    project_directory_path_from_root(&root, relative)
}

fn project_directory_path_from_root(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let normalized = crate::ir::normalize_path(relative)?;
    if normalized != relative {
        return Err(format!(
            "project directory path is not normalized: {relative}"
        ));
    }
    let mut current = root.to_path_buf();
    for component in relative.split('/') {
        current.push(component);
        let metadata = current
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
            return Err(format!(
                "project directory path must contain only regular directories: {relative}"
            ));
        }
    }
    let canonical = current
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", current.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!("project directory path escapes root: {relative}"));
    }
    Ok(canonical)
}

fn canonical_project_root(root: &Path) -> Result<PathBuf, String> {
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect project root {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "project root is not a regular non-symlink directory: {}",
            root.display()
        ));
    }
    root.canonicalize()
        .map_err(|error| format!("cannot resolve project root {}: {error}", root.display()))
}

fn load_validated_scenarios(
    root: &Path,
    reader: Option<&ProjectReader>,
    project_limits: Option<crate::config::ResourceLimits>,
) -> Result<Vec<ValidatedScenario>, Vec<String>> {
    let directory = if let Some(reader) = reader {
        reader.directory_path(".deshell/scenarios")
    } else {
        project_directory_path(root, ".deshell/scenarios")
    }
    .map_err(|error| vec![error])?;
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
    let mut scenarios = Vec::new();
    for path in paths {
        match read_utf8(&path)
            .map_err(|error| vec![error])
            .and_then(|input| Scenario::decode(&input))
        {
            Err(mut values) => errors.append(&mut values),
            Ok(scenario) if !names.insert(scenario.name.clone()) => {
                errors.push(format!("duplicate scenario name: {}", scenario.name))
            }
            Ok(scenario) => {
                if let Some(project_limits) = project_limits
                    && !scenario.limits.narrows(project_limits)
                {
                    errors.push(format!(
                        "scenario {} resource limits may only narrow project limits",
                        scenario.name
                    ));
                }
                match scenario.digest() {
                    Ok(digest) => scenarios.push(ValidatedScenario { scenario, digest }),
                    Err(error) => {
                        errors.push(format!("cannot digest scenario {}: {error}", scenario.name))
                    }
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(scenarios)
    } else {
        Err(errors)
    }
}

fn load_validated_replay(root: &Path) -> Result<Option<crate::replay::ReplayStore>, Vec<String>> {
    let path = root.join(".deshell/replay.json");
    let metadata = match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
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
        Ok(Some(store))
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
        assert_eq!(first.created.len(), 4);
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
        assert!(init(directory.path()).is_err());
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
            std::fs::create_dir(directory.path().join("real")).unwrap();
            write(directory.path(), "real/nested.sh", b"true\n");
            std::os::unix::fs::symlink("real", directory.path().join("alias")).unwrap();
            assert!(
                resolve_entry(directory.path(), "alias/nested.sh")
                    .unwrap_err()
                    .contains("symlink")
            );
        }
    }

    #[test]
    fn analyze_persists_canonical_bound_artifacts_and_check_validates_them() {
        let directory = configured_project(b"#!/bin/sh\n/usr/bin/printf '%s' \"$NAME\"\n");
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
        let plan_path = analyze(directory.path(), "build.sh").unwrap().plan_path;
        let mut plan = std::fs::read_to_string(&plan_path).unwrap();
        plan.push_str("{}\n");
        std::fs::write(plan_path, plan).unwrap();
        assert!(check(directory.path()).is_err());
    }

    #[test]
    fn validated_project_reuses_one_snapshot_and_fresh_validation_detects_tampering() {
        let directory = configured_project(b"true\n");
        analyze(directory.path(), "build.sh").unwrap();
        let project = ValidatedProject::load(directory.path()).unwrap();
        let first = project.entry("build.sh").unwrap();
        let second = project.entry("build.sh").unwrap();
        assert!(std::ptr::eq(first, second));
        assert_eq!(first.source, b"true\n");
        assert_eq!(project.manifest.entries, vec![first.manifest.clone()]);
        assert_eq!(project.scenarios.len(), 1);
        assert_eq!(
            project.runtime_lock_digest,
            crate::digest::sha256(&std::fs::read(directory.path().join("deshell.lock")).unwrap())
        );

        write(directory.path(), "build.sh", b"false\n");
        assert_eq!(project.entry("build.sh").unwrap().source, b"true\n");
        assert!(
            ValidatedProject::load(directory.path())
                .unwrap_err()
                .join("; ")
                .contains("source digest mismatch")
        );
    }

    #[test]
    fn check_binds_delegated_nodes_to_the_current_runtime_lock() {
        let directory = configured_project(b"true\n");
        analyze(directory.path(), "build.sh").unwrap();
        let lock_path = directory.path().join("deshell.lock");
        let lock = std::fs::read_to_string(&lock_path).unwrap();
        let parsed = Lockfile::decode(&lock).unwrap();
        let changed = lock.replace(
            &format!("posix_sh = \"{}\"", parsed.interpreters.posix_sh),
            &format!("posix_sh = \"sha256:{}\"", "0".repeat(64)),
        );
        assert_ne!(lock, changed);
        std::fs::write(lock_path, changed).unwrap();

        assert!(
            check(directory.path())
                .unwrap_err()
                .join("; ")
                .contains("interpreter pins do not match")
        );
    }

    #[test]
    fn scan_uses_project_boundary_and_stable_inventory() {
        let directory = tempfile::tempdir().unwrap();
        write(directory.path(), "z.sh", b"true\n");
        write(directory.path(), "a.fish", b"echo hello\n");
        let inventory = scan(directory.path()).unwrap();
        assert_eq!(
            inventory
                .findings
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
                scenario: "default".into(),
                key: crate::evidence::ObservationKey {
                    scenario_digest: "a".repeat(64),
                    provider_fingerprint: "b".repeat(64),
                    runtime_lock_digest: "c".repeat(64),
                },
                status: crate::evidence::ObservationStatus::Unavailable,
                provider: "test".into(),
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
    fn evidence_updates_reject_a_symlinked_project_root() {
        let directory = configured_project(b"true\n");
        let analysis = analyze(directory.path(), "build.sh").unwrap();
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("project");
        std::os::unix::fs::symlink(directory.path(), &alias).unwrap();

        assert!(
            save_evidence(&alias, &analysis.evidence)
                .unwrap_err()
                .contains("non-symlink")
        );
        assert_eq!(
            Evidence::decode(&std::fs::read(&analysis.evidence_path).unwrap()).unwrap(),
            analysis.evidence
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
