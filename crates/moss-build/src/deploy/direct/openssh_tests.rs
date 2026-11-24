//! Against the real `ssh` and `ssh-keygen`, reading fixture files only: the
//! config is passed with `-F` and names its own `known_hosts`.

use std::path::Path;
use std::process::Command;

use russh::keys::PublicKey;

use super::*;

fn have_openssh() -> bool {
    let ok = Command::new("ssh").arg("-V").output().is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipped: no OpenSSH client on this system");
    }
    ok
}

fn key(dir: &Path, kind: &str, name: &str) -> PublicKey {
    let path = dir.join(name);
    let ok = Command::new("ssh-keygen").args(["-q", "-t", kind, "-N", "", "-C", name, "-f"]).arg(&path).status().unwrap();
    assert!(ok.success());
    PublicKey::from_openssh(std::fs::read_to_string(path.with_extension("pub")).unwrap().trim()).unwrap()
}

fn line(hosts: &str, key: &PublicKey) -> String {
    let text = key.to_openssh().unwrap();
    let mut words = text.split_whitespace();
    format!("{hosts} {} {}\n", words.next().unwrap(), words.next().unwrap())
}

struct Fixture {
    dir: tempfile::TempDir,
    ssh: Ssh,
}

/// A config whose `known_hosts` holds `entries`, hashed when `hashed`.
fn fixture(config: &str, entries: &str, hashed: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let kh = dir.path().join("known_hosts");
    std::fs::write(&kh, entries).unwrap();
    if hashed {
        assert!(Command::new("ssh-keygen").arg("-H").arg("-f").arg(&kh).output().unwrap().status.success());
    }
    let cfg = dir.path().join("config");
    let all = format!("{config}\nHost *\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n", kh.display());
    std::fs::write(&cfg, all).unwrap();
    Fixture { dir, ssh: Ssh { config: Some(cfg) } }
}

fn server(text: &str) -> Server {
    Server::parse(text).unwrap()
}

#[test]
fn a_config_alias_resolves_to_its_host_user_port_and_key_files() {
    if !have_openssh() {
        return;
    }
    let f = fixture("Host box\n  HostName box.example\n  User deploy\n  Port 2222\n  IdentityFile ~/.ssh/id_box\n  IdentitiesOnly yes\n  IdentityAgent none\n", "", false);
    let resolved = f.ssh.resolve(&server("box")).unwrap();
    assert_eq!((resolved.host.as_str(), resolved.user.as_str(), resolved.port), ("box.example", "deploy", 2222));
    assert!(resolved.identities_only);
    assert_eq!(resolved.agent, Agent::None);
    let home = std::env::var("HOME").unwrap();
    assert_eq!(resolved.identity_files, vec![Path::new(&home).join(".ssh/id_box")]);
    // A user typed in the field wins over the config's.
    assert_eq!(f.ssh.resolve(&server("ops@box")).unwrap().user, "ops");
}

#[test]
fn a_listed_key_differing_and_new_key_types_are_told_apart() {
    if !have_openssh() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (a, b, c) = (key(tmp.path(), "ed25519", "a"), key(tmp.path(), "ed25519", "b"), key(tmp.path(), "ecdsa", "c"));
    for hashed in [false, true] {
        let entries = line("one.example", &a) + &line("@cert-authority *.example", &b) + &line("other.example", &c);
        let f = fixture("", &entries, hashed);
        let one = f.ssh.resolve(&server("deploy@one.example")).unwrap();
        assert_eq!(f.ssh.known(&one, &a), Known::Listed, "hashed: {hashed}");
        assert_eq!(f.ssh.known(&one, &b), Known::Differs, "a CA line is not a listing; hashed: {hashed}");
        // Listed under another key type only: new, as OpenSSH treats it.
        assert_eq!(f.ssh.known(&one, &c), Known::New, "hashed: {hashed}");
        assert_eq!(f.ssh.known_kinds(&one), vec!["ssh-ed25519".to_string()]);
        let nowhere = f.ssh.resolve(&server("deploy@nowhere.example")).unwrap();
        assert_eq!(f.ssh.known(&nowhere, &a), Known::New);
        drop(f.dir);
    }
}

#[test]
fn a_port_and_an_alias_are_looked_up_the_way_ssh_files_them() {
    if !have_openssh() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let a = key(tmp.path(), "ed25519", "a");
    let entries = line("[port.example]:2222", &a) + &line("pinned-name", &a);
    let f = fixture("Host aliased\n  HostName real.example\n  Port 2200\n  HostKeyAlias pinned-name\n", &entries, true);
    assert_eq!(f.ssh.known(&f.ssh.resolve(&server("sftp://deploy@port.example:2222/")).unwrap(), &a), Known::Listed);
    assert_eq!(f.ssh.known(&f.ssh.resolve(&server("deploy@port.example")).unwrap(), &a), Known::New, "port 22 is another entry");
    assert_eq!(f.ssh.known(&f.ssh.resolve(&server("deploy@aliased")).unwrap(), &a), Known::Listed);
}

#[test]
fn a_revoked_key_is_revoked_even_when_also_listed() {
    if !have_openssh() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let a = key(tmp.path(), "ed25519", "a");
    let f = fixture("", &(line("one.example", &a) + &line("@revoked one.example", &a)), false);
    assert_eq!(f.ssh.known(&f.ssh.resolve(&server("deploy@one.example")).unwrap(), &a), Known::Revoked);
}

#[test]
fn only_literal_hosts_are_suggested() {
    let text = "Host box web.example\n  User x\nHost *.internal !bad\nhost box  other\nMatch host y\n";
    assert_eq!(hosts_in(text), vec!["box", "web.example", "other"]);
}
