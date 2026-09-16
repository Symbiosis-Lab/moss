//! `moss env` — read or set the per-project hosting environment.
//!
//! Read:  `moss env <folder>`                              → prints environment (or "production (default)")
//! Write: `moss env <staging|production|local> <folder>`  → writes environment to .moss/config.toml
//!
//! Exit codes:
//!   0  success
//!   1  usage / error
//!
//! Both binaries answer both halves: the write goes through
//! `vault::config::save_environment`, the same door the app's modals use
//! (ADR-059 amendment, 2026-09-07).

use crate::build::site_config::get_environment_field;
use crate::config::environment::parse_env_name;
use crate::vault::config::save_environment;
use crate::vault_root::VaultRoot;

/// `args` is everything after `moss env`. Disambiguation:
/// - two args → write (an invalid name is rejected before anything is
///   written, so `moss env bogus <folder>` fails loudly rather than being
///   misread as a read);
/// - one arg that IS an env name → the user meant to write but forgot the folder → error;
/// - one (other) arg → read that folder.
/// Read: `moss env <folder>` ; Write: `moss env <staging|production|local> <folder>`.
pub fn run(args: &[String]) -> i32 {
    match args {
        [] => {
            print_help();
            1
        }
        [a] if a == "--help" || a == "-h" || a == "help" => {
            print_help();
            0
        }
        [name] if parse_env_name(name).is_some() => {
            eprintln!("Usage: moss env {} <folder>   (a folder is required to set the environment)", name);
            1
        }
        [folder] => match get_environment_field(VaultRoot::resolve(folder).as_str()) {
            Ok(Some(env)) => {
                println!("{}", env);
                0
            }
            Ok(None) => {
                println!("production (default)");
                0
            }
            Err(e) => {
                eprintln!("error: {}", e);
                1
            }
        },
        [name, folder] => match parse_env_name(name) {
            None => {
                eprintln!("error: invalid environment '{}': use staging|production|local", name);
                1
            }
            Some(env) => match save_environment(VaultRoot::resolve(folder).as_str(), env) {
                Ok(()) => {
                    println!("Set environment of {} to {}", folder, name);
                    0
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    1
                }
            },
        },
        _ => {
            eprintln!("Usage: moss env <staging|production|local> <folder>   (or)   moss env <folder>");
            1
        }
    }
}

fn print_help() {
    println!(
        "Usage:
  moss env <staging|production|local> <folder>   Set environment
  moss env <folder>                              Show environment"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_with_no_args_prints_help_and_returns_1() {
        assert_eq!(run(&[]), 1);
    }

    #[test]
    fn run_help_returns_0() {
        assert_eq!(run(&["--help".to_string()]), 0);
        assert_eq!(run(&["-h".to_string()]), 0);
        assert_eq!(run(&["help".to_string()]), 0);
    }

    #[test]
    fn run_missing_folder_arg_for_write_returns_1() {
        assert_eq!(run(&["staging".to_string()]), 1);
    }

    #[test]
    fn run_read_nonexistent_folder_returns_0_with_default() {
        // A missing file reads as `Ok(None)`: "production (default)", exit 0.
        assert_eq!(run(&["/tmp/moss-env-cli-test-nonexistent-folder-xyz".to_string()]), 0);
    }

    #[test]
    fn run_invalid_env_name_with_folder_errors_before_writing() {
        // Two args with an invalid env name must fail loudly, not be misread as
        // a read of a folder named "bogus", and nothing may be written.
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().to_str().unwrap().to_string();
        assert_eq!(run(&["bogus".to_string(), folder]), 1);
        assert!(!tmp.path().join(".moss").exists());
    }
}
