use moss_build::ops::serve::ui::sessions::SessionStore;
use std::path::PathBuf;

pub fn session_file() -> Result<PathBuf, String> {
    let home = moss_build::infra::home::moss_home()?;
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
