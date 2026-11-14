//! A direct publish through the folder destination, from a built site
//! directory to what lands on the destination.

use super::folder::FolderDestination;
use super::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// A built site: its directory and the manifest a seal would give it.
struct Site {
    dir: tempfile::TempDir,
    files: HashMap<String, String>,
}

fn site(pages: &[(&str, &str)]) -> Site {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().expect("site dir");
    let mut files = HashMap::new();
    for (path, body) in pages {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
        let hash = format!("{:x}", Sha256::digest(body.as_bytes()));
        files.insert(path.to_string(), crate::types::content::file_entry(&hash));
    }
    Site { dir, files }
}

const OWNER: &str = "site-a";

async fn publish_to(dest: &dyn Destination, site: &Site) -> Result<Published, String> {
    publish(dest, OWNER, site.dir.path(), &site.files, &crate::deploy::progress::silent(), false).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Put,
    Delete,
}

/// The folder destination, failing the operations `fails` picks and counting
/// the writes it lets through.
struct Faulty {
    inner: FolderDestination,
    fails: fn(Op, &str) -> bool,
    writes: AtomicU32,
}

fn faulty(inner: &FolderDestination, fails: fn(Op, &str) -> bool) -> Faulty {
    Faulty { inner: inner.clone(), fails, writes: AtomicU32::new(0) }
}

#[async_trait::async_trait]
impl Destination for Faulty {
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        self.inner.get(path).await
    }
    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String> {
        if (self.fails)(Op::Put, path) {
            return Err("connection lost".to_string());
        }
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.inner.put_atomic(path, body).await
    }
    async fn delete(&self, path: &str) -> Result<(), String> {
        if (self.fails)(Op::Delete, path) {
            return Err("connection lost".to_string());
        }
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.inner.delete(path).await
    }
}

fn read(root: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(root.join(path)).ok()
}

fn recorded(root: &Path) -> BTreeSet<String> {
    let bytes = std::fs::read(root.join(RECORD_NAME)).expect("a record");
    Record::parse(&bytes).unwrap().files().keys().cloned().collect()
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

const V1: &[(&str, &str)] = &[
    ("index.html", "<link href=\"/_moss/site.1.css\">home v1"),
    ("about/index.html", "<link href=\"/_moss/site.1.css\">about"),
    ("_moss/site.1.css", "body{color:red}"),
    ("photo.jpg", "jpeg"),
];

const V2: &[(&str, &str)] = &[
    ("index.html", "<link href=\"/_moss/site.2.css\">home v2"),
    ("new/index.html", "<link href=\"/_moss/site.2.css\">new"),
    ("_moss/site.2.css", "body{color:blue}"),
    ("photo.jpg", "jpeg"),
];

#[tokio::test]
async fn a_first_publish_puts_every_file_and_records_them() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    let v1 = site(V1);

    let done = publish_to(&dest, &v1).await.unwrap();

    assert_eq!(done, Published { uploaded: 4, removed: 0 });
    for (path, body) in V1 {
        assert_eq!(read(out.path(), path).as_deref(), Some(*body), "{path}");
    }
    assert_eq!(recorded(out.path()), set(&["index.html", "about/index.html", "_moss/site.1.css", "photo.jpg"]));
}

#[tokio::test]
async fn a_second_publish_puts_what_changed_and_deletes_what_went() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();

    let done = publish_to(&dest, &site(V2)).await.unwrap();

    assert_eq!(done, Published { uploaded: 3, removed: 2 });
    assert_eq!(read(out.path(), "index.html").as_deref(), Some(V2[0].1));
    assert!(read(out.path(), "about/index.html").is_none());
    assert!(!out.path().join("about").exists(), "a folder the deletions emptied goes too");
    assert!(read(out.path(), "_moss/site.1.css").is_none());
    assert_eq!(recorded(out.path()), set(&["index.html", "new/index.html", "_moss/site.2.css", "photo.jpg"]));
}

#[tokio::test]
async fn a_publish_with_nothing_changed_writes_nothing() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V2)).await.unwrap();
    let counting = faulty(&dest, |_, _| false);

    assert_eq!(publish_to(&counting, &site(V2)).await.unwrap(), Published { uploaded: 0, removed: 0 });
    assert_eq!(counting.writes.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_destination_another_site_published_to_is_refused_before_anything_is_written() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();
    let other = site(V2);

    let refused = publish(&dest, "site-b", other.dir.path(), &other.files, &crate::deploy::progress::silent(), false)
        .await
        .unwrap_err();

    assert!(refused.contains("another site"), "{refused}");
    for (path, body) in V1 {
        assert_eq!(read(out.path(), path).as_deref(), Some(*body), "{path}");
    }
}

#[tokio::test]
async fn a_file_moss_did_not_put_there_survives_every_publish() {
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join(".htaccess"), "Options -Indexes").unwrap();
    std::fs::create_dir_all(out.path().join("about")).unwrap();
    std::fs::write(out.path().join("about/notes.txt"), "mine").unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();

    publish_to(&dest, &site(V1)).await.unwrap();
    publish_to(&dest, &site(V2)).await.unwrap();

    assert_eq!(read(out.path(), ".htaccess").as_deref(), Some("Options -Indexes"));
    assert_eq!(read(out.path(), "about/notes.txt").as_deref(), Some("mine"));
    assert!(read(out.path(), "about/index.html").is_none());
}

#[tokio::test]
async fn with_no_record_on_the_destination_nothing_is_deleted() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();
    std::fs::remove_file(out.path().join(RECORD_NAME)).unwrap();

    let done = publish_to(&dest, &site(V2)).await.unwrap();

    assert_eq!(done.removed, 0);
    assert!(read(out.path(), "about/index.html").is_some());
    assert!(read(out.path(), "_moss/site.1.css").is_some());
}

#[tokio::test]
async fn a_publish_stopped_before_its_deletions_leaves_a_record_that_finishes_the_job() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();

    let stopped = publish_to(&faulty(&dest, |op, _| op == Op::Delete), &site(V2)).await;

    assert!(stopped.is_err());
    assert_eq!(read(out.path(), "index.html").as_deref(), Some(V2[0].1), "the puts landed");
    let record = recorded(out.path());
    assert!(record.contains("about/index.html") && record.contains("new/index.html"), "old and new: {record:?}");

    let done = publish_to(&dest, &site(V2)).await.unwrap();
    assert_eq!(done, Published { uploaded: 0, removed: 2 });
    assert!(read(out.path(), "about/index.html").is_none());
    assert_eq!(recorded(out.path()), set(&["index.html", "new/index.html", "_moss/site.2.css", "photo.jpg"]));
}

#[tokio::test]
async fn a_publish_that_would_delete_most_of_the_site_is_refused_before_any_byte_moves() {
    let pages: Vec<(String, String)> = (0..40).map(|i| (format!("p{i}.html"), format!("page {i}"))).collect();
    let big: Vec<(&str, &str)> = pages.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect();
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(&big)).await.unwrap();
    let before = std::fs::read(out.path().join(RECORD_NAME)).unwrap();
    let collapsed = site(&[("index.html", "almost nothing")]);

    let refused = publish_to(&dest, &collapsed).await.unwrap_err();

    assert!(refused.contains("Deploy aborted"), "{refused}");
    assert!(read(out.path(), "index.html").is_none(), "nothing was put");
    assert_eq!(std::fs::read(out.path().join(RECORD_NAME)).unwrap(), before);

    let sink = crate::deploy::progress::silent();
    let allowed = publish(&dest, OWNER, collapsed.dir.path(), &collapsed.files, &sink, true).await.unwrap();
    assert_eq!(allowed.removed, 40);
}

/// After every operation, looks at the folder the way a visitor would.
struct Visitor {
    inner: FolderDestination,
    root: PathBuf,
    old: BTreeSet<String>,
    new: BTreeSet<String>,
    looks: AtomicU32,
    seen_missing: Mutex<Vec<String>>,
}

impl Visitor {
    fn look(&self) {
        self.looks.fetch_add(1, Ordering::Relaxed);
        let present = |p: &String| self.root.join(p).is_file();
        let mut missing = self.seen_missing.lock().unwrap();
        // Until every new file is in place, every old one must still be.
        if !self.new.iter().all(present) {
            missing.extend(self.old.iter().filter(|p| !present(p)).cloned());
        }
        // Every page on the destination finds what it links.
        for page in self.old.union(&self.new).filter(|p| p.ends_with(".html")) {
            let Ok(html) = std::fs::read_to_string(self.root.join(page)) else { continue };
            for link in html.split("href=\"/").skip(1).filter_map(|s| s.split('"').next()) {
                if !self.root.join(link).is_file() {
                    missing.push(format!("{link} (linked from {page})"));
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Destination for Visitor {
    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        self.inner.get(path).await
    }
    async fn put_atomic(&self, path: &str, body: Body) -> Result<(), String> {
        let r = self.inner.put_atomic(path, body).await;
        self.look();
        r
    }
    async fn delete(&self, path: &str) -> Result<(), String> {
        let r = self.inner.delete(path).await;
        self.look();
        r
    }
}

#[tokio::test]
async fn a_visitor_during_a_publish_never_meets_a_missing_file() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();
    let visitor = Visitor {
        inner: dest,
        root: out.path().to_path_buf(),
        old: V1.iter().map(|(p, _)| p.to_string()).collect(),
        new: V2.iter().map(|(p, _)| p.to_string()).collect(),
        looks: AtomicU32::new(0),
        seen_missing: Mutex::new(Vec::new()),
    };

    publish_to(&visitor, &site(V2)).await.unwrap();

    assert!(visitor.looks.load(Ordering::Relaxed) >= 5, "the visitor looked after every operation");
    assert_eq!(*visitor.seen_missing.lock().unwrap(), Vec::<String>::new());
}

/// A reader holding the old file keeps reading the old bytes whole: the put
/// replaced the file rather than rewriting it in place.
#[tokio::test]
async fn a_put_replaces_the_file_in_one_step() {
    use std::io::Read;
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    dest.put_atomic("page.html", Body::Bytes(b"old page".to_vec())).await.unwrap();
    let mut reader = std::fs::File::open(out.path().join("page.html")).unwrap();

    dest.put_atomic("page.html", Body::Bytes(b"the new page, longer".to_vec())).await.unwrap();

    let mut held = String::new();
    reader.read_to_string(&mut held).unwrap();
    assert_eq!(held, "old page");
    assert_eq!(read(out.path(), "page.html").as_deref(), Some("the new page, longer"));
    let leftovers: Vec<_> = std::fs::read_dir(out.path()).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
    assert_eq!(leftovers, ["page.html"], "no temporary file is left behind");
}

/// The record may have been written by someone else: a path it names that a
/// link inside the destination carries outside is never deleted.
#[cfg(unix)]
#[tokio::test]
async fn a_recorded_path_that_leads_outside_the_destination_is_not_deleted() {
    let out = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("victim.txt"), "keep").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), out.path().join("link")).unwrap();
    let mut forged = std::collections::BTreeMap::new();
    forged.insert("link/victim.txt".to_string(), "100644:x".to_string());
    forged.insert("index.html".to_string(), "100644:y".to_string());
    let forged = serde_json::json!({"version": 1, "owner": OWNER, "files": forged});
    std::fs::write(out.path().join(RECORD_NAME), forged.to_string()).unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();

    let result = publish_to(&dest, &site(&[("index.html", "home")])).await;

    assert!(result.unwrap_err().contains("leads outside"));
    assert_eq!(read(elsewhere.path(), "victim.txt").as_deref(), Some("keep"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_in_the_site_is_published_as_a_link() {
    let mut s = site(&[("app/index.html", "app")]);
    std::os::unix::fs::symlink("app", s.dir.path().join("latest")).unwrap();
    s.files.insert("latest".to_string(), crate::types::content::symlink_entry("app"));
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();

    publish_to(&dest, &s).await.unwrap();

    assert_eq!(std::fs::read_link(out.path().join("latest")).unwrap(), PathBuf::from("app"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_out_of_the_site_stops_the_publish_before_anything_is_written() {
    let mut s = site(&[("index.html", "home")]);
    std::os::unix::fs::symlink("../../etc", s.dir.path().join("escape")).unwrap();
    s.files.insert("escape".to_string(), crate::types::content::symlink_entry("../../etc"));
    let out = tempfile::tempdir().unwrap();

    let refused = publish_to(&FolderDestination::new(out.path()).unwrap(), &s).await.unwrap_err();

    assert!(refused.contains("points outside the site"), "{refused}");
    assert!(read(out.path(), RECORD_NAME).is_none());
}

#[test]
fn a_destination_folder_that_does_not_exist_is_refused() {
    let gone = tempfile::tempdir().unwrap().path().join("missing");
    assert!(FolderDestination::new(&gone).err().unwrap().starts_with("No folder at"));
}

/// The stopped publish put v2's stylesheet and home page. Going back to v1
/// must put v1's home page again although the record last named v1's bytes
/// for it, and delete the stylesheet although no finished publish named it.
#[tokio::test]
async fn a_publish_stopped_partway_through_its_puts_still_owns_what_it_put() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    publish_to(&dest, &site(V1)).await.unwrap();

    assert!(publish_to(&faulty(&dest, |op, path| op == Op::Put && path == "new/index.html"), &site(V2)).await.is_err());
    assert_eq!(read(out.path(), "index.html").as_deref(), Some(V2[0].1));

    publish_to(&dest, &site(V1)).await.unwrap();
    assert_eq!(read(out.path(), "index.html").as_deref(), Some(V1[0].1));
    assert!(read(out.path(), "_moss/site.2.css").is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn a_published_file_is_readable_by_the_web_server() {
    use std::os::unix::fs::PermissionsExt;
    let s = site(&[("index.html", "home")]);
    std::fs::set_permissions(s.dir.path().join("index.html"), std::fs::Permissions::from_mode(0o600)).unwrap();
    let out = tempfile::tempdir().unwrap();

    publish_to(&FolderDestination::new(out.path()).unwrap(), &s).await.unwrap();

    let mode = std::fs::metadata(out.path().join("index.html")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);
}

#[tokio::test]
async fn a_site_file_whose_path_leaves_the_destination_is_refused() {
    let mut s = site(&[("index.html", "home")]);
    s.files.insert("../escape.html".to_string(), "100644:x".to_string());
    let out = tempfile::tempdir().unwrap();

    let refused = publish_to(&FolderDestination::new(out.path()).unwrap(), &s).await.unwrap_err();

    assert!(refused.contains("../escape.html"), "{refused}");
    assert!(read(out.path(), RECORD_NAME).is_none(), "refused before anything was written");
}

#[tokio::test]
async fn a_plain_file_host_is_sent_no_members_only_file() {
    let out = tempfile::tempdir().unwrap();
    let dest = FolderDestination::new(out.path()).unwrap();
    let built = site(&[
        ("index.html", "home"),
        ("post/index.html", "public part"),
        ("_gated/post/index.html", "members part"),
        ("_gated/map/rows.csv", "members rows"),
    ]);

    let done = publish_to(&dest, &built).await.unwrap();

    assert_eq!(done, Published { uploaded: 2, removed: 0 });
    assert_eq!(recorded(out.path()), set(&["index.html", "post/index.html"]));
    assert!(!out.path().join("_gated").exists());
}
