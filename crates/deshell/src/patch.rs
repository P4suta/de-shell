use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Expectation {
    Existing(String),
    Missing,
    /// Content-addressed creation. The target path is derived from the digest of
    /// the very bytes being written, so any writer that reached this path wrote
    /// these bytes. Observing the file already present is the same write completing
    /// twice, not a conflict. Differing content at the path means the digest no
    /// longer addresses the content, which stays an error.
    ///
    /// Unlike `Missing`, this intent carries no pre-check: a caller cannot inspect
    /// the path first and then decide, so there is no window between the decision
    /// and the write for a racing writer to invalidate.
    MissingOrIdentical,
}

#[derive(Clone, Debug)]
pub(crate) struct Proposal {
    pub path: PathBuf,
    pub expected: Expectation,
    pub replacement: Vec<u8>,
    pub permissions: u32,
    mutation: Mutation,
}

impl Proposal {
    pub(crate) fn deletes(&self) -> bool {
        self.mutation == Mutation::Delete
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mutation {
    Write,
    Delete,
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
        mutation: Mutation::Write,
    })
}

pub(crate) fn prepare_expected(
    path: &Path,
    expected_digest: &str,
    replacement: Vec<u8>,
) -> Result<Proposal, String> {
    if !crate::digest::valid_sha256(expected_digest) {
        return Err("patch expected digest must be a lowercase SHA-256".into());
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "patch target is not a regular non-symlink file: {}",
            path.display()
        ));
    }
    let actual = crate::digest::sha256(
        &std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    );
    if actual != expected_digest {
        return Err(format!(
            "content hash mismatch for {} (expected {expected_digest}, found {actual})",
            path.display()
        ));
    }
    Ok(Proposal {
        path: path.to_path_buf(),
        expected: Expectation::Existing(expected_digest.into()),
        replacement,
        permissions: file_permissions(&metadata),
        mutation: Mutation::Write,
    })
}

pub(crate) fn prepare_create(
    _path: &Path,
    _replacement: Vec<u8>,
    _permissions: u32,
) -> Result<Proposal, String> {
    checked_parent_directory(_path, "create target")?;
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
    Ok(Proposal {
        path: _path.to_path_buf(),
        expected: Expectation::Missing,
        replacement: _replacement,
        permissions: _permissions,
        mutation: Mutation::Write,
    })
}

/// Remove a directory that this process created and then abandoned.
///
/// Used only on the rollback path, where the directories being removed are the
/// ones [`ensure_directory`] reported as [`DirectoryState::Created`]. A directory
/// a concurrent writer created is never passed here, and a non-empty directory is
/// left alone rather than emptied, so a failure is not worth propagating: the
/// caller is already unwinding a different error.
pub(crate) fn remove_empty_directory(path: &Path) {
    let _ = std::fs::remove_dir(path);
}

/// Operations on a scratch tree that lies outside the project.
///
/// Isolated generator copies, decoded snippets handed to an interpreter, and
/// private workspace snapshots all live in a temporary directory that is deleted
/// wholesale afterwards. They need no staging, no rollback and no digest
/// expectation, because nothing in the project can observe them.
///
/// They still do not get the raw API. Naming the destination as scratch is what
/// keeps "this is outside the project" an assertion a reader can check, rather
/// than something inferred from how the path was built several frames up.
pub(crate) mod scratch {
    use std::path::Path;

    /// Write `bytes` to a path inside a scratch tree.
    pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        std::fs::write(path, bytes)
    }

    /// Copy a file into a scratch tree.
    pub(crate) fn copy(from: &Path, to: &Path) -> std::io::Result<u64> {
        std::fs::copy(from, to)
    }

    /// Set permissions on a path inside a scratch tree.
    pub(crate) fn set_permissions(
        path: &Path,
        permissions: std::fs::Permissions,
    ) -> std::io::Result<()> {
        std::fs::set_permissions(path, permissions)
    }

    /// Remove a file inside a scratch tree.
    pub(crate) fn remove_file(path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
}

/// Whether [`ensure_directory`] is what brought the directory into existence.
///
/// Callers that roll back on a later failure must remove only what they created;
/// deleting a directory a concurrent writer created would destroy someone else's
/// work. Returning that distinction as a value — rather than leaving each caller
/// to infer it from an error kind — is what makes the rollback decision checkable.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectoryState {
    /// This call created the directory, so this call owns removing it.
    Created,
    /// The directory was already present. It is not ours to remove.
    Existing,
}

/// Why [`ensure_directory`] could not establish a directory at the path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DirectoryError {
    /// Something that is not a directory (or is a symlink) occupies the path.
    /// This is a genuine conflict, not a lost race.
    Occupied,
    /// The directory could not be created or inspected.
    Io(String),
}

/// Establish a directory at `path`, tolerating a writer that got there first.
///
/// The postcondition callers need is "a directory exists at this path", never
/// "this call is what created it" — and where the latter does matter, it comes
/// back as [`DirectoryState`] instead of being recovered from an error kind.
///
/// `std::fs::create_dir` reports only the latter, so every call site had to decide
/// for itself what `AlreadyExists` means, and an existence check placed *before* it
/// merely narrows the race window rather than closing it. Here the attempt comes
/// first and the inspection second, so no window exists between deciding and acting.
pub(crate) fn ensure_directory(path: &Path) -> Result<DirectoryState, DirectoryError> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(DirectoryState::Created),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = path
                .symlink_metadata()
                .map_err(|error| DirectoryError::Io(error.to_string()))?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                return Err(DirectoryError::Occupied);
            }
            Ok(DirectoryState::Existing)
        }
        Err(error) => Err(DirectoryError::Io(error.to_string())),
    }
}

/// Create a directory that must not already exist.
///
/// The counterpart to [`ensure_directory`]. Isolation boundaries — a private
/// workspace root, a fresh snapshot tree — are only isolated because nothing was
/// there before, so a pre-existing directory is a failure rather than a race that
/// was lost. Making the caller name which of the two it means is the point: the
/// raw `std::fs::create_dir` has one behaviour and two possible intents, and
/// picking the wrong one silently is exactly how the concurrent-approval defect
/// and six untreated `AlreadyExists` sites happened.
pub(crate) fn create_new_directory(path: &Path) -> Result<(), DirectoryError> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(DirectoryError::Occupied)
        }
        Err(error) => Err(DirectoryError::Io(error.to_string())),
    }
}

/// Propose a content-addressed creation.
///
/// This deliberately performs no existence pre-check. `prepare_create` asks
/// whether the target is absent *now* and then writes later, which is a
/// time-of-check/time-of-use window: a racing writer between the two makes the
/// proposal fail even when it wrote byte-identical content. Content-addressed
/// writes cannot express that as a conflict, so the intent is declared instead
/// and resolved once, inside this module.
pub(crate) fn prepare_create_idempotent(
    path: &Path,
    replacement: Vec<u8>,
    permissions: u32,
) -> Result<Proposal, String> {
    checked_parent_directory(path, "create target")?;
    Ok(Proposal {
        path: path.to_path_buf(),
        expected: Expectation::MissingOrIdentical,
        replacement,
        permissions,
        mutation: Mutation::Write,
    })
}

fn checked_parent_directory<'a>(path: &'a Path, label: &str) -> Result<&'a Path, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{label} has no parent: {}", path.display()))?;
    let metadata = parent.symlink_metadata().map_err(|error| {
        format!(
            "cannot inspect {label} parent {}: {error}",
            parent.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(format!(
            "{label} parent is not a directory: {}",
            parent.display()
        ));
    }
    Ok(parent)
}

pub(crate) fn prepare_delete(path: &Path) -> Result<Proposal, String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "delete target is not a regular non-symlink file: {}",
            path.display()
        ));
    }
    let contents =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(Proposal {
        path: path.to_path_buf(),
        expected: Expectation::Existing(crate::digest::sha256(&contents)),
        replacement: Vec::new(),
        permissions: file_permissions(&metadata),
        mutation: Mutation::Delete,
    })
}

pub(crate) fn apply_all(proposals: &[Proposal]) -> Result<(), String> {
    apply_all_inner(proposals, None)
}

fn apply_all_inner(
    proposals: &[Proposal],
    fail_after_commits: Option<usize>,
) -> Result<(), String> {
    let validated = validate_all(proposals)?;
    let mut staged = Vec::with_capacity(validated.len());
    for item in &validated {
        if item.proposal.mutation == Mutation::Delete {
            staged.push(None);
            continue;
        }
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
        staged.push(Some(temporary));
    }
    // Revalidate the complete read set after all writes are staged. No target
    // has been changed at this point.
    validate_current(&validated)?;

    let mut backups: Vec<(PathBuf, PathBuf)> = Vec::new();
    for item in &validated {
        if matches!(item.proposal.expected, Expectation::Existing(_)) {
            let parent = item.canonical.parent().ok_or_else(|| {
                format!("patch target has no parent: {}", item.canonical.display())
            })?;
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
            if let Err(error) = sync_parent(&item.canonical) {
                let restore_errors = restore_backups(&backups);
                let suffix = if restore_errors.is_empty() {
                    String::new()
                } else {
                    format!("; rollback failed: {}", restore_errors.join("; "))
                };
                return Err(format!(
                    "cannot sync rollback stage for {}: {error}{suffix}",
                    item.canonical.display()
                ));
            }
        }
    }

    let mut committed = Vec::new();
    let mut commit_count = 0;
    for (item, temporary) in validated.iter().zip(staged) {
        let mut commit_error = if item.proposal.mutation == Mutation::Delete {
            sync_parent(&item.canonical).err()
        } else {
            match temporary
                .expect("write mutation has a staged file")
                .persist(&item.canonical)
            {
                Ok(_) => {
                    committed.push((
                        item.canonical.clone(),
                        crate::digest::sha256(&item.proposal.replacement),
                    ));
                    sync_parent(&item.canonical).err()
                }
                Err(error) => Some(format!("{}", error.error)),
            }
        };
        commit_count += usize::from(commit_error.is_none());
        if commit_error.is_none() && fail_after_commits == Some(commit_count) {
            commit_error = Some("injected commit failure".into());
        }
        if let Some(error) = commit_error {
            let mut restore_errors = remove_committed(&committed);
            restore_errors.extend(restore_backups(&backups));
            let suffix = if restore_errors.is_empty() {
                String::new()
            } else {
                format!("; rollback failed: {}", restore_errors.join("; "))
            };
            return Err(format!(
                "cannot commit {}: {error}{suffix}",
                item.canonical.display()
            ));
        }
    }
    for (_, backup) in backups {
        let _ = std::fs::remove_file(&backup);
        let _ = sync_parent(&backup);
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
    if proposal.mutation == Mutation::Delete
        && (!matches!(proposal.expected, Expectation::Existing(_))
            || !proposal.replacement.is_empty())
    {
        return Err("delete proposal must target existing content and have no replacement".into());
    }
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
            let parent = checked_parent_directory(&proposal.path, "create target")?;
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
            let parent = parent.canonicalize().map_err(|error| {
                format!("cannot resolve create parent {}: {error}", parent.display())
            })?;
            let name = proposal.path.file_name().ok_or_else(|| {
                format!("create target has no filename: {}", proposal.path.display())
            })?;
            Ok(parent.join(name))
        }
        Expectation::MissingOrIdentical => {
            let parent = checked_parent_directory(&proposal.path, "create target")?;
            match proposal.path.symlink_metadata() {
                Ok(metadata)
                    if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
                {
                    let canonical = proposal.path.canonicalize().map_err(|error| {
                        format!("cannot resolve {}: {error}", proposal.path.display())
                    })?;
                    let current = std::fs::read(&canonical)
                        .map_err(|error| format!("cannot read {}: {error}", canonical.display()))?;
                    if current != proposal.replacement {
                        return Err(format!(
                            "content-addressed target differs: {}",
                            proposal.path.display()
                        ));
                    }
                    Ok(canonical)
                }
                Ok(_) => Err(format!(
                    "content-addressed target is not a regular non-symlink file: {}",
                    proposal.path.display()
                )),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let parent = parent.canonicalize().map_err(|error| {
                        format!("cannot resolve create parent {}: {error}", parent.display())
                    })?;
                    let name = proposal.path.file_name().ok_or_else(|| {
                        format!("create target has no filename: {}", proposal.path.display())
                    })?;
                    Ok(parent.join(name))
                }
                Err(error) => Err(format!(
                    "cannot inspect {}: {error}",
                    proposal.path.display()
                )),
            }
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
            Expectation::MissingOrIdentical => match std::fs::read(&item.canonical) {
                // A writer that won the race wrote these exact bytes, so the staged
                // write remains correct and the rename below is idempotent.
                Ok(current) if current == item.proposal.replacement => {}
                Ok(_) => {
                    return Err(format!(
                        "content-addressed target differs: {}",
                        item.canonical.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "cannot re-read {}: {error}",
                        item.canonical.display()
                    ));
                }
            },
        }
    }
    Ok(())
}

fn restore_backups(backups: &[(PathBuf, PathBuf)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (target, backup) in backups.iter().rev() {
        if target.symlink_metadata().is_ok() {
            errors.push(format!(
                "refusing to overwrite a concurrent rollback target: {}",
                target.display()
            ));
            continue;
        }
        if let Err(error) = std::fs::rename(backup, target) {
            errors.push(format!("{}: {error}", target.display()));
        } else if let Err(error) = sync_parent(target) {
            errors.push(format!(
                "cannot sync {} after rollback: {error}",
                target.display()
            ));
        }
    }
    errors
}

fn remove_committed(committed: &[(PathBuf, String)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (target, expected) in committed.iter().rev() {
        if let Err(error) = checked_parent_directory(target, "rollback target") {
            errors.push(error);
            continue;
        }
        let metadata = match target.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                errors.push(format!(
                    "cannot inspect {} for rollback: {error}",
                    target.display()
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            errors.push(format!(
                "refusing to remove a non-regular rollback target: {}",
                target.display()
            ));
            continue;
        }
        let actual = match std::fs::read(target) {
            Ok(bytes) => crate::digest::sha256(&bytes),
            Err(error) => {
                errors.push(format!(
                    "cannot read {} for rollback: {error}",
                    target.display()
                ));
                continue;
            }
        };
        if actual != *expected {
            errors.push(format!(
                "refusing to overwrite a concurrent rollback edit at {}",
                target.display()
            ));
            continue;
        }
        if let Err(error) = std::fs::remove_file(target) {
            errors.push(format!(
                "cannot remove {} for rollback: {error}",
                target.display()
            ));
        } else if let Err(error) = sync_parent(target) {
            errors.push(format!(
                "cannot sync {} after rollback removal: {error}",
                target.display()
            ));
        }
    }
    errors
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent directory: {}", path.display()))?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync directory {}: {error}", parent.display()))
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), String> {
    Ok(())
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
// Tests reach for the raw APIs on purpose: they stage corrupt trees, race two
// writers against one path, and assert on what the transactional layer does with
// the result. Constructing those situations is precisely what the production ban
// exists to prevent, so the ban is lifted here and nowhere else.
#[expect(clippy::disallowed_methods, reason = "tests construct the races and corrupt trees the production ban prevents")]
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
    fn delete_participates_in_the_same_transaction_and_is_rollback_safe() {
        let temporary = tempfile::tempdir().unwrap();
        let removed = temporary.path().join("removed.txt");
        let created = temporary.path().join("created.txt");
        fs::write(&removed, b"archive me").unwrap();
        let proposals = [
            prepare_delete(&removed).unwrap(),
            prepare_create(&created, b"replacement".to_vec(), 0o644).unwrap(),
        ];
        apply_all(&proposals).unwrap();
        assert!(!removed.exists());
        assert_eq!(fs::read(&created).unwrap(), b"replacement");

        let rollback_source = temporary.path().join("rollback.txt");
        let rollback_create = temporary.path().join("rollback-created.txt");
        fs::write(&rollback_source, b"must survive").unwrap();
        let rollback = [
            prepare_delete(&rollback_source).unwrap(),
            prepare_create(&rollback_create, b"temporary".to_vec(), 0o644).unwrap(),
        ];
        assert!(apply_all_inner(&rollback, Some(1)).is_err());
        assert_eq!(fs::read(&rollback_source).unwrap(), b"must survive");
        assert!(!rollback_create.exists());
    }

    #[test]
    fn injected_commit_failure_restores_the_complete_write_set() {
        let temporary = tempfile::tempdir().unwrap();
        let existing = temporary.path().join("existing.txt");
        let created = temporary.path().join("created.txt");
        fs::write(&existing, b"before").unwrap();
        let proposals = [
            prepare(&existing, b"after".to_vec()).unwrap(),
            prepare_create(&created, b"new".to_vec(), 0o640).unwrap(),
        ];

        let error = apply_all_inner(&proposals, Some(1)).unwrap_err();
        assert!(error.contains("injected commit failure"), "{error}");
        assert_eq!(fs::read(existing).unwrap(), b"before");
        assert!(!created.exists());
    }

    #[test]
    fn rollback_refuses_to_overwrite_a_concurrent_target() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("target.txt");
        let backup = temporary.path().join("backup.txt");
        fs::write(&target, b"concurrent").unwrap();
        fs::write(&backup, b"original").unwrap();

        let errors = restore_backups(&[(target.clone(), backup.clone())]);
        assert!(errors.join("; ").contains("concurrent"));
        assert_eq!(fs::read(target).unwrap(), b"concurrent");
        assert_eq!(fs::read(backup).unwrap(), b"original");
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
    fn explicit_expected_hash_never_overwrites_a_concurrent_change() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("file.txt");
        fs::write(&path, b"committed replacement").unwrap();
        let expected = crate::digest::sha256(b"committed replacement");
        let proposal = prepare_expected(&path, &expected, b"original".to_vec()).unwrap();

        fs::write(&path, b"concurrent edit").unwrap();
        assert!(
            apply_all(&[proposal])
                .unwrap_err()
                .contains("content hash mismatch")
        );
        assert_eq!(fs::read(path).unwrap(), b"concurrent edit");
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

    #[test]
    fn preparation_rejects_missing_non_regular_and_stale_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing.txt");
        let directory = temporary.path().join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(prepare(&missing, vec![]).is_err());
        assert!(prepare(&directory, vec![]).is_err());
        assert!(prepare_delete(&missing).is_err());
        assert!(prepare_delete(&directory).is_err());
        assert!(prepare_expected(&missing, "not-a-digest", vec![]).is_err());

        let file = temporary.path().join("file.txt");
        fs::write(&file, b"current").unwrap();
        assert!(
            prepare_expected(&file, &crate::digest::sha256(b"stale"), vec![])
                .unwrap_err()
                .contains("content hash mismatch")
        );
        assert!(prepare_expected(&directory, &crate::digest::sha256(b""), vec![]).is_err());
        assert!(prepare_create(&file, vec![], 0o644).is_err());
        assert!(
            prepare_create(&temporary.path().join("absent/child"), vec![], 0o644)
                .unwrap_err()
                .contains("parent")
        );

        let write = prepare(&file, b"replacement".to_vec()).unwrap();
        let delete = prepare_delete(&file).unwrap();
        assert!(!write.deletes());
        assert!(delete.deletes());

        #[cfg(unix)]
        {
            let file_link = temporary.path().join("file-link");
            std::os::unix::fs::symlink(&file, &file_link).unwrap();
            assert!(prepare_delete(&file_link).is_err());

            let directory_link = temporary.path().join("directory-link");
            std::os::unix::fs::symlink(&directory, &directory_link).unwrap();
            assert!(
                prepare_create(&directory_link.join("new"), vec![], 0o644)
                    .unwrap_err()
                    .contains("not a directory")
            );
        }
    }

    #[test]
    fn validation_rejects_post_preparation_tampering_and_races() {
        let temporary = tempfile::tempdir().unwrap();
        let existing = temporary.path().join("existing.txt");
        let created = temporary.path().join("created.txt");
        fs::write(&existing, b"before").unwrap();

        let mut malformed_delete = prepare_delete(&existing).unwrap();
        malformed_delete.replacement.push(1);
        assert!(
            validate_proposal(&malformed_delete)
                .unwrap_err()
                .contains("delete proposal")
        );
        malformed_delete.replacement.clear();
        malformed_delete.expected = Expectation::Missing;
        assert!(validate_proposal(&malformed_delete).is_err());

        let create = prepare_create(&created, b"new".to_vec(), 0o644).unwrap();
        fs::write(&created, b"concurrent").unwrap();
        assert!(
            validate_proposal(&create)
                .unwrap_err()
                .contains("now exists")
        );

        let absent_parent = Proposal {
            path: temporary.path().join("gone/new.txt"),
            expected: Expectation::Missing,
            replacement: vec![],
            permissions: 0o644,
            mutation: Mutation::Write,
        };
        assert!(
            validate_proposal(&absent_parent)
                .unwrap_err()
                .contains("create target parent")
        );
        let no_parent = Proposal {
            path: PathBuf::new(),
            expected: Expectation::Missing,
            replacement: vec![],
            permissions: 0o644,
            mutation: Mutation::Write,
        };
        assert!(validate_proposal(&no_parent).is_err());

        let valid = prepare(&existing, b"after".to_vec()).unwrap();
        let directory = temporary.path().join("directory");
        fs::create_dir(&directory).unwrap();
        let mut retargeted = valid.clone();
        retargeted.path = directory;
        assert!(
            validate_proposal(&retargeted)
                .unwrap_err()
                .contains("regular non-symlink")
        );

        let parent_file = temporary.path().join("parent-file");
        fs::write(&parent_file, b"not a directory").unwrap();
        let inaccessible_child = parent_file.join("child");
        let prepare_error = prepare_create(&inaccessible_child, vec![], 0o644).unwrap_err();
        assert!(
            prepare_error.contains("create target parent is not a directory"),
            "unexpected platform-specific prepare error: {prepare_error}"
        );
        let invalid_create = Proposal {
            path: inaccessible_child.clone(),
            expected: Expectation::Missing,
            replacement: vec![],
            permissions: 0o644,
            mutation: Mutation::Write,
        };
        let validation_error = validate_proposal(&invalid_create).unwrap_err();
        assert!(
            validation_error.contains("create target parent is not a directory"),
            "unexpected platform-specific validation error: {validation_error}"
        );
        let rollback_errors =
            remove_committed(&[(inaccessible_child, crate::digest::sha256(b"anything"))]);
        assert!(
            rollback_errors
                .iter()
                .any(|error| error.contains("rollback target parent is not a directory")),
            "unexpected platform-specific rollback result: {rollback_errors:?}"
        );

        let canonical = existing.canonicalize().unwrap();
        let validated = Validated {
            proposal: valid.clone(),
            canonical: canonical.clone(),
        };
        fs::write(&existing, b"drift").unwrap();
        assert!(validate_current(std::slice::from_ref(&validated)).is_err());
        fs::remove_file(&existing).unwrap();
        assert!(validate_current(&[validated]).is_err());

        let concurrent = Validated {
            proposal: create,
            canonical: created,
        };
        assert!(validate_current(&[concurrent]).is_err());
        assert!(validate_all(&[]).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_create_and_rollback_targets_fail_closed() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().unwrap();
        let inaccessible_parent = temporary.path().join("inaccessible");
        fs::create_dir(&inaccessible_parent).unwrap();
        let target = inaccessible_parent.join("target");
        let proposal = Proposal {
            path: target.clone(),
            expected: Expectation::Missing,
            replacement: vec![],
            permissions: 0o644,
            mutation: Mutation::Write,
        };

        fs::set_permissions(&inaccessible_parent, fs::Permissions::from_mode(0o000)).unwrap();
        let prepare_error = prepare_create(&target, vec![], 0o644).err();
        let validation_error = validate_proposal(&proposal).err();
        let rollback_errors = remove_committed(&[(target, crate::digest::sha256(b"expected"))]);
        fs::set_permissions(&inaccessible_parent, fs::Permissions::from_mode(0o700)).unwrap();

        assert!(
            prepare_error.is_some_and(|error| error.contains("cannot inspect create target")),
            "an inaccessible create target must not be treated as missing"
        );
        assert!(
            validation_error.is_some_and(|error| error.contains("cannot inspect")),
            "validation must preserve an inaccessible target as an error"
        );
        assert!(
            rollback_errors
                .iter()
                .any(|error| error.contains("for rollback")),
            "rollback must not silently skip an inaccessible target"
        );
    }

    #[test]
    fn rollback_helpers_restore_only_unchanged_regular_files() {
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("target.txt");
        let backup = temporary.path().join("backup.txt");
        fs::write(&backup, b"original").unwrap();
        assert!(restore_backups(&[(target.clone(), backup)]).is_empty());
        assert_eq!(fs::read(&target).unwrap(), b"original");

        let absent_target = temporary.path().join("absent-target.txt");
        let absent_backup = temporary.path().join("absent-backup.txt");
        assert!(!restore_backups(&[(absent_target, absent_backup)]).is_empty());

        let missing = temporary.path().join("missing.txt");
        assert!(remove_committed(&[(missing, crate::digest::sha256(b"x"))]).is_empty());

        let directory = temporary.path().join("directory");
        fs::create_dir(&directory).unwrap();
        assert!(!remove_committed(&[(directory, crate::digest::sha256(b""))]).is_empty());

        let edited = temporary.path().join("edited.txt");
        fs::write(&edited, b"concurrent").unwrap();
        assert!(
            remove_committed(&[(edited.clone(), crate::digest::sha256(b"original"))])
                .join("; ")
                .contains("concurrent")
        );
        assert_eq!(fs::read(&edited).unwrap(), b"concurrent");

        let committed = temporary.path().join("committed.txt");
        fs::write(&committed, b"replacement").unwrap();
        assert!(
            remove_committed(&[(committed.clone(), crate::digest::sha256(b"replacement"))])
                .is_empty()
        );
        assert!(!committed.exists());

        #[cfg(unix)]
        {
            let link = temporary.path().join("link.txt");
            std::os::unix::fs::symlink(&edited, &link).unwrap();
            assert!(!remove_committed(&[(link, crate::digest::sha256(b"concurrent"))]).is_empty());
            assert!(sync_parent(Path::new("/")).is_err());
            assert!(set_permissions(&temporary.path().join("absent"), 0o600).is_err());
        }
    }

    #[test]
    fn identical_concurrent_creation_is_not_a_conflict() {
        // A content-addressed path is derived from the digest of its own bytes, so a
        // racing writer that produced the same path necessarily produced the same
        // bytes. Losing that race is not a conflict; it is the same write completing
        // twice. Callers must not have to re-check or retry to discover that.
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("a61b1897.json");
        let bytes = b"{\n  \"schema_version\": 1\n}\n".to_vec();

        let create = prepare_create_idempotent(&target, bytes.clone(), 0o644).unwrap();
        fs::write(&target, &bytes).unwrap();

        apply_all(&[create]).unwrap();
        assert_eq!(fs::read(&target).unwrap(), bytes);
    }

    #[test]
    fn differing_concurrent_creation_is_still_a_conflict() {
        // Same path, different bytes, means the digest no longer addresses the
        // content. That must stay an error even under the idempotent intent.
        let temporary = tempfile::tempdir().unwrap();
        let target = temporary.path().join("a61b1897.json");

        let create = prepare_create_idempotent(&target, b"expected".to_vec(), 0o644).unwrap();
        fs::write(&target, b"different").unwrap();

        assert!(
            apply_all(&[create])
                .unwrap_err()
                .contains("content-addressed target")
        );
        assert_eq!(fs::read(&target).unwrap(), b"different");
    }
}
