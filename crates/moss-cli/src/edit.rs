mod args;
mod present;
#[cfg(test)]
mod tests;

use args::EditArgs;
use moss_build::ops::serve::ui::{auth::UiAuth, sign_in::SignInLink};
use moss_build::ops::serve::{HeadlessServe, UiOriginConfig, UiPort};
use moss_build::ops::{run_headless_build, BuildFlags, HeadlessBuildRun};
use std::{io::IsTerminal, path::Path, sync::Arc};

pub fn run(args: &[String]) -> i32 {
    match execute(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.replace(['\r', '\n'], " "));
            1
        }
    }
}

fn execute(args: &[String]) -> Result<i32, String> {
    let parsed = EditArgs::parse(args)?;
    let terminal = std::io::stdin().is_terminal();
    if terminal && !parsed.serve && crate::desktop::locate().is_some() {
        return Ok(crate::desktop::hand_off("edit", &parsed.folder));
    }
    let folder = Path::new(&parsed.folder)
        .canonicalize()
        .map_err(|_| "folder does not exist")?;
    if !folder.is_dir() {
        return Err("serving the editor requires a folder".into());
    }
    if let Some(link) = moss_build::ops::serve::ui::client::link_for_owner(&folder)? {
        present::present(
            terminal,
            parsed.no_browser,
            &link.local,
            &link.public,
            &present::open_browser,
            &mut std::io::stdout(),
        )?;
        return Ok(0);
    }
    let assets = crate::bundle::require_bundle()?;
    let auth = Arc::new(SignInLink::new(moss_build::ops::serve::ui::sessions::session_file()?, &folder)?);
    // `--port` fixes the UI and puts the preview on the next port, exactly
    // there behind a public URL; without it both scan, the preview from
    // `MOSS_PREVIEW_PORT_BASE`.
    let (ui_port, port_base) = match parsed.port {
        Some(port) => (UiPort::Fixed(port), port.checked_add(1)),
        None => (UiPort::ScanFrom(8081), None),
    };
    let ui = UiOriginConfig {
        port: ui_port,
        assets,
        auth: auth.clone(),
        public_url: parsed.public_url.clone(),
        site_root: folder.clone(),
    };
    let serve = HeadlessServe {
        ui: Some(ui),
        port_base,
        on_ready: Some(Arc::new(move |preview, port| {
            let port = port.ok_or("UI listener did not start")?;
            if let Some(public) = &parsed.public_url {
                println!("Proxy {} to http://127.0.0.1:{port}", public.ui.as_str());
                println!("Proxy {} to http://127.0.0.1:{preview}", public.preview.as_str());
            }
            let sign_in = auth.mint_sign_in()?;
            let local = format!("http://127.0.0.1:{port}{}", sign_in.url_path());
            let public = parsed.public_url.as_ref().map_or_else(
                || local.clone(),
                |public| format!("{}{}", public.ui.as_str(), sign_in.url_path()),
            );
            present::present(
                terminal,
                parsed.no_browser,
                &local,
                &public,
                &present::open_browser,
                &mut std::io::stdout(),
            )
        })),
        ..Default::default()
    };
    run_headless_build(HeadlessBuildRun {
        folder_path: folder.to_string_lossy().into_owned(),
        flags: BuildFlags {
            serve: true,
            watch: true,
            ..Default::default()
        },
        host_ports: Arc::new(moss_build::cli::host::cli_host_ports),
        serve,
        start_watch: Box::new(|folder, plugins| {
            Box::pin(async move {
                moss_build::ops::watch::headless::start(
                    moss_build::ops::watch::headless::HeadlessWatchConfig {
                        folder_path: folder,
                        host_ports: Arc::new(moss_build::cli::host::cli_host_ports),
                        plugins,
                    },
                )
                .await
            })
        }),
    });
}
