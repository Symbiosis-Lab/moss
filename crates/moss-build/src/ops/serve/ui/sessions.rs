use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created: u64,
    pub last_seen: u64,
    pub user_agent: String,
}

#[derive(Deserialize, Serialize)]
struct StoredSession {
    #[serde(flatten)]
    info: SessionInfo,
    hash: Vec<u8>,
}

pub const SESSION_SECONDS: u64 = 30 * 24 * 3600;
use super::secrets;

pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err("session file needs an absolute file path".into());
        }
        Ok(Self { path })
    }

    pub fn validate_location(&self, served: &Path) -> Result<(), String> {
        if resolved(&self.path)?.starts_with(resolved(served)?) {
            return Err("session file must be outside the served folder".into());
        }
        Ok(())
    }

    /// Read on every admission: atomic replacement, deletion, and even a write
    /// preserving mtime must take effect before another request is admitted.
    fn read(&self) -> Result<Vec<StoredSession>, String> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn list(&self) -> Result<Vec<SessionInfo>, String> {
        Ok(self.read()?.into_iter().map(|s| s.info).collect())
    }

    pub fn open(&self, user_agent: &str) -> Result<String, String> {
        let secret = secrets::random()?;
        let record = StoredSession {
            info: SessionInfo {
                id: secrets::random()?,
                created: secrets::now(),
                last_seen: secrets::now(),
                user_agent: user_agent.chars().take(512).collect(),
            },
            hash: secrets::hash(&secret),
        };
        self.update(|sessions| sessions.push(record))?;
        Ok(secret)
    }

    pub fn revoke(&self, id: &str) -> Result<(), String> {
        self.update(|sessions| sessions.retain(|s| s.info.id != id))
    }

    pub fn revoke_all(&self) -> Result<(), String> {
        self.update(|sessions| sessions.clear())
    }

    pub fn remove_secret(&self, secret: &str) -> Result<(), String> {
        self.update(|sessions| sessions.retain(|s| s.hash != secrets::hash(secret)))
    }

    pub fn admits(&self, secret: &str, refresh: bool) -> Result<bool, String> {
        let hash = secrets::hash(secret);
        let now = secrets::now();
        let sessions = self.read()?;
        let Some(session) = sessions.iter().find(|s| s.hash == hash
            && now.saturating_sub(s.info.last_seen) < SESSION_SECONDS) else {
            return Ok(false);
        };
        if !refresh || now.saturating_sub(session.info.last_seen) <= 300 {
            return Ok(true);
        }
        let mut present = false;
        self.update(|sessions| {
            if let Some(session) = sessions
                .iter_mut()
                .find(|s| s.hash == hash)
            {
                session.info.last_seen = secrets::now();
                present = true;
            }
        })?;
        Ok(present)
    }

    fn update(&self, change: impl FnOnce(&mut Vec<StoredSession>)) -> Result<(), String> {
        let parent = self.path.parent().ok_or("session file has no parent")?;
        // allow:raw_write the session file is machine state outside the served folder, not build output
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // allow:raw_write the session file's lock sidecar, beside it
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // A separate inode keeps writers serialized across atomic replacements.
        let lock = options
            .open(self.path.with_extension("json.lock"))
            .map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let mut sessions = self.read()?;
        sessions.retain(|s| secrets::now().saturating_sub(s.info.last_seen) < SESSION_SECONDS);
        change(&mut sessions);
        let contents = serde_json::to_string_pretty(&sessions).map_err(|e| e.to_string())?;
        crate::infra::atomic_write::write_private_atomic(&self.path, &contents)
    }
}

fn resolved(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("path has no existing ancestor")?;
    let name = path.file_name().ok_or("path has no file name")?;
    Ok(resolved(parent)?.join(name))
}

pub(crate) fn cookie(headers: &axum::http::HeaderMap) -> Option<&str> {
    let mut found = None;
    for header in headers.get_all("cookie") {
        for pair in header.to_str().ok()?.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name == "moss_ui" {
                if found.is_some() {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}

pub(super) fn set_cookie(secret: &str, origin: &str, max_age: u64) -> String {
    format!(
        "moss_ui={secret}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{}",
        if origin.starts_with("https://") {
            "; Secure"
        } else {
            ""
        }
    )
}

// `moss sessions`: list and revoke the browser sessions `moss edit --serve`
// signed in, from either binary.

/// The sessions file every `moss edit --serve` on this machine shares, under a
/// moss home only its owner can read.
pub fn session_file() -> Result<PathBuf, String> {
    let home = crate::infra::home::moss_home()?;
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    Ok(home
        .canonicalize()
        .map_err(|e| e.to_string())?
        .join("ui-sessions.json"))
}

pub fn run(args: &[String]) -> i32 {
    match execute(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{}", error.replace(['\r', '\n'], " "));
            1
        }
    }
}

fn execute(args: &[String]) -> Result<(), String> {
    let store = SessionStore::new(session_file()?)?;
    match args {
        [] => {
            println!("ID\tCREATED\tLAST SEEN\tUSER AGENT");
            for session in store.list()? {
                let agent: String = session
                    .user_agent
                    .chars()
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                println!(
                    "{}\t{}\t{}\t{}",
                    session.id, session.created, session.last_seen, agent
                );
            }
            Ok(())
        }
        [verb, id] if verb == "revoke" && id == "--all" => store.revoke_all(),
        [verb, id] if verb == "revoke" && !id.starts_with('-') => store.revoke(id),
        _ => Err("Usage: moss sessions [revoke <id> | revoke --all]".into()),
    }
}
