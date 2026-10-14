//! `moss tools download <ffmpeg|git>` — fetch a command-line tool moss needs.
//!
//! Running this command is the user's consent to the download; nothing else
//! downloads these tools on the CLI.
//!
//! Exit codes:
//!   0  the tool is available
//!   1  the download or verification failed
//!   2  usage error

use crate::build::assets::binary_resolver::{download_tool, Tool};

/// `args` is everything after `moss tools`.
pub fn run(args: &[String]) -> i32 {
    match args {
        [sub, name] if sub == "download" => {
            let Some(tool) = Tool::from_name(name) else {
                eprintln!("error: unknown tool '{name}': use ffmpeg|git");
                return 2;
            };
            match download_tool(tool, None) {
                Ok(()) => {
                    println!("{} is ready", tool.name());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!("Usage: moss tools download <ffmpeg|git>");
            2
        }
    }
}
