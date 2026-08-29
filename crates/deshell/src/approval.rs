use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const ZERO_PINNED_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Subject {
    Scenario { name: String, path: String },
    Matrix { id: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    pub schema_version: u32,
    pub approval_digest: String,
    pub subject: Subject,
    pub subject_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewStatus {
    Draft,
    Approved,
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Review {
    pub kind: String,
    pub name: String,
    pub path: Option<String>,
    pub digest: String,
    pub status: ReviewStatus,
    pub approval_digest: Option<String>,
}

impl Approval {
    fn computed_digest(&self) -> Result<String, String> {
        let mut unsigned = self.clone();
        unsigned.approval_digest = ZERO_PINNED_DIGEST.into();
        Ok(format!("sha256:{}", canonical_digest(&unsigned)?))
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("approval schema_version must be 1".into());
        }
        if !crate::digest::valid_pinned_sha256(&self.approval_digest)
            || !crate::digest::valid_pinned_sha256(&self.subject_digest)
        {
            return Err("approval digests must use sha256:<64 lowercase hex>".into());
        }
        match &self.subject {
            Subject::Scenario { name, path } => {
                if name.trim().is_empty() {
                    return Err("approval scenario name must not be empty".into());
                }
                let normalized = crate::ir::normalize_path(path)?;
                if normalized != *path || !path.starts_with(".deshell/scenarios/") {
                    return Err("approval scenario path is not canonical".into());
                }
            }
            Subject::Matrix { id } => {
                if !portable_id(id) {
                    return Err("approval matrix id is not portable".into());
                }
            }
        }
        if self.computed_digest()? != self.approval_digest {
            return Err("approval digest does not match its canonical content".into());
        }
        Ok(())
    }
}

pub(crate) fn scenario_reviews(root: &Path) -> Result<Vec<Review>, String> {
    let approvals = load_approvals(root)?;
    let mut output = Vec::new();
    for (path, scenario) in load_scenarios(root)? {
        let subject = Subject::Scenario {
            name: scenario.name.clone(),
            path: path.clone(),
        };
        let digest = scenario_review_digest(&path, &scenario)?;
        let (status, approval_digest) = review_state(
            &approvals,
            &subject,
            &digest,
            cfg!(test) && scenario.approval == crate::config::ScenarioApproval::Approved,
        )?;
        output.push(Review {
            kind: "scenario".into(),
            name: scenario.name,
            path: Some(path),
            digest,
            status,
            approval_digest,
        });
    }
    output.sort_by(|left, right| left.name.cmp(&right.name).then(left.path.cmp(&right.path)));
    Ok(output)
}

pub(crate) fn matrix_reviews(root: &Path) -> Result<Vec<Review>, String> {
    let approvals = load_approvals(root)?;
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    let mut output = Vec::new();
    for cell in config.platform_cells {
        let subject = Subject::Matrix {
            id: cell.id.clone(),
        };
        let digest = matrix_review_digest(&cell)?;
        let (status, approval_digest) = review_state(
            &approvals,
            &subject,
            &digest,
            cfg!(test) && cell.approval == crate::config::Approval::Approved,
        )?;
        output.push(Review {
            kind: "matrix".into(),
            name: cell.id,
            path: None,
            digest,
            status,
            approval_digest,
        });
    }
    output.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(output)
}

pub(crate) fn approve_scenario(
    root: &Path,
    name: &str,
    supplied_digest: &str,
) -> Result<Approval, String> {
    let matches = load_scenarios(root)?
        .into_iter()
        .filter(|(_, scenario)| scenario.name == name)
        .collect::<Vec<_>>();
    let [(path, scenario)] = matches.as_slice() else {
        return Err(if matches.is_empty() {
            format!("scenario not found: {name}")
        } else {
            format!("scenario name is ambiguous: {name}")
        });
    };
    let subject = Subject::Scenario {
        name: name.into(),
        path: path.clone(),
    };
    persist_approval(
        root,
        subject,
        scenario_review_digest(path, scenario)?,
        supplied_digest,
    )
}

pub(crate) fn approve_matrix(
    root: &Path,
    id: &str,
    supplied_digest: &str,
) -> Result<Approval, String> {
    let config = crate::project::load_config(root).map_err(|errors| errors.join("; "))?;
    let cell = config
        .platform_cells
        .iter()
        .find(|cell| cell.id == id)
        .ok_or_else(|| format!("matrix cell not found: {id}"))?;
    persist_approval(
        root,
        Subject::Matrix { id: id.into() },
        matrix_review_digest(cell)?,
        supplied_digest,
    )
}

pub(crate) fn scenario_approval(
    root: &Path,
    path: &str,
    scenario: &crate::config::Scenario,
) -> Result<Option<String>, String> {
    let subject = Subject::Scenario {
        name: scenario.name.clone(),
        path: path.into(),
    };
    current_approval(
        root,
        &subject,
        &scenario_review_digest(path, scenario)?,
        cfg!(test) && scenario.approval == crate::config::ScenarioApproval::Approved,
    )
}

pub(crate) fn matrix_approval(
    root: &Path,
    cell: &crate::config::PlatformCell,
) -> Result<Option<String>, String> {
    current_approval(
        root,
        &Subject::Matrix {
            id: cell.id.clone(),
        },
        &matrix_review_digest(cell)?,
        cfg!(test) && cell.approval == crate::config::Approval::Approved,
    )
}

fn current_approval(
    root: &Path,
    subject: &Subject,
    digest: &str,
    inline_approved: bool,
) -> Result<Option<String>, String> {
    let approvals = load_approvals(root)?;
    let (_, approval) = review_state(&approvals, subject, digest, inline_approved)?;
    Ok(approval)
}

fn review_state(
    approvals: &[Approval],
    subject: &Subject,
    digest: &str,
    inline_approved: bool,
) -> Result<(ReviewStatus, Option<String>), String> {
    if let Some(approval) = approvals
        .iter()
        .find(|approval| &approval.subject == subject && approval.subject_digest == digest)
    {
        return Ok((
            ReviewStatus::Approved,
            Some(approval.approval_digest.clone()),
        ));
    }
    if inline_approved {
        let approval = signed_approval(subject.clone(), digest.into())?;
        return Ok((ReviewStatus::Approved, Some(approval.approval_digest)));
    }
    if approvals
        .iter()
        .any(|approval| &approval.subject == subject)
    {
        Ok((ReviewStatus::Stale, None))
    } else {
        Ok((ReviewStatus::Draft, None))
    }
}

fn persist_approval(
    root: &Path,
    subject: Subject,
    actual_digest: String,
    supplied_digest: &str,
) -> Result<Approval, String> {
    if supplied_digest != actual_digest {
        return Err(format!(
            "review digest mismatch (expected {actual_digest}, supplied {supplied_digest})"
        ));
    }
    let approval = signed_approval(subject, actual_digest)?;
    let raw_digest = approval
        .approval_digest
        .strip_prefix("sha256:")
        .expect("validated pinned digest");
    let root = canonical_root(root)?;
    let deshell = safe_existing_directory(&root.join(".deshell"))?;
    let approvals = ensure_child_directory(&deshell, "approvals")?;
    let sha256 = ensure_child_directory(&approvals, "sha256")?;
    let path = sha256.join(format!("{raw_digest}.json"));
    let bytes = pretty_bytes(&approval)?;
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            let current = std::fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            if current != bytes {
                return Err(format!(
                    "content-addressed approval differs at {}",
                    path.display()
                ));
            }
        }
        Ok(_) => {
            return Err(format!(
                "approval target is not a regular file: {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let proposal = crate::patch::prepare_create(&path, bytes, 0o644)?;
            if let Err(error) = crate::patch::apply_all(&[proposal]) {
                let mut matched_concurrent_write = false;
                for _ in 0..32 {
                    match std::fs::read(&path) {
                        Ok(current) if current == pretty_bytes(&approval)? => {
                            matched_concurrent_write = true;
                            break;
                        }
                        Ok(_) => break,
                        Err(read_error) if read_error.kind() == std::io::ErrorKind::NotFound => {
                            std::thread::yield_now();
                        }
                        Err(_) => break,
                    }
                }
                if !matched_concurrent_write {
                    return Err(error);
                }
            }
        }
        Err(error) => return Err(format!("cannot inspect {}: {error}", path.display())),
    }
    Ok(approval)
}

fn signed_approval(subject: Subject, subject_digest: String) -> Result<Approval, String> {
    let mut approval = Approval {
        schema_version: 1,
        approval_digest: ZERO_PINNED_DIGEST.into(),
        subject,
        subject_digest,
    };
    approval.approval_digest = approval.computed_digest()?;
    approval.validate()?;
    Ok(approval)
}

fn load_scenarios(root: &Path) -> Result<Vec<(String, crate::config::Scenario)>, String> {
    let root = canonical_root(root)?;
    let directory = safe_existing_directory(&root.join(".deshell/scenarios"))?;
    let mut entries = std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .map(|entry| entry.map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut output = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(format!("unsafe scenario file: {}", path.display()));
        }
        let input = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let scenario =
            crate::config::Scenario::decode(&input).map_err(|errors| errors.join("; "))?;
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| "scenario escaped project root".to_owned())?
            .to_str()
            .ok_or("scenario path is not UTF-8")?
            .replace('\\', "/");
        output.push((relative, scenario));
    }
    Ok(output)
}

fn load_approvals(root: &Path) -> Result<Vec<Approval>, String> {
    let root = canonical_root(root)?;
    let directory = root.join(".deshell/approvals/sha256");
    let metadata = match directory.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot inspect {}: {error}", directory.display())),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "approval directory is unsafe: {}",
            directory.display()
        ));
    }
    let mut paths = std::fs::read_dir(&directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut output = Vec::new();
    for path in paths {
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let metadata = path
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(format!("approval artifact is unsafe: {}", path.display()));
        }
        let approval: Approval = crate::strict_json::decode(
            &std::fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?,
        )?;
        approval.validate()?;
        let expected = format!(
            "{}.json",
            approval
                .approval_digest
                .strip_prefix("sha256:")
                .expect("validated approval digest")
        );
        if path.file_name().and_then(|value| value.to_str()) != Some(expected.as_str()) {
            return Err(format!(
                "approval filename does not match digest: {}",
                path.display()
            ));
        }
        output.push(approval);
    }
    Ok(output)
}

fn scenario_review_digest(
    path: &str,
    scenario: &crate::config::Scenario,
) -> Result<String, String> {
    let value = serde_json::json!({
        "contract": "deshell-scenario-review-v1",
        "path": path,
        "scenario": scenario,
    });
    Ok(format!("sha256:{}", canonical_value_digest(&value)?))
}

fn matrix_review_digest(cell: &crate::config::PlatformCell) -> Result<String, String> {
    let value = serde_json::json!({
        "cell": cell,
        "contract": "deshell-matrix-review-v1",
    });
    Ok(format!("sha256:{}", canonical_value_digest(&value)?))
}

fn canonical_digest<T: Serialize>(value: &T) -> Result<String, String> {
    canonical_value_digest(&serde_json::to_value(value).map_err(|error| error.to_string())?)
}

fn canonical_value_digest(value: &serde_json::Value) -> Result<String, String> {
    Ok(crate::digest::sha256(
        &crate::canonical_json::canonical_bytes(value)?,
    ))
}

fn pretty_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    crate::canonical_json::pretty_bytes(
        &serde_json::to_value(value).map_err(|error| error.to_string())?,
    )
}

fn canonical_root(root: &Path) -> Result<PathBuf, String> {
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

fn safe_existing_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        Err(format!("path is not a safe directory: {}", path.display()))
    } else {
        Ok(path.to_path_buf())
    }
}

fn ensure_child_directory(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let path = parent.join(name);
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(path)
        }
        Ok(_) => Err(format!("path is not a safe directory: {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(&path) {
                Ok(()) => Ok(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    safe_existing_directory(&path)
                }
                Err(error) => Err(format!("cannot create {}: {error}", path.display())),
            }
        }
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn portable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialized_project() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("build.sh"),
            b"#!/bin/sh\n/usr/bin/printf '%s\\n' ok\n",
        )
        .unwrap();
        crate::project::init_cli(directory.path(), &[], crate::project::InitTarget::Auto).unwrap();
        directory
    }

    #[test]
    fn approval_requires_the_displayed_digest_and_becomes_stale_after_change() {
        let directory = initialized_project();
        let review = scenario_reviews(directory.path()).unwrap().remove(0);
        let wrong = format!("sha256:{}", "0".repeat(64));
        assert!(
            approve_scenario(directory.path(), &review.name, &wrong)
                .unwrap_err()
                .contains("review digest mismatch")
        );
        assert!(!directory.path().join(".deshell/approvals").exists());

        let approval = approve_scenario(directory.path(), &review.name, &review.digest).unwrap();
        assert_eq!(
            scenario_reviews(directory.path()).unwrap()[0].status,
            ReviewStatus::Approved
        );
        assert!(
            directory
                .path()
                .join(".deshell/approvals/sha256")
                .join(format!(
                    "{}.json",
                    approval.approval_digest.trim_start_matches("sha256:")
                ))
                .is_file()
        );

        let scenario_path = directory.path().join(review.path.unwrap());
        let changed = std::fs::read_to_string(&scenario_path)
            .unwrap()
            .replace("timeout_ms = 30000", "timeout_ms = 31000");
        std::fs::write(scenario_path, changed).unwrap();
        assert_eq!(
            scenario_reviews(directory.path()).unwrap()[0].status,
            ReviewStatus::Stale
        );

        let matrix = matrix_reviews(directory.path()).unwrap().remove(0);
        approve_matrix(directory.path(), &matrix.name, &matrix.digest).unwrap();
        assert_eq!(
            matrix_reviews(directory.path()).unwrap()[0].status,
            ReviewStatus::Approved
        );
        let config_path = directory.path().join(".deshell/project.toml");
        let changed = std::fs::read_to_string(&config_path)
            .unwrap()
            .replace("runtime = \"native\"", "runtime = \"native-v2\"");
        std::fs::write(config_path, changed).unwrap();
        assert_eq!(
            matrix_reviews(directory.path()).unwrap()[0].status,
            ReviewStatus::Stale
        );
    }

    #[test]
    fn identical_parallel_approval_updates_are_idempotent() {
        let directory = initialized_project();
        let review = scenario_reviews(directory.path()).unwrap().remove(0);
        let root = directory.path().to_path_buf();
        let handles = (0..8)
            .map(|_| {
                let root = root.clone();
                let name = review.name.clone();
                let digest = review.digest.clone();
                std::thread::spawn(move || approve_scenario(&root, &name, &digest))
            })
            .collect::<Vec<_>>();
        let approvals = handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        assert!(
            approvals
                .windows(2)
                .all(|pair| pair[0].approval_digest == pair[1].approval_digest)
        );
        assert_eq!(
            std::fs::read_dir(root.join(".deshell/approvals/sha256"))
                .unwrap()
                .count(),
            1
        );
    }
}
