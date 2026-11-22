//! Your own server as a [`Destination`]: a folder on it, reached over SSH and
//! written with SFTP.
//!
//! Every way a connection can fail is a [`Refusal`] whose text is the one
//! sentence a person needs, so the check and a failed publish say the same
//! thing.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::{error::Error as SftpError, RawSftpSession};
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};

use super::destination::{Body, Destination};
use super::server::Server;

/// How long a server has to answer before it counts as not answering.
const ANSWER_WITHIN: Duration = Duration::from_secs(15);
/// Bytes per write request, and how many are in flight at once: one request
/// per round trip would make a far server crawl.
const CHUNK: usize = 32 * 1024;
const IN_FLIGHT: usize = 16;
/// The host keys moss accepts. russh offers RSA even built without it, and
/// then fails the signature; left out, an RSA-only server is named as one.
const HOST_KEYS: &[Algorithm] = &[
    Algorithm::Ed25519,
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP256 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP384 },
    Algorithm::Ecdsa { curve: EcdsaCurve::NistP521 },
];
/// The OpenSSH extension that renames over an existing file in one step.
const POSIX_RENAME: &str = "posix-rename@openssh.com";

/// Why moss could not reach, trust, sign in to or write to a server. Typed
/// on the wire, so each surface words it in its own language.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
// The bindings already carry the plugin registry's `Refusal`.
#[specta(rename = "ServerRefusal")]
pub enum Refusal {
    NoSuchServer { host: String },
    NoAnswer { host: String, port: u16 },
    PortRefused { host: String, port: u16 },
    /// Seen for the first time: the person decides whether to trust it.
    UnknownHostKey { host: String, fingerprint: String },
    HostKeyChanged { host: String, was: String, now: String },
    RsaOnly { host: String },
    KeyRefused { host: String },
    /// Signing in failed for a reason other than a refused key.
    SignInFailed { host: String, reason: String },
    /// Signed in, but the server runs no SFTP.
    NoSftp { host: String },
    NoFolder { folder: String },
    /// No folder given, and none of the usual web folders is on the server.
    NoWebFolder { host: String },
    /// No folder given, and several web folders take files but none shows
    /// at the address: the person picks one.
    WhichFolder { host: String },
    CannotWrite { folder: String },
    /// The folder takes files, but the address doesn't serve them (yet).
    NotServed { address: String },
    /// A write in the folder failed; `reason` is the server's own words.
    WriteFailed { path: String, reason: String },
    /// The server cannot replace a file in one step, so a visitor could meet
    /// a missing file mid-publish.
    NoAtomicRename { host: String },
    /// The connection broke before moss could sign in.
    Broken { host: String, reason: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NoSuchServer { host } => write!(f, "No server named {host}."),
            Refusal::NoAnswer { host, port } => write!(f, "{host} didn't answer on port {port}. Is the port right?"),
            Refusal::PortRefused { host, port } => write!(f, "{host} refused port {port}."),
            Refusal::UnknownHostKey { host, fingerprint } => write!(f, "{host}'s key is {fingerprint}."),
            Refusal::HostKeyChanged { host, was, now } => write!(
                f,
                "{host}'s key changed (was {was}, now {now}). Trust the new key only if you rebuilt the server."
            ),
            Refusal::RsaOnly { host } => write!(
                f,
                "{host} only offers an RSA key, which moss doesn't use. Add an ed25519 host key to the server."
            ),
            Refusal::KeyRefused { host } => {
                write!(f, "{host} refused the key. Add the line above to ~/.ssh/authorized_keys.")
            }
            Refusal::SignInFailed { host, reason } => write!(f, "Couldn't sign in to {host}: {reason}"),
            Refusal::NoSftp { host } => write!(f, "Signed in, but {host} doesn't offer SFTP."),
            Refusal::NoFolder { folder } => write!(f, "Signed in, but there is no folder {folder}."),
            Refusal::NoWebFolder { host } => write!(f, "No web folder found on {host}. Add its path after the server, like deploy@{host}:/var/www/html."),
            Refusal::WhichFolder { host } => write!(f, "Which folder on {host} holds the site?"),
            Refusal::NotServed { address } => write!(f, "{address} doesn't show this folder yet."),
            Refusal::CannotWrite { folder } => write!(f, "Signed in, but can't write to {folder}."),
            Refusal::WriteFailed { path, reason } => write!(f, "write {path}: {reason}"),
            Refusal::NoAtomicRename { host } => write!(
                f,
                "{host} can't replace a file in one step (its SFTP server lacks {POSIX_RENAME}), so moss won't publish there."
            ),
            Refusal::Broken { host, reason } => write!(f, "Couldn't talk to {host}: {reason}"),
        }
    }
}

/// Why a server's key was not trusted. Only a plain key has a fingerprint
/// worth trusting; without one there is nothing to pin.
pub(super) fn untrusted(host: String, pinned: Option<&str>, seen: Option<String>) -> Refusal {
    let Some(now) = seen else {
        return Refusal::Broken { host, reason: "it showed no plain host key".into() };
    };
    match pinned {
        None => Refusal::UnknownHostKey { host, fingerprint: now },
        Some(was) => Refusal::HostKeyChanged { host, was: was.to_string(), now },
    }
}

/// Checks the server's host key against the one pinned for it, and keeps the
/// one it saw so a refusal can name it.
struct Trust {
    pinned: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

impl russh::client::Handler for Trust {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return Ok(false);
        };
        let seen = key.fingerprint(HashAlg::Sha256).to_string();
        let trusted = self.pinned.as_deref() == Some(seen.as_str());
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(seen);
        Ok(trusted)
    }
}

/// Where web servers usually serve sites from; a relative path is under the
/// home folder, `{host}` is the server's name.
pub(super) const WEB_FOLDERS: &[&str] =
    &["public_html", "www", "htdocs", "domains/{host}/public_html", "/var/www/{host}", "/var/www/html", "/srv/www"];

/// A folder on a server moss has signed in to.
pub struct SftpDestination {
    sftp: RawSftpSession,
    /// The folder's real path on the server, with no trailing `/`.
    root: String,
    /// Folders already found to lie inside `root`.
    inside: Mutex<HashSet<String>>,
    _ssh: russh::client::Handle<Trust>,
}

/// Each step of reaching a server, run in order; the first that fails is the
/// refusal. `host_key` is the fingerprint the folder pinned, if any.
pub async fn connect(server: &Server, host_key: Option<&str>, key: Arc<PrivateKey>) -> Result<SftpDestination, Refusal> {
    let host = server.host.clone();
    let tcp = reach(&host, server.port).await?;
    let seen = Arc::new(Mutex::new(None));
    let trust = Trust { pinned: host_key.map(str::to_string), seen: seen.clone() };
    let config = russh::client::Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        preferred: russh::Preferred { key: std::borrow::Cow::Borrowed(HOST_KEYS), ..russh::Preferred::DEFAULT },
        ..Default::default()
    };
    let mut ssh = match russh::client::connect_stream(Arc::new(config), tcp, trust).await {
        Ok(ssh) => ssh,
        Err(russh::Error::UnknownKey) => {
            return Err(untrusted(host, host_key, seen.lock().unwrap_or_else(|p| p.into_inner()).clone()));
        }
        Err(russh::Error::NoCommonAlgo { kind: russh::AlgorithmKind::Key, theirs, .. })
            if theirs.iter().all(|a| a.contains("rsa")) =>
        {
            return Err(Refusal::RsaOnly { host })
        }
        Err(e) => return Err(Refusal::Broken { host, reason: e.to_string() }),
    };
    let signed_in = ssh
        .authenticate_publickey(&server.user, PrivateKeyWithHashAlg::new(key, None))
        .await
        .map_err(|e| Refusal::SignInFailed { host: host.clone(), reason: e.to_string() })?;
    if !signed_in.success() {
        return Err(Refusal::KeyRefused { host });
    }
    let no_sftp = || Refusal::NoSftp { host: host.clone() };
    let channel = ssh.channel_open_session().await.map_err(|_| no_sftp())?;
    channel.request_subsystem(true, "sftp").await.map_err(|_| no_sftp())?;
    let sftp = RawSftpSession::new(channel.into_stream());
    sftp.set_timeout(ANSWER_WITHIN.as_secs());
    let version = sftp.init().await.map_err(|_| no_sftp())?;
    if !version.extensions.contains_key(POSIX_RENAME) {
        return Err(Refusal::NoAtomicRename { host });
    }
    // OpenSSH answers realpath for a path that is not there.
    let no_folder = || Refusal::NoFolder { folder: server.folder.clone() };
    // No folder yet: open at the home folder, and look for the web folder there.
    let wanted = if server.folder.is_empty() { "." } else { server.folder.as_str() };
    let root = match sftp.realpath(wanted).await {
        Ok(name) => name.files.into_iter().next().map(|f| f.filename).unwrap_or_default(),
        Err(_) => return Err(no_folder()),
    };
    if !sftp.stat(&root).await.is_ok_and(|attrs| attrs.attrs.is_dir()) {
        return Err(no_folder());
    }
    let root = root.trim_end_matches('/').to_string();
    Ok(SftpDestination { sftp, root, inside: Mutex::new(HashSet::new()), _ssh: ssh })
}

async fn reach(host: &str, port: u16) -> Result<tokio::net::TcpStream, Refusal> {
    let addrs: Vec<_> = match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => addrs.collect(),
        Err(_) => return Err(Refusal::NoSuchServer { host: host.to_string() }),
    };
    if addrs.is_empty() {
        return Err(Refusal::NoSuchServer { host: host.to_string() });
    }
    match tokio::time::timeout(ANSWER_WITHIN, tokio::net::TcpStream::connect(&addrs[..])).await {
        Err(_) => Err(Refusal::NoAnswer { host: host.to_string(), port }),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            Err(Refusal::PortRefused { host: host.to_string(), port })
        }
        Ok(Err(_)) => Err(Refusal::NoAnswer { host: host.to_string(), port }),
        Ok(Ok(tcp)) => Ok(tcp),
    }
}

fn is_missing(e: &SftpError) -> bool {
    matches!(e, SftpError::Status(s) if s.status_code == StatusCode::NoSuchFile)
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

impl SftpDestination {
    /// Of `candidates`, the folders on the server, as full paths, in order.
    pub async fn folders(&self, candidates: &[String]) -> Vec<String> {
        let mut found = Vec::new();
        for candidate in candidates {
            let Ok(real) = self.sftp.realpath(candidate).await else { continue };
            let Some(path) = real.files.into_iter().next().map(|f| f.filename) else { continue };
            if !found.contains(&path) && self.sftp.stat(&path).await.is_ok_and(|a| a.attrs.is_dir()) {
                found.push(path);
            }
        }
        found
    }

    /// Write into `folder` from now on.
    pub fn move_to(&mut self, folder: &str) {
        self.root = folder.trim_end_matches('/').to_string();
        self.inside = Mutex::new(HashSet::new());
    }

    /// Write a file under a temporary name, then rename it over the live one,
    /// so a visitor never meets a half written file. A refusal stays typed for
    /// the check, which words it in each surface's language.
    pub async fn put(&self, path: &str, body: Body) -> Result<(), Refusal> {
        let (dir, name) = split(path);
        self.make_dirs(dir).await.map_err(|e| Refusal::WriteFailed { path: path.into(), reason: e.to_string() })?;
        let full = self.full(path);
        let temp = self.full(&format!("{}{}.{name}.moss-tmp-{}", dir, if dir.is_empty() { "" } else { "/" }, std::process::id()));
        let written = match &body {
            Body::File(_) | Body::Bytes(_) => self.write_file(&temp, &body).await,
            // The SFTP draft sends the link path first; OpenSSH reads the
            // target first, so the arguments go in OpenSSH's order.
            Body::Link(target) => self.sftp.symlink(target.as_str(), temp.as_str()).await.map(|_| ()),
        };
        let landed = match written {
            Ok(()) => self.rename_over(&temp, &full).await,
            failed => failed,
        };
        match landed {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.sftp.remove(temp).await;
                Err(match e {
                    SftpError::Status(s) if s.status_code == StatusCode::PermissionDenied => {
                        Refusal::CannotWrite { folder: self.full(dir) }
                    }
                    e => Refusal::WriteFailed { path: path.into(), reason: e.to_string() },
                })
            }
        }
    }

    /// The folder's real path, as the server named it.
    pub fn root(&self) -> &str {
        &self.root
    }

    fn full(&self, path: &str) -> String {
        if path.is_empty() {
            self.root.clone()
        } else {
            format!("{}/{path}", self.root)
        }
    }

    /// Refuse `dir` (relative) when its real path on the server is outside
    /// the root: a link inside the destination can lead out of it.
    async fn check_inside(&self, dir: &str) -> Result<(), SftpError> {
        if dir.is_empty() || self.inside.lock().unwrap_or_else(|p| p.into_inner()).contains(dir) {
            return Ok(());
        }
        let real = self.sftp.realpath(self.full(dir)).await?;
        let real = real.files.into_iter().next().map(|f| f.filename).unwrap_or_default();
        if real != self.root && !real.starts_with(&format!("{}/", self.root)) {
            return Err(SftpError::UnexpectedBehavior(format!("{dir} leads outside {}", self.root)));
        }
        self.inside.lock().unwrap_or_else(|p| p.into_inner()).insert(dir.to_string());
        Ok(())
    }

    /// Create `dir` and its parents, each checked to stay inside the root.
    async fn make_dirs(&self, dir: &str) -> Result<(), SftpError> {
        let mut at = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            if !at.is_empty() {
                at.push('/');
            }
            at.push_str(part);
            if self.inside.lock().unwrap_or_else(|p| p.into_inner()).contains(&at) {
                continue;
            }
            // Already there is the common case; whatever else went wrong
            // shows at the realpath or the write that follows.
            let _ = self.sftp.mkdir(self.full(&at), FileAttributes::default()).await;
            self.check_inside(&at).await?;
        }
        Ok(())
    }

    async fn rename_over(&self, from: &str, to: &str) -> Result<(), SftpError> {
        let mut data = Vec::with_capacity(from.len() + to.len() + 8);
        for s in [from, to] {
            data.extend_from_slice(&(s.len() as u32).to_be_bytes());
            data.extend_from_slice(s.as_bytes());
        }
        match self.sftp.extended(POSIX_RENAME, data).await? {
            russh_sftp::protocol::Packet::Status(s) if s.status_code == StatusCode::Ok => Ok(()),
            russh_sftp::protocol::Packet::Status(s) => Err(SftpError::Status(s)),
            _ => Err(SftpError::UnexpectedPacket),
        }
    }

    async fn write_file(&self, temp: &str, body: &Body) -> Result<(), SftpError> {
        let reader: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match body {
            // allow:raw_read built output being uploaded — dataless is absent
            Body::File(local) => Box::new(tokio::fs::File::open(local).await?),
            Body::Bytes(data) => Box::new(std::io::Cursor::new(data.clone())),
            Body::Link(_) => unreachable!("a link is not written as a file"),
        };
        let attrs = FileAttributes { permissions: Some(0o644), ..Default::default() };
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        let handle = self.sftp.open(temp, flags, attrs).await?.handle;
        let written = self.write_all(&handle, reader).await;
        let closed = self.sftp.close(handle).await;
        written?;
        closed.map(|_| ())
    }

    /// Send the reader's bytes, `IN_FLIGHT` requests at a time.
    async fn write_all(&self, handle: &str, mut reader: Box<dyn tokio::io::AsyncRead + Unpin + Send>) -> Result<(), SftpError> {
        use tokio::io::AsyncReadExt;
        let mut pending = futures::stream::FuturesUnordered::new();
        let mut offset = 0u64;
        loop {
            let mut chunk = vec![0; CHUNK];
            let mut filled = 0;
            while filled < CHUNK {
                match reader.read(&mut chunk[filled..]).await? {
                    0 => break,
                    n => filled += n,
                }
            }
            if filled == 0 {
                break;
            }
            chunk.truncate(filled);
            pending.push(self.sftp.write(handle.to_string(), offset, chunk));
            offset += filled as u64;
            if pending.len() >= IN_FLIGHT {
                if let Some(done) = pending.next().await {
                    done?;
                }
            }
        }
        while let Some(done) = pending.next().await {
            done?;
        }
        Ok(())
    }

    /// Remove a file, and the folders that removing it emptied.
    pub(super) async fn remove(&self, path: &str) -> Result<(), SftpError> {
        let (dir, _) = split(path);
        match self.check_inside(dir).await {
            Err(e) if is_missing(&e) => return Ok(()),
            other => other?,
        }
        match self.sftp.remove(self.full(path)).await {
            Ok(_) => {}
            Err(e) if is_missing(&e) => return Ok(()),
            Err(e) => return Err(e),
        }
        // Folders the deletion emptied go too, up to the root; the server
        // refuses to remove one with anything left in it.
        let mut dir = dir;
        while !dir.is_empty() {
            if self.sftp.rmdir(self.full(dir)).await.is_err() {
                break;
            }
            self.inside.lock().unwrap_or_else(|p| p.into_inner()).remove(dir);
            dir = split(dir).0;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Destination for SftpDestination {
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        match self.check_inside(split(path).0).await {
            Err(e) if is_missing(&e) => return Ok(None),
            other => other.map_err(|e| format!("read {path}: {e}"))?,
        }
        let handle = match self.sftp.open(self.full(path), OpenFlags::READ, FileAttributes::default()).await {
            Ok(h) => h.handle,
            Err(e) if is_missing(&e) => return Ok(None),
            Err(e) => return Err(format!("read {path}: {e}")),
        };
        let mut bytes = Vec::new();
        let read = loop {
            match self.sftp.read(&handle, bytes.len() as u64, CHUNK as u32).await {
                Ok(data) => bytes.extend_from_slice(&data.data),
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break Ok(()),
                Err(e) => break Err(format!("read {path}: {e}")),
            }
        };
        let _ = self.sftp.close(handle).await;
        read.map(|()| Some(bytes))
    }

    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String> {
        self.put(path, body).await.map_err(|r| r.to_string())
    }

    async fn delete(&self, path: &str) -> Result<(), String> {
        self.remove(path).await.map_err(|e| format!("delete {path}: {e}"))
    }
}
