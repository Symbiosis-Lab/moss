//! What this computer's OpenSSH knows about a server: its config, resolved by
//! `ssh -G`, and the host keys `known_hosts` lists for it, looked up with
//! `ssh-keygen -F`. Asking OpenSSH itself gets `Include`, `Match`, hashed
//! entries and first-value-wins right, where a reader of our own would
//! silently misread them. Without OpenSSH there is no config and no host is
//! known, as on a computer with no `~/.ssh`.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use russh::keys::PublicKey;

use super::server::Server;
use crate::system::bounded_command::run_bounded;

const WITHIN: Duration = Duration::from_secs(5);

/// The OpenSSH on this computer, reading the person's own config, or `config`
/// in its place (`ssh -F`).
#[derive(Debug, Clone, Default)]
pub struct Ssh {
    pub config: Option<PathBuf>,
}

/// Where an agent is, as the config names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Agent {
    /// The one `SSH_AUTH_SOCK` names.
    Env,
    Socket(PathBuf),
    None,
}

/// A server as OpenSSH would reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_files: Vec<PathBuf>,
    pub identities_only: bool,
    pub agent: Agent,
    /// The name `known_hosts` files the server's keys under.
    known_as: String,
    known_hosts: Vec<PathBuf>,
}

/// What `known_hosts` says of the key a server showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    Listed,
    /// Listed with a different key of the same type: what `ssh` refuses.
    Differs,
    Revoked,
    New,
}

impl Resolved {
    /// The server as reached, for the words and the address a person sees.
    pub fn reached(&self, folder: &str) -> Server {
        Server { user: self.user.clone(), host: self.host.clone(), port: self.port, folder: folder.to_string() }
    }

    /// The server as given, when there is no OpenSSH to ask.
    fn as_given(server: &Server) -> Resolved {
        Resolved {
            host: server.host.clone(),
            user: server.user.clone(),
            port: server.port,
            identity_files: Vec::new(),
            identities_only: false,
            agent: Agent::Env,
            known_as: known_name(&server.host, server.port),
            known_hosts: Vec::new(),
        }
    }
}

fn known_name(host: &str, port: u16) -> String {
    if port == 22 { host.to_string() } else { format!("[{host}]:{port}") }
}

fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

impl Ssh {
    /// `server` resolved through the config. A port of 22 counts as not
    /// given, so the config's `Port` applies. Only a computer with no `ssh`
    /// gets the server as given; an `ssh` that fails is an error, since
    /// guessing would drop the `known_hosts` it names.
    pub fn resolve(&self, server: &Server) -> Result<Resolved, String> {
        if !crate::deploy::publish_setup::binary_on_path(&format!("ssh{}", std::env::consts::EXE_SUFFIX)) {
            return Ok(Resolved::as_given(server));
        }
        let mut cmd = Command::new("ssh");
        cmd.arg("-G");
        if let Some(config) = &self.config {
            cmd.arg("-F").arg(config);
        }
        if !server.user.is_empty() {
            cmd.arg("-l").arg(&server.user);
        }
        if server.port != 22 {
            cmd.arg("-p").arg(server.port.to_string());
        }
        cmd.arg("--").arg(&server.host);
        let out = run_bounded(&mut cmd, WITHIN, "ssh -G")?;
        match out.status.success() {
            true => Ok(parse_resolved(&out.stdout, server)),
            false => Err(out.stderr.lines().next().unwrap_or("ssh -G failed").to_string()),
        }
    }

    /// What `known_hosts` says of `key` for the resolved server. A lookup
    /// that fails counts as nothing listed.
    pub fn known(&self, resolved: &Resolved, key: &PublicKey) -> Known {
        let Ok(shown) = key.to_openssh() else { return Known::New };
        let mut shown = shown.split_whitespace();
        let (Some(kind), Some(body)) = (shown.next(), shown.next()) else { return Known::New };
        let lines = self.listed(resolved);
        let same = |l: &&Line| l.kind == kind && l.body == body;
        if lines.iter().filter(|l| l.revoked).any(|l| same(&l)) {
            return Known::Revoked;
        }
        let listed: Vec<_> = lines.iter().filter(|l| !l.revoked).collect();
        if listed.iter().any(same) {
            Known::Listed
        } else if listed.iter().any(|l| l.kind == kind) {
            Known::Differs
        } else {
            Known::New
        }
    }

    /// The key types `known_hosts` lists for the server, so they can be asked
    /// for first.
    pub fn known_kinds(&self, resolved: &Resolved) -> Vec<String> {
        let mut kinds: Vec<String> = Vec::new();
        for line in self.listed(resolved).into_iter().filter(|l| !l.revoked) {
            if !kinds.contains(&line.kind) {
                kinds.push(line.kind);
            }
        }
        kinds
    }

    fn listed(&self, resolved: &Resolved) -> Vec<Line> {
        let mut lines = Vec::new();
        for file in resolved.known_hosts.iter().filter(|f| f.is_file()) {
            let mut cmd = Command::new("ssh-keygen");
            cmd.arg("-F").arg(&resolved.known_as).arg("-f").arg(file);
            if let Ok(out) = run_bounded(&mut cmd, WITHIN, "ssh-keygen -F") {
                lines.extend(out.stdout.lines().filter_map(Line::parse));
            }
        }
        lines
    }
}

/// One host key line `ssh-keygen -F` printed.
struct Line {
    revoked: bool,
    kind: String,
    body: String,
}

impl Line {
    fn parse(text: &str) -> Option<Line> {
        let mut fields = text.split_whitespace().peekable();
        let marker = fields.next_if(|f| f.starts_with('@'));
        // A CA line vouches for certificates, not for a plain key.
        if marker.is_some_and(|m| m != "@revoked") || text.starts_with('#') {
            return None;
        }
        let _hosts = fields.next()?;
        Some(Line { revoked: marker.is_some(), kind: fields.next()?.to_string(), body: fields.next()?.to_string() })
    }
}

fn parse_resolved(text: &str, server: &Server) -> Resolved {
    let mut resolved = Resolved::as_given(server);
    let mut alias = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(' ') else { continue };
        match key {
            "hostname" => resolved.host = value.to_string(),
            "user" => resolved.user = value.to_string(),
            "port" => resolved.port = value.parse().unwrap_or(resolved.port),
            "identityfile" => resolved.identity_files.push(expand(value)),
            "identitiesonly" => resolved.identities_only = value == "yes",
            "identityagent" => {
                resolved.agent = match value {
                    "none" => Agent::None,
                    "SSH_AUTH_SOCK" | "$SSH_AUTH_SOCK" => Agent::Env,
                    path => Agent::Socket(expand(path)),
                }
            }
            "hostkeyalias" => alias = Some(value.to_string()),
            "userknownhostsfile" | "globalknownhostsfile" => {
                resolved.known_hosts.extend(value.split_whitespace().map(expand));
            }
            _ => {}
        }
    }
    // ssh files a host key under its alias as given, with no port.
    resolved.known_as = alias.unwrap_or_else(|| known_name(&resolved.host, resolved.port));
    resolved
}

/// The hosts the person's `~/.ssh/config` names, to offer as they type. A
/// literal scan: a pattern or an included file is not offered, which costs
/// only a suggestion.
pub fn config_hosts() -> Vec<String> {
    // allow:raw_read ~/.ssh/config, never a vault input
    let Some(text) = dirs::home_dir().and_then(|h| std::fs::read_to_string(h.join(".ssh/config")).ok()) else {
        return Vec::new();
    };
    hosts_in(&text)
}

fn hosts_in(text: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("host")) {
            continue;
        }
        for host in words.filter(|h| !h.contains(['*', '?', '!'])) {
            if !hosts.iter().any(|h: &String| h == host) {
                hosts.push(host.to_string());
            }
        }
    }
    hosts
}

#[cfg(test)]
#[path = "openssh_tests.rs"]
mod tests;
