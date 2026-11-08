use moss_build::ops::serve::ui::PublicOrigin;

#[derive(Debug, Default, PartialEq)]
pub struct EditArgs {
    pub folder: String,
    pub serve: bool,
    pub no_browser: bool,
    pub port: Option<u16>,
    pub public_url: Option<PublicOrigin>,
}

impl EditArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut folder = None;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--serve" => parsed.serve = true,
                "--no-browser" => parsed.no_browser = true,
                "--" => {
                    for arg in args.by_ref() {
                        if folder.replace(arg.clone()).is_some() {
                            return Err("expected one folder".into());
                        }
                    }
                }
                arg if arg.starts_with('-') => {
                    let (flag, inline) = arg
                        .split_once('=')
                        .map_or((arg, None), |(k, v)| (k, Some(v)));
                    if !matches!(flag, "--port" | "--public-url") {
                        return Err(format!("unknown edit option: {flag}"));
                    }
                    let value = inline
                        .or_else(|| args.next().map(String::as_str))
                        .filter(|s| !s.is_empty() && !s.starts_with('-'))
                        .ok_or_else(|| format!("{flag} needs a value"))?;
                    match flag {
                        "--port" => {
                            parsed.port = Some(
                                value
                                    .parse::<u16>()
                                    .ok()
                                    .filter(|p| *p > 0)
                                    .ok_or("--port must be between 1 and 65535")?,
                            )
                        }
                        _ => parsed.public_url = Some(PublicOrigin::parse(value)?),
                    }
                }
                _ => {
                    if folder.replace(arg.clone()).is_some() {
                        return Err("expected one folder".into());
                    }
                }
            }
        }
        parsed.folder = folder.unwrap_or_else(|| ".".into());
        Ok(parsed)
    }
}
