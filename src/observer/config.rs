use crate::{Error, Result};
use serde::Deserialize;
use std::{collections::HashSet, io::Read, net::SocketAddr, path::Path};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_origin: String,
    pub allowed_logins: Vec<String>,
    #[serde(default)]
    pub allow_best_effort_output: bool,
    pub endpoints: Vec<EndpointConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointConfig {
    pub id: String,
    pub label: String,
    pub socket: String,
    #[serde(default)]
    pub ssh: Option<SshConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshConfig {
    pub target: String,
    pub helper: String,
}

pub(super) fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn remote_path(s: &str) -> bool {
    s.starts_with('/')
        && !s.ends_with('/')
        && s.len() <= 1024
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
        && !s.split('/').any(|part| matches!(part, "." | ".."))
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).map_err(|_| Error("config_read_failed"))?;
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| Error("config_read_failed"))?;
        if bytes.len() > 65536 {
            return Err(Error("config_limit"));
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|_| Error("invalid_config"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.listen.ip().is_loopback() {
            return Err(Error("invalid_listen"));
        }
        // A deliberately narrow HTTPS origin grammar: no credentials, paths or URL rewriting.
        let authority = self
            .public_origin
            .strip_prefix("https://")
            .ok_or(Error("invalid_origin"))?;
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        if host.len() > 253
            || host.is_empty()
            || host.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
            || port.is_some_and(|p| {
                !p.bytes().all(|b| b.is_ascii_digit())
                    || p.parse::<u16>().ok().filter(|p| *p > 0).is_none()
                    || p.starts_with('0')
            })
        {
            return Err(Error("invalid_origin"));
        }
        if self.allowed_logins.is_empty()
            || self.allowed_logins.len() > 64
            || self.allowed_logins.iter().any(|s| {
                s.is_empty()
                    || s.len() > 254
                    || !s.is_ascii()
                    || s.bytes()
                        .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            })
        {
            return Err(Error("invalid_logins"));
        }
        if self.endpoints.is_empty() || self.endpoints.len() > 8 {
            return Err(Error("endpoint_limit"));
        }
        let mut ids = HashSet::new();
        for endpoint in &self.endpoints {
            if !token(&endpoint.id) || !ids.insert(&endpoint.id) {
                return Err(Error("invalid_endpoint_id"));
            }
            if endpoint.label.is_empty()
                || endpoint.label.len() > 256
                || endpoint.label.chars().any(char::is_control)
            {
                return Err(Error("invalid_endpoint_label"));
            }
            crate::observation_socket::validate_path(&endpoint.socket)?;
            if endpoint.ssh.as_ref().is_some_and(|ssh| {
                !token(&ssh.target)
                    || ssh.target.starts_with('-')
                    || ssh.target.split('.').any(|s| s.is_empty())
                    || !remote_path(&ssh.helper)
                    || !remote_path(&endpoint.socket)
            }) {
                return Err(Error("invalid_ssh_config"));
            }
        }
        Ok(())
    }
}
