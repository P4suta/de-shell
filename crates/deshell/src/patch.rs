use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Expectation {
    Existing(String),
    Missing,
}

#[derive(Clone, Debug)]
pub(crate) struct Proposal {
    pub path: PathBuf,
    pub expected: Expectation,
    pub replacement: Vec<u8>,
    pub permissions: u32,
}

pub(crate) fn prepare(_path: &Path, _replacement: Vec<u8>) -> Result<Proposal, String> {
    let metadata = _path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", _path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "patch target is not a regular non-symlink file: {}",
            _path.display()
        ));
    }
    let contents = std::fs::read(_path)
        .map_err(|error| format!("cannot read {}: {error}", _path.display()))?;
    Ok(Proposal {
        path: _path.to_path_buf(),
        expected: Expectation::Existing(crate::digest::sha256(&contents)),
        replacement: _replacement,
        permissions: file_permissions(&metadata),
    })
}

pub(crate) fn prepare_create(
    _path: &Path,
    _replacement: Vec<u8>,
    _permissions: u32,
) -> Result<Proposal, String> {
    match _path.symlink_metadata() {
        Ok(_) => return Err(format!("create target already exists: {}", _path.display())),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!(
                "cannot inspect create target {}: {error}",
                _path.display()
            ));
        }
        Err(_) => {}
    }
    let parent = _path
        .parent()
        .ok_or_else(|| format!("create target has no parent: {}", _path.display()))?;
    let metadata = parent
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect create parent {}: {error}", parent.display()))?;
    if !metadata.file_type().is_dir() {
        return Err(format!(
            "create target parent is not a directory: {}",
            parent.display()
        ));
    }
    Ok(Proposal {
        path: _path.to_path_buf(),
        expected: Expectation::Missing,
        replacement: _replacement,
        permissions: _permissions,
    })
}

pub(crate) fn apply_all(proposals: &[Proposal]) -> Result<(), String> {
    let validated = validate_all(proposals)?;
    let mut staged = Vec::with_capacity(validated.len());
    for item in &validated {
        let parent = item
            .canonical
            .parent()
            .ok_or_else(|| format!("patch target has no parent: {}", item.canonical.display()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| format!("cannot stage {}: {error}", item.canonical.display()))?;
        use std::io::Write as _;
        temporary
            .write_all(&item.proposal.replacement)
            .map_err(|error| format!("cannot stage {}: {error}", item.canonical.display()))?;
        temporary.flush().map_err(|error| {
            format!(
                "cannot flush stage for {}: {error}",
                item.canonical.display()
            )
        })?;
        set_permissions(temporary.path(), item.proposal.permissions)?;
        temporary.as_file().sync_all().map_err(|error| {
            format!(
                "cannot sync stage for {}: {error}",
                item.canonical.display()
            )
        })?;
        staged.push(temporary);
    }
    // Revalidate the complete read set after all writes are staged. No target
    // has been changed at this point.
    validate_current(&validated)?;

    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    for item in &validated {
        if matches!(item.proposal.expected, Expectation::Existing(_)) {
            let parent = item.canonical.parent().expect("validated target parent");
            let placeholder = tempfile::NamedTempFile::new_in(parent)
                .map_err(|error| format!("cannot allocate rollback path: {error}"))?;
            let backup = placeholder.path().to_path_buf();
            placeholder
                .close()
                .map_err(|error| format!("cannot release rollback path: {error}"))?;
            if let Err(error) = std::fs::rename(&item.canonical, &backup) {
                restore_backups(&backups);
                return Err(format!(
                    "cannot stage rollback for {}: {error}",
                    item.canonical.display()
                ));
            }
            backups.push((item.canonical.clone(), backup));
        }
    }

    let targets: Vec<PathBuf> = validated
        .iter()
        .map(|item| item.canonical.clone())
        .collect();
    for (item, temporary) in validated.iter().zip(staged) {
        if let Err(error) = temporary.persist(&item.canonical) {
            for target in &targets {
                let _ = std::fs::remove_file(target);
            }
            let restore_errors = restore_backups(&backups);
            let suffix = if restore_errors.is_empty() {
                String::new()
            } else {
                format!("; rollback failed: {}", restore_errors.join("; "))
            };
            return Err(format!(
                "cannot commit {}: {}{suffix}",
                item.canonical.display(),
                error.error
            ));
        }
    }
    for (_, backup) in backups {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

#[derive(Clone)]
struct Validated {
    proposal: Proposal,
    canonical: PathBuf,
}

fn validate_all(proposals: &[Proposal]) -> Result<Vec<Validated>, String> {
    let mut output = Vec::with_capacity(proposals.len());
    let mut seen = std::collections::BTreeSet::new();
    for proposal in proposals {
        let canonical = validate_proposal(proposal)?;
        let key = canonical.to_string_lossy().to_string();
        if !seen.insert(key) {
            return Err(format!("duplicate patch target: {}", canonical.display()));
        }
        output.push(Validated {
            proposal: proposal.clone(),
            canonical,
        });
    }
    Ok(output)
}

fn validate_proposal(proposal: &Proposal) -> Result<PathBuf, String> {
    match &proposal.expected {
        Expectation::Existing(expected) => {
            let metadata = proposal
                .path
                .symlink_metadata()
                .map_err(|error| format!("cannot inspect {}: {error}", proposal.path.display()))?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(format!(
                    "patch target is not a regular non-symlink file: {}",
                    proposal.path.display()
                ));
            }
            let canonical = proposal
                .path
                .canonicalize()
                .map_err(|error| format!("cannot resolve {}: {error}", proposal.path.display()))?;
            let actual = crate::digest::sha256(
                &std::fs::read(&canonical)
                    .map_err(|error| format!("cannot read {}: {error}", canonical.display()))?,
            );
            if actual != *expected {
                return Err(format!(
                    "content hash mismatch for {} (expected {expected}, found {actual})",
                    proposal.path.display()
                ));
            }
            Ok(canonical)
        }
        Expectation::Missing => {
            match proposal.path.symlink_metadata() {
                Ok(_) => {
                    return Err(format!(
                        "create target now exists: {}",
                        proposal.path.display()
                    ));
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!(
                        "cannot inspect {}: {error}",
                        proposal.path.display()
                    ));
                }
                Err(_) => {}
            }
            let parent = proposal.path.parent().ok_or_else(|| {
                format!("create target has no parent: {}", proposal.path.display())
            })?;
            let parent = parent.canonicalize().map_err(|error| {
                format!("cannot resolve create parent {}: {error}", parent.display())
            })?;
            let name = proposal.path.file_name().ok_or_else(|| {
                format!("create target has no filename: {}", proposal.path.display())
            })?;
            Ok(parent.join(name))
        }
    }
}

fn validate_current(validated: &[Validated]) -> Result<(), String> {
    for item in validated {
        match &item.proposal.expected {
            Expectation::Existing(expected) => {
                let actual =
                    crate::digest::sha256(&std::fs::read(&item.canonical).map_err(|error| {
                        format!("cannot re-read {}: {error}", item.canonical.display())
                    })?);
                if actual != *expected {
                    return Err(format!(
                        "content hash mismatch for {} (expected {expected}, found {actual})",
                        item.canonical.display()
                    ));
                }
            }
            Expectation::Missing if item.canonical.symlink_metadata().is_ok() => {
                return Err(format!(
                    "create target now exists: {}",
                    item.canonical.display()
                ));
            }
            Expectation::Missing => {}
        }
    }
    Ok(())
}

fn restore_backups(backups: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (target, backup) in backups.iter().rev() {
        let _ = std::fs::remove_file(target);
        if let Err(error) = std::fs::rename(backup, target) {
            errors.push(format!("{}: {error}", target.display()));
        }
    }
    errors
}

#[cfg(unix)]
fn file_permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn file_permissions(_metadata: &std::fs::Metadata) -> u32 {
    0o666
}

fn set_permissions(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|error| {
            format!(
                "cannot set staged permissions for {}: {error}",
                path.display()
            )
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn applies_existing_and_created_files_as_one_transaction() {
        let temporary = tempfile::tempdir().unwrap();
        let existing = temporary.path().join("existing.txt");
        let created = temporary.path().join("created.txt");
        fs::write(&existing, b"before").unwrap();
        let proposals = [
            prepare(&existing, b"after".to_vec()).unwrap(),
            prepare_create(&created, b"new".to_vec(), 0o640).unwrap(),
        ];
        apply_all(&proposals).unwrap();
        assert_eq!(fs::read(existing).unwrap(), b"after");
        assert_eq!(fs::read(created).unwrap(), b"new");
    }

    #[test]
    fn stale_hash_prevents_every_mutation() {
        let temporary = tempfile::tempdir().unwrap();
        let first = temporary.path().join("first.txt");
        let second = temporary.path().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let proposals = [
            prepare(&first, b"changed-first".to_vec()).unwrap(),
            prepare(&second, b"changed-second".to_vec()).unwrap(),
        ];
        fs::write(&second, b"drifted").unwrap();
        assert!(
            apply_all(&proposals)
                .unwrap_err()
                .contains("content hash mismatch")
        );
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"drifted");
    }

    #[test]
    fn duplicate_and_symlink_targets_are_rejected_without_mutation() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("file.txt");
        fs::write(&path, b"before").unwrap();
        let proposal = prepare(&path, b"after".to_vec()).unwrap();
        assert!(
            apply_all(&[proposal.clone(), proposal])
                .unwrap_err()
                .contains("duplicate")
        );
        assert_eq!(fs::read(&path).unwrap(), b"before");
        #[cfg(unix)]
        {
            let link = temporary.path().join("link.txt");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(prepare(&link, b"bad".to_vec()).is_err());
        }
    }
}
