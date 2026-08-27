use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub(crate) fn sha256(bytes: &[u8]) -> String {
    lowercase_hex(Sha256::digest(bytes))
}

pub(crate) fn lowercase_hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn valid_pinned_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(valid_sha256)
}

pub(crate) fn file_sha256(path: &Path) -> Result<(u64, String), String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(format!(
            "path is not a regular non-symlink file: {}",
            path.display()
        ));
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or("file length overflow")?;
        digest.update(&buffer[..count]);
    }
    if bytes != metadata.len() {
        return Err(format!("file changed while hashing: {}", path.display()));
    }
    Ok((bytes, lowercase_hex(digest.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_has_a_known_vector() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn file_digest_rejects_symlinks_and_hashes_streams() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("asset");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(file_sha256(&path).unwrap(), (3, sha256(b"abc")));
        #[cfg(unix)]
        {
            let link = directory.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(file_sha256(&link).unwrap_err().contains("non-symlink"));
        }
    }
}
