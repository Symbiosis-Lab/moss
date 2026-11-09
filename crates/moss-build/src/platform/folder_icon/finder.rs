//! Finder's custom folder icon, written without AppKit.
//!
//! Finder draws a folder with a custom icon when the folder's `FinderInfo` has
//! `kHasCustomIcon` set and an invisible file named `Icon` + CR inside it
//! carries the icon in its resource fork: an `'icns'` resource with ID -16455
//! inside a resource map. The fork must be that map, not raw `.icns` bytes:
//! raw bytes store and read back fine, and Finder silently draws the plain
//! folder, because the Resource Manager reads the first 16 bytes as a header.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub(super) const MARKER: &str = "Icon\r";
/// Which icon moss wrote, so a later recipe or OS replaces moss's own icon
/// while a person's own icon (no tag) stays.
const TAG_NAME: &str = "host.moss.folder-icon";
/// SHA-256 of the icon resource of every design the desktop app wrote, as
/// `NSWorkspace` stored it. The first three predate its tag; the leaf was
/// tagged, but a failed tag write left it untagged. Untagged, only content
/// tells them apart from a person's own icon.
const UNTAGGED_MOSS_ICONS: [&str; 4] = [
    "9b6345a78ee7e3ed1da366f4a3c487973b75beaf674fdadfa8c9ae4b3fdc9843", // splat in the corner
    "c1147dd6c815f7080e4e5fd535b38f65d349bc87eb87eefae52a0beeeb4773f2", // ink drop in the corner
    "0090b514a52d1ec4ad9a07d200ff1136d2efa288e4ff422fd7fad661ca31fa7d", // ink drop, redrawn
    "fab97a2e21121929336709d7fea7d86617d77107a2d0eeadbf22ee7d4ae81cc1", // centred leaf
];
const ICNS_ID: i16 = -16455;
const HAS_CUSTOM_ICON: u8 = 0x04; // high byte of the Finder flags
const IS_INVISIBLE: u8 = 0x40;

#[derive(Debug, PartialEq)]
pub(super) enum Existing {
    /// No icon, or a marker whose fork a copy dropped (git, zip, scp).
    None,
    Current,
    /// An icon moss wrote with an earlier recipe or on an earlier OS.
    Moss,
    /// Someone else's icon: never replaced.
    Foreign,
}

/// Runs on every full build, so the common answer (`Current`) costs a few
/// metadata reads; the fork is read only for an untagged icon.
pub(super) fn existing(dir: &Path, tag: &str) -> Existing {
    let marker = dir.join(MARKER);
    match std::fs::symlink_metadata(&marker) {
        Err(_) => return Existing::None,
        // A link or a directory under that name is not ours to follow or touch.
        Ok(m) if !m.is_file() => return Existing::Foreign,
        Ok(_) => {}
    }
    // No fork, or an empty one, is a marker a copy stripped; anything moss
    // cannot read is treated as someone else's.
    match std::fs::metadata(marker.join("..namedfork/rsrc")) {
        Ok(m) if m.len() > 0 => {}
        Ok(_) => return Existing::None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Existing::None,
        Err(_) => return Existing::Foreign,
    }
    match get_xattr(&marker, TAG_NAME, NOFOLLOW) {
        // The tag is written last, so a tagged icon is complete unless the
        // folder's flag was lost since (a copy, or a failed write): redraw then.
        Some(t) if t == tag.as_bytes() && has_custom_icon_flag(dir) => Existing::Current,
        Some(_) => Existing::Moss,
        None => match first_resource(&marker) {
            Some(body) if UNTAGGED_MOSS_ICONS.contains(&sha256_hex(&body).as_str()) => Existing::Moss,
            _ => Existing::Foreign,
        },
    }
}

/// Give `dir` the icon `icns`, tagged `tag`.
pub(super) fn write(dir: &Path, icns: &[u8], tag: &str) -> Result<(), String> {
    let marker = dir.join(MARKER);
    // `create_new` never follows a link left under the marker's name.
    // allow:raw_write the marker is Finder metadata in the site folder, not build output
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&marker) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("create marker: {e}")),
    }
    // Every File Provider then keeps the marker on this Mac (Dropbox otherwise
    // refuses it with a lasting sync error). Without the key, write nothing.
    crate::moss_paths::exclude_from_cloud_sync(&marker);
    if !crate::moss_paths::has_cloud_sync_marker(&marker) {
        return Err("the marker could not be kept out of cloud sync".into());
    }
    // Untagged until the icon is complete, so a failure part-way is redrawn.
    remove_xattr(&marker, TAG_NAME, NOFOLLOW);
    // A shorter fork written over a longer one would keep the old tail.
    remove_xattr(&marker, "com.apple.ResourceFork", NOFOLLOW);
    set_xattr(&marker, "com.apple.ResourceFork", &resource_fork(icns), NOFOLLOW)?;
    let mut marker_info = [0u8; 32];
    marker_info[..8].copy_from_slice(b"iconMACS");
    marker_info[8] = IS_INVISIBLE;
    set_xattr(&marker, "com.apple.FinderInfo", &marker_info, NOFOLLOW)?;

    let mut info = get_xattr(dir, "com.apple.FinderInfo", 0).filter(|i| i.len() == 32).unwrap_or_else(|| vec![0; 32]);
    info[8] |= HAS_CUSTOM_ICON;
    set_xattr(dir, "com.apple.FinderInfo", &info, 0)?;
    set_xattr(&marker, TAG_NAME, tag.as_bytes(), NOFOLLOW)?;
    // Finder and the icon cache redraw a folder whose modification time moved;
    // replacing an existing marker's fork does not move it.
    let path = cstr(dir)?;
    // SAFETY: `path` is a NUL-terminated string that outlives the call; a null
    // `times` means "now".
    unsafe { libc::utimes(path.as_ptr(), std::ptr::null()) };
    Ok(())
}

/// Keep the marker out of `git status` in a repository rooted at `dir`. The
/// exclude file is local to this clone; nothing tracked changes. git strips
/// one trailing CR from a pattern, so the line carries two.
pub(super) fn exclude_from_git(dir: &Path) {
    // In a synced folder `.git` syncs too, so the edit would upload; leave it.
    let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if crate::build::cloud_provider::detect_from_path(&real).is_some() {
        return;
    }
    let Some(common) = git_common_dir(dir) else { return };
    let exclude = common.join("info").join("exclude");
    let existing = std::fs::read(&exclude).unwrap_or_default();
    if existing.split(|b| *b == b'\n').any(|line| line == b"Icon\r\r") {
        return;
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(b"Icon\r\r\n");
    let _ = std::fs::create_dir_all(common.join("info"));
    // allow:raw_write a git exclude file, not build output
    if let Err(e) = std::fs::write(&exclude, out) {
        log::debug!(target: "build", "folder icon: git exclude not written: {e}");
    }
}

/// The directory holding `info/exclude` for a repository rooted at `dir`: its
/// `.git` directory, or for a linked worktree (`.git` is a `gitdir:` file) the
/// main repository's, which every worktree reads.
fn git_common_dir(dir: &Path) -> Option<std::path::PathBuf> {
    let git = dir.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    let pointer = std::fs::read_to_string(&git).ok()?;
    let gitdir = dir.join(pointer.strip_prefix("gitdir:")?.trim());
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(common) => Some(gitdir.join(common.trim())),
        Err(_) => Some(gitdir),
    }
}

fn has_custom_icon_flag(dir: &Path) -> bool {
    get_xattr(dir, "com.apple.FinderInfo", 0).is_some_and(|i| i.len() == 32 && i[8] & HAS_CUSTOM_ICON != 0)
}

/// A resource fork holding one `'icns'` resource: a 16-byte header, 240
/// reserved bytes, the data (length-prefixed), then the map.
pub(super) fn resource_fork(icns: &[u8]) -> Vec<u8> {
    const DATA_OFF: u32 = 256;
    const MAP_LEN: u32 = 50; // header copy 16, handle 4, file ref 2, attrs 2, two offsets 4, type list 10, ref list 12
    let data_len = 4 + icns.len() as u32;
    let map_off = DATA_OFF + data_len;
    let header = [DATA_OFF, map_off, data_len, MAP_LEN].map(u32::to_be_bytes).concat();

    let mut fork = header.clone();
    fork.resize(DATA_OFF as usize, 0);
    fork.extend_from_slice(&(icns.len() as u32).to_be_bytes());
    fork.extend_from_slice(icns);

    fork.extend_from_slice(&header);
    fork.extend_from_slice(&[0; 8]); // next-map handle, file ref, attributes
    fork.extend_from_slice(&28u16.to_be_bytes()); // type list, from the map start
    fork.extend_from_slice(&(MAP_LEN as u16).to_be_bytes()); // name list: empty, at the end
    fork.extend_from_slice(&0u16.to_be_bytes()); // one type, stored minus one
    fork.extend_from_slice(b"icns");
    fork.extend_from_slice(&0u16.to_be_bytes()); // one resource, stored minus one
    fork.extend_from_slice(&10u16.to_be_bytes()); // its reference list, from the type list
    fork.extend_from_slice(&ICNS_ID.to_be_bytes());
    fork.extend_from_slice(&u16::MAX.to_be_bytes()); // no name
    fork.extend_from_slice(&[0, 0, 0, 0]); // attributes, then a 24-bit data offset of 0
    fork.extend_from_slice(&[0; 4]); // reserved handle
    fork
}

/// The bytes of the fork's first resource; `None` when the marker has no fork.
pub(super) fn first_resource(marker: &Path) -> Option<Vec<u8>> {
    let fork = std::fs::read(marker.join("..namedfork/rsrc")).ok()?;
    let be32 = |o: usize| Some(u32::from_be_bytes(fork.get(o..o + 4)?.try_into().ok()?) as usize);
    let data_off = be32(0)?;
    let len = be32(data_off)?;
    let start = data_off.checked_add(4)?;
    fork.get(start..start.checked_add(len)?).map(<[u8]>::to_vec)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn cstr(path: &Path) -> Result<CString, String> {
    CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())
}

/// For the marker: never act through a link under its name.
const NOFOLLOW: libc::c_int = libc::XATTR_NOFOLLOW;

pub(super) fn get_xattr(path: &Path, name: &str, opts: libc::c_int) -> Option<Vec<u8>> {
    let (path, name) = (cstr(path).ok()?, CString::new(name).ok()?);
    // SAFETY: NUL-terminated strings that outlive the calls; the first call
    // only measures, the second writes at most `buf.len()` bytes into `buf`.
    unsafe {
        let len = libc::getxattr(path.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0, 0, opts);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let n = libc::getxattr(path.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), 0, opts);
        (n > 0).then(|| {
            buf.truncate(n as usize);
            buf
        })
    }
}

fn set_xattr(path: &Path, name: &str, value: &[u8], opts: libc::c_int) -> Result<(), String> {
    let (p, n) = (cstr(path)?, CString::new(name).map_err(|e| e.to_string())?);
    // SAFETY: NUL-terminated strings and a live slice, all outliving the call.
    let rc = unsafe { libc::setxattr(p.as_ptr(), n.as_ptr(), value.as_ptr().cast(), value.len(), 0, opts) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("setxattr {name} on {}: {}", path.display(), std::io::Error::last_os_error()))
    }
}

fn remove_xattr(path: &Path, name: &str, opts: libc::c_int) {
    if let (Ok(p), Ok(n)) = (cstr(path), CString::new(name)) {
        // SAFETY: NUL-terminated strings that outlive the call.
        unsafe { libc::removexattr(p.as_ptr(), n.as_ptr(), opts) };
    }
}
