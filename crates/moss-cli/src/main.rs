//! The open moss CLI: `moss build` with no window system in
//! the dependency graph. `preview` and `edit` hand off to moss desktop rather
//! than running a window here (`desktop.rs`).
//!
//! `build` drives the same headless driver the app binary's CLI interception
//! does (`moss_build::ops::run_headless_build` — the server relocation
//! folded the two mirrored bodies into it), so a `moss-cli build`
//! and an app-binary `moss build` are byte-identical (the build-parity
//! harness runs this binary as one leg). `--serve`/`--watch` run the crossed
//! preview server and watch driver (`moss_build::ops::{serve,watch}`), and
//! plugin hooks run through the crossed manager: plugins follow
//! `--no-plugins`/`--allow-plugins`/`--wait-plugins` exactly as they do in
//! the app binary, because the one headless driver decides.

use std::sync::Arc;

use moss_build::ops::{run_headless_build, BuildArgs, HeadlessBuildRun};

mod desktop;

/// The verbs that crossed to moss-build, dispatched and advertised from this
/// one table so the usage text and the match arms cannot disagree — two
/// hand-kept lists of the same fact had to move in step once already. `build`
/// is its own arm below; `preview` and `edit` hand off to moss desktop
/// (`desktop.rs`) instead of needing it directly; every other name in the
/// shared command table needs the moss app.
const CROSSED: &[(&str, fn(&[String]) -> i32)] = &[
    ("guide", moss_build::cli::guide::run),
    ("agents", moss_build::cli::agents::run),
    ("list", moss_build::cli::list::run),
    ("doctor", moss_build::cli::doctor::run),
    ("describe", |args| moss_build::cli::describe::run(args, env!("CARGO_PKG_VERSION"))),
    ("env", moss_build::cli::env::run),
    ("domain", moss_build::cli::domain::run),
    ("import", moss_build::cli::import::run),
    ("deploy", moss_build::cli::deploy::run),
    // `mv` is the app binary's alias for `rename` (`startup/run_mode.rs`).
    ("rename", moss_build::cli::rename::run),
    ("mv", moss_build::cli::rename::run),
    // Lives under `deploy/history/`, not `cli/` (file budget — see the
    // publish-history design's "Placement and budgets").
    ("history", moss_build::deploy::history::cli::run),
];

fn answers(name: &str) -> bool {
    name == "build" || name == "desktop" || CROSSED.iter().any(|(crossed, _)| *crossed == name)
}

/// `moss --help`, from the shared table — the same text the app binary prints,
/// minus the two commands that need a window. The parity test compares them.
fn usage() -> String {
    moss_build::cli::commands::top_level_help(env!("CARGO_PKG_VERSION"), &answers)
}

fn main() {
    // The app binary does this before dispatch too, so a crossed verb reads the
    // same `MOSS_ENV` / `MOSS_SETA_URL` in both binaries.
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    // `--help` only when it is the whole request, so `<cmd> --help` falls
    // through to the per-command table below instead of printing this one.
    if args.len() <= 2 && args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", usage());
        std::process::exit(0);
    }
    // The Obsidian plugin shells out to `moss --version` to decide whether the
    // binary it found is usable (packages/obsidian-moss/src/cli.ts), so a
    // binary without it reads as broken rather than as older. `env!` expands in
    // THIS crate deliberately — the number has to describe the binary that
    // printed it, which is the bug `moss-core/src/contract/describe.rs`
    // documents from the other direction.
    //
    // The name is the product's, not the crate's: this binary ships as `moss`
    // (the npm wrapper and the Obsidian plugin both install it under that
    // name), and a script matching `^moss ` must not care which of the two
    // binaries answered. The parity test compares the whole line, so the two
    // versions cannot drift either — the release version bump has to reach
    // `crates/moss-cli/Cargo.toml`.
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("moss {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    // The same number is what a plugin's `min_moss_version` is a floor on and
    // what every registry request carries as `User-Agent`; moss-build panics
    // on an unseeded read rather than guess.
    moss_build::system::seed_app_version(env!("CARGO_PKG_VERSION"));
    // `moss-cli <subcommand> --help` from the same table the app binary reads,
    // so the two cannot describe a command differently. Before this, `guide
    // --help` reached `guide`, which rejects unknown flags — the app exited 0
    // with usage, this binary exited 1 with an error. Caught by
    // `cli_binary_parity_test.rs` on its first run.
    if args.len() > 2 && args[2..].iter().any(|a| a == "--help" || a == "-h") {
        if let Some(text) = moss_build::cli::commands::help_for(&args[1]) {
            println!("{text}");
            std::process::exit(0);
        }
    }
    // What moss-build already owns, dispatched exactly as the app binary
    // dispatches it (`startup/run_mode.rs`); the parity test compares the two
    // binaries' answers byte for byte.
    if let Some((_, run)) = args.get(1).and_then(|verb| CROSSED.iter().find(|(name, _)| name == verb)) {
        std::process::exit(run(&args[2..]));
    }
    match args.get(1).map(String::as_str) {
        // A window is the whole point of these two; hand off to moss desktop
        // when it is installed, or say how to get it when it is not.
        Some(verb @ ("preview" | "edit")) => {
            let Some(path) = args.get(2) else {
                eprintln!("Usage: moss {verb} <path>");
                eprintln!("  <path> can be a folder or a .md/.markdown file");
                std::process::exit(1);
            };
            let code = desktop::hand_off(verb, path);
            if code != 0 && verb == "preview" {
                eprintln!("For a served build, `moss build <folder_path> --serve`.");
            }
            std::process::exit(code);
        }
        Some("build") => {
            // Folder, flags and every refusal from the one parser the app
            // binary also reads: a usage error either binary spelled itself is
            // a divergence `build_argv_handling_is_identical` reports.
            let parsed = match BuildArgs::parse(&args[2..]) {
                Ok(parsed) => parsed,
                Err(message) => {
                    eprintln!("{}", message);
                    std::process::exit(1);
                }
            };
            run_headless_build(HeadlessBuildRun {
                folder_path: parsed.folder.clone(),
                flags: parsed.flags,
                host_ports: Arc::new(|folder| moss_build::cli::host::cli_host_ports(folder)),
                start_watch: Box::new(|folder, plugins| {
                    Box::pin(async move {
                        // No sweep here: the periodic disk-vs-baseline
                        // backbone is app-side today; moss-cli's `--watch`
                        // is watcher-only (see `ops/watch/headless.rs`).
                        moss_build::ops::watch::headless::start(
                            moss_build::ops::watch::headless::HeadlessWatchConfig {
                                folder_path: folder,
                                host_ports: Arc::new(|f| moss_build::cli::host::cli_host_ports(f)),
                                plugins,
                            },
                        )
                        .await
                    })
                }),
            });
        }
        Some("desktop") => {
            std::process::exit(desktop::run(&args[2..]));
        }
        Some("compile") => {
            eprintln!(
                "{}",
                moss_build::cli::commands::renamed_compile_hint(args.get(2).map(String::as_str))
            );
            std::process::exit(2);
        }
        // `moss <path>` opens a preview in the app — it is what every file
        // association funnels into (`startup/run_mode.rs::bare_path_mode`). This
        // binary has no window, so it says so rather than printing the whole
        // help screen at someone whose path was fine. The way out is offered
        // only for a folder: `build` takes one, so suggesting it for a file
        // would send the reader to a second error.
        Some(path) if std::path::Path::new(path).exists() => {
            let code = desktop::hand_off("preview", path);
            if code != 0 && std::path::Path::new(path).is_dir() {
                eprintln!("For a served build, `moss build {path} --serve`.");
            }
            std::process::exit(code);
        }
        _ => {
            eprintln!("{}", usage());
            std::process::exit(1);
        }
    }
}
