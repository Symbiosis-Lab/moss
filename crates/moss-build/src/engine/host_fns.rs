//! __TAURI__.core.invoke dispatch. The async host_invoke
//! serializes the args object to JSON (json_stringify), hands it to the host over
//! a Send channel (DispatchBridge), and resolves/rejects the JS Promise.
//!
//! LOAD-BEARING LOCK DISCIPLINE: `Func::from(Async(closure))` runs the closure
//! SYNCHRONOUSLY (lock held) to PRODUCE the future, then spawns it. So all JS-handle
//! access (`js_to_json`/`json_stringify`) MUST happen in the synchronous prelude,
//! and the spawned `async move` body must hold ONLY Send owned data (String) + the
//! bridge channel + host I/O — never a JS handle. `invoke`'s future RETURNS a
//! `Value<'js>` (json_parse), which a closure cannot generalize over the invariant
//! `'js` (the `for<'js>` HRTB fails — same constraint the fetch `json()` hit), so the
//! closure forwards to the FREE async fn [`invoke_call`] (explicit `<'js>` param).

use crate::plugins::hook_state::AskedBy;
use rquickjs::{
    function::{Async, Opt},
    prelude::Func,
    Ctx, Exception, Object, Result, Value,
};
use tokio::sync::oneshot;

#[path = "host_fns/grants.rs"]
mod grants;
pub use grants::{Declared, GrantRegistry};
use grants::require_binary_grant;
#[path = "host_fns/secrets.rs"]
mod secrets;

use super::app_host::AppHost;
use super::marshal;

/// A request from the engine thread to the Tauri host: run `cmd(args_json)` and
/// reply with Ok(json) | Err(message). Send across threads (engine -> host).
pub struct DispatchRequest {
    pub cmd: String,
    pub args_json: String,
    /// Plugin whose Context issued the call; `None` denies gated capabilities.
    pub plugin: Option<String>,
    /// Who asked for the runs that Context serves.
    pub asked_by: Option<AskedBy>,
    pub reply: oneshot::Sender<std::result::Result<String, String>>,
}

/// Cloneable bridge handle stored per-Context. Sends DispatchRequest to the host task.
#[derive(Clone)]
pub struct DispatchBridge {
    sender: tokio::sync::mpsc::UnboundedSender<DispatchRequest>,
    /// Set when the bridge is installed into a specific plugin's Context.
    /// Without it, gated capabilities deny (fail-closed).
    plugin: Option<String>,
    asked_by: Option<AskedBy>,
}

impl DispatchBridge {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<DispatchRequest>) -> Self {
        Self { sender, plugin: None, asked_by: None }
    }

    /// A view of this bridge bound to the plugin owning the Context it is
    /// installed into, and to who asked for the runs that Context serves. Every
    /// call the Context makes carries both, a timer or promise chain a run left
    /// behind included, so neither can be spoofed from JS.
    pub fn for_plugin(&self, plugin: &str, asked_by: AskedBy) -> Self {
        Self { sender: self.sender.clone(), plugin: Some(plugin.to_string()), asked_by: Some(asked_by) }
    }
    /// `pub(crate)`: tauri_bridge.rs's emit/listen async bodies call this
    /// cross-module (Task 11a — plugin-message routing + __engine_listen__).
    pub(crate) async fn call(&self, cmd: String, args_json: String) -> std::result::Result<String, String> {
        let (reply, rx) = oneshot::channel();
        self.sender
            .send(DispatchRequest {
                cmd,
                args_json,
                plugin: self.plugin.clone(),
                asked_by: self.asked_by,
                reply,
            })
            .map_err(|_| "dispatch bridge closed".to_string())?;
        let reply = rx.await.map_err(|_| "host dropped dispatch reply".to_string())?;
        super::budgets::paused();
        reply
    }
}

/// Run the dispatch over the bridge and marshal the reply back into JS. Free async
/// fn with an explicit `<'js>` param so the `Value<'js>` return generalizes over the
/// JS lifetime (a closure cannot — `Value` is invariant in `'js`). `args_json` is an
/// owned `String` (Send), so nothing JS is held across the `bridge.call(...).await`.
async fn invoke_call<'js>(
    ctx: Ctx<'js>,
    bridge: DispatchBridge,
    cmd: String,
    args_json: String,
) -> Result<Value<'js>> {
    match bridge.call(cmd, args_json).await {
        Ok(result_json) => ctx.json_parse(result_json),
        Err(msg) => Err(Exception::throw_message(&ctx, &msg)),
    }
}

/// Install __TAURI__.core.invoke backed by `bridge`.
///
/// invoke(cmd, args?) -> Promise<any>. The SYNCHRONOUS PRELUDE (lock held) reads the
/// `args` object and serializes it to a JSON String via `js_to_json`; the spawned
/// async body holds only that String + the bridge and forwards to [`invoke_call`].
pub fn install_invoke<'js>(
    ctx: &Ctx<'js>,
    bridge: DispatchBridge,
    plugin: &str,
    asked_by: AskedBy,
    ended: std::rc::Rc<std::cell::Cell<bool>>,
) -> Result<()> {
    let core = Object::new(ctx.clone())?;
    // Bind the bridge to THIS Context's plugin so the host can gate privileged
    // commands per-plugin. A Context is per-plugin, so this binding is
    // the authoritative caller identity — it cannot be spoofed from JS.
    let b = bridge.for_plugin(plugin, asked_by);
    // The `ctx` and `args` params share a single `'js` so `js_to_json(&ctx, obj)` and
    // the `invoke_call(ctx, …) -> Value<'js>` return type-check — distinct `'_`s make
    // `Value<'js>` invariance reject it (the same trap the fetch `json()` shim hit).
    core.set(
        "invoke",
        Func::from(Async(move |ctx: Ctx<'js>, cmd: String, args: Opt<Object<'js>>| {
            let bridge = b.clone();
            // SYNCHRONOUS PRELUDE (lock held): serialize the args object to JSON HERE.
            // Bind to a Result so the `?`/early-return stays inside the prelude — the
            // async body must never touch a JS handle.
            let args_json: Result<String> = match args.0 {
                Some(obj) => marshal::js_to_json(&ctx, obj.into_value()).map(|j| j.to_string()),
                None => Ok("null".to_string()),
            };
            let cmd_owned = cmd;
            let ended = ended.get();
            async move {
                if ended {
                    return Err(super::web_shims::run_ended(&ctx));
                }
                let args_json = args_json?;
                invoke_call(ctx, bridge, cmd_owned, args_json).await
            }
        })),
    )?;
    super::tauri_bridge::set_tauri_member(ctx, "core", core)?;
    Ok(())
}

/// Parsed args from the `execute_binary` engine arm.
/// Wire keys mirror binary.ts EXACTLY: `binaryPath`, `args`, `workingDir`,
/// `timeoutMs`, `env`, `stdinData`, `streamId`.
#[derive(Debug, PartialEq)]
pub(crate) struct ExecuteBinaryArgs {
    pub binary_path: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub env_vars: Option<std::collections::HashMap<String, String>>,
    pub timeout_ms: Option<u64>,
    pub stdin_data: Option<String>,
}

/// Parse `execute_binary` args from the engine JSON wire.
/// `streamId` is parsed-and-DROPPED with a warn: the stderr-streaming path is
/// dead in production anyway (`binary-output` vs `binary-stderr` name+key
/// mismatch — github-matters probe R5) and engine-side event streaming is not
/// wired; the blocking result path is what github's deploy actually consumes.
pub(crate) fn parse_execute_binary_args(
    v: &serde_json::Value,
) -> std::result::Result<ExecuteBinaryArgs, String> {
    if v.get("streamId").and_then(|s| s.as_str()).is_some() {
        log::warn!(
            target: "plugin",
            "execute_binary streamId ignored on the engine path (streaming is webview-only and currently dead)"
        );
    }
    Ok(ExecuteBinaryArgs {
        binary_path: v
            .get("binaryPath")
            .and_then(|x| x.as_str())
            .ok_or("missing required string arg 'binaryPath'")?
            .to_string(),
        args: serde_json::from_value(
            v.get("args").cloned().unwrap_or(serde_json::json!([])),
        )
        .map_err(|e| format!("bad args array: {e}"))?,
        working_dir: v.get("workingDir").and_then(|x| x.as_str()).map(String::from),
        env_vars: match v.get("env") {
            // VERIFIED against binary.ts: env key (not envVars)
            None | Some(serde_json::Value::Null) => None,
            Some(x) => serde_json::from_value(x.clone())
                .map(Some)
                .map_err(|e| format!("bad env: {e}"))?,
        },
        timeout_ms: v.get("timeoutMs").and_then(|x| x.as_u64()),
        stdin_data: v.get("stdinData").and_then(|x| x.as_str()).map(String::from),
    })
}

/// State a build-capable host fn needs, INJECTED by whoever constructed the
/// engine rather than reached through Tauri's managed-state container.
/// That is the whole difference between a host fn that can
/// run in a headless `moss build` and one that cannot.
///
/// The app builds this from its managed singletons, so both routes mutate the
/// same registries. A headless build calls [`HostState::standalone`], whose
/// registry has no emitter — the tasks are tracked, nothing paints them,
/// which is the honest answer when there is no window.
#[derive(Clone)]
pub struct HostState {
    pub tasks: std::sync::Arc<crate::tasks::TaskRegistry>,
    pub task_handles: std::sync::Arc<crate::plugins::runtime::portable::PluginTaskHandleStore>,
    /// Hook-execution state: what the inactivity watchdog reads, and what the
    /// lifecycle arm and the host-call guard stamp into. One per process — the
    /// app's managed singleton, or the manager's own headless. It used to be
    /// chosen per call ("app or standalone?"); resolving it here once is what
    /// let that fork and its stand-in struct go.
    pub hooks: crate::plugins::hook_state::PluginHookState,
}

impl HostState {
    /// Fresh registries with no emitter, for a host that has no windows.
    /// `TaskRegistry`'s emitter is optional by design (see `tasks.rs`) — pure-Rust
    /// callers leave it unset.
    pub fn standalone() -> Self {
        Self {
            tasks: std::sync::Arc::new(crate::tasks::TaskRegistry::new()),
            task_handles: std::sync::Arc::new(
                crate::plugins::runtime::portable::PluginTaskHandleStore::new(),
            ),
            hooks: crate::plugins::hook_state::PluginHookState::default(),
        }
    }
}

/// What every engine command arm may need. `app: None` in unit tests and on the
/// headless build path — arms that are app-only BY NATURE (windows, the system
/// browser, the live webview cookie jar) reply a structured error there.
pub struct EngineHost<'a> {
    pub app: Option<&'a dyn AppHost>,
    /// Task registries and the run table every gate reads.
    pub state: &'a HostState,
    pub project_path: &'a str,
    /// Name of the plugin whose QuickJS Context made this call. `None` means the
    /// caller could not be identified, which makes every gated capability deny
    /// (see [`grants::require_binary_grant`]).
    pub plugin: Option<&'a str>,
    /// Who asked for the runs the calling Context serves; `None` when unknown.
    pub asked_by: Option<AskedBy>,
    /// What `plugin`'s LOADED manifest declared — grants and secret custody in
    /// one resolution from [`GrantRegistry`]. `None` = the plugin is not in the
    /// discovered set, which denies every gated arm.
    pub declared: Option<&'a Declared>,
}

/// A page the plugin opened holds the watchdog off until the panel closes
/// (the adapter's `browser-closed` listener) or the hook ends.
fn window_opened(host: &EngineHost<'_>) {
    if let Some(plugin) = host.plugin {
        host.state.hooks.window_opened(plugin);
    }
}

fn holder(host: &EngineHost<'_>) -> Option<super::app_host::Holder> {
    host.plugin.map(|plugin| super::app_host::Holder { folder: host.project_path.to_string(), plugin: plugin.to_string() })
}

/// A copy whose turn has passed closes nothing: the page showing is the next copy's.
async fn close_panel(host: &EngineHost<'_>, app: &dyn AppHost) -> std::result::Result<(), String> {
    if let Some(me) = holder(host) {
        if app.panel_turn().holder().is_some_and(|h| h != me) {
            log::info!(target: "plugin", "{}: its panel close was refused; another copy holds the panel", me.plugin);
            return Ok(());
        }
    }
    app.close_action_panel(host.plugin.map(String::from)).await
}

/// A copy hook waits for the panel's turn before it shows a page, and keeps
/// it until the hook ends (released in the manager's `run_hook`). Waiting
/// in line is waiting on a person, so the watchdog does not end it.
async fn take_panel_turn(host: &EngineHost<'_>, app: &dyn AppHost) {
    let Some(me) = holder(host) else { return };
    let turn = app.panel_turn();
    let copying = host.state.hooks.executing_hook(&me.plugin).is_some_and(|h| crate::syndication::job::Op::ALL.iter().any(|op| op.hook() == h));
    if !copying || turn.holder().as_ref() == Some(&me) {
        return;
    }
    let _in_line = host.state.hooks.begin_person_wait(&me.plugin);
    turn.take(me).await;
}

fn need_app<'a>(host: &EngineHost<'a>, cmd: &str) -> std::result::Result<&'a dyn AppHost, String> {
    host.app.ok_or_else(|| format!("{cmd} requires an app handle (not available in this test host)"))
}

/// The keystore scope for the calling plugin.
///
/// The scope is the plugin bound at the dispatch seam ([`EngineHost::plugin`]),
/// NOT anything in the args — that is what makes one plugin unable to reach
/// another's keys. A call with no identified caller is refused rather than
/// defaulting to any shared scope.
fn keystore_caller_scope(
    host: &EngineHost<'_>,
    cmd: &str,
) -> std::result::Result<crate::identity::keystore::Scope, String> {
    let plugin = host.plugin.ok_or_else(|| {
        format!("'{cmd}' could not identify the calling plugin — refusing")
    })?;
    crate::identity::keystore::Scope::plugin(plugin).map_err(|e| format!("'{cmd}': {e:?}"))
}

/// The plugin whose private storage or cookies an arm may touch.
///
/// Taken from the dispatch seam ([`EngineHost::plugin`]), never from the args.
/// This module gates "another plugin's isolation — one plugin must not read or
/// write another's keys, storage, or identity", and the args are the caller's
/// to write: scoping by them let any approved plugin read `matters`' stored
/// `auth.json`, or take its live session cookies, simply by naming it.
fn caller_plugin(
    host: &EngineHost<'_>,
    cmd: &str,
) -> std::result::Result<String, String> {
    host.plugin
        .map(str::to_string)
        .ok_or_else(|| format!("'{cmd}' could not identify the calling plugin — refusing"))
}

/// How far a host call may reach into the folder.
enum Reach<'a> {
    /// The plugin is running a hook the person started.
    Folder(&'a str),
    /// Any other call from this plugin: a build's run, or one with no run
    /// registered at all (a timer that outlived its hook, a path that skipped
    /// the run gate). It writes only its own storage (`write_plugin_file`) and
    /// its own comment-data file, runs no program and reaches no network.
    Confined(&'a str),
    /// moss could not tell which plugin called.
    Unidentified,
}

/// Fails closed: only a call from a Context serving the person's runs, while
/// one is registered, reaches the folder.
fn reach<'a>(host: &EngineHost<'a>) -> Reach<'a> {
    match host.plugin {
        Some(plugin) if host.asked_by == Some(AskedBy::Person) && host.state.hooks.running_for(plugin) == Some(AskedBy::Person) => Reach::Folder(plugin),
        Some(plugin) => Reach::Confined(plugin),
        None => Reach::Unidentified,
    }
}

/// The hosts the calling plugin may reach: its discovered declaration, and the
/// host of each setting it names, read only from the `config.toml` moss writes
/// for the person, never from the plugin's own `config.json`.
fn allowed_hosts(host: &EngineHost<'_>) -> crate::plugins::runtime::hosts::AllowedHosts {
    use crate::plugins::runtime::hosts::{AllowedHosts, Refusal};
    let plugin = match reach(host) {
        Reach::Folder(plugin) => plugin,
        Reach::Confined(plugin) if host.asked_by == Some(AskedBy::Build) => return AllowedHosts::refused(Refusal::Building(plugin.into())),
        Reach::Confined(plugin) => return AllowedHosts::refused(Refusal::OutsideARun(plugin.into())),
        Reach::Unidentified => return AllowedHosts::refused(Refusal::Unidentified),
    };
    let settings = std::path::Path::new(host.project_path).join(".moss/plugins").join(plugin).join("config.toml");
    let settings = settings.is_file().then(|| crate::plugins::discovery::read_plugin_config_file(&settings).ok()).flatten().unwrap_or_default();
    AllowedHosts::for_plugin(
        plugin,
        host.declared.and_then(|d| d.network.as_ref()),
        |key| settings.get(key).and_then(|v| v.as_str()).map(str::to_string),
        || typed_site_host(host.project_path),
    )
}

/// The host of the site address the person typed in `.moss/config.toml`. Never
/// the address a publish reported, which plugin code can choose.
fn typed_site_host(project_path: &str) -> Option<String> {
    let address = crate::build::site_config::read_project_config(project_path).ok()?.site_address().ok()??;
    let url = crate::build::site_url::SiteUrl::parse(&address).ok()?;
    url.is_deployed().then(|| url.host_name().to_string())
}

/// The engine's `fetch` global (fetch_shim.rs).
async fn dispatch_fetch(host: &EngineHost<'_>, args: &serde_json::Value) -> std::result::Result<String, String> {
    use crate::plugins::runtime::download::{do_http_request_full, FetchMethod, Redirect};
    use base64::Engine as _;
    let url = req_str(args, "url")?;
    let body = || args.get("body").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let method = match args.get("method").and_then(|v| v.as_str()).unwrap_or("GET").to_uppercase().as_str() {
        "GET" => FetchMethod::Get,
        "HEAD" => FetchMethod::Head,
        "POST" => FetchMethod::Post { body: body() },
        "PUT" => FetchMethod::Put { body: body() },
        "DELETE" => FetchMethod::Delete,
        other => return Err(format!("Unsupported method: {other}")),
    };
    let redirect = match args.get("redirect").and_then(|v| v.as_str()) {
        None | Some("follow") => Redirect::Follow,
        Some("error") => Redirect::Error,
        Some("manual") => Redirect::Manual,
        Some(_) => return Err("redirect must be follow, error or manual".into()),
    };
    let r = do_http_request_full(url, method, opt_headers(args)?, Some(30_000), redirect, allowed_hosts(host)).await?;
    reply_json(&serde_json::json!({
        "status": r.status,
        "ok": r.ok,
        "headers": r.headers,
        "body_base64": base64::engine::general_purpose::STANDARD.encode(r.body),
    }))
}

/// The calling plugin, when it runs a hook the person started; otherwise a
/// refusal saying it `does` that only then, and what it `tried`.
fn need_person_run<'a>(host: &EngineHost<'a>, does: &str, tried: &str) -> std::result::Result<&'a str, String> {
    match reach(host) {
        Reach::Folder(plugin) => Ok(plugin),
        Reach::Confined(plugin) => Err(format!(
            "{plugin} {does} only when you start it yourself, and it isn't running a hook you started. It tried to {tried}.")),
        Reach::Unidentified => Err(crate::plugins::runtime::hosts::unidentified(tried)),
    }
}

fn refuse_confined_write(host: &EngineHost<'_>, relative_path: &str) -> std::result::Result<(), String> {
    match reach(host) {
        Reach::Folder(_) => Ok(()),
        Reach::Confined(plugin) if crate::vault::fs::PluginPath::shared_social_data(plugin, relative_path).is_ok() => Ok(()),
        Reach::Confined(plugin) => Err(format!(
            "{plugin} writes into the folder only when you start it yourself; it isn't running a hook you started, \
             so it may write only its own storage and its comment data. It tried to write {relative_path}."
        )),
        Reach::Unidentified => Err(crate::plugins::runtime::hosts::unidentified(&format!("write {relative_path}"))),
    }
}

fn req_str(args: &serde_json::Value, key: &str) -> std::result::Result<String, String> {
    args.get(key).and_then(|v| v.as_str()).map(str::to_string)
        .ok_or_else(|| format!("missing required string arg '{key}'"))
}

fn opt_headers(args: &serde_json::Value)
    -> std::result::Result<Option<std::collections::HashMap<String, String>>, String> {
    match args.get("headers") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| format!("bad headers: {e}")),
    }
}

fn reply_json<T: serde::Serialize>(v: &T) -> std::result::Result<String, String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

/// Core dispatch table shared between the production path (has AppHandle) and the
/// test stub path (no AppHandle). `host.project_path` provides the authority for
/// project-relative arms (`download_asset`, file commands) — the JS `projectPath`
/// arg is IGNORED (webview-command parity: AppState is the authority there; the
/// per-manager project_path via D1 is the authority here).
async fn dispatch_command_core(
    host: EngineHost<'_>,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    let args: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("bad args json: {e}"))?;
    match cmd {
        // camelCase keys mirror http.ts EXACTLY (timeoutMs, targetDir). Reading
        // timeout_ms here would silently default to 30s — the documented footgun.
        "fetch_url" => {
            let url = req_str(&args, "url")?;
            let timeout_ms = args.get("timeoutMs").and_then(|v| v.as_u64());
            let r = crate::plugins::runtime::portable::fetch_url_to(url, timeout_ms, allowed_hosts(&host)).await?;
            reply_json(&r) // snake_case FetchResult — the SDK reads body_base64
        }
        "http_get" => {
            let url = req_str(&args, "url")?;
            let headers = opt_headers(&args)?;
            let timeout_ms = args.get("timeoutMs").and_then(|v| v.as_u64());
            let r = crate::plugins::runtime::portable::http_get_to(url, headers, timeout_ms, allowed_hosts(&host)).await?;
            reply_json(&r)
        }
        "http_post" => {
            let url = req_str(&args, "url")?;
            let body = req_str(&args, "body")?; // pre-stringified JSON (http.ts:185)
            let headers = opt_headers(&args)?;
            let timeout_ms = args.get("timeoutMs").and_then(|v| v.as_u64());
            let r = crate::plugins::runtime::portable::http_post_to(url, body, headers, timeout_ms, allowed_hosts(&host)).await?;
            reply_json(&r)
        }
        "fetch" => dispatch_fetch(&host, &args).await,
        "http_post_multipart" => {
            // Multipart upload (text fields + base64 file parts). camelCase keys
            // match the SDK wire format (http.ts: textFields/files/contentBase64).
            let url = req_str(&args, "url")?;
            let text_fields: Vec<crate::plugins::runtime::portable::MultipartTextField> =
                serde_json::from_value(args.get("textFields").cloned().unwrap_or_else(|| serde_json::json!([])))
                    .map_err(|e| format!("bad textFields: {e}"))?;
            let files: Vec<crate::plugins::runtime::portable::MultipartFilePart> =
                serde_json::from_value(args.get("files").cloned().unwrap_or_else(|| serde_json::json!([])))
                    .map_err(|e| format!("bad files: {e}"))?;
            let headers = opt_headers(&args)?;
            let timeout_ms = args.get("timeoutMs").and_then(|v| v.as_u64());
            let r = crate::plugins::runtime::portable::http_post_multipart_to(url, text_fields, files, headers, timeout_ms, allowed_hosts(&host)).await?;
            reply_json(&r)
        }
        "download_asset" => {
            let url = req_str(&args, "url")?;
            let target_dir = req_str(&args, "targetDir")?;
            refuse_confined_write(&host, &format!("{}/", target_dir.trim_end_matches('/')))?;
            let timeout_ms = args.get("timeoutMs").and_then(|v| v.as_u64());
            // The JS projectPath arg is IGNORED — same authority model as the
            // webview command (AppState); here the per-manager project_path (D1).
            let filename = args.get("filename").and_then(|v| v.as_str()).map(str::to_string);
            let headers = opt_headers(&args)?;
            let r = crate::plugins::runtime::download::download_asset_to(url, host.project_path, target_dir, timeout_ms, filename, headers, allowed_hosts(&host)).await?;
            reply_json(&r)
        }
        "html_to_markdown" => {
            // Delegates to the shared `html_to_markdown`.
            let html = req_str(&args, "html")?;
            let md = crate::plugins::runtime::portable::html_to_markdown(html).await?;
            reply_json(&md)
        }
        "get_plugin_cookie" => {
            let plugin = caller_plugin(&host, "get_plugin_cookie")?;
            let app = need_app(&host, "get_plugin_cookie")?;
            app.get_plugin_cookies(&plugin, host.project_path).await
        }
        "set_plugin_cookie" => {
            let app = need_app(&host, "set_plugin_cookie")?;
            let plugin = caller_plugin(&host, "set_plugin_cookie")?;
            let cookies = args.get("cookies").cloned().ok_or("missing 'cookies'")?;
            app.set_plugin_cookies(&plugin, host.project_path, cookies).await?;
            Ok("null".into())
        }
        // Credential-leak fix: `clear_plugin_cookies` was registered in the WEBVIEW
        // invoke_handler (lib.rs, a0b18c048) but NOT here in the plugin-engine host
        // bridge — so the matters plugin's `beginFreshLogin → clearPluginCookies()`
        // (which runs in this quickjs engine, not a webview) hit `unknown command`
        // and silently no-op'd. The domain cookies were never cleared, so a fresh
        // folder's login browser inherited the previous account's still-live session
        // and auto-logged-in as the wrong account. Expose it here (mirror of
        // get/set) so the force-fresh-login clear actually runs.
        "clear_plugin_cookies" => {
            let app = need_app(&host, "clear_plugin_cookies")?;
            let plugin = caller_plugin(&host, "clear_plugin_cookies")?;
            app.clear_plugin_cookies(&plugin, host.project_path).await?;
            Ok("null".into())
        }
        // B.8b: file/env/site command arms (camelCase keys: pluginName, relativePath,
        // content, data, name — matching the SDK's http.ts and file.ts wire format).
        "read_plugin_file" => {
            let r = crate::plugins::project_files::read_plugin_file_impl(
                host.project_path, &caller_plugin(&host, "read_plugin_file")?, &req_str(&args, "relativePath")?).await?;
            reply_json(&r)
        }
        "write_plugin_file" => {
            crate::plugins::project_files::write_plugin_file_impl(
                host.project_path, &caller_plugin(&host, "write_plugin_file")?,
                &req_str(&args, "relativePath")?, &req_str(&args, "content")?).await?;
            Ok("null".into())
        }
        "plugin_file_exists" => {
            let r = crate::plugins::project_files::plugin_file_exists_impl(
                host.project_path, &caller_plugin(&host, "plugin_file_exists")?, &req_str(&args, "relativePath")?).await?;
            reply_json(&r)
        }
        "read_project_file" => {
            // host.plugin is the engine's own authoritative identity for the
            // running plugin (never client-supplied), so the shared
            // social-data door in read_project_file_impl gets a trustworthy
            // plugin_id the same way the Tauri command path gets one from
            // moss-api's ctx.plugin_name.
            let r = crate::plugins::project_files::read_project_file_impl(
                host.project_path, host.plugin, &req_str(&args, "relativePath")?).await?;
            reply_json(&r)
        }
        "read_site_file" => {
            // Built-site bytes (base64) from the current generation — needed by
            // syndication plugins to upload local image/audio bytes.
            let r = crate::plugins::project_files::read_site_file_impl(
                host.project_path, &req_str(&args, "relativePath")?).await?;
            reply_json(&r)
        }
        "write_project_file" => {
            let relative_path = req_str(&args, "relativePath")?;
            refuse_confined_write(&host, &relative_path)?;
            crate::plugins::project_files::write_project_file_impl(
                host.project_path, host.plugin, &relative_path, &req_str(&args, "data")?).await?;
            Ok("null".into())
        }
        "write_project_file_binary" => {
            // Bytes cross the boundary as base64, like every other binary here.
            let relative_path = req_str(&args, "relativePath")?;
            refuse_confined_write(&host, &relative_path)?;
            crate::plugins::project_files::write_project_file_binary_impl(
                host.project_path, host.plugin, &relative_path, &req_str(&args, "dataBase64")?).await?;
            Ok("null".into())
        }
        "list_project_files" => {
            let r = crate::plugins::project_files::list_project_files_impl(host.project_path).await?;
            reply_json(&r)
        }
        "list_project_tree" => {
            let r = crate::plugins::project_files::list_project_tree_impl(host.project_path).await?;
            reply_json(&r)
        }
        "list_site_files_with_sizes" => {
            let r = crate::plugins::project_files::list_site_files_with_sizes_impl(
                std::path::Path::new(host.project_path));
            reply_json(&r)
        }
        // Resolve the environment from the project path directly rather than from
        // `AppState`: `AppState::active_environment` IS
        // `domain::config::resolve_environment(project_path)` for any project-scoped
        // call (infra/state.rs), so this is the same answer without an app.
        "get_plugin_env_var" => {
            let name = req_str(&args, "name")?;
            let env = crate::build::site_config::resolve_environment(host.project_path);
            let v = crate::plugins::runtime::portable::get_plugin_env_var_impl(&name, &env);
            reply_json(&v)
        }
        // Keystore. Not gated: the caller signs only with
        // its own scoped key. The scope is the calling plugin, resolved from the
        // dispatch seam — never a parameter, so a caller cannot reach another's
        // key.
        "key_get_or_create" => {
            use base64::Engine as _;
            let scope = keystore_caller_scope(&host, "key_get_or_create")?;
            let name = req_str(&args, "name")?;
            let algorithm = req_str(&args, "algorithm")?;
            let algorithm = crate::identity::keystore::Algorithm::from_wire(&algorithm)
                .ok_or_else(|| format!("unknown key algorithm '{algorithm}'"))?;
            let ks = crate::identity::keystore::Keystore::for_project(
                std::path::Path::new(host.project_path),
            );
            let info = ks
                .get_or_create(&scope, &name, algorithm)
                .map_err(|e| format!("keystore: {e:?}"))?;
            Ok(serde_json::json!({
                "name": info.name,
                "algorithm": info.algorithm.as_wire(),
                "publicKeyBase64": base64::engine::general_purpose::STANDARD.encode(info.public_key),
            })
            .to_string())
        }
        // Secrets. The bodies are in [`secrets`], which owns the custody rules;
        // the arms stay here so the seam still lists them.
        "get_plugin_secret" => secrets::get(&host, &args),
        "set_plugin_secret" => secrets::set(&host, &args),
        "reject_plugin_secret" => secrets::reject(&host, &args).await,
        "key_list" => {
            use base64::Engine as _;
            let scope = keystore_caller_scope(&host, "key_list")?;
            let ks = crate::identity::keystore::Keystore::for_project(
                std::path::Path::new(host.project_path),
            );
            let keys = ks.list(&scope).map_err(|e| format!("keystore: {e:?}"))?;
            let arr: Vec<_> = keys
                .into_iter()
                .map(|info| {
                    serde_json::json!({
                        "name": info.name,
                        "algorithm": info.algorithm.as_wire(),
                        "publicKeyBase64": base64::engine::general_purpose::STANDARD.encode(info.public_key),
                    })
                })
                .collect();
            Ok(serde_json::Value::Array(arr).to_string())
        }
        "key_sign" => {
            use base64::Engine as _;
            let scope = keystore_caller_scope(&host, "key_sign")?;
            let name = req_str(&args, "name")?;
            let payload_b64 = req_str(&args, "payloadBase64")?;
            let payload = base64::engine::general_purpose::STANDARD
                .decode(payload_b64.as_bytes())
                .map_err(|e| format!("payloadBase64 is not valid base64: {e}"))?;
            let ks = crate::identity::keystore::Keystore::for_project(
                std::path::Path::new(host.project_path),
            );
            let sig = ks
                .sign(&scope, &name, &payload)
                .map_err(|e| format!("keystore: {e:?}"))?;
            Ok(serde_json::json!({
                "signatureBase64": base64::engine::general_purpose::STANDARD.encode(sig),
            })
            .to_string())
        }
        "execute_binary" => {
            let p = parse_execute_binary_args(&args)?;
            need_person_run(&host, "runs programs", &format!("run {}", p.binary_path))?;
            require_binary_grant(&host, &p.binary_path)?;
            // The headless impl, not the Tauri command: the grant gate above
            // IS this seam's gate, and the command's window-label check has no
            // meaning here (there is no calling webview). Streaming is
            // webview-only (see `parse_execute_binary_args`), so this seam
            // takes the blocking half of the headless split directly.
            let r = crate::plugins::runtime::download::execute_binary_blocking_impl(
                std::path::Path::new(host.project_path), p.binary_path, p.args, p.working_dir, p.env_vars,
                p.timeout_ms, p.stdin_data).await?;
            reply_json(&r)
        }
        "open_action_panel" => {
            let app = need_app(&host, "open_action_panel")?;
            // `url: null` (openBrowser() with no page) and a missing key both
            // mean "open the panel in its loading state".
            let url = args.get("url").filter(|v| !v.is_null())
                .map(|v| v.as_str().map(String::from)
                    .ok_or_else(|| "'url' must be a string or null".to_string()))
                .transpose()?;
            let title = args.get("title").and_then(|v| v.as_str()).map(String::from);
            let below = args.get("belowTitlebar").and_then(|v| v.as_bool()).unwrap_or(true);
            take_panel_turn(&host, app).await;
            app.open_action_panel(url, title, below, host.plugin.map(String::from)).await.map(|()| window_opened(&host))?;
            Ok("null".into())
        }
        "set_action_panel_html" => {
            let app = need_app(&host, "set_action_panel_html")?;
            let html = req_str(&args, "html")?;
            let title = args.get("title").and_then(|v| v.as_str()).map(String::from);
            take_panel_turn(&host, app).await;
            app.set_action_panel_html(html, title, host.plugin.map(String::from)).await.map(|()| window_opened(&host))?;
            Ok("null".into())
        }
        "close_action_panel" => {
            let app = need_app(&host, "close_action_panel")?;
            close_panel(&host, app).await?;
            Ok("null".into())
        }
        "open_system_browser" => {
            let app = need_app(&host, "open_system_browser")?;
            need_person_run(&host, "opens the browser", "open the browser")?;
            let url = crate::plugins::runtime::browser_sign_in::check_browser_address(&req_str(&args, "url")?)?;
            app.open_system_browser(url.into()).await?;
            Ok("null".into())
        }
        // Without an app nothing can open the browser and wait for it to come back.
        "browser_sign_in_start" => {
            need_app(&host, "browser_sign_in_start").map_err(|_| "Sign in from the moss app".to_string())?;
            let plugin = need_person_run(&host, "signs in", "sign in")?;
            let channel = host.declared.and_then(|d| d.title.clone()).unwrap_or_else(|| plugin.to_string());
            reply_json(&serde_json::json!({ "redirectUri": host.state.hooks.sign_ins.start(plugin, &channel).await? }))
        }
        "browser_sign_in_wait" => {
            let app = need_app(&host, "browser_sign_in_wait").map_err(|_| "Sign in from the moss app".to_string())?;
            let plugin = need_person_run(&host, "signs in", "sign in")?;
            // The person is in their browser: the watchdog waits for them, as for any question moss asks.
            let _waiting = host.state.hooks.begin_person_wait(plugin);
            let params = host.state.hooks.sign_ins.wait(plugin, &req_str(&args, "url")?, |url| app.open_system_browser(url)).await?;
            reply_json(&serde_json::json!({ "params": params }))
        }
        // Bridged 2026-08-28: a Tauri command since onboarding, but never an
        // engine arm, so every plugin call since QuickJS became the default
        // (2026-06-13) threw `unknown command` — and matters' `.catch(() => {})`
        // swallowed it, leaving an empty action panel on login cancel.
        "return_to_editor" => {
            let app = need_app(&host, "return_to_editor")?;
            app.return_to_editor().await?;
            Ok("null".into())
        }
        "fill_browser_panel" => {
            let app = need_app(&host, "fill_browser_panel")?;
            let plugin = need_person_run(&host, "fills the browser panel", "fill the browser panel")?;
            let fill = args.get("fill").cloned().ok_or("missing 'fill'")?;
            reply_json(&app.fill_browser_panel(plugin, host.project_path, fill).await?)
        }
        "resolve_git_path" => {
            let r = crate::plugins::runtime::portable::resolve_git_path_impl().await?;
            reply_json(&r)
        }
        // Runs headless: the registries come from the injected
        // [`HostState`], not from managed state. The `contributes.jobs`
        // descriptor comes from the caller's manifest snapshot, the same one
        // the grant arms read.
        "report_plugin_task_lifecycle_command" => {
            let plugin_name = caller_plugin(&host, "report_plugin_task_lifecycle_command")?;
            let hook: crate::plugins::types::PluginHook =
                serde_json::from_value(args.get("hook").cloned()
                    .ok_or("missing 'hook'")?).map_err(|e| format!("bad hook: {e}"))?;
            let trigger: crate::plugins::types::TriggerContext =
                serde_json::from_value(args.get("trigger").cloned()
                    .ok_or("missing 'trigger'")?).map_err(|e| format!("bad trigger: {e}"))?;
            let task_id: Option<crate::tasks::TaskId> = match args.get("taskId") {
                None | Some(serde_json::Value::Null) => None,
                Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("bad taskId: {e}"))?,
            };
            let lifecycle: crate::plugins::runtime::portable::PluginTaskLifecycle =
                serde_json::from_value(args.get("lifecycle").cloned()
                    .ok_or("missing 'lifecycle'")?).map_err(|e| format!("bad lifecycle: {e}"))?;
            // Same shared body as the Tauri command — watchdog record,
            // descriptor resolution (contributes.jobs, R13), spawn, stash.
            // Routing this arm through anything narrower silently drops Job
            // verb/amount stamping for quickjs-routed plugins. `attended` is
            // whether a person could answer a question this hook asks: headless
            // the watchdog must still cut an awaiting hook off, or a CI build
            // hangs on a prompt nobody will ever see.
            let resolved = crate::plugins::runtime::portable::resolve_started_job_descriptor(
                &lifecycle,
                |job| host.declared.and_then(|d| d.jobs.get(job).cloned()),
            );
            let id = crate::plugins::runtime::portable::report_plugin_task_lifecycle_with_descriptor(
                host.state,
                host.app.is_some(),
                resolved,
                crate::tasks::WindowId::from("main".to_string()),
                &plugin_name,
                hook,
                trigger,
                task_id,
                lifecycle,
            )?;
            reply_json(&id)
        }
        other => Err(format!("unknown command: {other}")),
    }
}

/// HOST-SIDE dispatcher: runs on a Tauri async task. Wired by
/// `QuickJsEngineAdapter`'s host dispatch task.
///
/// `app` is `None` on the headless build path. That is not a degraded
/// mode — every arm a build-time hook reaches works from `state` alone. The arms
/// that genuinely need windows refuse by name, which is the deliberate shape: a
/// build never silently drops plugin content, it stops and says which command
/// wanted a desktop.
pub async fn dispatch_command(
    app: Option<&dyn AppHost>,
    state: &HostState,
    project_path: &str,
    plugin: Option<&str>,
    asked_by: Option<AskedBy>,
    declared: Option<&Declared>,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    dispatch_command_core(
        EngineHost {
            app,
            state,
            project_path,
            plugin,
            asked_by,
            declared,
        },
        cmd,
        args_json,
    )
    .await
}

/// AppHandle-free entry for tests that call the arms. Its run table is empty, so a plugin's call is confined to the
/// plugin's own storage; [`dispatch_command_test_stub_for_person`] asks for a
/// person's run instead.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn dispatch_command_test_stub(
    project_path: &str,
    plugin: Option<&str>,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    let state = HostState::standalone();
    dispatch_command_core(
        EngineHost { app: None, state: &state, project_path, plugin, asked_by: None, declared: None },
        cmd,
        args_json,
    )
    .await
}

/// As [`dispatch_command_test_stub`], for a plugin named `local` that
/// declares `127.0.0.1`, the address the local test servers bind.
#[cfg(test)]
pub async fn dispatch_command_test_stub_reaching_local(
    project_path: &str,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    dispatch_command_test_stub_declaring(project_path, "local", &["127.0.0.1"], cmd, args_json).await
}

/// As [`dispatch_command_test_stub_for_person`], for `plugin` declaring `hosts`.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn dispatch_command_test_stub_declaring(
    project_path: &str,
    plugin: &str,
    hosts: &[&str],
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    dispatch_command_test_stub_asked_by(AskedBy::Person, project_path, plugin, Some(hosts), cmd, args_json).await
}

/// As [`dispatch_command_test_stub_declaring`], from a run `asked_by` began;
/// `hosts: None` is a manifest with no `network` field.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn dispatch_command_test_stub_asked_by(
    asked_by: AskedBy,
    project_path: &str,
    plugin: &str,
    hosts: Option<&[&str]>,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    let state = HostState::standalone();
    let _run = match asked_by {
        AskedBy::Person => Some(state.hooks.begin_person_run(plugin).await),
        AskedBy::Build => state.hooks.try_begin_build_run(plugin),
    };
    let declared = Declared {
        network: hosts.map(|hosts| crate::plugins::types::NetworkDecl { hosts: hosts.iter().map(|h| h.to_string()).collect(), ..Default::default() }),
        ..Default::default()
    };
    dispatch_command_core(
        EngineHost { app: None, state: &state, project_path, plugin: Some(plugin), asked_by: Some(asked_by), declared: Some(&declared) },
        cmd,
        args_json,
    )
    .await
}

/// [`dispatch_command_test_stub`] as a call from `plugin`'s run that a person
/// started, which reaches the whole folder.
#[cfg(any(test, feature = "test-fixtures"))]
pub async fn dispatch_command_test_stub_for_person(
    project_path: &str,
    plugin: &str,
    cmd: &str,
    args_json: &str,
) -> std::result::Result<String, String> {
    dispatch_command_test_stub_asked_by(AskedBy::Person, project_path, plugin, Some(&["127.0.0.1"]), cmd, args_json).await
}

#[cfg(test)]
mod tests {
    /// One state for hosts that never begin a run; a test that registers runs
    /// builds its own.
    fn bare<'a>(project: &'a str, plugin: Option<&'a str>) -> EngineHost<'a> {
        static EMPTY: std::sync::OnceLock<HostState> = std::sync::OnceLock::new();
        EngineHost {
            app: None,
            state: EMPTY.get_or_init(HostState::standalone),
            project_path: project,
            plugin,
            asked_by: None,
            declared: None,
        }
    }

    use super::*;

    // ── requires-gate: fail-closed grants for privileged commands ──

    /// Write a plugin dir with a manifest declaring `requires`.
    fn plugin_with_requires(root: &std::path::Path, name: &str, requires: Option<&str>) {
        let dir = root.join(".moss").join("plugins").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let requires_field = match requires {
            Some(r) => format!(r#","requires":["{r}"]"#),
            None => String::new(),
        };
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"name":"{name}","version":"1.0.0","entry":"main.js","capabilities":[]{requires_field}}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "").unwrap();
    }

    /// Snapshot the grants the way the manager does: from the manifest LOADED
    /// off disk once, not re-read per call. An unloadable dir yields an empty
    /// registry, so the plugin resolves to `None` (fail-closed).
    fn discovered(root: &std::path::Path, name: &str) -> GrantRegistry {
        let dir = root.join(".moss").join("plugins").join(name);
        match crate::plugins::discovery::load_plugin(&dir) {
            Ok(p) => GrantRegistry::from_plugins(&[p]),
            Err(_) => GrantRegistry::default(),
        }
    }

    /// Both secret arms are bridged, and both refuse a caller the seam could not
    /// identify. They share `keystore_caller_scope` with the keystore arms —
    /// whose own test below proves the scope comes from the seam — and neither
    /// accepts a plugin name in its args, so there is nothing to forge. What is
    /// left to prove here is the fail-closed half: an unidentified caller gets
    /// a refusal, not somebody's token.
    ///
    /// Storage semantics live in `identity::secrets`' own tests, against a
    /// temp dir. They cannot be exercised through this seam: the store is
    /// app-global by design, so a test reaching it would write into the
    /// developer's real credentials.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn secret_arms_are_bridged_and_refuse_an_unidentified_caller() {
        for cmd in ["get_plugin_secret", "reject_plugin_secret", "set_plugin_secret"] {
            let err = dispatch_command_test_stub("/tmp/none", None, cmd, r#"{"key":"pinata_jwt"}"#)
                .await
                .expect_err("the test stub has no plugin identity, so the arm must refuse");
            assert!(
                !err.contains("unknown command"),
                "{cmd} must be bridged into the plugin engine, got: {err}"
            );
            // The read/reject arms refuse in `keystore_caller_scope`, the write
            // arm one step earlier in `require_plugin_owned_key`; both say the
            // caller is the thing they could not name.
            assert!(
                err.contains("could not identify the calling plugin")
                    || err.contains("could not be identified"),
                "{cmd} must fail closed on an unidentified caller, got: {err}"
            );
        }
    }

    /// A plugin manifest at `.moss/plugins/<name>/` declaring credentials moss
    /// collects from the user.
    fn plugin_with_credentials(root: &std::path::Path, name: &str, keys: &[&str]) {
        let dir = root.join(".moss").join("plugins").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let creds = keys
            .iter()
            .map(|k| format!(r#"{{"key":"{k}","label":"{k}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            dir.join("manifest.json"),
            format!(
                r#"{{"name":"{name}","version":"1.0.0","entry":"main.js",
                     "contributes":{{"channel":{{"login":true,
                       "setup":{{"credentials":[{creds}]}}}}}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "").unwrap();
    }

    fn host_for<'a>(
        project: &'a str,
        plugin: &'a str,
        declared: &'a Option<Declared>,
    ) -> EngineHost<'a> {
        EngineHost { declared: declared.as_ref(), ..bare(project, Some(plugin)) }
    }

    /// The write arm is scoped to the KEY, not to the plugin. One plugin can
    /// hold a token its login returned and a token the user pasted; it may
    /// deposit into the first slot and not the second.
    ///
    /// Both halves are proven on the refusal path, before the store is touched
    /// — which is what makes this safe against the developer's real store.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_write_arm_refuses_a_key_the_user_fills_in() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().to_str().unwrap();
        plugin_with_credentials(tmp.path(), "matters", &["matters_token"]);
        let secrets_declared = discovered(tmp.path(), "matters").declared_for("matters");
        assert_eq!(
            secrets_declared.as_ref().map(|d| d.user_supplied_secrets.as_slice()),
            Some(["matters_token".to_string()].as_slice())
        );

        let err = dispatch_command_core(
            host_for(project, "matters", &secrets_declared),
            "set_plugin_secret",
            r#"{"key":"matters_token","value":"substituted"}"#,
        )
        .await
        .expect_err("the user types this one, so the plugin may not write it");
        assert!(err.contains("the user gives moss directly"), "{err}");
        assert!(err.contains("rejectSecret"), "the refusal must name the way through: {err}");

        // The flow-obtained slot beside it is the plugin's own. matters
        // declares NO hidden secrets — it predates declared custody — so the
        // undeclared key admits on the deprecation path rather than refusing.
        assert!(secrets::require_plugin_owned_key(
            &host_for(project, "matters", &secrets_declared),
            "access_token",
        )
        .is_ok());
    }

    /// A manifest that HAS declared custody (any hidden `secret` setting) is
    /// held to it: the declared key admits, an undeclared one refuses. This is
    /// the same migration posture as the blanket `execute_binary` grant — the
    /// strict rule binds the manifests that adopted the contract.
    #[test]
    fn declared_custody_admits_its_keys_and_refuses_the_rest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().to_str().unwrap();
        let dir = tmp.path().join(".moss").join("plugins").join("gh");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"name":"gh","version":"1.0.0","entry":"main.js",
                "contributes":{"deploy_target":{"setup":{"settings":[
                  {"key":"github_token","type":"secret","label":"GitHub token",
                   "description":"Earned by the device flow.","hidden":true}]}}}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("main.js"), "").unwrap();

        let declared = discovered(tmp.path(), "gh").declared_for("gh");
        assert_eq!(
            declared.as_ref().map(|d| d.plugin_owned_secrets.as_slice()),
            Some(["github_token".to_string()].as_slice())
        );

        let host = host_for(project, "gh", &declared);
        assert!(secrets::require_plugin_owned_key(&host, "github_token").is_ok());
        let err = secrets::require_plugin_owned_key(&host, "somewhere_else")
            .expect_err("declared custody closes the undeclared keys");
        assert!(err.contains("hidden `secret` setting"), "{err}");
    }

    /// Fail-closed like the grant gate: a plugin moss never discovered, and a
    /// caller the seam could not name, are both refused rather than defaulted
    /// to an empty exclusion set.
    #[test]
    fn the_write_gate_fails_closed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().to_str().unwrap();
        let none: Option<Declared> = None;

        let err = secrets::require_plugin_owned_key(
            &host_for(project, "ghost", &none),
            "anything",
        )
        .expect_err("an undiscovered plugin must refuse");
        assert!(err.contains("not in the discovered plugin set"), "{err}");

        let anon = bare(project, None);
        let err = secrets::require_plugin_owned_key(&anon, "anything")
            .expect_err("an unidentified caller must refuse");
        assert!(err.contains("could not be identified"), "{err}");
    }

    /// The keystore arms take the scope from the calling plugin (the dispatch
    /// seam), not the args — so ungated, and one plugin cannot reach another's
    /// key even by naming it. Verified end-to-end through dispatch_command_core.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn keystore_is_scoped_to_the_calling_plugin() {
        use base64::Engine as _;
        let tmp = tempfile::TempDir::new().unwrap();
        let project = tmp.path().to_str().unwrap();

        // Plugin "alice" creates a key. No manifest, no `requires` — ungated.
        let alice = bare(project, Some("alice"));
        let a_json = dispatch_command_core(
            alice,
            "key_get_or_create",
            r#"{"name":"ipns","algorithm":"ed25519"}"#,
        )
        .await
        .expect("ungated create must succeed");
        let a_pub = serde_json::from_str::<serde_json::Value>(&a_json).unwrap()
            ["publicKeyBase64"].as_str().unwrap().to_string();

        // Plugin "bob" asks for the SAME name — gets a DIFFERENT key, because
        // the scope is bob, resolved from the seam, not from the args.
        let bob = bare(project, Some("bob"));
        let b_json = dispatch_command_core(
            bob,
            "key_get_or_create",
            r#"{"name":"ipns","algorithm":"ed25519"}"#,
        )
        .await
        .unwrap();
        let b_pub = serde_json::from_str::<serde_json::Value>(&b_json).unwrap()
            ["publicKeyBase64"].as_str().unwrap().to_string();
        assert_ne!(a_pub, b_pub, "same name, different caller = different key");

        // bob lists — sees only bob's key, never alice's.
        let bob = bare(project, Some("bob"));
        let list = dispatch_command_core(bob, "key_list", "null").await.unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&list).unwrap();
        assert_eq!(arr.len(), 1, "a caller lists only its own keys");
        assert_eq!(arr[0]["publicKeyBase64"].as_str().unwrap(), b_pub);

        // alice signs with her key; the signature verifies under her public key.
        let alice = bare(project, Some("alice"));
        let msg = b"ipns record";
        let args = format!(
            r#"{{"name":"ipns","payloadBase64":"{}"}}"#,
            base64::engine::general_purpose::STANDARD.encode(msg)
        );
        let sig_json = dispatch_command_core(alice, "key_sign", &args).await.unwrap();
        let sig_b64 = serde_json::from_str::<serde_json::Value>(&sig_json).unwrap()
            ["signatureBase64"].as_str().unwrap().to_string();

        use ed25519_dalek::{Signature, Verifier, VerifyingKey};
        let pk: [u8; 32] = base64::engine::general_purpose::STANDARD.decode(&a_pub).unwrap().try_into().unwrap();
        let sig = Signature::from_slice(&base64::engine::general_purpose::STANDARD.decode(&sig_b64).unwrap()).unwrap();
        assert!(VerifyingKey::from_bytes(&pk).unwrap().verify(msg, &sig).is_ok());

        // An unidentified caller (no plugin bound) is refused, not defaulted.
        let anon = bare(project, None);
        assert!(dispatch_command_core(anon, "key_list", "null").await.is_err());
    }

    #[test]
    fn grant_refused_when_plugin_declares_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        plugin_with_requires(tmp.path(), "ipfs", None);
        let grants = discovered(tmp.path(), "ipfs").declared_for("ipfs");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("ipfs")) };
        let err = require_binary_grant(&host, "git").expect_err("undeclared must refuse");
        assert!(err.contains("does not declare"), "{err}");
    }

    /// A named grant admits exactly its binary and nothing else —
    /// `execute_binary:git` is not `execute_binary:rm`, nor any other file
    /// that happens to be called `git`.
    #[test]
    fn a_named_binary_grant_admits_only_its_basename() {
        let tmp = tempfile::TempDir::new().unwrap();
        plugin_with_requires(tmp.path(), "gh", Some("execute_binary:git"));
        let grants = discovered(tmp.path(), "gh").declared_for("gh");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("gh")) };
        assert!(require_binary_grant(&host, "git").is_ok());
        let planted = tmp.path().join("git");
        std::fs::write(&planted, "").unwrap();
        let err = require_binary_grant(&host, planted.to_str().unwrap())
            .expect_err("a file named git outside where moss looks must refuse");
        assert!(err.contains("is not the `git` moss runs"), "{err}");
        let err = require_binary_grant(&host, "rm").expect_err("ungranted binary must refuse");
        assert!(err.contains("execute_binary:rm"), "{err}");
        // A path with no basename can match no grant.
        assert!(require_binary_grant(&host, "/").is_err());
    }

    /// An absolute path to the granted binary where the PATH finds it passes,
    /// as `/bin/sh` does for a plugin that starts a daemon through it.
    #[cfg(unix)]
    #[test]
    fn a_named_grant_admits_the_binary_on_the_path_by_absolute_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        plugin_with_requires(tmp.path(), "node", Some("execute_binary:sh"));
        let grants = discovered(tmp.path(), "node").declared_for("node");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("node")) };
        assert!(require_binary_grant(&host, "/bin/sh").is_ok());
    }

    /// The pre-2026-08-30 blanket token still grants every binary — an
    /// installed plugin must keep deploying across the migration — and only
    /// deprecation noise marks it.
    #[test]
    fn the_blanket_execute_binary_grant_still_admits_everything() {
        let tmp = tempfile::TempDir::new().unwrap();
        plugin_with_requires(tmp.path(), "old", Some("execute_binary"));
        let grants = discovered(tmp.path(), "old").declared_for("old");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("old")) };
        assert!(require_binary_grant(&host, "git").is_ok());
        assert!(require_binary_grant(&host, "anything-at-all").is_ok());
    }

    /// Fail-closed: an unidentified caller is denied, never allowed.
    #[test]
    fn grant_refused_when_caller_unknown() {
        let tmp = tempfile::TempDir::new().unwrap();
        let host = bare(tmp.path().to_str().unwrap(), None);
        let err = require_binary_grant(&host, "git").expect_err("unknown caller must refuse");
        assert!(err.contains("could not be identified"), "{err}");
    }

    /// Fail-closed: a plugin moss never discovered denies rather than defaulting
    /// open — and is never looked up on disk to find out.
    #[test]
    fn grant_refused_when_plugin_was_never_discovered() {
        let tmp = tempfile::TempDir::new().unwrap();
        let grants = discovered(tmp.path(), "ghost").declared_for("ghost");
        assert!(grants.is_none(), "an unloadable plugin dir must not resolve");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("ghost")) };
        let err = require_binary_grant(&host, "git").expect_err("unknown plugin must refuse");
        assert!(err.contains("not in the discovered plugin set"), "{err}");
    }

    /// Authority is what moss LOADED, never what is on disk now: `.moss/` is
    /// writable by the running app, so a manifest rewritten after discovery must
    /// not widen the granted set of an already-running plugin.
    #[test]
    fn grants_ignore_a_manifest_rewritten_after_discovery() {
        let tmp = tempfile::TempDir::new().unwrap();
        plugin_with_requires(tmp.path(), "ipfs", None);
        let registry = discovered(tmp.path(), "ipfs");

        // Post-discovery rewrite claiming the capability the loaded manifest lacks.
        plugin_with_requires(tmp.path(), "ipfs", Some("execute_binary"));

        let grants = registry.declared_for("ipfs");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("ipfs")) };
        let err = require_binary_grant(&host, "git")
            .expect_err("a post-load rewrite must not grant a capability");
        assert!(err.contains("does not declare"), "{err}");

        // …and a re-snapshot of the rewritten set is the ONLY way it changes.
        registry.replace(&[crate::plugins::discovery::load_plugin(
            &tmp.path().join(".moss").join("plugins").join("ipfs"),
        )
        .unwrap()]);
        let grants = registry.declared_for("ipfs");
        let host = EngineHost { declared: grants.as_ref(), ..bare(tmp.path().to_str().unwrap(), Some("ipfs")) };
        assert!(require_binary_grant(&host, "git").is_ok());
    }

    /// B.8a RED: HTTP arms camelCase-in, snake_case-out. http_post must accept
    /// `timeoutMs` (camelCase) and return snake_case `body_base64`/`content_type`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_arms_speak_camelcase_in_snakecase_out() {
        let mut server = mockito::Server::new_async().await;
        let m = server.mock("POST", "/graphql")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data":{"ok":true}}"#)
            .create_async().await;
        let args = serde_json::json!({
            "url": format!("{}/graphql", server.url()),
            "body": "{\"query\":\"{ viewer { id } }\"}",
            "headers": { "x-access-token": "tok" },
            "timeoutMs": 5000
        }).to_string();
        let reply = dispatch_command_test_stub_reaching_local("/tmp/none", "http_post", &args).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        // Wire-shape parity with the webview path (and mock-tauri's served shape):
        // snake_case keys, base64 body.
        assert_eq!(v["ok"], serde_json::json!(true));
        assert!(v.get("body_base64").is_some(), "reply must use snake_case body_base64; got {v}");
        assert!(v.get("content_type").is_some());
        assert!(v.get("bodyBase64").is_none(), "camelCase reply would silently break every plugin");
        m.assert_async().await;
    }

    /// B.8a: fetch_url and http_get arms roundtrip through a mock server.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fetch_url_arm_and_http_get_arm_roundtrip() {
        let mut server = mockito::Server::new_async().await;
        let _m = server.mock("GET", "/x").with_status(200)
            .with_header("content-type", "text/plain").with_body("hi")
            .create_async().await;
        for cmd in ["fetch_url", "http_get"] {
            let args = serde_json::json!({ "url": format!("{}/x", server.url()), "timeoutMs": 5000 }).to_string();
            let reply = dispatch_command_test_stub_reaching_local("/tmp/none", cmd, &args).await.unwrap();
            let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
            assert_eq!(v["status"], serde_json::json!(200), "{cmd}");
            use base64::Engine as _;
            let body = base64::engine::general_purpose::STANDARD
                .decode(v["body_base64"].as_str().unwrap()).unwrap();
            assert_eq!(body, b"hi", "{cmd}");
        }
    }

    /// B.8a: download_asset writes under the HOST's project_path, ignoring the JS arg.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn download_asset_arm_uses_host_project_path() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let mut server = mockito::Server::new_async().await;
        let _m = server.mock("GET", "/a.png").with_status(200)
            .with_header("content-type", "image/png").with_body(vec![1u8, 2, 3])
            .create_async().await;
        let args = serde_json::json!({
            "url": format!("{}/a.png", server.url()),
            "projectPath": "/somewhere/else/IGNORED",
            "targetDir": "assets",
            "timeoutMs": 5000
        }).to_string();
        let reply = dispatch_command_test_stub_for_person(proj.path().to_str().unwrap(), "p", "download_asset", &args)
            .await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true));
        let written = proj.path().join(v["actual_path"].as_str().unwrap());
        assert!(written.exists(), "asset must land under the HOST project path: {written:?}");
    }

    /// `write_project_file_binary` lands the exact bytes, through the same
    /// sandbox as the text write: `.moss/` and path escapes are refused.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn binary_write_arm_roundtrips_bytes_and_keeps_the_sandbox() {
        use base64::Engine;
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let pp = proj.path().to_str().unwrap();
        let bytes: Vec<u8> = vec![0x00, 0xFF, 0xD8, 0x80, 0x00, 0x0A, 0x0D];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let write = |rel: &str, data: &str| {
            serde_json::json!({ "relativePath": rel, "dataBase64": data }).to_string()
        };

        dispatch_command_test_stub_for_person(pp, "p", "write_project_file_binary", &write("maps/a.jpg", &b64))
            .await.unwrap();
        assert_eq!(std::fs::read(proj.path().join("maps/a.jpg")).unwrap(), bytes);

        for refused in [".moss/config.toml", ".moss/plugins/x/main.js", "../escape.bin", "a/../../escape.bin", "/abs.bin"] {
            let err = dispatch_command_test_stub_for_person(pp, "p", "write_project_file_binary", &write(refused, &b64))
                .await.expect_err(refused);
            assert!(!err.contains("unknown command"), "{refused}: {err}");
            assert!(!err.contains("only when you start it yourself") && !err.contains("could not identify"), "{refused}: refused by reach, not the path rule: {err}");
        }
        assert!(!proj.path().join(".moss/config.toml").exists());
        assert!(!proj.path().parent().unwrap().join("escape.bin").exists());

        let err = dispatch_command_test_stub_for_person(pp, "p", "write_project_file_binary", &write("b.bin", "not base64!!"))
            .await.expect_err("bad base64");
        assert!(err.contains("base64"), "{err}");
    }

    /// `download_asset` accepts a file name and request headers, sends moss's
    /// `User-Agent`, and refuses a name that is not a single path segment.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn download_asset_arm_honours_filename_headers_and_user_agent() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let pp = proj.path().to_str().unwrap();
        let mut server = mockito::Server::new_async().await;
        let m = server.mock("GET", "/iiif/1/full/max/0/default.jpg")
            .match_header("user-agent", mockito::Matcher::Regex("^moss/".into()))
            .match_header("x-probe", "1")
            .with_status(200).with_header("content-type", "image/jpeg").with_body(vec![9u8, 8, 7])
            .create_async().await;
        let args = |name: Option<&str>| serde_json::json!({
            "url": format!("{}/iiif/1/full/max/0/default.jpg", server.url()),
            "targetDir": "maps", "timeoutMs": 5000,
            "filename": name, "headers": { "X-Probe": "1" }
        }).to_string();

        let reply = dispatch_command_test_stub_for_person(pp, "p", "download_asset", &args(Some("sheet-1.jpg"))).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["actual_path"], "maps/sheet-1.jpg");
        assert_eq!(std::fs::read(proj.path().join("maps/sheet-1.jpg")).unwrap(), vec![9u8, 8, 7]);
        m.assert_async().await;

        for bad in ["../x.jpg", "a/b.jpg", "a\\b.jpg", ""] {
            let err = dispatch_command_test_stub_for_person(pp, "p", "download_asset", &args(Some(bad))).await.expect_err(bad);
            assert!(err.contains("Invalid file name"), "{bad}: {err}");
        }
    }

    /// A download streams to disk. One byte past its limit, or a body cut off
    /// midway, is refused, and the file already there is kept as it was.
    #[tokio::test]
    async fn a_failed_download_keeps_the_file_it_would_replace() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        std::fs::create_dir_all(proj.path().join("media")).unwrap();
        std::fs::write(proj.path().join("media/a.bin"), b"good").unwrap();
        let mut server = mockito::Server::new_async().await;
        server.mock("GET", "/big.bin").with_status(200).with_chunked_body(|w| {
            let chunk = vec![0u8; 1 << 20];
            for _ in 0..512 { std::io::Write::write_all(w, &chunk)?; }
            std::io::Write::write_all(w, &[0])
        }).create_async().await;
        server.mock("GET", "/cut.bin").with_status(200).with_chunked_body(|w| {
            std::io::Write::write_all(w, &[1u8; 4096])?;
            Err(std::io::Error::other("cut"))
        }).create_async().await;
        let args = |path: &str| serde_json::json!({ "url": format!("{}{path}", server.url()), "targetDir": "media", "filename": "a.bin", "timeoutMs": 60000 }).to_string();

        let pp = proj.path().to_str().unwrap();
        let err = dispatch_command_test_stub_for_person(pp, "p", "download_asset", &args("/big.bin")).await.unwrap_err();
        assert!(err.contains("at most 512 MB"), "{err}");
        dispatch_command_test_stub_for_person(pp, "p", "download_asset", &args("/cut.bin")).await.unwrap_err();
        assert_eq!(std::fs::read(proj.path().join("media/a.bin")).unwrap(), b"good");
        assert_eq!(std::fs::read_dir(proj.path().join("media")).unwrap().count(), 1, "no partial file is left");
    }

    /// `download_asset` writes only inside the site folder: a target directory
    /// that is absolute, names `.moss/`, or is a Windows drive path is refused
    /// before anything is fetched, and nothing lands outside.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn download_asset_arm_refuses_a_target_outside_the_site() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let container = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = container.path().join("site");
        let outside = container.path().join("outside");
        std::fs::create_dir_all(proj.join(".moss/plugins/other")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let pp = proj.to_str().unwrap();
        let mut server = mockito::Server::new_async().await;
        let m = server.mock("GET", "/payload").with_status(200).with_body("payload").expect(0).create_async().await;
        let args = |dir: &str| serde_json::json!({
            "url": format!("{}/payload", server.url()), "targetDir": dir, "timeoutMs": 5000, "filename": "x.txt"
        }).to_string();

        for bad in [outside.to_str().unwrap(), ".moss/plugins/other", "C:\\Users\\x"] {
            let err = dispatch_command_test_stub(pp, None, "download_asset", &args(bad)).await.expect_err(bad);
            assert!(!err.is_empty(), "{bad}");
        }
        assert!(!outside.join("x.txt").exists());
        assert!(!proj.join(".moss/plugins/other/x.txt").exists());
        m.assert_async().await;
    }

    /// Every plugin HTTP request carries moss's `User-Agent`, and a plugin
    /// header of the same name still wins.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_get_arm_sends_the_moss_user_agent() {
        let mut server = mockito::Server::new_async().await;
        let ua = server.mock("GET", "/ua")
            .match_header("user-agent", mockito::Matcher::Regex("^moss/".into()))
            .with_status(200).create_async().await;
        let own = server.mock("GET", "/own")
            .match_header("user-agent", "custom/1")
            .with_status(200).create_async().await;
        let get = |path: &str, headers: serde_json::Value| serde_json::json!({
            "url": format!("{}{}", server.url(), path), "headers": headers, "timeoutMs": 5000
        }).to_string();
        dispatch_command_test_stub_reaching_local("/tmp/none", "http_get", &get("/ua", serde_json::json!({}))).await.unwrap();
        dispatch_command_test_stub_reaching_local("/tmp/none", "http_get", &get("/own", serde_json::json!({ "User-Agent": "custom/1" }))).await.unwrap();
        ua.assert_async().await;
        own.assert_async().await;
    }

    /// Outside a run the person started, a host call is confined; an
    /// unidentified caller is refused even its comment-data path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plugin_with_no_run_of_the_persons_writes_only_inside_moss() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let container = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let pp = container.path().to_str().unwrap();
        let state = HostState::standalone();
        let host = |plugin| EngineHost { app: None, state: &state, project_path: pp, plugin, asked_by: Some(AskedBy::Person), declared: None };
        let write = serde_json::json!({ "relativePath": "notes/a.md", "data": "# a" }).to_string();
        let run = serde_json::json!({ "binaryPath": "git", "args": [], "workingDir": pp }).to_string();

        let err = dispatch_command_core(host(Some("p")), "write_project_file", &write).await.expect_err("no run of the person's");
        assert!(err.contains("p writes into the folder only when you start it yourself"), "{err}");
        let err = dispatch_command_core(host(Some("p")), "execute_binary", &run).await.expect_err("no run of the person's");
        assert!(err.contains("p runs programs only when you start it yourself"), "{err}");
        let social = serde_json::json!({ "relativePath": ".moss/data/social/p.json", "data": "{}" }).to_string();
        for args in [&write, &social] {
            let err = dispatch_command_core(host(None), "write_project_file", args).await.expect_err("unidentified");
            assert!(err.contains("could not identify"), "{err}");
        }
        let err = dispatch_command_core(host(None), "execute_binary", &run).await.expect_err("unidentified");
        assert!(err.contains("could not identify"), "{err}");
        assert!(!container.path().join("notes").exists() && !container.path().join(".moss").exists());

        let _run = state.hooks.begin_person_run("p").await;
        let from_a_build = EngineHost { asked_by: Some(AskedBy::Build), ..host(Some("p")) };
        let err = dispatch_command_core(from_a_build, "write_project_file", &write).await.expect_err("a build's Context");
        assert!(err.contains("only when you start it yourself"), "{err}");
        dispatch_command_core(host(Some("p")), "write_project_file", &write).await.unwrap();
        assert_eq!(std::fs::read_to_string(container.path().join("notes/a.md")).unwrap(), "# a");
    }

    /// B.8b RED: file arms roundtrip against a temp project (write→exists→read for
    /// plugin storage; write→read for project file; list_project_files includes the
    /// written file).
    /// A host that no person's run stands behind never reaches the folder, the
    /// test stub included.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_plain_stub_confines_a_plugins_folder_write() {
        let proj = tempfile::TempDir::new().unwrap();
        let pp = proj.path().to_str().unwrap();
        let wp = serde_json::json!({ "relativePath": "notes/a.md", "data": "# a" }).to_string();
        let err = dispatch_command_test_stub(pp, Some("p"), "write_project_file", &wp).await.expect_err("confined");
        assert!(err.contains("only when you start it yourself"), "{err}");
        assert!(!proj.path().join("notes/a.md").exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn file_arms_roundtrip_against_a_temp_project() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        // tempfile::TempDir creates dirs with names starting with `.tmp` on macOS,
        // which list_project_files_impl's `!name.starts_with('.')` filter would reject
        // as the root. Use the TempDir as a container and create a non-hidden subdir.
        let container = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj_dir = container.path().join("project");
        std::fs::create_dir_all(&proj_dir).unwrap();
        let pp = proj_dir.to_str().unwrap();
        // write → exists → read (plugin storage), write → read (project), list.
        let w = serde_json::json!({ "pluginName": "matters", "relativePath": "auth.json", "content": "{\"id\":\"x\"}" }).to_string();
        dispatch_command_test_stub(pp, Some("matters"), "write_plugin_file", &w).await.unwrap();
        let e = serde_json::json!({ "pluginName": "matters", "relativePath": "auth.json" }).to_string();
        assert_eq!(dispatch_command_test_stub(pp, Some("matters"), "plugin_file_exists", &e).await.unwrap(), "true");
        let r = dispatch_command_test_stub(pp, Some("matters"), "read_plugin_file", &e).await.unwrap();
        assert!(r.contains("\\\"id\\\"") || r.contains("id"), "got {r}");
        let wp = serde_json::json!({ "relativePath": "notes/a.md", "data": "# hi" }).to_string();
        dispatch_command_test_stub_for_person(pp, "p", "write_project_file", &wp).await.unwrap();
        let rp = serde_json::json!({ "relativePath": "notes/a.md" }).to_string();
        assert!(dispatch_command_test_stub(pp, None, "read_project_file", &rp).await.unwrap().contains("# hi"));
        let listed = dispatch_command_test_stub(pp, None, "list_project_files", "null").await.unwrap();
        assert!(listed.contains("notes/a.md"), "got {listed}");
    }

    /// "One plugin must not read or write another's keys, storage, or
    /// identity". The plugin-storage arms take their scope from the dispatch
    /// seam, so naming a victim in the args reaches the CALLER's own storage,
    /// never the victim's. Before this was enforced, `read_plugin_file` with
    /// `pluginName: "matters"` returned matters' stored `auth.json` — its
    /// access token — to any approved plugin that asked.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_storage_is_scoped_to_the_calling_plugin() {
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let container = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj_dir = container.path().join("project");
        std::fs::create_dir_all(&proj_dir).unwrap();
        let pp = proj_dir.to_str().unwrap();

        // The victim stores a secret in its own plugin storage.
        let w = serde_json::json!({ "relativePath": "auth.json", "content": "{\"token\":\"SECRET\"}" }).to_string();
        dispatch_command_test_stub(pp, Some("matters"), "write_plugin_file", &w).await.unwrap();

        // An attacker names the victim. The read is scoped to the attacker, so
        // the victim's file is simply not there.
        let e = serde_json::json!({ "pluginName": "matters", "relativePath": "auth.json" }).to_string();
        assert_eq!(
            dispatch_command_test_stub(pp, Some("attacker"), "plugin_file_exists", &e).await.unwrap(),
            "false",
            "naming another plugin must not reach its storage",
        );
        let read = dispatch_command_test_stub(pp, Some("attacker"), "read_plugin_file", &e).await;
        assert!(
            read.as_ref().map_or(true, |r| !r.contains("SECRET")),
            "attacker read the victim's token: {read:?}",
        );

        // And a write under the victim's name lands in the attacker's own
        // storage, leaving the victim's file untouched.
        let w2 = serde_json::json!({ "pluginName": "matters", "relativePath": "auth.json", "content": "OVERWRITTEN" }).to_string();
        dispatch_command_test_stub(pp, Some("attacker"), "write_plugin_file", &w2).await.unwrap();
        let victim = dispatch_command_test_stub(pp, Some("matters"), "read_plugin_file", &e).await.unwrap();
        assert!(victim.contains("SECRET"), "victim's file was overwritten: {victim}");

        // An unidentified caller is refused rather than defaulting to a shared scope.
        let err = dispatch_command_test_stub(pp, None, "read_plugin_file", &e).await
            .expect_err("no caller identity must Err");
        assert!(err.contains("could not identify the calling plugin"), "got {err}");
    }

    /// Regression: the `read_site_file` arm must be bridged into the plugin engine
    /// (it was missing → "unknown command: read_site_file" → syndication byte-upload
    /// silently fell back to deployed URLs). Reads built-site bytes from current_ptr.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn read_site_file_arm_reads_built_site_bytes() {
        use base64::Engine as _;
        std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let container = tempfile::TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-tmp")).unwrap();
        let proj = container.path().join("project");
        // Write a fake built asset under .moss/build.nosync/current (what current_ptr resolves to).
        let current = crate::infra::moss_paths::MossPaths::new(&proj).current_ptr();
        std::fs::create_dir_all(current.join("image/photography")).unwrap();
        std::fs::write(current.join("image/photography/x.bin"), [0xFFu8, 0xD8, 0x00, 0x10]).unwrap();

        let args = serde_json::json!({ "relativePath": "image/photography/x.bin" }).to_string();
        let reply = dispatch_command_test_stub(proj.to_str().unwrap(), None, "read_site_file", &args)
            .await
            .expect("read_site_file must be bridged (not 'unknown command')");
        // reply is a JSON string of the base64 contents.
        let b64: String = serde_json::from_str(&reply).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert_eq!(bytes, vec![0xFFu8, 0xD8, 0x00, 0x10]);
    }

    /// Regression: the `http_post_multipart` arm must be bridged (byte-upload to a
    /// syndication target's singleFileUpload). Roundtrips a multipart POST through a
    /// mock server and asserts the body is multipart/form-data carrying the file part.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_post_multipart_arm_roundtrips() {
        use base64::Engine as _;
        let mut server = mockito::Server::new_async().await;
        let m = server.mock("POST", "/graphql")
            .match_header("content-type", mockito::Matcher::Regex("multipart/form-data.*boundary=.*".into()))
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::Regex("name=\"operations\"".into()),
                mockito::Matcher::Regex("filename=\"photo.jpg\"".into()),
            ]))
            .with_status(200).with_header("content-type", "application/json")
            .with_body(r#"{"data":{"singleFileUpload":{"id":"a","path":"https://cdn/x.jpg"}}}"#)
            .create_async().await;

        let img_b64 = base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3]);
        let args = serde_json::json!({
            "url": format!("{}/graphql", server.url()),
            "textFields": [{ "name": "operations", "value": "{\"query\":\"q\"}" }],
            "files": [{ "field": "0", "filename": "photo.jpg", "contentType": "image/jpeg", "contentBase64": img_b64 }],
            "headers": { "x-access-token": "tok", "apollo-require-preflight": "true" },
            "timeoutMs": 5000
        }).to_string();
        let reply = dispatch_command_test_stub_reaching_local("/tmp/none", "http_post_multipart", &args)
            .await
            .expect("http_post_multipart must be bridged (not 'unknown command')");
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true));
        assert!(v.get("body_base64").is_some());
        m.assert_async().await;
    }

    /// Regression (credential leak): ALL three cookie-command arms — get / set /
    /// **clear** — must be bridged into the plugin engine. `clear_plugin_cookies`
    /// was registered only in the WEBVIEW invoke_handler (lib.rs) and NOT here, so
    /// the matters plugin's force-fresh-login `clearPluginCookies()` (which runs in
    /// this quickjs engine) hit "unknown command" and silently no-op'd — leaving the
    /// previous account's session live so a fresh folder auto-logged-in as the wrong
    /// user. These commands all need an AppHandle (absent in the test stub), so a
    /// REGISTERED command fails with the need_app message, while an UNREGISTERED one
    /// fails with "unknown command: X". Assert the latter can never happen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cookie_command_arms_are_all_bridged() {
        for cmd in ["get_plugin_cookie", "set_plugin_cookie", "clear_plugin_cookies"] {
            // A bound caller: the arms resolve their scope from the seam, so an
            // unidentified one is refused before it ever reaches `need_app`.
            let args = serde_json::json!({ "cookies": [] }).to_string();
            let err = dispatch_command_test_stub("/tmp/none", Some("matters"), cmd, &args)
                .await
                .expect_err("cookie commands need an app; the test stub has none, so they must Err");
            assert!(
                !err.contains("unknown command"),
                "{cmd} must be bridged into the plugin engine (not 'unknown command'), got: {err}"
            );
            assert!(
                err.contains("requires an app handle"),
                "{cmd} should reach need_app (proving it's registered), got: {err}"
            );
        }
    }

    /// Every command the moss-api SDK can `invoke` must have a dispatch arm.
    /// The cookie test above guards one incident; this sweep guards the CLASS:
    /// `return_to_editor` was a Tauri command but never an engine arm, so from
    /// the day QuickJS became the default engine (2026-06-13) every plugin call
    /// threw `unknown command` — which matters' `.catch(() => {})` swallowed,
    /// leaving the user an empty action panel on login cancel. An arm may
    /// refuse for a missing app, args, grant or reach — it must never be
    /// unknown. The list mirrors `grep -r "\.invoke" packages/moss-api/src`;
    /// `plugin_message` is absent because the host dispatch task intercepts it
    /// before this match (plugin_engine.rs).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_sdk_command_is_bridged() {
        const SDK_COMMANDS: &[&str] = &[
            "browser_sign_in_start",
            "browser_sign_in_wait",
            "clear_plugin_cookies",
            "close_action_panel",
            "download_asset",
            "execute_binary",
            "fetch_url",
            "fill_browser_panel",
            "get_plugin_cookie",
            "get_plugin_env_var",
            "html_to_markdown",
            "http_get",
            "http_post",
            "http_post_multipart",
            "key_get_or_create",
            "key_list",
            "key_sign",
            "list_project_files",
            "list_project_tree",
            "list_site_files_with_sizes",
            "open_action_panel",
            "open_system_browser",
            "plugin_file_exists",
            "read_plugin_file",
            "read_project_file",
            "read_site_file",
            "report_plugin_task_lifecycle_command",
            "return_to_editor",
            "set_action_panel_html",
            "set_plugin_cookie",
            "write_plugin_file",
            "write_project_file",
            "write_project_file_binary",
        ];
        for cmd in SDK_COMMANDS {
            // Empty args: every arm parses args (or checks app/grants/reach)
            // before doing work, so nothing executes — the reply just proves
            // the arm exists.
            if let Err(err) = dispatch_command_test_stub("/tmp/none", None, cmd, "{}").await {
                assert!(
                    !err.contains("unknown command"),
                    "{cmd} is invoked by the moss-api SDK but has no engine arm: {err}"
                );
            }
        }
    }

    /// `get_plugin_env_var` resolves the environment from the project path,
    /// so it ANSWERS without an app instead of erroring — matters' process hook
    /// reads `MOSS_MATTERS_TEST_PROFILE` and `MOSS_MATTERS_DOMAIN` on the build
    /// path. The allow-list still holds: an unlisted var reads `null` even when it
    /// is set in the process environment.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_plugin_env_var_answers_without_an_app() {
        let args = serde_json::json!({ "name": "MOSS_MATTERS_TEST_PROFILE" }).to_string();
        let reply = dispatch_command_test_stub("/tmp/none", None, "get_plugin_env_var", &args)
            .await
            .expect("allow-listed var must resolve without an app");
        // Unset in the test process → JSON null, NOT an error.
        assert_eq!(reply, "null", "expected a null reply for an unset var, got: {reply}");

        let args = serde_json::json!({ "name": "PATH" }).to_string();
        let reply = dispatch_command_test_stub("/tmp/none", None, "get_plugin_env_var", &args)
            .await
            .expect("a refused var replies null, it does not Err");
        assert_eq!(reply, "null", "the allow-list must still refuse PATH; got: {reply}");
    }

    // =========================================================================
    // B.8c tests: execute_binary parsing + app-gated arm errors
    // =========================================================================

    /// B.8c: parse_execute_binary_args — happy path with all fields.
    #[test]
    fn parse_execute_binary_args_happy_path() {
        let v = serde_json::json!({
            "binaryPath": "/usr/bin/git",
            "args": ["status", "--short"],
            "workingDir": "/proj",
            "env": { "GIT_TERMINAL_PROMPT": "0" },
            "timeoutMs": 10000,
            "stdinData": "input"
        });
        let p = parse_execute_binary_args(&v).unwrap();
        assert_eq!(p.binary_path, "/usr/bin/git");
        assert_eq!(p.args, vec!["status".to_string(), "--short".to_string()]);
        assert_eq!(p.working_dir, Some("/proj".to_string()));
        assert_eq!(
            p.env_vars.as_ref().unwrap().get("GIT_TERMINAL_PROMPT"),
            Some(&"0".to_string())
        );
        assert_eq!(p.timeout_ms, Some(10000));
        assert_eq!(p.stdin_data, Some("input".to_string()));
    }

    /// B.8c: parse_execute_binary_args — missing binaryPath returns Err.
    #[test]
    fn parse_execute_binary_args_missing_binary_path() {
        let v = serde_json::json!({ "args": [] });
        let err = parse_execute_binary_args(&v).expect_err("missing binaryPath must Err");
        assert!(
            err.contains("binaryPath"),
            "error must mention binaryPath; got: {err}"
        );
    }

    /// B.8c: parse_execute_binary_args — streamId present is parsed-and-dropped
    /// (with a warn), parse still succeeds.
    #[test]
    fn parse_execute_binary_args_stream_id_is_dropped() {
        let v = serde_json::json!({
            "binaryPath": "echo",
            "args": [],
            "streamId": "some-uuid-here"
        });
        let p = parse_execute_binary_args(&v).expect("parse must succeed even with streamId");
        assert_eq!(p.binary_path, "echo");
    }

    use crate::plugins::manager::contract::ScriptedPerson;

    /// What the system browser was asked to open, in order.
    fn browser(desk: &ScriptedPerson) -> Vec<String> {
        desk.seen("browser-opened").iter().map(|b| b["url"].as_str().unwrap().to_string()).collect()
    }

    /// The `url` the last `open_action_panel` received: `None` if none opened, `Some(None)` for "no page".
    fn panel(desk: &ScriptedPerson) -> Option<Option<String>> {
        desk.seen("panel-opened").last().map(|p| p["url"].as_str().map(str::to_string))
    }

    /// The caller holds a person run for "mastodon" on `state`.
    async fn browser_call(desk: &ScriptedPerson, state: &HostState, cmd: &str, args: serde_json::Value) -> std::result::Result<String, String> {
        let host = EngineHost { app: Some(desk), state, project_path: "/tmp/none", plugin: Some("mastodon"), asked_by: Some(AskedBy::Person), declared: None };
        dispatch_command_core(host, cmd, &args.to_string()).await
    }

    /// Each way a plugin reaches the browser can carry data out in a URL, so a
    /// build's run, or a call outside any run, reaches none of them.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_browser_is_reached_only_from_a_run_the_person_started() {
        let desk = ScriptedPerson::default();
        let state = HostState::standalone();
        let _build = state.hooks.try_begin_build_run("leaky");
        let args = serde_json::json!({ "url": "https://example.com/?d=secret", "fill": {} }).to_string();
        for asked_by in [Some(AskedBy::Build), None] {
            for cmd in ["open_system_browser", "browser_sign_in_start", "browser_sign_in_wait", "fill_browser_panel"] {
                let host = EngineHost { app: Some(&desk), state: &state, project_path: "/tmp/none", plugin: Some("leaky"), asked_by, declared: None };
                let err = dispatch_command_core(host, cmd, &args).await.unwrap_err();
                assert!(err.contains("only when you start it yourself"), "{cmd}: {err}");
            }
        }
        assert!(browser(&desk).is_empty(), "nothing may reach the browser: {:?}", browser(&desk));
    }

    /// A plugin can hand the system browser a web page and nothing else, on
    /// both ways in: the plain open and the sign-in.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_system_browser_opens_only_web_pages() {
        let (desk, state) = (ScriptedPerson::default(), HostState::standalone());
        let _run = state.hooks.begin_person_run("mastodon").await;
        for refused in ["file:///etc/passwd", "javascript:alert(1)", "moss://open?path=/"] {
            assert!(browser_call(&desk, &state, "open_system_browser", serde_json::json!({ "url": refused })).await.is_err(), "{refused}");
            browser_call(&desk, &state, "browser_sign_in_start", serde_json::json!({})).await.unwrap();
            let waited = browser_call(&desk, &state, "browser_sign_in_wait", serde_json::json!({ "url": refused })).await;
            assert!(waited.is_err(), "{refused}");
        }
        assert!(browser(&desk).is_empty(), "nothing may reach the browser: {:?}", browser(&desk));
        browser_call(&desk, &state, "open_system_browser", serde_json::json!({ "url": "https://example.com/a" })).await.unwrap();
        assert_eq!(browser(&desk), vec!["https://example.com/a".to_string()]);
    }

    /// The fill goes to the plugin that called, whatever name it passes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn fill_browser_panel_reaches_the_app_as_the_calling_plugin() {
        let (desk, state) = (ScriptedPerson::default(), HostState::standalone());
        let _run = state.hooks.begin_person_run("mastodon").await;
        let fill = serde_json::json!({ "editor": { "body": ".editor" }, "parts": [{ "html": "<p>a</p>" }] });
        let reply = browser_call(&desk, &state, "fill_browser_panel", serde_json::json!({ "pluginName": "matters", "fill": fill })).await.unwrap();
        assert_eq!(reply, r#"{"filled":true}"#);
        assert_eq!(desk.seen("panel-filled"), vec![serde_json::json!({ "plugin": "mastodon", "project": "/tmp/none", "fill": fill })]);
    }

    /// A plugin tells a cancel from a failure by this text alone, so the
    /// engine's rejection and moss-api's `SignInCancelled` must agree on it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_superseded_sign_in_rejects_with_the_text_moss_api_reads_as_cancelled() {
        use crate::plugins::runtime::browser_sign_in::CANCELLED;
        let (desk, state) = (std::sync::Arc::new(ScriptedPerson::default()), HostState::standalone());
        let _run = state.hooks.begin_person_run("mastodon").await;
        browser_call(&desk, &state, "browser_sign_in_start", serde_json::json!({})).await.unwrap();
        let waiting = {
            let (desk, state) = (desk.clone(), state.clone());
            tokio::spawn(async move { browser_call(&desk, &state, "browser_sign_in_wait", serde_json::json!({ "url": "https://example.com/authorize" })).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while browser(&desk).is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the sign-in never opened the browser");
        browser_call(&desk, &state, "browser_sign_in_start", serde_json::json!({})).await.unwrap();
        assert_eq!(waiting.await.unwrap(), Err(CANCELLED.to_string()));
        let moss_api = include_str!("../../../../packages/moss-api/src/utils/browser.ts");
        assert!(moss_api.contains(&format!("const CANCELLED = \"{CANCELLED}\";")), "moss-api's SignInCancelled reads a different text");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn signing_in_needs_the_app() {
        for cmd in ["browser_sign_in_start", "browser_sign_in_wait"] {
            let err = dispatch_command_test_stub("/tmp/none", Some("mastodon"), cmd, r#"{"url":"https://example.com"}"#).await.unwrap_err();
            assert_eq!(err, "Sign in from the moss app");
        }
    }

    /// Runs a plugin bundle whose only action is one `open_action_panel` invoke
    /// with `invoke_args` as its argument object, and returns what the desktop
    /// seam received.
    async fn open_panel_url_from_bundle(invoke_args: &str) -> Option<Option<String>> {
        let desk = std::sync::Arc::new(ScriptedPerson::default());
        let (engine, mut dispatch_rx) = crate::engine::QuickJsEngine::new(None);
        let desk2 = desk.clone();
        tokio::spawn(async move {
            while let Some(req) = dispatch_rx.recv().await {
                let host = EngineHost {
                    app: Some(desk2.as_ref()),
                    state: &HostState::standalone(),
                    project_path: "/tmp/none",
                    plugin: req.plugin.as_deref(),
                    asked_by: None,
                    declared: None,
                };
                let r = dispatch_command_core(host, &req.cmd, &req.args_json).await;
                let _ = req.reply.send(r);
            }
        });
        let bundle = format!(
            r#""use strict"; var P = (() => ({{
                async process() {{
                    await globalThis.__TAURI__.core.invoke('open_action_panel', {invoke_args});
                    return {{ ok: true }};
                }}
            }}))();"#
        );
        engine
            .dispatch_hook_raw("p", &bundle, "P", "process", "{}", "", "")
            .await
            .expect("bundle must run");
        engine.shutdown().await;
        panel(&desk)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_open_action_panel_with_null_url_reaches_the_seam_as_none() {
        let got = open_panel_url_from_bundle("{ url: null, belowTitlebar: true }").await;
        assert_eq!(got, Some(None), "null url must arrive as None");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_open_action_panel_without_url_key_reaches_the_seam_as_none() {
        let got = open_panel_url_from_bundle("{ belowTitlebar: true }").await;
        assert_eq!(got, Some(None), "missing url must arrive as None");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_action_panel_refuses_a_non_string_url_without_opening() {
        let desk = ScriptedPerson::default();
        let host = EngineHost {
            app: Some(&desk),
            state: &HostState::standalone(),
            project_path: "/tmp/none",
            plugin: Some("p"),
            asked_by: None,
            declared: None,
        };
        let err = dispatch_command_core(host, "open_action_panel", r#"{"url":42}"#)
            .await
            .expect_err("a numeric url must be refused");
        assert!(err.contains("must be a string or null"), "got: {err}");
        assert_eq!(panel(&desk), None, "the panel must not open");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_open_action_panel_with_url_string_reaches_the_seam_as_some() {
        let got = open_panel_url_from_bundle(
            "{ url: 'https://example.com/login', belowTitlebar: true }",
        )
        .await;
        assert_eq!(got, Some(Some("https://example.com/login".to_string())));
    }

    /// B.8c: app-needing arms via stub return structured Err mentioning "requires an
    /// app handle".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn app_needing_arms_return_structured_error_without_app() {
        // These are the arms that are app-only BY NATURE: they
        // drive windows and the system browser, which have no headless meaning.
        // The arms that merely *reached* the app for state or an event sink —
        // get_plugin_env_var, execute_binary, resolve_git_path — were ported and
        // now answer without one; see their own tests.
        //
        // `execute_binary` is deliberately NOT here: it is gated and the
        // grant check runs BEFORE anything else, so it refuses with the
        // capability message instead. That ordering is the point — deny first.
        // Its refusal is covered by the `grant_*` tests above.
        let cases: &[(&str, &str)] = &[
            ("open_action_panel", r#"{"url":"https://example.com"}"#),
            ("set_action_panel_html", r#"{"html":"<p>hi</p>"}"#),
            ("close_action_panel", "null"),
            ("open_system_browser", r#"{"url":"https://example.com"}"#),
            ("return_to_editor", "null"),
            ("fill_browser_panel", r#"{"fill":{}}"#),
        ];
        for (cmd, args_json) in cases {
            let err = dispatch_command_test_stub("/tmp/none", None, cmd, args_json)
                .await
                .expect_err(&format!("{cmd} without app must Err"));
            assert!(
                err.contains("requires an app handle"),
                "{cmd}: expected 'requires an app handle'; got: {err}"
            );
        }
    }

    /// The lifecycle arm is what matters' `process` hook reaches through
    /// `startTask()`, so a headless build depends on it working with NO app. It
    /// spawns a real task in the injected registry; the descriptor lookup — the
    /// only genuinely app-shaped part — is simply skipped.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn task_lifecycle_arm_spawns_into_the_injected_registry_without_an_app() {
        let state = HostState::standalone();
        let host = EngineHost {
            app: None,
            state: &state,
            project_path: "/tmp/none",
            plugin: Some("matters"),
            asked_by: None,
            declared: None,
        };
        let reply = dispatch_command_core(
            host,
            "report_plugin_task_lifecycle_command",
            r#"{"pluginName":"matters","hook":"process","trigger":"background",
                "lifecycle":{"type":"started","label":"Importing from Matters",
                "has_progress":false,"cancellable":false}}"#,
        )
        .await
        .expect("the lifecycle arm must work headless — matters' process hook needs it");

        let id: crate::tasks::TaskId = serde_json::from_str(&reply).unwrap();
        // A background process hook routes to the quiet Workspace surface.
        let tasks = state
            .tasks
            .tasks(&crate::tasks::WindowId::from("main"), crate::tasks::TaskScope::Workspace);
        assert!(
            tasks.iter().any(|t| t.id == id),
            "the minted task must be live in the INJECTED registry, not a discarded one"
        );
    }

    /// B.8c: unknown command still errors with the command name in the message.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_command_errors_with_command_name() {
        let err = dispatch_command_test_stub("/tmp/none", None, "nonexistent_command_xyz", "null")
            .await
            .expect_err("unknown command must Err");
        assert!(
            err.contains("nonexistent_command_xyz"),
            "error must mention the command name; got: {err}"
        );
    }

    // ── the lifecycle bridge (Task 2 watchdog) ──────────────────────────────

    /// Every lifecycle report reaches the watchdog, including the one that
    /// carries no task id, and a question marks the hook as waiting only while
    /// someone could answer it.
    ///
    /// `startTask()` invokes `started` with `taskId: undefined` (moss-api's
    /// `messaging.ts`) and the registry mints the id; every later transition
    /// carries one. So `None` here IS the first `Started` — it records no
    /// waiter, but it is the hook speaking, and the report is what restarts
    /// the 60 s clock. Skipping it killed webview plugins that were quiet and
    /// then started work. Nothing else exercised the wiring between a
    /// lifecycle report and the watchdog, and that regression sat in exactly
    /// this gap.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn every_lifecycle_report_reaches_the_watchdog() {
        struct NoDesktop;
        #[async_trait::async_trait]
        impl crate::engine::app_host::AppHost for NoDesktop {
            // The lifecycle arm never reaches the desktop; a future arm that
            // starts to fails here loudly instead of silently succeeding.
            async fn get_plugin_cookies(&self, _: &str, _: &str) -> std::result::Result<String, String> { unreachable!() }
            async fn set_plugin_cookies(&self, _: &str, _: &str, _: serde_json::Value) -> std::result::Result<(), String> { unreachable!() }
            async fn clear_plugin_cookies(&self, _: &str, _: &str) -> std::result::Result<(), String> { unreachable!() }
            async fn open_action_panel(&self, _: Option<String>, _: Option<String>, _: bool, _: Option<String>) -> std::result::Result<(), String> { unreachable!() }
            async fn set_action_panel_html(&self, _: String, _: Option<String>, _: Option<String>) -> std::result::Result<(), String> { unreachable!() }
            async fn close_action_panel(&self, _: Option<String>) -> std::result::Result<(), String> { unreachable!() }
            async fn open_system_browser(&self, _: String) -> std::result::Result<(), String> { unreachable!() }
            async fn return_to_editor(&self) -> std::result::Result<(), String> { unreachable!() }
            async fn fill_browser_panel(&self, _: &str, _: &str, _: serde_json::Value) -> std::result::Result<serde_json::Value, String> { unreachable!() }
            async fn prompt_credential(&self, _: &str, _: &str, _: &str, _: Option<String>) -> std::result::Result<Option<String>, String> { unreachable!() }
            fn listen(&self, _: String, _: Box<dyn Fn(&str) + Send + Sync + 'static>) -> crate::engine::app_host::ListenerId { unreachable!() }
            fn unlisten(&self, _: crate::engine::app_host::ListenerId) { unreachable!() }
            fn emit_plugin_event(&self, _: &str, _: Option<String>) { unreachable!() }
        }

        let report = |lifecycle: &str, task_id: &str| {
            format!(
                r#"{{"pluginName":"github","hook":"deploy","trigger":"settings_manual",
                     "taskId":{task_id},"lifecycle":{lifecycle}}}"#
            )
        };
        async fn call(
            state: &HostState,
            app: Option<&dyn crate::engine::app_host::AppHost>,
            args: String,
        ) -> std::result::Result<String, String> {
            dispatch_command_core(
                EngineHost {
                    app,
                    state,
                    project_path: "/tmp/none",
                    plugin: Some("github"),
                    asked_by: None,
                    declared: None,
                },
                "report_plugin_task_lifecycle_command",
                &args,
            )
            .await
        }
        let started = r#"{"type":"started","label":"Publishing","has_progress":false,"cancellable":false}"#;
        let awaiting = r#"{"type":"awaiting","directive":"name the repository","escape":"cancel"}"#;
        let progress = r#"{"type":"progress","fraction":null,"message":null}"#;

        // Attended: the question holds the watchdog off, the answer arms it again.
        let state = HostState::standalone();
        let desktop = NoDesktop;
        state.hooks.register_executing_hook("github", "deploy");
        let before = state.hooks.get_last_activity("github", "deploy");
        let reply = call(&state, Some(&desktop), report(started, "null"))
            .await
            .expect("started must be accepted");
        let id: crate::tasks::TaskId =
            serde_json::from_str(&reply).expect("started replies with the minted id");
        assert!(
            state.hooks.get_last_activity("github", "deploy") > before,
            "the first Started carries no task id and must still restamp the clock"
        );
        assert!(!state.hooks.is_hook_awaiting("github", "deploy"));
        call(&state, Some(&desktop), report(awaiting, &id.0.to_string()))
            .await
            .expect("awaiting must be accepted");
        assert!(state.hooks.is_hook_awaiting("github", "deploy"), "a question holds the watchdog off");
        call(&state, Some(&desktop), report(progress, &id.0.to_string()))
            .await
            .expect("progress must be accepted");
        assert!(!state.hooks.is_hook_awaiting("github", "deploy"), "the answer arms it again");

        // Unattended: nobody can answer, so the question must not park the hook.
        let headless = HostState::standalone();
        headless.hooks.register_executing_hook("github", "deploy");
        let reply = call(&headless, None, report(started, "null")).await.unwrap();
        let id: crate::tasks::TaskId = serde_json::from_str(&reply).unwrap();
        call(&headless, None, report(awaiting, &id.0.to_string())).await.unwrap();
        assert!(
            !headless.hooks.is_hook_awaiting("github", "deploy"),
            "headless, an awaiting hook must stay killable — a CI build cannot wait on a prompt"
        );
    }

    #[tokio::test]
    async fn a_plugin_reaches_only_the_hosts_it_declares() {
        let mut server = mockito::Server::new_async().await;
        let port = server.socket_address().port();
        let _ok = server.mock("GET", "/x").with_status(200).create_async().await;
        let _bounce = server.mock("GET", "/bounce").with_status(302)
            .with_header("location", &format!("http://localhost:{port}/x")).create_async().await;
        let get = |path: &str| serde_json::json!({ "url": format!("{}{path}", server.url()) }).to_string();
        for cmd in ["fetch_url", "http_get"] {
            let err = dispatch_command_test_stub_declaring("/tmp/none", "social", &["api.example.com"], cmd, &get("/x")).await.unwrap_err();
            assert_eq!(err, "social tried to reach 127.0.0.1, which it does not list in its manifest.", "{cmd}");
            let err = dispatch_command_test_stub_reaching_local("/tmp/none", cmd, &get("/bounce")).await.unwrap_err();
            assert_eq!(err, "local tried to reach localhost, which it does not list in its manifest.", "{cmd}");
            let err = dispatch_command_test_stub_asked_by(AskedBy::Person, "/tmp/none", "old", None, cmd, &get("/x")).await.unwrap_err();
            assert!(err.contains("old does not list the hosts it contacts"), "{cmd}: {err}");
        }
        let post = serde_json::json!({ "url": format!("{}/x", server.url()), "body": "{}" }).to_string();
        let err = dispatch_command_test_stub_declaring("/tmp/none", "social", &["api.example.com"], "http_post", &post).await.unwrap_err();
        assert!(err.contains("which it does not list in its manifest"), "{err}");
        let err = dispatch_command_test_stub_declaring("/tmp/none", "social", &["api.example.com"], "fetch", &get("/x")).await.unwrap_err();
        assert_eq!(err, "social tried to reach 127.0.0.1, which it does not list in its manifest.");
        let elsewhere = serde_json::json!({ "url": format!("http://localhost:{port}/x"), "targetDir": "assets", "timeoutMs": 5000 }).to_string();
        let err = dispatch_command_test_stub_for_person("/tmp/none", "p", "download_asset", &elsewhere).await.unwrap_err();
        assert_eq!(err, "p tried to reach localhost, which it does not list in its manifest.");
    }

    #[tokio::test]
    async fn a_build_reaches_no_network() {
        let args = serde_json::json!({ "url": "https://api.example.com/x", "body": "{}" }).to_string();
        for cmd in ["fetch_url", "http_get", "http_post", "http_post_multipart", "fetch"] {
            let err = dispatch_command_test_stub_asked_by(AskedBy::Build, "/tmp/none", "social", Some(&["api.example.com"]), cmd, &args).await.unwrap_err();
            assert_eq!(err, "social tried to reach api.example.com during a build; plugins reach the network only when you Sync or Publish.", "{cmd}");
        }
    }

    #[tokio::test]
    async fn a_plugin_reads_at_most_32_mb_in_one_response() {
        let cap = 32usize << 20;
        let mut server = mockito::Server::new_async().await;
        let _at = server.mock("GET", "/at").with_body(vec![b'a'; cap]).create_async().await;
        let _over = server.mock("GET", "/over").with_body(vec![b'a'; cap + 1]).create_async().await;
        let get = |path: &str| serde_json::json!({ "url": format!("{}{path}", server.url()) }).to_string();
        for cmd in ["http_get", "fetch"] {
            dispatch_command_test_stub_reaching_local("/tmp/none", cmd, &get("/at")).await.unwrap();
            let err = dispatch_command_test_stub_reaching_local("/tmp/none", cmd, &get("/over")).await.unwrap_err();
            assert_eq!(err, "127.0.0.1 sent more than 32 MB; local can read at most 32 MB in one response.", "{cmd}");
        }
    }

    #[tokio::test]
    async fn a_plugin_declaring_its_site_reaches_only_the_address_the_person_typed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".moss")).unwrap();
        // allow:raw_write a test fixture in a temporary folder
        std::fs::write(dir.path().join(".moss/state.toml"), "[deployment]\nsite_url = \"https://chosen-by-plugin.example\"\n").unwrap();
        let pp = dir.path().to_str().unwrap();
        let state = HostState::standalone();
        let _run = state.hooks.begin_person_run("deploy").await;
        let declared = Declared { network: Some(crate::plugins::types::NetworkDecl { site: true, ..Default::default() }), ..Default::default() };
        let host = EngineHost { app: None, state: &state, project_path: pp, plugin: Some("deploy"), asked_by: Some(AskedBy::Person), declared: Some(&declared) };
        let reach = |u: &str| allowed_hosts(&host).check(&u.parse().unwrap());
        assert_eq!(reach("https://chosen-by-plugin.example/").unwrap_err(), "deploy tried to reach chosen-by-plugin.example, which it does not list in its manifest.");
        std::fs::write(dir.path().join(".moss/config.toml"), "[site]\nurl = \"https://blog.example.org\"\n").unwrap(); // allow:raw_write a test fixture in a temporary folder
        assert_eq!(reach("https://blog.example.org/"), Ok(()));
        assert!(reach("https://chosen-by-plugin.example/").is_err());
    }

    #[tokio::test]
    async fn a_plugin_cannot_write_the_settings_that_grant_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let pp = dir.path().to_str().unwrap();
        let w = serde_json::json!({ "relativePath": "config.toml", "content": "server = \"evil.example\"" }).to_string();
        let err = dispatch_command_test_stub(pp, Some("social"), "write_plugin_file", &w).await.unwrap_err();
        assert!(err.contains("cannot write config.toml"), "{err}");
    }
}
