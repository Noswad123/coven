use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Read, net::SocketAddr, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub push: Option<super::push::PushConfig>,
    pub listen: SocketAddr,
    pub public_origin: String,
    pub allowed_logins: Vec<String>,
    #[serde(default)]
    pub allow_best_effort_output: bool,
    pub endpoints: Vec<EndpointConfig>,
    #[serde(default)]
    pub conversations: Vec<ConversationConfig>,
    #[serde(default)]
    pub conversation_sources: Vec<ConversationSourceConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationSourceConfig {
    pub id: String,
    pub label: String,
    pub team: String,
    pub directory: String,
    pub allowed_logins: Vec<String>,
    #[serde(default)]
    pub submit_logins: Vec<String>,
    #[serde(default)]
    pub decision_logins: Vec<String>,
    #[serde(default)]
    pub control_workspace: Option<String>,
}

fn local_directory(value: &str) -> bool {
    value.starts_with('/')
        && value.len() <= 4096
        && !value.chars().any(char::is_control)
        && !value.split('/').any(|s| matches!(s, "." | ".."))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationConfig {
    pub id: String,
    pub label: String,
    pub team: String,
    pub allowed_logins: Vec<String>,
    pub native_socket: String,
    pub expected_runtime: String,
    pub session: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointConfig {
    pub id: String,
    pub label: String,
    pub socket: String,
    #[serde(default)]
    pub ssh: Option<SshConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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
        if let Some(push) = &self.push {
            push.validate()?;
        }
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
        if self.conversations.len() > 64 {
            return Err(Error("conversation_limit"));
        }
        for target in &self.conversations {
            if !token(&target.id)
                || !ids.insert(&target.id)
                || !token(&target.team)
                || target.label.is_empty()
                || target.label.len() > 256
                || target.label.chars().any(char::is_control)
                || target.allowed_logins.is_empty()
                || target.allowed_logins.len() > 64
                || target.allowed_logins.iter().collect::<HashSet<_>>().len()
                    != target.allowed_logins.len()
                || target
                    .allowed_logins
                    .iter()
                    .any(|login| !self.allowed_logins.contains(login))
                || !crate::session_prompts::protocol::uuid(&target.expected_runtime)
                || !crate::session_prompts::protocol::identifier(&target.session, "ses_")
                || target.native_socket.len() > 100
                || target.native_socket.chars().any(char::is_control)
                || target.native_socket.split('/').any(|p| p == ".")
            {
                return Err(Error("invalid_conversation_config"));
            }
            // Local Unix sockets only. Remote native conversation targets are unsupported.
            crate::observation_socket::validate_path(&target.native_socket)?;
        }
        if self.conversation_sources.len() > 8 {
            return Err(Error("conversation_source_limit"));
        }
        for source in &self.conversation_sources {
            if !token(&source.id)
                || !ids.insert(&source.id)
                || !token(&source.team)
                || source.label.is_empty()
                || source.label.len() > 256
                || source.label.chars().any(char::is_control)
                || !local_directory(&source.directory)
                || source.allowed_logins.is_empty()
                || source.allowed_logins.len() > 64
                || source.allowed_logins.iter().collect::<HashSet<_>>().len()
                    != source.allowed_logins.len()
                || source
                    .allowed_logins
                    .iter()
                    .any(|l| !self.allowed_logins.contains(l))
                || source.submit_logins.iter().collect::<HashSet<_>>().len()
                    != source.submit_logins.len()
                || source
                    .submit_logins
                    .iter()
                    .any(|l| !source.allowed_logins.contains(l))
                || (!source.submit_logins.is_empty() && source.control_workspace.is_none())
                || source.decision_logins.iter().collect::<HashSet<_>>().len()
                    != source.decision_logins.len()
                || source
                    .decision_logins
                    .iter()
                    .any(|l| !source.allowed_logins.contains(l))
                || (!source.decision_logins.is_empty() && source.control_workspace.is_none())
                || source
                    .control_workspace
                    .as_ref()
                    .is_some_and(|p| !local_directory(p) || !Path::new(p).is_dir())
            {
                return Err(Error("invalid_conversation_source"));
            }
        }
        ids.clear();
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
