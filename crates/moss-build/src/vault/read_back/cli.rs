//! `moss data pull`: bring a hosted site's records home to its folder.

use std::path::Path;

use super::{pull, Part, Sources};

fn usage() -> String {
    let parts: Vec<&str> = Part::ALL.iter().map(|p| p.name()).collect();
    format!(
        "usage: moss data pull [<folder>] [{}]...\n\n\
         Pulls the hosted site's records into <folder>/.moss/data/, every part unless some are named,\n\
         and prints the records each part added as JSON. A part that fails is named with its error.",
        parts.join("|")
    )
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("pull") => {}
        Some("-h" | "--help") => {
            println!("{}", usage());
            return 0;
        }
        _ => {
            eprintln!("{}", usage());
            return 2;
        }
    }
    let mut folder: Option<&str> = None;
    let mut parts = Vec::new();
    for arg in &args[1..] {
        match (Part::parse(arg), folder) {
            (Some(part), _) => parts.push(part),
            (None, None) if !arg.starts_with('-') => folder = Some(arg),
            _ => {
                eprintln!("error: unexpected '{arg}'\n{}", usage());
                return 2;
            }
        }
    }
    if parts.is_empty() {
        parts = Part::ALL.to_vec();
    }
    let folder = Path::new(folder.unwrap_or("."));
    let ready = crate::cli::client_for_folder(folder)
        .and_then(|client| crate::build::site_config::site_id_for_folder(folder).map(|id| (client, id)));
    let (client, site_id) = match ready {
        Ok(ready) => ready,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let env = crate::build::site_config::resolve_environment(&folder.to_string_lossy());
    crate::cli::run_async(async move {
        let sources = Sources::new(&client, &env, &site_id);
        let mut report = serde_json::Map::new();
        let mut failed = false;
        for (part, pulled) in pull(&sources, folder, &parts).await {
            let value = match pulled {
                Ok(n) => serde_json::json!(n),
                Err(e) => {
                    failed = true;
                    serde_json::json!({ "error": e.to_string() })
                }
            };
            report.insert(part.name().to_string(), value);
        }
        println!("{}", serde_json::Value::Object(report));
        i32::from(failed)
    })
}
