//! Publishing to your own server: the check that says, step by step, whether
//! moss can publish there, and the publish itself once a build is sealed.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use super::server::{self, Keys, Settings};
use super::sftp::{self, Refusal, SftpDestination};
use super::{Allow, Body, Destination, Published};
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
}

impl Checked {
    pub fn passed(&self) -> bool {
        self.steps.last().is_some_and(|s| s.passed && s.kind == StepKind::Write)
    }
}

impl Refusal {
    /// The step this refusal stops.
    fn step(&self) -> StepKind {
        match self {
            Refusal::NoSuchServer { .. } | Refusal::NoAnswer { .. } | Refusal::PortRefused { .. } | Refusal::Broken { .. } => {
                StepKind::Reach
            }
            Refusal::UnknownHostKey { .. } | Refusal::HostKeyChanged { .. } | Refusal::RsaOnly { .. } => StepKind::Trust,
            Refusal::KeyRefused { .. } | Refusal::SignInFailed { .. } => StepKind::SignIn,
            Refusal::NoSftp { .. }
            | Refusal::NoFolder { .. }
            | Refusal::CannotWrite { .. }
            | Refusal::WriteFailed { .. }
            | Refusal::NoAtomicRename { .. } => {
                StepKind::Write
            }
        }
    }
}

fn passed(kind: StepKind, settings: &Settings) -> Step {
    let host = &settings.server.host;
    let text = match kind {
        StepKind::Reach => format!("Reached {host}"),
        StepKind::Trust => format!("Trusted {host}"),
        StepKind::SignIn => format!("Signed in as {}", settings.server.user),
        StepKind::Write => format!("Can publish to {}", settings.server.folder),
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
async fn open(folder: &Path, keys: &Keys, settings: &Settings, trust: Option<&str>) -> Result<Result<SftpDestination, Refusal>, String> {
    let key = keys.key_or_new(&server::publisher(folder)?)?;
    let pinned = trust.or(settings.host_key.as_deref());
    let connected = sftp::connect(&settings.server, pinned, key).await;
    let past_trust = connected.as_ref().err().is_none_or(|r| matches!(r.step(), StepKind::SignIn | StepKind::Write));
    if let Some(fingerprint) = trust.filter(|t| past_trust && settings.host_key.as_deref() != Some(*t)) {
        server::save_host_key(folder, fingerprint)?;
    }
    Ok(connected)
}

/// Run every step against the folder's server: reach it, trust its key, sign
/// in, and write, rename and remove a file in its folder.
pub async fn check(folder: &Path, keys: &Keys, trust: Option<&str>) -> Result<Checked, String> {
    let settings = server::settings(folder)?.ok_or("Set the server first.")?;
    let refused = match open(folder, keys, &settings, trust).await? {
        Ok(dest) => probe(&dest).await.err(),
        Err(refusal) => Some(refusal),
    };
    let mut steps = Vec::new();
    for kind in [StepKind::Reach, StepKind::Trust, StepKind::SignIn, StepKind::Write] {
        match &refused {
            Some(r) if r.step() == kind => {
                steps.push(Step { kind, passed: false, text: r.to_string(), refusal: Some(r.clone()) });
                break;
            }
            _ => steps.push(passed(kind, &settings)),
        }
    }
    let trust = match &refused {
        Some(Refusal::UnknownHostKey { fingerprint, .. }) => Some(fingerprint.clone()),
        Some(Refusal::HostKeyChanged { now, .. }) => Some(now.clone()),
        _ => None,
    };
    let authorized_key = match &refused {
        Some(Refusal::KeyRefused { .. }) => Some(keys.public_line(&server::publisher(folder)?, &label(folder))?),
        _ => None,
    };
    Ok(Checked { steps, trust, authorized_key })
}

/// Write a file, rename it over itself and remove it.
async fn probe(dest: &SftpDestination) -> Result<(), Refusal> {
    let name = format!(".moss-check-{}", std::process::id());
    dest.put(&name, Body::Bytes(b"moss".to_vec())).await?;
    dest.remove(&name).await.map_err(|e| Refusal::WriteFailed { path: name.clone(), reason: e.to_string() })
}

/// Publish a sealed build to the folder's server, after the gates every
/// upload passes. Both binaries enter here: the app with the seal its watcher
/// made, `moss deploy` through [`build_and_publish`]. Records the landing as
/// every publish does, and returns it with what changed there; its address is
/// the folder's `[site].url`, or the server folder when it names none.
pub async fn publish_sealed(
    folder: &Path,
    sealed: Option<&SealedManifest>,
    keys: &Keys,
    ports: &dyn crate::build::ports::deploy::DeployPorts,
    sink: &Arc<dyn DeploySink>,
    allow: Allow,
) -> Result<(DeploymentInfo, Published), String> {
    let settings = server::settings(folder)?
        .ok_or("Set the server first: moss deploy --to deploy@example.com:/var/www/html")?;
    if settings.host_key.is_none() {
        return Err(format!("Check {} first, to trust its key: moss deploy --check", settings.server.host));
    }
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy())?;
    let url = config.site_address()?.unwrap_or_else(|| settings.server.to_url());
    let sealed = crate::deploy::plugin_push::sealed_for_upload(folder, sealed).await?;
    let site_dir = crate::moss_paths::MossPaths::new(folder).generation_dir(sealed.generation_id());
    let dest = open(folder, keys, &settings, None).await?.map_err(|r| r.to_string())?;
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
    crate::deploy::landed::record_publish_landed(folder, &sealed, &deployment, None, ports).await;
    Ok((deployment, published))
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
    let _session = crate::system::folder_session::register_session(&folder_str);
    let sealed = crate::deploy::one_shot::build_for_publish(folder, host_ports(&folder_str), plugins, &overrides, sink).await?;
    let allow = Allow { mass_removal: overrides.accept_removals, take_over };
    publish_sealed(folder, sealed.as_ref(), keys, &crate::deploy::one_shot::HeadlessDeployPorts, sink, allow).await
}
