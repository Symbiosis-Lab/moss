//! Publishing to your own server: the check that says, step by step, whether
//! moss can publish there, and the publish itself once a build is sealed.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use super::server::{self, Keys, Server, Settings};
use super::openssh::{Resolved, Ssh};
use super::sftp::{self, Refusal, SftpDestination};
use super::{Allow, Body, Published};
use crate::build::manifest::SealedManifest;
use crate::deploy::progress::DeploySink;
use crate::plugins::types::DeploymentInfo;

/// The steps of reaching a server, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Reach,
    Trust,
    SignIn,
    Write,
    /// The address serves what was written: a question, never a stop, since
    /// a new server's domain often points elsewhere until it moves.
    Address,
}

/// One step's outcome, in the words a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct Step {
    pub kind: StepKind,
    pub passed: bool,
    pub text: String,
    /// Why a failed step failed.
    pub refusal: Option<Refusal>,
}

/// What a check found. The steps stop at the first that failed, and the
/// fields after them say what would get past it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct Checked {
    pub steps: Vec<Step>,
    /// The server's host key, when it is new or changed: the person trusts
    /// it by checking again with it.
    pub trust: Option<String>,
    /// The line to add to the server's `authorized_keys`, when it refused
    /// moss's key.
    pub authorized_key: Option<String>,
    /// The web folders that take files, when there are several and none
    /// showed at the address: the person picks one.
    pub folders: Vec<String>,
    /// The address the last step fetched.
    pub address: String,
    /// The folder checked: the one found and saved when none was given.
    pub folder: String,
}

impl Checked {
    pub fn passed(&self) -> bool {
        self.steps.iter().any(|s| s.passed && s.kind == StepKind::Write)
    }
}

impl Refusal {
    /// The step this refusal stops.
    fn step(&self) -> StepKind {
        match self {
            Refusal::NoSuchServer { .. } | Refusal::NoAnswer { .. } | Refusal::PortRefused { .. } | Refusal::Broken { .. } => {
                StepKind::Reach
            }
            Refusal::UnknownHostKey { .. }
            | Refusal::HostKeyChanged { .. }
            | Refusal::HostKeyDiffers { .. }
            | Refusal::HostKeyRevoked { .. }
            | Refusal::RsaOnly { .. } => StepKind::Trust,
            Refusal::KeyRefused { .. }
            | Refusal::NoUser { .. }
            | Refusal::SignInFailed { .. }
            | Refusal::NeedsPassword { .. }
            | Refusal::PasswordRefused { .. }
            | Refusal::KeysOnly { .. } => StepKind::SignIn,
            Refusal::NotServed { .. } => StepKind::Address,
            Refusal::NoSftp { .. }
            | Refusal::NoFolder { .. }
            | Refusal::NoWebFolder { .. }
            | Refusal::WhichFolder { .. }
            | Refusal::CannotWrite { .. }
            | Refusal::WriteFailed { .. }
            | Refusal::NoAtomicRename { .. } => {
                StepKind::Write
            }
        }
    }
}

fn passed(kind: StepKind, reached: &Server, folder: &str, address: &str) -> Step {
    let host = &reached.host;
    let text = match kind {
        StepKind::Reach => format!("Reached {host}"),
        StepKind::Trust => format!("Trusted {host}"),
        StepKind::SignIn => format!("Signed in as {}", reached.user),
        StepKind::Write => format!("Can publish to {folder}"),
        StepKind::Address => format!("{address} shows this folder"),
    };
    Step { kind, passed: true, text, refusal: None }
}

/// The label on the folder's public key line: the folder's name.
pub fn label(folder: &Path) -> String {
    folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "moss".into())
}

/// Connect as a publish would. `trust` is a host key the person just
/// accepted, new or in place of a changed one; it is pinned once the server
/// shows that key. The outer `Err` is this computer failing (its config or
/// its key store), not the server.
async fn open(
    ssh: &Ssh,
    resolved: &Resolved,
    folder: &Path,
    keys: &Keys,
    settings: &Settings,
    trust: Option<&str>,
) -> Result<Result<SftpDestination, Refusal>, String> {
    let key = keys.key_or_new(&server::publisher(folder)?)?;
    let pinned = trust.or(settings.host_key.as_deref());
    let (trusted, connected) = sftp::connect(ssh, resolved, &settings.server.folder, pinned, key).await;
    if let Some(fingerprint) = trusted.filter(|t| settings.host_key.as_deref() != Some(t.as_str())) {
        server::save_host_key(folder, &fingerprint)?;
    }
    Ok(connected)
}

/// Run every step against the folder's server: reach it, trust its key, sign
/// in, write, rename and remove a file in its folder, and fetch that file at
/// the site's address. Without a folder, the usual web folders are tried, and
/// the one the address shows is saved.
pub async fn check(folder: &Path, keys: &Keys, trust: Option<&str>) -> Result<Checked, String> {
    check_among(&Ssh::default(), folder, keys, trust, &web_folders()).await
}

pub(super) fn web_folders() -> Vec<String> {
    sftp::WEB_FOLDERS.iter().map(|f| f.to_string()).collect()
}

pub(super) async fn check_among(ssh: &Ssh, folder: &Path, keys: &Keys, trust: Option<&str>, candidates: &[String]) -> Result<Checked, String> {
    let settings = server::settings(folder)?.ok_or("Set the server first.")?;
    match sftp::resolve(ssh, &settings.server).await {
        Ok(resolved) => check_resolved(ssh, &resolved, folder, keys, trust, candidates).await,
        Err(refusal) => refused(folder, keys, refusal, &settings.server),
    }
}

/// [`check_among`] for a server already resolved.
pub(super) async fn check_resolved(
    ssh: &Ssh,
    resolved: &Resolved,
    folder: &Path,
    keys: &Keys,
    trust: Option<&str>,
    candidates: &[String],
) -> Result<Checked, String> {
    let mut settings = server::settings(folder)?.ok_or("Set the server first.")?;
    let reached = resolved.reached(&settings.server.folder);
    let address = address(folder, &reached)?;
    let mut folders = Vec::new();
    let opened = open(ssh, resolved, folder, keys, &settings, trust).await?;
    if opened.is_ok() && settings.server != reached {
        settings.server = reached.clone();
        server::save_reached(folder, &settings.server)?;
    }
    let refused = match opened {
        Err(refusal) => Some(refusal),
        Ok(dest) if !settings.server.folder.is_empty() => probe(&dest, &address).await.err(),
        Ok(mut dest) => {
            // Only a miss at the address moves on to the next folder; any other
            // refusal means moss could not publish there.
            let (mut served, mut writable, mut stop) = (None, Vec::new(), None);
            let candidates = candidates.iter().map(|f| f.replace("{host}", &settings.server.host)).collect::<Vec<_>>();
            for candidate in dest.folders(&candidates).await {
                dest.move_to(&candidate);
                match probe(&dest, &address).await {
                    Ok(()) => {
                        served = Some(candidate);
                        break;
                    }
                    Err(Refusal::NotServed { .. }) => writable.push(candidate),
                    Err(refusal) => {
                        stop.get_or_insert(refusal);
                    }
                }
            }
            // One writable folder is the folder: the address is then the question.
            let only = (writable.len() == 1).then(|| writable[0].clone());
            if let Some(found) = served.clone().or(only) {
                settings.server.folder = found;
                server::save_server(folder, &settings.server)?;
            }
            match (served, writable.len(), stop) {
                (Some(_), ..) => None,
                (None, 1, _) => Some(Refusal::NotServed { address: address.clone() }),
                (None, 0, Some(refusal)) => Some(refusal),
                (None, 0, None) => Some(Refusal::NoWebFolder { host: settings.server.host.clone() }),
                (None, ..) => {
                    folders = writable;
                    Some(Refusal::WhichFolder { host: settings.server.host.clone() })
                }
            }
        }
    };
    report(folder, keys, refused, &reached, &settings.server.folder, address, folders)
}

/// The steps up to `refused`, and what would get past it.
pub(super) fn report(
    folder: &Path,
    keys: &Keys,
    refused: Option<Refusal>,
    reached: &Server,
    found: &str,
    address: String,
    folders: Vec<String>,
) -> Result<Checked, String> {
    let mut steps = Vec::new();
    for kind in [StepKind::Reach, StepKind::Trust, StepKind::SignIn, StepKind::Write, StepKind::Address] {
        match &refused {
            Some(r) if r.step() == kind => {
                steps.push(Step { kind, passed: false, text: r.to_string(), refusal: Some(r.clone()) });
                break;
            }
            _ => steps.push(passed(kind, reached, found, &address)),
        }
    }
    let trust = match &refused {
        Some(Refusal::UnknownHostKey { fingerprint, .. }) => Some(fingerprint.clone()),
        Some(Refusal::HostKeyChanged { now, .. }) => Some(now.clone()),
        _ => None,
    };
    let authorized_key = match &refused {
        Some(
            Refusal::KeyRefused { .. } | Refusal::NeedsPassword { .. } | Refusal::PasswordRefused { .. } | Refusal::KeysOnly { .. },
        ) => Some(keys.public_line(&server::publisher(folder)?, &label(folder))?),
        _ => None,
    };
    Ok(Checked { steps, trust, authorized_key, folders, address, folder: found.to_string() })
}

/// The steps up to a refusal met before any folder was looked at.
pub(super) fn refused(folder: &Path, keys: &Keys, refusal: Refusal, reached: &Server) -> Result<Checked, String> {
    report(folder, keys, Some(refusal), reached, &reached.folder, address(folder, reached)?, Vec::new())
}

/// The site's address: the folder's own, or the server's as reached.
pub(super) fn address(folder: &Path, reached: &Server) -> Result<String, String> {
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy())?;
    Ok(config.site_address()?.unwrap_or_else(|| reached.web_address()))
}

/// Write a file, rename it over itself, fetch it at `address`, and remove it.
/// A failed fetch is [`Refusal::NotServed`], returned after the file is gone.
async fn probe(dest: &SftpDestination, address: &str) -> Result<(), Refusal> {
    let name = format!("moss-check-{}.txt", std::process::id());
    let token = format!("moss {}", uuid::Uuid::new_v4());
    dest.put(&name, Body::Bytes(token.clone().into_bytes())).await?;
    let served = shows(address, &name, &token).await;
    dest.remove(&name).await.map_err(|e| Refusal::WriteFailed { path: name.clone(), reason: e.to_string() })?;
    if served { Ok(()) } else { Err(Refusal::NotServed { address: address.to_string() }) }
}

/// Whether `address` serves the file `name` holding `token`.
async fn shows(address: &str, name: &str, token: &str) -> bool {
    let url = format!("{}/{name}", address.trim_end_matches('/'));
    let token = token.to_string();
    tokio::task::spawn_blocking(move || {
        let (builder, _) = crate::system::proxy::proxied_agent_builder(&url, std::time::Duration::from_secs(10), 3);
        let body = builder.build().get(&url).call().ok().and_then(|r| r.into_string().ok());
        body.is_some_and(|b| b.trim() == token)
    })
    .await
    .unwrap_or(false)
}

/// Publish a sealed build to the folder's server, after the gates every
/// upload passes. Both binaries enter here: the app with the seal its watcher
/// made, `moss deploy` through [`build_and_publish`]. Records the landing as
/// every publish does, and returns it with what changed there; its address is
/// the one the check fetches.
pub async fn publish_sealed(
    folder: &Path,
    sealed: Option<&SealedManifest>,
    keys: &Keys,
    ports: &dyn crate::build::ports::deploy::DeployPorts,
    sink: &Arc<dyn DeploySink>,
    allow: Allow,
) -> Result<(DeploymentInfo, Published), String> {
    let settings = publishable(folder)?;
    let ssh = Ssh::default();
    let resolved = sftp::resolve(&ssh, &settings.server).await.map_err(|r| r.to_string())?;
    let url = address(folder, &resolved.reached(&settings.server.folder))?;
    let sealed = crate::deploy::plugin_push::sealed_for_upload(folder, sealed).await?;
    let site_dir = crate::moss_paths::MossPaths::new(folder).generation_dir(sealed.generation_id());
    let dest = open(&ssh, &resolved, folder, keys, &settings, None).await?.map_err(|r| r.to_string())?;
    sink.stage(crate::deploy::progress::DeployStage::Uploading, 0, 0, &format!("Uploading to {}", settings.server.host));
    let published = super::publish(&dest, &server::publisher(folder)?, &site_dir, &sealed.files(), sink, allow).await?;
    let deployment = DeploymentInfo {
        method: crate::config::deployment::SERVER_TARGET_ID.into(),
        url,
        deployed_at: chrono::Utc::now().to_rfc3339(),
        metadata: Default::default(),
        dns_target: None,
        addresses: Vec::new(),
    };
    crate::deploy::landed::record_publish_landed(folder, sealed, &deployment, None, ports).await;
    Ok((deployment, published))
}

/// The folder's server, if a publish may go there: one is set and its key
/// trusted. Asked before the build too, so a refusal costs no build.
fn publishable(folder: &Path) -> Result<Settings, String> {
    let settings = server::settings(folder)?
        .ok_or("Set the server first: moss deploy --to deploy@example.com:/var/www/html")?;
    if settings.host_key.is_none() {
        return Err(format!("Check {} first, to trust its key: moss deploy --check", settings.server.host));
    }
    Ok(settings)
}

/// `moss deploy` to your own server: build the folder, then
/// [`publish_sealed`].
pub async fn build_and_publish(
    folder: &Path,
    host_ports: &(dyn Fn(&str) -> crate::build::HostPorts + Send + Sync),
    plugins: crate::build::PluginMode,
    overrides: crate::deploy::push::PublishOverrides,
    take_over: bool,
    keys: &Keys,
    sink: &Arc<dyn DeploySink>,
) -> Result<(DeploymentInfo, Published), String> {
    let folder_str = folder.to_string_lossy().to_string();
    publishable(folder)?;
    let _session = crate::system::folder_session::register_session(&folder_str);
    let sealed = crate::deploy::one_shot::build_for_publish(folder, host_ports(&folder_str), plugins, &overrides, sink).await?;
    let allow = Allow { mass_removal: overrides.accept_removals, take_over };
    publish_sealed(folder, sealed.as_ref(), keys, &crate::deploy::one_shot::HeadlessDeployPorts, sink, allow).await
}
