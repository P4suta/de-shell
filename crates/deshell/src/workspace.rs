use crate::config::{ExpectedFile, Fixture};
use std::path::Path;

pub(crate) struct PrivateWorkspace {
    _directory: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl PrivateWorkspace {
    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

/// Make a private, non-symlink snapshot suitable for a disposable provider.
/// The live project is never mounted writable or used as a process working tree.
pub(crate) fn private_snapshot(source: &Path) -> Result<PrivateWorkspace, String> {
    let source = canonical_directory(source)?;
    let directory = tempfile::Builder::new()
        .prefix("deshell-workspace-")
        .tempdir()
        .map_err(|error| format!("cannot create private workspace: {error}"))?;
    let root = directory.path().join("workspace");
    std::fs::create_dir(&root)
        .map_err(|error| format!("cannot create private workspace root: {error}"))?;
    for entry in walkdir::WalkDir::new(&source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || entry.file_name().to_str() != Some(".git")
        })
    {
        let entry = entry
            .map_err(|error| format!("cannot snapshot workspace {}: {error}", source.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&source)
            .map_err(|_| "workspace snapshot entry escaped its source root")?;
        let target = root.join(relative);
        let kind = entry.file_type();
        if kind.is_symlink() {
            return Err(format!(
                "private workspace refuses symlink: {}",
                relative.display()
            ));
        }
        if kind.is_dir() {
            std::fs::create_dir(&target).map_err(|error| {
                format!(
                    "cannot create snapshot directory {}: {error}",
                    target.display()
                )
            })?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|error| {
                format!("cannot copy snapshot file {}: {error}", relative.display())
            })?;
            let permissions = entry
                .metadata()
                .map_err(|error| {
                    format!(
                        "cannot inspect snapshot file {}: {error}",
                        relative.display()
                    )
                })?
                .permissions();
            std::fs::set_permissions(&target, permissions).map_err(|error| {
                format!(
                    "cannot preserve snapshot permissions {}: {error}",
                    relative.display()
                )
            })?;
        } else {
            return Err(format!(
                "private workspace refuses non-regular entry: {}",
                relative.display()
            ));
        }
    }
    Ok(PrivateWorkspace {
        _directory: directory,
        root,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileState {
    pub path: String,
    pub sha256: String,
    pub executable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Snapshot {
    pub files: Vec<FileState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChangeKind {
    Created,
    Modified,
    Removed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Change {
    pub path: String,
    pub kind: ChangeKind,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
}

pub(crate) fn materialize(root: &Path, fixtures: &[Fixture]) -> Result<(), String> {
    let root = canonical_directory(root)?;
    let mut seen = std::collections::BTreeSet::new();
    for fixture in fixtures {
        validate_path(&fixture.path)?;
        if !seen.insert(fixture.path.clone()) {
            return Err(format!("duplicate fixture path: {}", fixture.path));
        }
    }
    for path in &seen {
        let prefix = format!("{path}/");
        if seen.iter().any(|candidate| candidate.starts_with(&prefix)) {
            return Err(format!(
                "fixture path is both a file and a directory prefix: {path}"
            ));
        }
    }
    for fixture in fixtures {
        ensure_parents(&root, &fixture.path)?;
    }
    let mut proposals = Vec::new();
    for fixture in fixtures {
        let permissions = if fixture.executable { 0o755 } else { 0o644 };
        proposals.push(crate::patch::prepare_create(
            &root.join(&fixture.path),
            fixture.contents.bytes()?,
            permissions,
        )?);
    }
    crate::patch::apply_all(&proposals)
}

pub(crate) fn capture(root: &Path) -> Result<Snapshot, String> {
    let root = canonical_directory(root)?;
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !matches!(entry.file_name().to_str(), Some(".deshell" | ".git"))
        })
    {
        let entry =
            entry.map_err(|error| format!("cannot walk workspace {}: {error}", root.display()))?;
        if entry.depth() == 0 {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(format!(
                "workspace capture refuses symlink: {}",
                entry.path().display()
            ));
        }
        if entry.file_type().is_dir() {
            continue;
        }
        if !entry.file_type().is_file() {
            return Err(format!(
                "workspace capture refuses non-regular entry: {}",
                entry.path().display()
            ));
        }
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|_| "workspace entry escaped root")?;
        let relative = relative
            .to_str()
            .ok_or_else(|| format!("workspace path is not valid UTF-8: {}", relative.display()))?
            .replace('\\', "/");
        validate_path(&relative)?;
        let bytes = std::fs::read(entry.path())
            .map_err(|error| format!("cannot read workspace file {relative}: {error}"))?;
        let metadata = entry
            .path()
            .symlink_metadata()
            .map_err(|error| format!("cannot inspect workspace file {relative}: {error}"))?;
        files.push(FileState {
            path: relative,
            sha256: crate::digest::sha256(&bytes),
            executable: executable(&metadata),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Snapshot { files })
}

pub(crate) fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let before: std::collections::BTreeMap<_, _> = before
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let after: std::collections::BTreeMap<_, _> = after
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let paths: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
    paths
        .into_iter()
        .filter_map(|path| match (before.get(path), after.get(path)) {
            (None, Some(file)) => Some(Change {
                path: path.into(),
                kind: ChangeKind::Created,
                before_sha256: None,
                after_sha256: Some(file.sha256.clone()),
            }),
            (Some(file), None) => Some(Change {
                path: path.into(),
                kind: ChangeKind::Removed,
                before_sha256: Some(file.sha256.clone()),
                after_sha256: None,
            }),
            (Some(left), Some(right))
                if left.sha256 != right.sha256 || left.executable != right.executable =>
            {
                Some(Change {
                    path: path.into(),
                    kind: ChangeKind::Modified,
                    before_sha256: Some(left.sha256.clone()),
                    after_sha256: Some(right.sha256.clone()),
                })
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn validate_expected(root: &Path, expected: &[ExpectedFile]) -> Result<(), Vec<String>> {
    let root = canonical_directory(root).map_err(|error| vec![error])?;
    let mut errors = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for expected in expected {
        if let Err(error) = validate_path(&expected.path) {
            errors.push(error);
            continue;
        }
        if !seen.insert(&expected.path) {
            errors.push(format!("duplicate expected file path: {}", expected.path));
            continue;
        }
        if !crate::digest::valid_sha256(&expected.sha256) {
            errors.push(format!(
                "expected file digest is invalid: {}",
                expected.path
            ));
            continue;
        }
        let mut path = root.clone();
        let components = expected.path.split('/').collect::<Vec<_>>();
        let mut invalid_component = false;
        for (index, component) in components.iter().enumerate() {
            path.push(component);
            let metadata = match path.symlink_metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    errors.push(format!(
                        "cannot inspect expected file {}: {error}",
                        expected.path
                    ));
                    invalid_component = true;
                    break;
                }
            };
            let final_component = index + 1 == components.len();
            if metadata.file_type().is_symlink()
                || (!final_component && !metadata.file_type().is_dir())
                || (final_component && !metadata.file_type().is_file())
            {
                errors.push(format!(
                    "expected file path contains a symlink or non-regular component: {}",
                    expected.path
                ));
                invalid_component = true;
                break;
            }
        }
        if invalid_component {
            continue;
        }
        let canonical = match path.canonicalize() {
            Ok(path) if path.starts_with(&root) => path,
            Ok(_) => {
                errors.push(format!(
                    "expected file escapes workspace: {}",
                    expected.path
                ));
                continue;
            }
            Err(error) => {
                errors.push(format!(
                    "cannot resolve expected file {}: {error}",
                    expected.path
                ));
                continue;
            }
        };
        match std::fs::read(&canonical) {
            Ok(bytes) if crate::digest::sha256(&bytes) == expected.sha256 => {}
            Ok(_) => errors.push(format!("expected file digest mismatch: {}", expected.path)),
            Err(error) => errors.push(format!(
                "cannot read expected file {}: {error}",
                expected.path
            )),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn canonical_directory(root: &Path) -> Result<std::path::PathBuf, String> {
    let metadata = root
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect workspace {}: {error}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "workspace is not a regular directory: {}",
            root.display()
        ));
    }
    root.canonicalize()
        .map_err(|error| format!("cannot resolve workspace {}: {error}", root.display()))
}

fn validate_path(path: &str) -> Result<(), String> {
    let normalized = crate::ir::normalize_path(path)?;
    if normalized != path {
        Err(format!("workspace path is not normalized: {path}"))
    } else {
        Ok(())
    }
}

fn ensure_parents(root: &Path, relative: &str) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.split('/').collect();
    for component in &components[..components.len() - 1] {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => {
                return Err(format!(
                    "fixture parent is not a regular directory: {}",
                    current.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    format!(
                        "cannot create fixture directory {}: {error}",
                        current.display()
                    )
                })?
            }
            Err(error) => {
                return Err(format!(
                    "cannot inspect fixture directory {}: {error}",
                    current.display()
                ));
            }
        }
        let canonical = current.canonicalize().map_err(|error| {
            format!(
                "cannot resolve fixture directory {}: {error}",
                current.display()
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(format!("fixture directory escapes workspace: {relative}"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_project_relative_unique_and_preserve_executable_intent() {
        let directory = tempfile::tempdir().unwrap();
        materialize(
            directory.path(),
            &[
                Fixture {
                    path: "input/data.txt".into(),
                    contents: "hello".into(),
                    executable: false,
                },
                Fixture {
                    path: "tool.sh".into(),
                    contents: "#!/bin/sh\n".into(),
                    executable: true,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("input/data.txt")).unwrap(),
            b"hello"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_ne!(
                std::fs::metadata(directory.path().join("tool.sh"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        assert!(
            materialize(
                directory.path(),
                &[Fixture {
                    path: "../escape".into(),
                    contents: "bad".into(),
                    executable: false
                }]
            )
            .is_err()
        );
        assert!(
            materialize(
                directory.path(),
                &[
                    Fixture {
                        path: "same".into(),
                        contents: "one".into(),
                        executable: false
                    },
                    Fixture {
                        path: "same".into(),
                        contents: "two".into(),
                        executable: false
                    },
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn snapshots_are_sorted_ignore_internal_metadata_and_reject_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("z")).unwrap();
        std::fs::write(directory.path().join("z/b"), b"b").unwrap();
        std::fs::write(directory.path().join("a"), b"a").unwrap();
        std::fs::create_dir(directory.path().join(".deshell")).unwrap();
        std::fs::write(directory.path().join(".deshell/private"), b"ignore").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("a", directory.path().join("link")).unwrap();
        #[cfg(unix)]
        {
            assert!(capture(directory.path()).unwrap_err().contains("symlink"));
            std::fs::remove_file(directory.path().join("link")).unwrap();
        }
        let snapshot = capture(directory.path()).unwrap();
        assert_eq!(
            snapshot
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "z/b"]
        );
        assert_eq!(snapshot.files[0].sha256, crate::digest::sha256(b"a"));
    }

    #[test]
    fn workspace_diff_reports_created_modified_and_removed_in_path_order() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a"), b"old").unwrap();
        std::fs::write(directory.path().join("gone"), b"gone").unwrap();
        let before = capture(directory.path()).unwrap();
        std::fs::write(directory.path().join("a"), b"new").unwrap();
        std::fs::remove_file(directory.path().join("gone")).unwrap();
        std::fs::write(directory.path().join("new"), b"new").unwrap();
        let changes = diff(&before, &capture(directory.path()).unwrap());
        assert_eq!(
            changes
                .iter()
                .map(|change| (change.path.as_str(), &change.kind))
                .collect::<Vec<_>>(),
            vec![
                ("a", &ChangeKind::Modified),
                ("gone", &ChangeKind::Removed),
                ("new", &ChangeKind::Created),
            ]
        );
    }

    #[test]
    fn expected_files_are_digest_checked_and_symlinks_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("out"), b"value").unwrap();
        let expected = [ExpectedFile {
            path: "out".into(),
            sha256: crate::digest::sha256(b"value"),
        }];
        validate_expected(directory.path(), &expected).unwrap();
        let wrong = [ExpectedFile {
            path: "out".into(),
            sha256: "0".repeat(64),
        }];
        assert!(
            validate_expected(directory.path(), &wrong)
                .unwrap_err()
                .join("; ")
                .contains("digest mismatch")
        );
        #[cfg(unix)]
        {
            std::fs::remove_file(directory.path().join("out")).unwrap();
            std::os::unix::fs::symlink("missing", directory.path().join("out")).unwrap();
            assert!(validate_expected(directory.path(), &expected).is_err());
            std::fs::create_dir(directory.path().join("inside")).unwrap();
            std::fs::write(directory.path().join("inside/value"), b"value").unwrap();
            std::os::unix::fs::symlink("inside", directory.path().join("inside-link")).unwrap();
            let through_parent = [ExpectedFile {
                path: "inside-link/value".into(),
                sha256: crate::digest::sha256(b"value"),
            }];
            assert!(validate_expected(directory.path(), &through_parent).is_err());
        }
    }
}
