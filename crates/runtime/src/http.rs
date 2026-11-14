use crate::host::{Host, MAX_RESOURCE_BYTES, http_allowed, public_url, sha256};
use lenso_cli::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Project-owned authentication, restricted to an explicit HTTP origin/path prefix.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    /// Authorized destination; must also match the extension's HTTP grant.
    pub prefix: String,
    /// Header values may reference explicitly granted environment variable names.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Query values may reference explicitly granted environment variable names.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    params: BTreeMap<String, String>,
    credential: Option<String>,
    max_bytes: Option<usize>,
    select: Option<BTreeMap<String, Selection>>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Selection {
    Value { pointer: String },
    Keys { pointer: String },
}

impl Host {
    pub(crate) fn http_request(&mut self, raw: &str) -> Result<String, Error> {
        let request: Request = serde_json::from_str(raw)?;
        let mut url = reqwest::Url::parse(&request.url)
            .map_err(|_| Error::Invalid("invalid HTTP URL".into()))?;
        let max = request.max_bytes.unwrap_or(MAX_RESOURCE_BYTES);
        if max == 0
            || max > 16 * MAX_RESOURCE_BYTES
            || (max > MAX_RESOURCE_BYTES && request.select.is_none())
        {
            return Err(Error::Invalid(
                "HTTP bodies above 1 MiB require a bounded JSON projection (maximum 16 MiB)".into(),
            ));
        }
        let mut headers = request.headers;
        let mut params = request.params;
        if headers.len() > 32 || params.len() > 32 {
            return Err(Error::Invalid("too many HTTP headers or parameters".into()));
        }
        if let Some(name) = request.credential {
            let credential = self.credentials.get(&name).ok_or_else(|| {
                Error::Permission(format!("credential binding {name} is not configured"))
            })?;
            if !http_allowed(&request.url, std::slice::from_ref(&credential.prefix)) {
                return Err(Error::Permission(
                    "credential destination does not match its bound prefix".into(),
                ));
            }
            for (key, value) in &credential.headers {
                if headers
                    .keys()
                    .any(|header| header.eq_ignore_ascii_case(key))
                {
                    return Err(Error::Invalid(
                        "HTTP header conflicts with bound credentials".into(),
                    ));
                }
                headers.insert(key.clone(), expand(value, &self.env)?);
            }
            for (key, value) in &credential.params {
                if params.contains_key(key)
                    || url.query_pairs().any(|(name, _)| name == key.as_str())
                {
                    return Err(Error::Invalid(
                        "HTTP parameter conflicts with bound credentials".into(),
                    ));
                }
                params.insert(key.clone(), expand(value, &self.env)?);
            }
        }
        for (key, value) in params {
            url.query_pairs_mut().append_pair(&key, &value);
        }
        let mut operation = self.client.get(url);
        for (name, value) in headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::Invalid("invalid HTTP header name".into()))?;
            if matches!(
                name.as_str(),
                "host"
                    | "connection"
                    | "content-length"
                    | "transfer-encoding"
                    | "proxy-authorization"
            ) {
                return Err(Error::Invalid(
                    "HTTP transport headers cannot be overridden".into(),
                ));
            }
            let mut value = reqwest::header::HeaderValue::from_str(&value)
                .map_err(|_| Error::Invalid("invalid HTTP header value".into()))?;
            value.set_sensitive(true);
            operation = operation.header(name, value);
        }
        let key = format!("{}#request={}", public_url(&request.url), sha256(raw));
        let response = self.request_bounded(&request.url, operation, &key, max)?;
        let response = if let Some(fields) = request.select {
            if fields.len() > 32 {
                return Err(Error::Invalid("JSON projection exceeds 32 fields".into()));
            }
            let value: serde_json::Value = serde_json::from_str(&response)
                .map_err(|_| Error::Invalid("HTTP response is not valid JSON".into()))?;
            let mut projected = serde_json::Map::new();
            for (name, selection) in fields {
                let pointer = match &selection {
                    Selection::Value { pointer } | Selection::Keys { pointer } => pointer,
                };
                if pointer.len() > 1024 {
                    return Err(Error::Invalid("JSON pointer is too long".into()));
                }
                let selected = value
                    .pointer(pointer)
                    .ok_or_else(|| Error::Invalid("JSON projection field is missing".into()))?;
                let selected = match selection {
                    Selection::Value { .. } => selected.clone(),
                    Selection::Keys { .. } => serde_json::to_value(
                        selected
                            .as_object()
                            .ok_or_else(|| {
                                Error::Invalid("JSON key projection requires an object".into())
                            })?
                            .keys()
                            .collect::<Vec<_>>(),
                    )?,
                };
                projected.insert(name, selected);
            }
            serde_json::to_string(&projected)?
        } else {
            response
        };
        if response.len() > MAX_RESOURCE_BYTES {
            return Err(Error::Invalid("HTTP result exceeds 1 MiB".into()));
        }
        Ok(response)
    }
}

fn expand(value: &str, allowed: &[String]) -> Result<String, Error> {
    let mut result = String::new();
    let mut rest = value;
    while let Some((before, after)) = rest.split_once("${") {
        result.push_str(before);
        let (variable, suffix) = after
            .split_once('}')
            .ok_or_else(|| Error::Invalid("unterminated credential variable".into()))?;
        let (name, fallback) = variable
            .split_once(":-")
            .map_or((variable, None), |(name, fallback)| (name, Some(fallback)));
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !allowed.iter().any(|entry| entry == name)
        {
            return Err(lenso_cli::Diagnostic::missing_capability("host.env").into());
        }
        let expanded = std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| fallback.map(str::to_owned))
            .ok_or_else(|| {
                Error::from(lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput)
                    .with_details(serde_json::json!({"environment_variable":name,"required":true}))
                    .with_retryable(false).with_recovery("configure_environment", "Set the named environment variable in the trusted Host before retrying."))
            })?;
        result.push_str(&expanded);
        rest = suffix;
    }
    result.push_str(rest);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_cli::{Context, serde_json::json};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn context(root: &std::path::Path, prefix: &str) -> Context {
        Context {
            events: Default::default(),
            abi_version: 1,
            project: root.into(),
            input: json!({}),
            answers: Default::default(),
            steps: Default::default(),
            hooks: Default::default(),
            capabilities: BTreeMap::from([
                ("host.http".into(), json!([prefix])),
                ("host.env".into(), json!(["TOKEN"])),
            ]),
        }
    }

    #[test]
    fn credentials_stay_in_the_host_and_are_bound_to_the_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                assert_eq!(connection.read(&mut byte).unwrap(), 1);
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap();
            assert!(header.contains("authorization: Bearer test-secret"));
            assert!(header.contains("token=query-secret"));
            let response = r#"{"name":"example","versions":{"1.0.0":{},"1.1.0":{}}}"#;
            write!(
                connection,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
        });
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path(), &base);
        let mut host = Host::new(&ctx).unwrap();
        host.credentials.insert(
            "private".into(),
            Credential {
                prefix: format!("{base}private/"),
                headers: BTreeMap::from([("Authorization".into(), "Bearer test-secret".into())]),
                params: BTreeMap::from([("token".into(), "${TOKEN:-query-secret}".into())]),
            },
        );
        // No global environment mutation is needed for the fallback expansion.
        host.env = vec!["LENSO_CREDENTIAL_TEST_UNSET_19F".into()];
        host.credentials.get_mut("private").unwrap().params.insert(
            "token".into(),
            "${LENSO_CREDENTIAL_TEST_UNSET_19F:-query-secret}".into(),
        );
        let denied = json!({"url":format!("{base}public/data"),"credential":"private"});
        assert!(
            host.http_request(&denied.to_string())
                .unwrap_err()
                .to_string()
                .contains("destination")
        );
        let request = json!({"url":format!("{base}private/data"),"credential":"private","max_bytes":16*1024*1024,"select":{"name":{"op":"value","pointer":"/name"},"versions":{"op":"keys","pointer":"/versions"}}});
        let result = host.http_request(&request.to_string()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result).unwrap(),
            json!({"name":"example","versions":["1.0.0","1.1.0"]})
        );
        let evidence = serde_json::to_string(&host.evidence).unwrap();
        assert!(!evidence.contains("test-secret") && !evidence.contains("query-secret"));
        assert!(!serde_json::to_string(&ctx).unwrap().contains("test-secret"));
        server.join().unwrap();
        let mut forged = ctx;
        forged.capabilities.insert(
            "host.credentials".into(),
            json!({"private":{"prefix":base,"headers":{"Authorization":"forged"}}}),
        );
        assert!(Host::new(&forged).unwrap().credentials.is_empty());
    }
}
