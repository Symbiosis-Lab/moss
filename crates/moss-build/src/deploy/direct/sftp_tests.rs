//! The SFTP destination against a real OpenSSH server: `sshd` run as the
//! current user on a free local port, with its own host keys and an
//! `authorized_keys` holding the key moss made. A system without
//! `/usr/sbin/sshd` skips these, saying so.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;

use super::check;
use super::server::{self, Keys, Server};
use super::sftp::{connect, Refusal};
use super::*;

const SSHD: &str = "/usr/sbin/sshd";

fn sftp_server() -> Option<&'static str> {
    ["/usr/libexec/sftp-server", "/usr/lib/openssh/sftp-server", "/usr/lib/ssh/sftp-server"]
        .into_iter()
        .find(|p| Path::new(p).exists())
}

/// A running `sshd`, stopped when dropped.
struct Sshd {
    child: Child,
    port: u16,
    dir: tempfile::TempDir,
    /// The folder the site is published into.
    www: PathBuf,
    keys: Keys,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn keygen(kind: &str, path: &Path) {
    let ok = Command::new("ssh-keygen").args(["-q", "-t", kind, "-N", "", "-f"]).arg(path).status().unwrap();
    assert!(ok.success(), "ssh-keygen -t {kind}");
}

/// The host key's fingerprint as moss shows it.
fn fingerprint(public: &Path) -> String {
    let text = std::fs::read_to_string(public).unwrap();
    let key = russh::keys::PublicKey::from_openssh(text.trim()).unwrap();
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}

impl Sshd {
    /// Start a server whose only host key is of `host_kind`, trusting
    /// moss's key for the publisher `PUBLISHER`.
    fn start(host_kind: &str) -> Option<Sshd> {
        let (Some(sftp), true) = (sftp_server(), Path::new(SSHD).exists()) else {
            eprintln!("skipped: no OpenSSH server on this system");
            return None;
        };
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        keygen(host_kind, &d.join("host"));
        let keys = Keys::in_folder(&d.join("moss-data"));
        let line = keys.public_line(PUBLISHER, "test-site").unwrap();
        std::fs::write(d.join("authorized_keys"), format!("{line}\n")).unwrap();
        let www = d.join("www");
        std::fs::create_dir(&www).unwrap();
        let port = free_port();
        let config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {host}\nPidFile {pid}\nAuthorizedKeysFile {auth}\n\
             StrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nSubsystem sftp {sftp}\n",
            host = d.join("host").display(),
            pid = d.join("sshd.pid").display(),
            auth = d.join("authorized_keys").display(),
        );
        std::fs::write(d.join("sshd_config"), config).unwrap();
        let child = Command::new(SSHD).arg("-D").arg("-f").arg(d.join("sshd_config")).arg("-E").arg(d.join("sshd.log")).spawn().unwrap();
        let sshd = Sshd { child, port, dir, www, keys };
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return Some(sshd);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("sshd never listened: {}", std::fs::read_to_string(sshd.dir.path().join("sshd.log")).unwrap_or_default());
    }

    fn server(&self) -> Server {
        Server {
            user: std::env::var("USER").unwrap_or_else(|_| whoami()),
            host: "127.0.0.1".into(),
            port: self.port,
            folder: self.www.display().to_string(),
        }
    }

    fn host_key(&self) -> String {
        fingerprint(&self.dir.path().join("host.pub"))
    }

    fn key(&self) -> Arc<russh::keys::PrivateKey> {
        self.keys.key(PUBLISHER).unwrap().expect("the key moss made")
    }
}

fn whoami() -> String {
    String::from_utf8(Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string()
}

const PUBLISHER: &str = "0123456789abcdef";

struct Site {
    dir: tempfile::TempDir,
    files: HashMap<String, String>,
}

/// A built site: files, and links given as `(path, "->target")`.
fn site(entries: &[(&str, &str)]) -> Site {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let mut files = HashMap::new();
    for (path, body) in entries {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        if let Some(target) = body.strip_prefix("->") {
            std::os::unix::fs::symlink(target, &full).unwrap();
            files.insert(path.to_string(), crate::types::content::symlink_entry(target));
        } else {
            std::fs::write(&full, body).unwrap();
            let hash = format!("{:x}", Sha256::digest(body.as_bytes()));
            files.insert(path.to_string(), crate::types::content::file_entry(&hash));
        }
    }
    Site { dir, files }
}

async fn publish_site(dest: &dyn Destination, site: &Site) -> Result<Published, String> {
    super::publish(dest, PUBLISHER, site.dir.path(), &site.files, &crate::deploy::progress::silent(), Allow::default()).await
}

#[tokio::test]
async fn a_site_published_over_sftp_lands_whole_and_a_second_publish_sends_only_the_change() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    std::fs::write(sshd.www.join(".htaccess"), "keep me").unwrap();
    let dest = connect(&sshd.server(), Some(&sshd.host_key()), sshd.key()).await.unwrap();

    let big = "x".repeat(200_000);
    let first = site(&[
        ("index.html", "home"),
        ("notes/a/index.html", "a"),
        ("notes/b/index.html", "b"),
        ("assets/big.css", &big),
        ("old/index.html", "->../notes/a/index.html"),
    ]);
    assert_eq!(publish_site(&dest, &first).await.unwrap(), Published { uploaded: 5, removed: 0 });
    assert_eq!(std::fs::read_to_string(sshd.www.join("notes/a/index.html")).unwrap(), "a");
    assert_eq!(std::fs::read_to_string(sshd.www.join("assets/big.css")).unwrap(), big);
    // The link was made with its target and its name the right way round.
    assert_eq!(std::fs::read_link(sshd.www.join("old/index.html")).unwrap(), Path::new("../notes/a/index.html"));
    assert_eq!(std::fs::read_to_string(sshd.www.join("old/index.html")).unwrap(), "a");

    let second = site(&[
        ("index.html", "home, changed"),
        ("notes/a/index.html", "a"),
        ("assets/big.css", &big),
        ("old/index.html", "->../notes/a/index.html"),
    ]);
    assert_eq!(publish_site(&dest, &second).await.unwrap(), Published { uploaded: 1, removed: 1 });
    assert_eq!(std::fs::read_to_string(sshd.www.join("index.html")).unwrap(), "home, changed");
    assert!(!sshd.www.join("notes/b").exists(), "a removed page's folder goes with it");
    assert_eq!(std::fs::read_to_string(sshd.www.join(".htaccess")).unwrap(), "keep me");
    let leftovers: Vec<_> = walk(&sshd.www).into_iter().filter(|p| p.contains("moss-tmp")).collect();
    assert!(leftovers.is_empty(), "no temporary file is left: {leftovers:?}");
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() && !path.is_symlink() {
            out.extend(walk(&path));
        } else {
            out.push(path.display().to_string());
        }
    }
    out
}

#[tokio::test]
async fn a_host_key_seen_for_the_first_time_is_named_and_not_trusted() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let refused = connect(&sshd.server(), None, sshd.key()).await.err().unwrap();
    assert_eq!(refused, Refusal::UnknownHostKey { host: "127.0.0.1".into(), fingerprint: sshd.host_key() });
}

#[tokio::test]
async fn a_changed_host_key_is_a_hard_stop_naming_both() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let was = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let refused = connect(&sshd.server(), Some(was), sshd.key()).await.err().unwrap();
    assert_eq!(refused, Refusal::HostKeyChanged { host: "127.0.0.1".into(), was: was.into(), now: sshd.host_key() });
}

#[tokio::test]
async fn a_server_offering_only_an_rsa_host_key_gets_its_own_sentence() {
    let Some(sshd) = Sshd::start("rsa") else { return };
    let refused = connect(&sshd.server(), None, sshd.key()).await.err().unwrap();
    assert_eq!(refused, Refusal::RsaOnly { host: "127.0.0.1".into() });
}

#[tokio::test]
async fn a_key_the_server_does_not_list_is_refused() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let stranger = Keys::in_folder(&sshd.dir.path().join("other-computer"));
    stranger.public_line(PUBLISHER, "test-site").unwrap();
    let refused = connect(&sshd.server(), Some(&sshd.host_key()), stranger.key(PUBLISHER).unwrap().unwrap()).await.err().unwrap();
    assert_eq!(refused, Refusal::KeyRefused { host: "127.0.0.1".into() });
}

#[tokio::test]
async fn a_folder_that_is_missing_or_read_only_is_refused_at_the_write() {
    use std::os::unix::fs::PermissionsExt;
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let mut missing = sshd.server();
    missing.folder = format!("{}/nope", missing.folder);
    let refused = connect(&missing, Some(&sshd.host_key()), sshd.key()).await.err().unwrap();
    assert_eq!(refused, Refusal::NoFolder { folder: missing.folder.clone() });

    std::fs::set_permissions(&sshd.www, std::fs::Permissions::from_mode(0o555)).unwrap();
    let dest = connect(&sshd.server(), Some(&sshd.host_key()), sshd.key()).await.unwrap();
    let failed = publish_site(&dest, &site(&[("index.html", "home")])).await.unwrap_err();
    std::fs::set_permissions(&sshd.www, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(failed, Refusal::CannotWrite { folder: dest.root().to_string() }.to_string());
}

/// A write that fails halfway leaves the live file as it was: the half
/// written temporary is never renamed over it.
#[tokio::test]
async fn a_write_that_fails_halfway_leaves_the_live_file_alone() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    std::fs::write(sshd.www.join("index.html"), "live").unwrap();
    let dest = connect(&sshd.server(), Some(&sshd.host_key()), sshd.key()).await.unwrap();
    // A directory opens, then fails its first read: the temporary on the
    // server is already created when the write fails.
    let unreadable = sshd.dir.path().join("a-directory");
    std::fs::create_dir(&unreadable).unwrap();
    assert!(dest.put_atomic("index.html", Body::File(unreadable)).await.is_err());
    assert_eq!(std::fs::read_to_string(sshd.www.join("index.html")).unwrap(), "live");
}

#[tokio::test]
async fn a_port_nothing_listens_on_is_refused_by_name() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let mut wrong = sshd.server();
    wrong.port = free_port();
    let refused = connect(&wrong, None, sshd.key()).await.err().unwrap();
    assert_eq!(refused, Refusal::PortRefused { host: "127.0.0.1".into(), port: wrong.port });
}

#[tokio::test]
async fn a_link_in_the_server_folder_leading_out_of_it_is_never_written_through() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let outside = sshd.dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, sshd.www.join("notes")).unwrap();
    let dest = connect(&sshd.server(), Some(&sshd.host_key()), sshd.key()).await.unwrap();
    let failed = publish_site(&dest, &site(&[("notes/a.html", "a")])).await.unwrap_err();
    assert!(failed.contains("leads outside"), "{failed}");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
}

/// The check a person runs from Settings: the first run names the host key,
/// trusting it pins it in the folder's config, and the steps then pass.
#[tokio::test]
async fn the_check_names_the_host_key_then_pins_it_once_trusted() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), format!("[deploy]\npublisher = \"{PUBLISHER}\"\n")).unwrap();
    server::save_server(folder.path(), &sshd.server()).unwrap();

    let first = check::check(folder.path(), &sshd.keys, None).await.unwrap();
    assert_eq!(first.trust, Some(sshd.host_key()));
    assert_eq!(first.steps.iter().map(|s| (s.kind, s.passed)).collect::<Vec<_>>(), [(check::StepKind::Reach, true), (check::StepKind::Trust, false)]);

    let trusted = check::check(folder.path(), &sshd.keys, Some(&sshd.host_key())).await.unwrap();
    assert!(trusted.passed(), "{trusted:?}");
    assert_eq!(server::settings(folder.path()).unwrap().unwrap().host_key, Some(sshd.host_key()));
    assert!(walk(&sshd.www).is_empty(), "the check leaves nothing behind");

    // A rebuilt server: its new key stops every check until it is trusted.
    server::save_host_key(folder.path(), "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
    let changed = check::check(folder.path(), &sshd.keys, None).await.unwrap();
    assert_eq!(changed.trust, Some(sshd.host_key()));
    assert!(check::check(folder.path(), &sshd.keys, Some(&sshd.host_key())).await.unwrap().passed());
    assert_eq!(server::settings(folder.path()).unwrap().unwrap().host_key, Some(sshd.host_key()));
}

/// A refused key: the check stops at signing in and hands over the line to
/// add, which then lets it through.
#[tokio::test]
async fn a_refused_key_comes_back_with_the_line_to_add() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), "").unwrap();
    server::save_server(folder.path(), &sshd.server()).unwrap();
    server::save_host_key(folder.path(), &sshd.host_key()).unwrap();
    let this_computer = Keys::in_folder(&sshd.dir.path().join("this-computer"));

    let refused = check::check(folder.path(), &this_computer, None).await.unwrap();
    assert_eq!(refused.steps.last().map(|s| (s.kind, s.passed)), Some((check::StepKind::SignIn, false)));
    let line = refused.authorized_key.expect("the line to add");
    std::fs::write(sshd.dir.path().join("authorized_keys"), format!("{line}\n")).unwrap();
    assert!(check::check(folder.path(), &this_computer, None).await.unwrap().passed());
}

/// A server that shows no plain key is never offered for trust: an empty
/// fingerprint, once pinned, would read as a changed key at every connect.
#[test]
fn a_server_showing_no_plain_key_has_nothing_to_trust() {
    let refusal = super::sftp::untrusted("example.com".into(), None, None);
    assert!(matches!(refusal, Refusal::Broken { .. }), "{refusal:?}");
    let seen = super::sftp::untrusted("example.com".into(), None, Some("SHA256:x".into()));
    assert_eq!(seen, Refusal::UnknownHostKey { host: "example.com".into(), fingerprint: "SHA256:x".into() });
}

/// The app words every refusal from moss-ui's strings, so a new kind cannot
/// ship without one.
#[test]
fn every_refusal_has_words_in_the_app() {
    let strings = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/moss-ui/src/ui/i18n/ui-strings.ts"),
    )
    .unwrap();
    let s = String::new;
    let all = [
        Refusal::NoSuchServer { host: s() },
        Refusal::NoAnswer { host: s(), port: 22 },
        Refusal::PortRefused { host: s(), port: 22 },
        Refusal::UnknownHostKey { host: s(), fingerprint: s() },
        Refusal::HostKeyChanged { host: s(), was: s(), now: s() },
        Refusal::RsaOnly { host: s() },
        Refusal::KeyRefused { host: s() },
        Refusal::SignInFailed { host: s(), reason: s() },
        Refusal::NoSftp { host: s() },
        Refusal::NoFolder { folder: s() },
        Refusal::NoWebFolder { host: s() },
        Refusal::WhichFolder { host: s() },
        Refusal::CannotWrite { folder: s() },
        Refusal::NotServed { address: s() },
        Refusal::WriteFailed { path: s(), reason: s() },
        Refusal::NoAtomicRename { host: s() },
        Refusal::Broken { host: s(), reason: s() },
    ];
    for refusal in all {
        // A new kind does not compile here until it is listed above.
        match refusal {
            Refusal::NoSuchServer { .. } | Refusal::NoAnswer { .. } | Refusal::PortRefused { .. } | Refusal::UnknownHostKey { .. }
            | Refusal::HostKeyChanged { .. } | Refusal::RsaOnly { .. } | Refusal::KeyRefused { .. } | Refusal::SignInFailed { .. }
            | Refusal::NoSftp { .. } | Refusal::NoFolder { .. } | Refusal::NoWebFolder { .. } | Refusal::WhichFolder { .. }
            | Refusal::CannotWrite { .. } | Refusal::NotServed { .. } | Refusal::WriteFailed { .. }
            | Refusal::NoAtomicRename { .. } | Refusal::Broken { .. } => {}
        }
        let kind = serde_json::to_value(&refusal).unwrap()["kind"].as_str().unwrap().to_string();
        assert_eq!(strings.matches(&format!("'deployment.server.refusal.{kind}'")).count(), 4, "{kind} in each locale");
    }
}

/// A key trusted at a check stays trusted when the same check then stops at
/// sign-in, so adding the key line is the only step left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_key_trusted_at_a_check_that_stops_at_sign_in_stays_trusted() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), "").unwrap();
    server::save_server(folder.path(), &sshd.server()).unwrap();
    let this_computer = Keys::in_folder(&sshd.dir.path().join("this-computer"));

    let refused = check::check(folder.path(), &this_computer, Some(&sshd.host_key())).await.unwrap();
    assert_eq!(refused.steps.last().map(|s| (s.kind, s.passed)), Some((check::StepKind::SignIn, false)));
    assert_eq!(server::settings(folder.path()).unwrap().unwrap().host_key, Some(sshd.host_key()));
}

/// A two-page folder whose server is `sshd`, its key already trusted.
fn site_on(sshd: &Sshd) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), format!("[deploy]\npublisher = \"{PUBLISHER}\"\n")).unwrap();
    std::fs::write(folder.path().join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    std::fs::write(folder.path().join("about.md"), "---\ntitle: About\n---\n\nAbout.\n").unwrap();
    server::save_server(folder.path(), &sshd.server()).unwrap();
    server::save_host_key(folder.path(), &sshd.host_key()).unwrap();
    folder
}

/// The server route passes the gates every upload passes: a seal whose
/// generation a later build replaced is refused before any byte moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seal_a_later_build_replaced_is_refused_before_upload() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = site_on(&sshd);
    let folder_str = folder.path().to_string_lossy().to_string();
    let _session = crate::system::folder_session::register_session(&folder_str);
    let root = crate::vault::paths::VaultRoot::resolve(folder.path());
    let build = || crate::deploy::one_shot::build_and_seal(&root, crate::cli::host::cli_host_ports(&folder_str), crate::build::PluginMode::Skip, false);
    let first = build().await.unwrap().expect("a seal");
    std::fs::write(folder.path().join("about.md"), "---\ntitle: About\n---\n\nAbout, changed.\n").unwrap();
    build().await.unwrap().expect("a second seal");

    let sink = crate::deploy::progress::silent();
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let refused = check::publish_sealed(folder.path(), Some(&first), &sshd.keys, &ports, &sink, Allow::default()).await.unwrap_err();
    assert!(refused.contains("active generation"), "{refused}");
    assert!(!sshd.www.join("index.html").exists());
}

/// What `moss deploy` runs: a real build of a folder, through the gates every
/// upload passes, onto the server; then one changed page sends one file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_built_and_published_to_its_server_sends_only_what_changed_the_second_time() {
    let _env = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = site_on(&sshd);
    let sink = crate::deploy::progress::silent();
    let publish = || check::build_and_publish(
        folder.path(),
        &crate::cli::host::cli_host_ports,
        crate::build::PluginMode::Skip,
        Default::default(),
        false,
        &sshd.keys,
        &sink,
    );

    let (landed, first) = publish().await.unwrap();
    assert_eq!(landed.url, sshd.server().to_url());
    let state = crate::build::site_config::read_deployment_state(&folder.path().to_string_lossy()).unwrap();
    assert_eq!(state.targets["server"].last_deployment_url.as_deref(), Some(landed.url.as_str()), "the landing is recorded");
    assert!(first.uploaded > 2, "{first:?}");
    assert!(std::fs::read_to_string(sshd.www.join("about/index.html")).unwrap().contains("About."));

    std::fs::write(folder.path().join("about.md"), "---\ntitle: About\n---\n\nAbout, changed.\n").unwrap();
    let (_, second) = publish().await.unwrap();
    assert!(std::fs::read_to_string(sshd.www.join("about/index.html")).unwrap().contains("About, changed."));
    assert!(second.uploaded < first.uploaded, "only the change goes: {second:?} after {first:?}");
}

/// A plugin's secrets and moss's server keys share the app's data folder;
/// no plugin name reaches a key.
#[test]
fn no_plugin_can_read_the_key_moss_signs_in_with() {
    let dir = tempfile::tempdir().unwrap();
    Keys::in_folder(dir.path()).public_line(PUBLISHER, "my-blog").unwrap();
    let plugins = crate::identity::secrets::SecretStore::in_app_data(dir.path());
    for name in [PUBLISHER.to_string(), format!("server-{PUBLISHER}"), "server-keys".into()] {
        let scope = crate::identity::keystore::Scope::plugin(&name).unwrap();
        assert_eq!(plugins.get(&scope, "key").unwrap(), None, "{name}");
    }
}

#[test]
fn a_server_address_is_read_in_the_url_and_scp_forms() {
    let url = Server::parse("sftp://deploy@example.com:2222/var/www/html").unwrap();
    assert_eq!(url, Server { user: "deploy".into(), host: "example.com".into(), port: 2222, folder: "/var/www/html".into() });
    assert_eq!(url.to_url(), "sftp://deploy@example.com:2222/var/www/html");
    let scp = Server::parse("deploy@example.com:/var/www/html").unwrap();
    assert_eq!((scp.port, scp.folder.as_str()), (22, "/var/www/html"));
    assert_eq!(scp.to_url(), "sftp://deploy@example.com/var/www/html");
    let v6 = Server::parse("sftp://deploy@[2001:db8::1]:2222/srv/www").unwrap();
    assert_eq!((v6.host.as_str(), v6.port, v6.folder.as_str()), ("2001:db8::1", 2222, "/srv/www"));
    assert_eq!(Server::parse(&v6.to_url()).unwrap(), v6);
    assert!(Server::parse("deploy@example.com:www").unwrap_err().contains("full path"), "a home-relative scp path is not read as /www");
    // No folder: the check looks for it.
    for bare in ["deploy@example.com", "sftp://deploy@example.com", "deploy@example.com:"] {
        let server = Server::parse(bare).unwrap();
        assert_eq!((server.host.as_str(), server.port, server.folder.as_str()), ("example.com", 22, ""), "{bare}");
        assert_eq!(Server::parse(&server.to_url()).unwrap(), server);
    }
    for bad in ["example.com", "@example.com:/x", "sftp://deploy@example.com:port/x"] {
        assert!(Server::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_key_moss_makes_is_kept_and_shown_as_one_authorized_keys_line() {
    let dir = tempfile::tempdir().unwrap();
    let keys = Keys::in_folder(dir.path());
    let line = keys.public_line(PUBLISHER, "my-blog").unwrap();
    assert!(line.starts_with("ssh-ed25519 AAAA") && line.ends_with(" my-blog"), "{line}");
    assert_eq!(keys.public_line(PUBLISHER, "my-blog").unwrap(), line, "made once");
    assert_ne!(Keys::in_folder(dir.path()).public_line("another-site", "my-blog").unwrap(), line);
}

/// A web server over `dir` on a free port, for as long as the test runs.
fn serve(dir: &Path) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port());
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            use std::io::{BufRead, Write};
            let mut stream = stream;
            let mut line = String::new();
            let _ = std::io::BufReader::new(&stream).read_line(&mut line);
            let path = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
            let reply = match std::fs::read(dir.join(&path)) {
                Ok(body) => [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes(), body].concat(),
                Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = stream.write_all(&reply);
        }
    });
    address
}

/// A folder publishing to `sshd` with no server folder given and `address`
/// as its site's address.
fn site_without_folder(sshd: &Sshd, address: &str) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), format!("[site]\nurl = \"{address}\"\n\n[deploy]\npublisher = \"{PUBLISHER}\"\n")).unwrap();
    server::save_server(folder.path(), &Server { folder: String::new(), ..sshd.server() }).unwrap();
    server::save_host_key(folder.path(), &sshd.host_key()).unwrap();
    folder
}

fn kinds(checked: &check::Checked) -> Vec<(check::StepKind, bool)> {
    checked.steps.iter().map(|s| (s.kind, s.passed)).collect()
}

/// Left out, the folder is the web folder the site's address shows, and it
/// is saved; another writable web folder is passed over.
#[tokio::test]
async fn a_folder_left_out_is_the_one_the_address_shows() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let other = sshd.dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let folder = site_without_folder(&sshd, &serve(&sshd.www));
    let candidates = [sshd.dir.path().join("absent"), other, sshd.www.clone()].map(|p| p.display().to_string());

    let checked = check::check_among(folder.path(), &sshd.keys, None, &candidates).await.unwrap();
    assert!(checked.passed() && checked.folders.is_empty(), "{checked:?}");
    assert_eq!(kinds(&checked).last(), Some(&(check::StepKind::Address, true)));
    let saved = server::settings(folder.path()).unwrap().unwrap().server.folder;
    assert_eq!(Path::new(&saved).canonicalize().unwrap(), sshd.www.canonicalize().unwrap());
    assert!(walk(&sshd.www).is_empty(), "the check leaves nothing behind");
}

/// When the address shows none of the web folders found, the person picks
/// one, and nothing is saved for them.
#[tokio::test]
async fn web_folders_the_address_does_not_show_are_offered_to_pick() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let nowhere = format!("http://127.0.0.1:{}/", free_port());
    let folder = site_without_folder(&sshd, &nowhere);
    let candidates = [sshd.www.display().to_string()];

    let checked = check::check_among(folder.path(), &sshd.keys, None, &candidates).await.unwrap();
    assert!(!checked.passed());
    assert!(matches!(checked.steps.last().and_then(|s| s.refusal.clone()), Some(Refusal::WhichFolder { .. })), "{checked:?}");
    assert_eq!(checked.folders.len(), 1);
    assert_eq!(server::settings(folder.path()).unwrap().unwrap().server.folder, "");

    let none = check::check_among(folder.path(), &sshd.keys, None, &[sshd.dir.path().join("absent").display().to_string()]).await.unwrap();
    assert!(matches!(none.steps.last().and_then(|s| s.refusal.clone()), Some(Refusal::NoWebFolder { .. })), "{none:?}");
}

/// A folder named by the person that the address doesn't show yet can still
/// be published to: the last step asks, it doesn't stop.
#[tokio::test]
async fn an_address_that_does_not_show_the_folder_yet_does_not_stop_publishing() {
    let Some(sshd) = Sshd::start("ed25519") else { return };
    let folder = site_on(&sshd);
    let checked = check::check(folder.path(), &sshd.keys, None).await.unwrap();
    assert!(checked.passed(), "{checked:?}");
    assert_eq!(kinds(&checked).last(), Some(&(check::StepKind::Address, false)));
    assert!(matches!(checked.steps.last().and_then(|s| s.refusal.clone()), Some(Refusal::NotServed { .. })));
}
