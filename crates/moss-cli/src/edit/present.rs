pub fn present(
    terminal: bool,
    no_browser: bool,
    local: &str,
    public: &str,
    open: &dyn Fn(&str) -> Result<(), String>,
    output: &mut dyn std::io::Write,
) -> Result<(), String> {
    if terminal && !no_browser {
        open(local)?;
    } else {
        writeln!(output, "Edit at {public}\n{}", moss_build::build::media::qr::terminal_qr(public)?).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if let Some(file) = std::env::var_os("MOSS_EDIT_EFFECTS_FILE") {
        return std::fs::write(file, format!("browser {url}")).map_err(|e| e.to_string());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    let status = command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| "could not open browser; use --no-browser to print a sign-in link")?;
    if status.success() {
        Ok(())
    } else {
        Err("could not open browser; use --no-browser to print a sign-in link".into())
    }
}
