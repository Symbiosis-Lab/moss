//! Connect against a real `sshd` whose SFTP sessions start in a scratch
//! folder standing in for the home folder, so no real `authorized_keys` is
//! ever written. The person's key and `known_hosts` are fixture files.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::super::check::StepKind;
use super::super::sftp_tests::{keygen, Sshd};
use super::*;
use super::super::server::KEY_OPTIONS;

/// A publisher whose moss key the server does not know yet.
const NEW_PUBLISHER: &str = "fedcba9876543210";

struct Rig {
    _scratch: tempfile::TempDir,
    sshd: Sshd,
    home: PathBuf,
    folder: tempfile::TempDir,
    ssh: Ssh,
}

/// `sshd` trusting the person's key when `person` is set; `known_hosts` lists
/// the server's own key, or a stranger's when `known_wrong`.
fn rig(person: bool, known_wrong: bool) -> Option<Rig> {
    let scratch_dir = tempfile::tempdir().unwrap();
    let scratch = scratch_dir.path().to_path_buf();
    let home = scratch.join("home");
    std::fs::create_dir(&home).unwrap();
    keygen("ed25519", &scratch.join("person"));
    let person_keys = scratch.join("person_keys");
    let trusted = if person { std::fs::read_to_string(scratch.join("person.pub")).unwrap() } else { String::new() };
    std::fs::write(&person_keys, trusted).unwrap();
    let auth = format!("{}/.ssh/authorized_keys {}", home.display(), person_keys.display());
    let sshd = Sshd::start_with("ed25519", &auth, &format!("-d {}", home.display()))?;

    let shown = if known_wrong {
        keygen("ed25519", &scratch.join("stranger"));
        std::fs::read_to_string(scratch.join("stranger.pub")).unwrap()
    } else {
        std::fs::read_to_string(sshd.dir.path().join("host.pub")).unwrap()
    };
    let mut words = shown.split_whitespace();
    let (kind, body) = (words.next().unwrap(), words.next().unwrap());
    std::fs::write(scratch.join("known_hosts"), format!("[127.0.0.1]:{} {kind} {body}\n", sshd.port)).unwrap();
    let config = format!(
        "Host *\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n",
        scratch.join("person").display(),
        scratch.join("known_hosts").display()
    );
    std::fs::write(scratch.join("config"), config).unwrap();

    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join(".moss")).unwrap();
    std::fs::write(folder.path().join(".moss/config.toml"), format!("[deploy]\npublisher = \"{NEW_PUBLISHER}\"\n")).unwrap();
    server::save_server(folder.path(), &sshd.server()).unwrap();
    Some(Rig { _scratch: scratch_dir, sshd, home, folder, ssh: Ssh { config: Some(scratch.join("config")) } })
}

fn authorized(home: &Path) -> String {
    std::fs::read_to_string(home.join(".ssh/authorized_keys")).unwrap_or_default()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// The person's own key signs in once; moss's key is added with the
/// options that stop forwarding, the key `known_hosts` lists is pinned with
/// no question, and the check that follows signs in with moss's key.
#[tokio::test]
async fn connect_adds_moss_key_and_the_check_then_passes() {
    let Some(rig) = rig(true, false) else { return };
    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    assert!(checked.passed(), "{checked:?}");
    let lines = authorized(&rig.home);
    let line = lines.lines().next().unwrap();
    assert!(line.starts_with(&format!("{KEY_OPTIONS} ssh-ed25519 ")), "{line}");
    let name = rig.folder.path().file_name().unwrap().to_string_lossy().into_owned();
    assert!(line.ends_with(&name), "labelled with the folder's name: {line}");
    assert_eq!((mode(&rig.home.join(".ssh")), mode(&rig.home.join(".ssh/authorized_keys"))), (0o700, 0o600));
    let settings = server::settings(rig.folder.path()).unwrap().unwrap();
    assert_eq!(settings.host_key, Some(rig.sshd.host_key()));

    // Connecting again adds nothing.
    connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    assert_eq!(authorized(&rig.home), lines);
}

/// A last line with no newline is ended first, so the two keys stay apart,
/// and an existing file keeps its mode.
#[tokio::test]
async fn connect_keeps_an_unterminated_last_line_whole() {
    let Some(rig) = rig(true, false) else { return };
    std::fs::create_dir(rig.home.join(".ssh")).unwrap();
    let file = rig.home.join(".ssh/authorized_keys");
    std::fs::write(&file, "ssh-ed25519 AAAAexisting someone").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    assert!(checked.passed(), "{checked:?}");
    let lines: Vec<String> = authorized(&rig.home).lines().map(str::to_string).collect();
    assert_eq!(lines[0], "ssh-ed25519 AAAAexisting someone");
    assert!(lines[1].starts_with(KEY_OPTIONS), "{lines:?}");
    assert_eq!(mode(&file), 0o640);
}

/// With no key of the person's that signs in and a server that takes no
/// password, the key line is what is left.
#[tokio::test]
async fn connect_with_nothing_that_signs_in_offers_the_key_line() {
    let Some(rig) = rig(false, false) else { return };
    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    let failed = checked.steps.last().unwrap();
    assert_eq!((failed.kind, failed.passed), (StepKind::SignIn, false));
    assert!(matches!(failed.refusal, Some(Refusal::KeysOnly { .. })), "{failed:?}");
    assert!(checked.authorized_key.is_some());
    assert_eq!(authorized(&rig.home), "");
}

/// A server whose key this computer's ssh lists differently is refused, as
/// `ssh` refuses it, with nothing offered to trust and nothing sent.
#[tokio::test]
async fn a_key_known_hosts_lists_differently_is_a_stop_not_a_question() {
    let Some(rig) = rig(true, true) else { return };
    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    let failed = checked.steps.last().unwrap();
    assert_eq!((failed.kind, failed.passed), (StepKind::Trust, false));
    assert!(matches!(failed.refusal, Some(Refusal::HostKeyDiffers { .. })), "{failed:?}");
    assert_eq!(checked.trust, None);
    assert_eq!(authorized(&rig.home), "");
    // The check refuses it the same way, even with the key offered to trust.
    let pinned = check::check_among(&rig.ssh, rig.folder.path(), &rig.sshd.keys, Some(&rig.sshd.host_key()), &[]).await.unwrap();
    assert!(matches!(pinned.steps.last().unwrap().refusal, Some(Refusal::HostKeyDiffers { .. })), "{pinned:?}");
}

#[test]
fn a_password_never_prints() {
    assert_eq!(format!("{:?}", Password::new("hunter2".into())), "Password(…)");
}

/// The check itself trusts a key `known_hosts` lists, asking nothing, and
/// pins it in the folder.
#[tokio::test]
async fn the_check_pins_a_key_known_hosts_lists_without_asking() {
    let Some(rig) = rig(false, false) else { return };
    let checked = check::check_among(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None, &[]).await.unwrap();
    assert_eq!(checked.trust, None);
    assert!(checked.steps.iter().any(|s| s.kind == StepKind::Trust && s.passed), "{checked:?}");
    assert_eq!(server::settings(rig.folder.path()).unwrap().unwrap().host_key, Some(rig.sshd.host_key()));
}

/// An `authorized_keys` too large to read whole is refused and left as it
/// was, never cut short and appended to.
#[tokio::test]
async fn an_oversized_authorized_keys_is_refused_untouched() {
    let Some(rig) = rig(true, false) else { return };
    std::fs::create_dir(rig.home.join(".ssh")).unwrap();
    let file = rig.home.join(".ssh/authorized_keys");
    let big = "# padding\n".repeat((MOST_BYTES / 10) + 1);
    std::fs::write(&file, &big).unwrap();
    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    let failed = checked.steps.last().unwrap();
    assert!(matches!(failed.refusal, Some(Refusal::WriteFailed { .. })), "{failed:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), big);
}

/// A key file in `dir` named `name`, with its public half beside it.
fn key_file(dir: &Path, name: &str) -> (PathBuf, PublicKey) {
    let path = dir.join(name);
    keygen("ed25519", &path);
    let public = PublicKey::from_openssh(std::fs::read_to_string(path.with_extension("pub")).unwrap().trim()).unwrap();
    (path, public)
}

fn kinds(offers: &[Offer]) -> Vec<(&'static str, PublicKey)> {
    offers.iter().map(|o| (if matches!(o, Offer::Agent(_)) { "agent" } else { "file" }, o.public().clone())).collect()
}

/// As OpenSSH orders them: the agent's keys a file names, then its others,
/// then files the agent doesn't hold; `IdentitiesOnly` drops the others.
#[test]
fn offers_follow_openssh_order() {
    let dir = tempfile::tempdir().unwrap();
    let (held_file, held) = key_file(dir.path(), "held");
    let (only_file, only) = key_file(dir.path(), "only");
    let (_, other) = key_file(dir.path(), "other");
    let files = [only_file, held_file];
    let agent = [other.clone(), held.clone()];
    assert_eq!(
        kinds(&offers(&files, false, &agent)),
        vec![("agent", held.clone()), ("agent", other), ("file", only.clone())]
    );
    assert_eq!(kinds(&offers(&files, true, &agent)), vec![("agent", held), ("file", only)]);
}

/// The same key named twice is offered once, and no more than five are.
#[test]
fn offers_are_deduped_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let (first, _) = key_file(dir.path(), "first");
    let copy = dir.path().join("copy");
    std::fs::copy(&first, &copy).unwrap();
    assert_eq!(offers(&[first, copy], true, &[]).len(), 1);
    let agent: Vec<PublicKey> = (0..7).map(|i| key_file(dir.path(), &format!("agent{i}")).1).collect();
    assert_eq!(offers(&[], false, &agent).len(), MOST_KEYS);
}

/// A key file with a passphrase is left to the agent, never offered.
#[test]
fn an_encrypted_key_file_is_not_offered() {
    let dir = tempfile::tempdir().unwrap();
    let (plain, _) = key_file(dir.path(), "plain");
    let locked = dir.path().join("locked");
    let ok = std::process::Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "secret", "-f"]).arg(&locked).status().unwrap();
    assert!(ok.success());
    assert_eq!(offers(&[locked, plain], true, &[]).len(), 1);
}

/// A server named by an alias in this computer's ssh config is stored as it
/// was reached once signed in, so the folder publishes from a computer that
/// has no such alias.
#[tokio::test]
async fn an_alias_is_stored_as_reached_and_publishes_without_it() {
    let Some(rig) = rig(true, false) else { return };
    let config = rig.ssh.config.clone().unwrap();
    let user = rig.sshd.server().user;
    let alias = format!("Host walkalias\n  HostName 127.0.0.1\n  Port {}\n  User {user}\n", rig.sshd.port);
    std::fs::write(&config, alias + &std::fs::read_to_string(&config).unwrap()).unwrap();
    let www = rig.sshd.server().folder;
    server::save_server(rig.folder.path(), &server::Server { user: String::new(), host: "walkalias".into(), port: 22, folder: www }).unwrap();

    let checked = connect_with(&rig.ssh, rig.folder.path(), &rig.sshd.keys, None).await.unwrap();
    assert!(checked.passed(), "{checked:?}");
    let stored = server::settings(rig.folder.path()).unwrap().unwrap();
    assert_eq!((stored.server.host.as_str(), stored.server.port, stored.server.user.as_str()), ("127.0.0.1", rig.sshd.port, user.as_str()));
    assert_eq!(stored.host_key, Some(rig.sshd.host_key()));

    let elsewhere = check::check_among(&super::super::sftp_tests::isolated(), rig.folder.path(), &rig.sshd.keys, None, &check::web_folders()).await.unwrap();
    assert!(elsewhere.passed(), "{elsewhere:?}");
}
