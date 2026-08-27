use crate::ir::SourceBytes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayStore {
    pub schema_version: u32,
    pub entries: Vec<ReplayEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReplayEntry {
    pub method: String,
    pub uri: String,
    pub request_body_sha256: String,
    pub status: u16,
    pub headers: Vec<Header>,
    pub body: SourceBytes,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Header {
    pub name: String,
    pub value: String,
}

impl ReplayStore {
    pub(crate) fn decode(input: &[u8]) -> Result<Self, Vec<String>> {
        let store: Self = crate::strict_json::decode(input).map_err(|error| vec![error])?;
        store.validate()?;
        Ok(store)
    }

    pub(crate) fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        self.validate().map_err(|errors| errors.join("; "))?;
        let value = serde_json::to_value(self).map_err(|error| error.to_string())?;
        crate::canonical_json::pretty_bytes(&value)
    }

    #[cfg(test)]
    pub(crate) fn lookup(
        &self,
        method: &str,
        uri: &str,
        request_body: &[u8],
    ) -> Result<Vec<u8>, String> {
        self.validate().map_err(|errors| errors.join("; "))?;
        self.lookup_prevalidated(method, uri, request_body)
    }

    pub(crate) fn lookup_prevalidated(
        &self,
        method: &str,
        uri: &str,
        request_body: &[u8],
    ) -> Result<Vec<u8>, String> {
        let digest = crate::digest::sha256(request_body);
        let entry = self
            .entries
            .iter()
            .find(|entry| {
                entry.method == method && entry.uri == uri && entry.request_body_sha256 == digest
            })
            .ok_or_else(|| {
                format!("network replay miss for {method} {uri} with request body sha256:{digest}")
            })?;
        entry.body.to_bytes()
    }

    fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != 1 {
            errors.push(format!(
                "replay schema_version must be 1 (found {})",
                self.schema_version
            ));
        }
        let mut keys = std::collections::BTreeSet::new();
        for entry in &self.entries {
            if !valid_method(&entry.method) {
                errors.push(format!("invalid replay HTTP method: {}", entry.method));
            }
            if let Err(error) = validate_uri(&entry.uri) {
                errors.push(error);
            }
            if !crate::digest::valid_sha256(&entry.request_body_sha256) {
                errors.push(format!(
                    "invalid replay request body digest for {} {}",
                    entry.method, entry.uri
                ));
            }
            if !(100..=599).contains(&entry.status) {
                errors.push(format!("invalid replay HTTP status: {}", entry.status));
            }
            if !keys.insert((
                entry.method.as_str(),
                entry.uri.as_str(),
                entry.request_body_sha256.as_str(),
            )) {
                errors.push(format!(
                    "duplicate replay request: {} {}",
                    entry.method, entry.uri
                ));
            }
            let mut headers = std::collections::BTreeSet::new();
            for header in &entry.headers {
                let lower = header.name.to_ascii_lowercase();
                if header.name.is_empty()
                    || !header.name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    })
                {
                    errors.push(format!("invalid replay header name: {}", header.name));
                }
                if !headers.insert(lower) {
                    errors.push(format!("duplicate replay header: {}", header.name));
                }
                if header.value.contains(['\r', '\n', '\0']) {
                    errors.push(format!("invalid replay header value: {}", header.name));
                }
            }
            if let Err(error) = entry.body.to_bytes() {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn valid_method(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
}

fn validate_uri(uri: &str) -> Result<(), String> {
    if uri.contains(char::is_whitespace) || uri.contains(['\r', '\n', '\0']) {
        return Err(format!("invalid replay URI: {uri}"));
    }
    let (scheme, remainder) = uri
        .split_once("://")
        .ok_or_else(|| format!("replay URI must be absolute: {uri}"))?;
    if !matches!(scheme, "http" | "https") {
        return Err(format!("unsupported replay URI scheme: {scheme}"));
    }
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("replay URI has no authority: {uri}"));
    }
    if authority.contains('@') {
        return Err(format!("replay URI must not contain userinfo: {uri}"));
    }
    if uri.contains('#') {
        return Err(format!("replay URI must not contain a fragment: {uri}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(method: &str, uri: &str, request: &[u8], response: &[u8]) -> ReplayEntry {
        ReplayEntry {
            method: method.into(),
            uri: uri.into(),
            request_body_sha256: crate::digest::sha256(request),
            status: 200,
            headers: vec![Header {
                name: "content-type".into(),
                value: "application/octet-stream".into(),
            }],
            body: SourceBytes::from_bytes(response),
        }
    }

    #[test]
    fn replay_round_trip_is_strict_canonical_and_binary_safe() {
        let store = ReplayStore {
            schema_version: 1,
            entries: vec![entry("GET", "https://example.test/data", b"", &[0, 0xff])],
        };
        let bytes = store.encode_pretty().unwrap();
        assert!(bytes.ends_with(b"\n"));
        assert_eq!(ReplayStore::decode(&bytes).unwrap(), store);
        let unknown =
            String::from_utf8(bytes)
                .unwrap()
                .replacen("{\n", "{\n  \"future\": true,\n", 1);
        assert!(ReplayStore::decode(unknown.as_bytes()).is_err());
    }

    #[test]
    fn replay_keys_are_exact_and_network_fallback_never_occurs() {
        let store = ReplayStore {
            schema_version: 1,
            entries: vec![entry(
                "POST",
                "https://example.test/data",
                b"one",
                b"response",
            )],
        };
        assert_eq!(
            store
                .lookup("POST", "https://example.test/data", b"one")
                .unwrap(),
            b"response"
        );
        for (method, uri, body) in [
            ("GET", "https://example.test/data", b"one".as_slice()),
            ("POST", "https://example.test/other", b"one".as_slice()),
            ("POST", "https://example.test/data", b"two".as_slice()),
        ] {
            assert!(
                store
                    .lookup(method, uri, body)
                    .unwrap_err()
                    .contains("replay miss")
            );
        }
    }

    #[test]
    fn duplicate_requests_headers_and_invalid_metadata_are_rejected() {
        let duplicate = entry("GET", "https://example.test", b"", b"one");
        let mut store = ReplayStore {
            schema_version: 1,
            entries: vec![duplicate.clone(), duplicate],
        };
        assert!(store.encode_pretty().is_err());
        store.entries.truncate(1);
        store.entries[0].headers.push(Header {
            name: "Content-Type".into(),
            value: "again".into(),
        });
        assert!(store.encode_pretty().is_err());
        store.entries[0].headers.truncate(1);
        store.entries[0].status = 99;
        assert!(store.encode_pretty().is_err());
        store.entries[0].status = 200;
        store.entries[0].uri = "http://user:secret@example.test".into();
        assert!(store.encode_pretty().unwrap_err().contains("userinfo"));
    }

    #[test]
    fn duplicate_json_keys_and_legacy_versions_are_rejected() {
        assert!(ReplayStore::decode(br#"{"entries":[],"entries":[],"schema_version":1}"#).is_err());
        assert!(ReplayStore::decode(br#"{"entries":[],"schema_version":0}"#).is_err());
    }
}
