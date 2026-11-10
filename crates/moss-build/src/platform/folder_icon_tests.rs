use super::compose::{compose_rep, mark_alpha};
use super::finder::{self, Existing, MARKER};
use super::*;
use std::path::Path;

const TAG: &str = "TEST1-000000";

/// A stand-in icon: what Finder decodes does not matter here, only where it goes.
fn icns() -> Option<Vec<u8>> {
    Some(b"icns\0\0\0\x08".to_vec())
}

fn site() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn has_custom_icon_flag(dir: &Path) -> bool {
    finder::get_xattr(dir, "com.apple.FinderInfo", 0).is_some_and(|i| i[8] & 0x04 != 0)
}

#[test]
fn the_fork_is_a_resource_map_holding_icns_at_the_custom_icon_id() {
    let body = b"icns-body".to_vec();
    let fork = finder::resource_fork(&body);
    let be32 = |o: usize| u32::from_be_bytes(fork[o..o + 4].try_into().unwrap()) as usize;
    let be16 = |o: usize| u16::from_be_bytes(fork[o..o + 2].try_into().unwrap());
    let (data_off, map_off) = (be32(0), be32(4));
    assert_eq!(&fork[data_off + 4..data_off + 4 + body.len()], &body[..]);
    let types = map_off + be16(map_off + 24) as usize;
    assert_eq!(be16(types), 0, "one resource type");
    assert_eq!(&fork[types + 2..types + 6], b"icns");
    let refs = types + be16(types + 8) as usize;
    assert_eq!(be16(refs) as i16, -16455);
    assert_eq!(map_off + be32(12), fork.len(), "the map runs to the end of the fork");
}

#[test]
fn a_bare_folder_gets_the_icon_the_flag_and_a_hidden_marker() {
    let dir = site();
    mark_with(dir.path(), TAG, icns);
    let marker = dir.path().join(MARKER);
    assert_eq!(finder::first_resource(&marker), icns());
    assert!(has_custom_icon_flag(dir.path()));
    let info = finder::get_xattr(&marker, "com.apple.FinderInfo", 0).unwrap();
    assert_eq!(&info[..8], b"iconMACS");
    assert!(info[8] & 0x40 != 0, "the marker is invisible");
    assert_eq!(finder::existing(dir.path(), TAG), Existing::Current);
}

#[test]
fn the_current_icon_is_not_written_again() {
    let dir = site();
    mark_with(dir.path(), TAG, icns);
    mark_with(dir.path(), TAG, || panic!("composed for a folder that already has the current icon"));
}

#[test]
fn a_persons_own_icon_is_never_replaced() {
    let dir = site();
    finder::write(dir.path(), b"someone else's icon", TAG).unwrap();
    // Their icon carries no moss tag.
    let marker = dir.path().join(MARKER);
    let name = std::ffi::CString::new("host.moss.folder-icon").unwrap();
    let path = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(marker.as_os_str())).unwrap();
    // SAFETY: NUL-terminated strings that outlive the call.
    unsafe { libc::removexattr(path.as_ptr(), name.as_ptr(), 0) };
    assert_eq!(finder::existing(dir.path(), TAG), Existing::Foreign);
    mark_with(dir.path(), TAG, || panic!("composed over a person's own icon"));
    assert_eq!(finder::first_resource(&marker).unwrap(), b"someone else's icon");
}

#[test]
fn an_icon_moss_wrote_earlier_is_redrawn() {
    let dir = site();
    finder::write(dir.path(), b"older recipe", "OLD-111111").unwrap();
    assert_eq!(finder::existing(dir.path(), TAG), Existing::Moss);
    mark_with(dir.path(), TAG, icns);
    assert_eq!(finder::first_resource(&dir.path().join(MARKER)), icns());
    let fork = std::fs::metadata(dir.path().join(MARKER).join("..namedfork/rsrc")).unwrap().len();
    assert_eq!(fork as usize, finder::resource_fork(&icns().unwrap()).len(), "no tail of the older, longer fork");
}

#[test]
fn a_marker_whose_icon_a_copy_dropped_is_drawn_again() {
    // git, zip and scp keep `Icon\r` as an empty file without its fork.
    let dir = site();
    std::fs::write(dir.path().join(MARKER), b"").unwrap();
    assert_eq!(finder::existing(dir.path(), TAG), Existing::None);
    mark_with(dir.path(), TAG, icns);
    assert_eq!(finder::first_resource(&dir.path().join(MARKER)), icns());
}

#[test]
fn the_marker_stays_on_this_mac_in_a_synced_folder() {
    let dir = site();
    mark_with(dir.path(), TAG, icns);
    assert!(crate::moss_paths::has_cloud_sync_marker(&dir.path().join(MARKER)));
}

#[test]
fn a_link_under_the_marker_name_is_left_alone() {
    let dir = site();
    let elsewhere = site();
    let target = elsewhere.path().join("notes.txt");
    std::fs::write(&target, "mine").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join(MARKER)).unwrap();
    mark_with(dir.path(), TAG, || panic!("composed for a folder whose marker is a link"));
    assert!(finder::get_xattr(&target, "com.apple.ResourceFork", 0).is_none());
}

#[test]
fn an_icon_whose_folder_flag_was_lost_is_drawn_again() {
    let dir = site();
    mark_with(dir.path(), TAG, icns);
    let mut info = finder::get_xattr(dir.path(), "com.apple.FinderInfo", 0).unwrap();
    info[8] &= !0x04;
    let (p, n) = (
        std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(dir.path().as_os_str())).unwrap(),
        std::ffi::CString::new("com.apple.FinderInfo").unwrap(),
    );
    // SAFETY: NUL-terminated strings and a live buffer, all outliving the call.
    unsafe { libc::setxattr(p.as_ptr(), n.as_ptr(), info.as_ptr().cast(), info.len(), 0, 0) };
    assert_eq!(finder::existing(dir.path(), TAG), Existing::Moss);
    mark_with(dir.path(), TAG, icns);
    assert!(has_custom_icon_flag(dir.path()));
}

#[test]
fn a_linked_worktree_excludes_the_marker_in_the_main_repository() {
    let repo = site();
    let common = repo.path().join("main/.git");
    let gitdir = common.join("worktrees/feature");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    let tree = repo.path().join("feature");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    mark_with(&tree, TAG, icns);
    assert_eq!(std::fs::read(common.join("info/exclude")).unwrap(), b"Icon\r\r\n");
}

#[test]
fn a_repository_in_a_synced_folder_is_not_edited() {
    let home = site();
    let dir = home.path().join("Library/CloudStorage/GoogleDrive-someone@example.com/My Drive/site");
    std::fs::create_dir_all(dir.join(".git/info")).unwrap();
    mark_with(&dir, TAG, icns);
    assert!(!dir.join(".git/info/exclude").exists());
}

#[test]
fn the_marker_stays_out_of_git_status() {
    let dir = site();
    std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
    std::fs::write(dir.path().join(".git/info/exclude"), "*.log").unwrap();
    mark_with(dir.path(), TAG, icns);
    mark_with(dir.path(), "NEXT-222222", icns);
    let exclude = std::fs::read(dir.path().join(".git/info/exclude")).unwrap();
    assert_eq!(exclude, b"*.log\nIcon\r\r\n", "added once, after the existing rules");
}

#[test]
fn the_mark_sits_centred_on_the_folder_front_and_not_below_32px() {
    let mark = mark_alpha();
    let folder = |dim: u32| {
        // A folder-like body: opaque across 10%..90% of the width.
        image::RgbaImage::from_fn(dim, dim, |x, y| {
            let inside = x >= dim / 10 && x < dim * 9 / 10 && y >= dim / 5 && y < dim * 9 / 10;
            image::Rgba(if inside { [91, 190, 235, 255] } else { [0, 0, 0, 0] })
        })
    };
    let plain = folder(256);
    let drawn = compose_rep(plain.clone(), &mark);
    let changed: Vec<(u32, u32)> =
        drawn.enumerate_pixels().filter(|(x, y, p)| **p != *plain.get_pixel(*x, *y)).map(|(x, y, _)| (x, y)).collect();
    assert!(!changed.is_empty());
    let mean = |f: fn(&(u32, u32)) -> u32| changed.iter().map(f).sum::<u32>() as f32 / changed.len() as f32;
    assert!((mean(|p| p.0) - 128.0).abs() < 12.0, "centred across the folder");
    assert!((mean(|p| p.1) - 142.5).abs() < 16.0, "centred on the front, below the tab");
    assert_eq!(compose_rep(folder(16), &mark), folder(16));
}

/// Composes from this Mac's real folder icon; needs a login session.
#[test]
#[ignore = "runs osascript against the window server"]
fn composes_an_icns_from_the_system_folder() {
    let tag = super::compose::current_tag().unwrap();
    let icns = super::compose::folder_icns(&tag).unwrap();
    assert_eq!(&icns[..4], b"icns");
}
