//! `moss-contract`: run a plugin's own bundle in this engine with a scripted
//! person in place of the desktop, and report what happened as JSON.
//!
//! A plugin's unit tests fake moss, so they prove the plugin agrees with its own
//! fake. This runs the real thing: the manifest's secret custody, the hook
//! watchdog, the host functions, a file secret store under `$HOME`. Only the
//! desktop is replaced, by [`ScriptedPerson`], which answers pages, pop-ups,
//! the system browser and credential prompts the way a script says a person
//! would, and as slowly. The plugin's warnings and errors, a refused secret
//! write among them, are reported with what the person saw.
//!
//! Built only with the `contract` feature; no shipped binary carries it.

use crate::plugins::adapter_host::AdapterHost;
use super::{HookWatchdog, PluginManager};
use crate::plugins::types::{Plugin, ProcessContext, ProjectInfo, TriggerContext};
use crate::engine::app_host::{AppHost, ListenerId};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

pub const USAGE: &str = "Usage: moss-contract <folder> <plugin> <login|logout|account>... [--person <script.json>] [--watchdog-secs N]";

/// What appeared for the person to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum On {
    Page,
    Panel,
    Browser,
    Event,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "lowercase", deny_unknown_fields)]
pub enum Do {
    /// Send the plugin an event, as a page's button does.
    Emit {
        event: String,
        #[serde(default)]
        payload: Value,
    },
    /// Fetch the URL, following redirects, as a browser reaching a loopback
    /// callback. With `via`, fetch `via` + the URL percent-encoded instead: a
    /// page that does the provider's own sign-in for the person and redirects on.
    Follow {
        #[serde(default)]
        via: Option<String>,
    },
}

/// One thing the person does when something appears: `matching` is a substring
/// of its HTML, URL or event name, and `""` matches anything. Each step acts once.
/// `Step` cannot deny unknown fields itself, as it flattens `Do`; `Do` does.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub on: On,
    pub matching: String,
    #[serde(default)]
    pub after_ms: u64,
    #[serde(flatten)]
    pub act: Do,
}

/// What the person types when moss asks for a credential whose key contains `matching`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub matching: String,
    #[serde(default)]
    pub after_ms: u64,
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub answers: Vec<Answer>,
    /// Cookies already in a plugin's jar, as a web-session channel's sign-in
    /// page would have left them.
    #[serde(default)]
    pub cookies: HashMap<String, Value>,
}

/// A live recorder's records and start, as the logger holds it.
type Sink = (Weak<Mutex<Vec<Value>>>, Instant);

/// Every live recorder, for the process-wide logger to write into.
static SINKS: Mutex<Vec<Sink>> = Mutex::new(Vec::new());

/// Adds a `plugin` or `plugin::*` warning or error to every live recorder. The
/// crate's tests install one logger for the whole process, which calls this.
pub(crate) fn forward(r: &log::Record<'_>) {
    let target = r.target();
    if !(target == "plugin" || target.starts_with("plugin::")) || r.level() > log::Level::Warn {
        return;
    }
    let mut sinks = SINKS.lock().unwrap();
    sinks.retain(|(seen, _)| seen.strong_count() > 0);
    for (seen, started) in sinks.iter() {
        if let Some(seen) = seen.upgrade() {
            Recorder { seen, started: *started }.note("log", json!({ "level": r.level().as_str(), "target": target, "message": r.args().to_string() }));
        }
    }
}

struct PluginLog;

impl log::Log for PluginLog {
    fn enabled(&self, m: &log::Metadata<'_>) -> bool {
        m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record<'_>) {
        forward(r);
    }
    fn flush(&self) {}
}

/// The binary's logger. Without it every plugin warning is lost and a run
/// reports nothing refused, so failing to install it is a setup error.
fn install_logger() -> Result<(), String> {
    log::set_logger(&PluginLog).map_err(|_| "another logger is installed; plugin warnings would go unreported".to_string())?;
    log::set_max_level(log::LevelFilter::Warn);
    Ok(())
}

/// What the person saw and the plugin logged, timed from the person's arrival.
#[derive(Clone)]
struct Recorder {
    seen: Arc<Mutex<Vec<Value>>>,
    started: Instant,
}

impl Recorder {
    fn new() -> Self {
        #[cfg(test)]
        crate::build::cache::tests::install_capture();
        let recorder = Self { seen: Arc::default(), started: Instant::now() };
        SINKS.lock().unwrap().push((Arc::downgrade(&recorder.seen), recorder.started));
        recorder
    }

    fn note(&self, what: &str, detail: Value) {
        self.seen.lock().unwrap().push(json!({ "at_ms": self.started.elapsed().as_millis() as u64, "what": what, "detail": detail }));
    }
}

type Handler = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// The first unused entry `hit` accepts, taken so it acts once.
fn take_first<T>(list: &Mutex<Vec<Option<T>>>, hit: impl Fn(&T) -> bool) -> Option<T> {
    list.lock().unwrap().iter_mut().find(|s| s.as_ref().is_some_and(&hit))?.take()
}

/// The desktop, played by a script. Records everything the plugin asked of it.
pub struct ScriptedPerson {
    steps: Mutex<Vec<Option<Step>>>,
    answers: Mutex<Vec<Option<Answer>>>,
    listeners: Arc<Mutex<HashMap<ListenerId, (String, Handler)>>>,
    next_id: AtomicU32,
    cookies: Mutex<HashMap<String, Value>>,
    record: Recorder,
}

impl Default for ScriptedPerson {
    fn default() -> Self {
        Self::new(Script::default())
    }
}

impl ScriptedPerson {
    pub fn new(script: Script) -> Self {
        Self {
            steps: Mutex::new(script.steps.into_iter().map(Some).collect()),
            answers: Mutex::new(script.answers.into_iter().map(Some).collect()),
            listeners: Arc::default(),
            next_id: AtomicU32::new(1),
            cookies: Mutex::new(script.cookies),
            record: Recorder::new(),
        }
    }

    /// Everything seen since the last call, oldest first.
    pub fn take_seen(&self) -> Vec<Value> {
        std::mem::take(&mut *self.record.seen.lock().unwrap())
    }

    /// The details of every record named `what` so far, oldest first.
    pub fn seen(&self, what: &str) -> Vec<Value> {
        self.record.seen.lock().unwrap().iter().filter(|s| s["what"] == what).map(|s| s["detail"].clone()).collect()
    }

    /// Act on the first unused step for `on`, after its delay, off the caller's task.
    fn react(&self, on: On, text: &str) {
        let Some(step) = take_first(&self.steps, |s| s.on == on && text.contains(&s.matching)) else { return };
        let (listeners, record, text) = (self.listeners.clone(), self.record.clone(), text.to_string());
        let run = async move {
            tokio::time::sleep(Duration::from_millis(step.after_ms)).await;
            match step.act {
                Do::Emit { event, payload } => {
                    // Called after the lock is released: a handler may listen or unlisten.
                    let heard: Vec<Handler> = listeners.lock().unwrap().values().filter(|(name, _)| *name == event).map(|(_, h)| h.clone()).collect();
                    let payload = payload.to_string();
                    for handler in &heard {
                        handler(&payload);
                    }
                    record.note("person-emitted", json!({ "event": event, "listeners": heard.len() }));
                }
                Do::Follow { via } => {
                    let url = match via {
                        Some(via) => format!("{via}{}", urlencoding::encode(&text)),
                        None => text,
                    };
                    let got = tokio::task::spawn_blocking(move || ureq::get(&url).call().map(|r| r.status()).map_err(|e| e.to_string())).await;
                    record.note("person-followed", json!({ "result": format!("{got:?}") }));
                }
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn(run)),
            Err(_) => self.record.note("person-cannot", json!({ "do": "act outside a runtime" })),
        }
    }
}

#[async_trait::async_trait]
impl AppHost for ScriptedPerson {
    async fn get_plugin_cookies(&self, plugin: &str, _: &str) -> Result<String, String> {
        Ok(self.cookies.lock().unwrap().get(plugin).cloned().unwrap_or(json!([])).to_string())
    }
    async fn set_plugin_cookies(&self, plugin: &str, _: &str, cookies: Value) -> Result<(), String> {
        self.cookies.lock().unwrap().insert(plugin.to_string(), cookies);
        Ok(())
    }
    async fn clear_plugin_cookies(&self, plugin: &str, _: &str) -> Result<(), String> {
        self.record.note("cookies-cleared", json!({ "plugin": plugin }));
        self.cookies.lock().unwrap().remove(plugin);
        Ok(())
    }
    async fn open_action_panel(&self, url: Option<String>, title: Option<String>, _: bool, _: Option<String>) -> Result<(), String> {
        self.record.note("panel-opened", json!({ "url": url, "title": title }));
        self.react(On::Panel, url.as_deref().unwrap_or_default());
        Ok(())
    }
    async fn set_action_panel_html(&self, html: String, title: Option<String>, _: Option<String>) -> Result<(), String> {
        self.record.note("page-shown", json!({ "title": title, "bytes": html.len() }));
        self.react(On::Page, &html);
        Ok(())
    }
    async fn close_action_panel(&self, _: Option<String>) -> Result<(), String> {
        self.record.note("panel-closed", Value::Null);
        Ok(())
    }
    async fn open_system_browser(&self, url: String) -> Result<(), String> {
        self.record.note("browser-opened", json!({ "url": url }));
        self.react(On::Browser, &url);
        Ok(())
    }
    async fn return_to_editor(&self) -> Result<(), String> {
        self.record.note("returned-to-editor", Value::Null);
        Ok(())
    }
    async fn fill_browser_panel(&self, plugin: &str, project: &str, fill: Value) -> Result<Value, String> {
        self.record.note("panel-filled", json!({ "plugin": plugin, "project": project, "fill": fill }));
        Ok(json!({ "filled": true }))
    }
    async fn prompt_credential(&self, _: &str, _: &str, key: &str, _: Option<String>) -> Result<Option<String>, String> {
        self.record.note("credential-asked", json!({ "key": key }));
        let answer = take_first(&self.answers, |a| key.contains(&a.matching));
        if let Some(a) = &answer {
            tokio::time::sleep(Duration::from_millis(a.after_ms)).await;
        }
        Ok(answer.map(|a| a.value))
    }
    fn listen(&self, name: String, handler: Box<dyn Fn(&str) + Send + Sync + 'static>) -> ListenerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.listeners.lock().unwrap().insert(id, (name, Arc::from(handler)));
        id
    }
    fn unlisten(&self, id: ListenerId) {
        self.listeners.lock().unwrap().remove(&id);
    }
    fn emit_plugin_event(&self, name: &str, _: Option<String>) {
        self.record.note("plugin-emitted", json!({ "event": name }));
        self.react(On::Event, name);
    }
}

struct Args {
    folder: String,
    plugin: String,
    hooks: Vec<String>,
    script: Script,
    watchdog: HookWatchdog,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mut positional, mut script, mut watchdog) = (Vec::new(), Script::default(), HookWatchdog::default());
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--person" => {
                let path = words.next().ok_or(USAGE)?;
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                script = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            }
            "--watchdog-secs" => {
                let secs: u64 = words.next().and_then(|s| s.parse().ok()).ok_or(USAGE)?;
                watchdog = HookWatchdog { limit: Duration::from_secs(secs), check: Duration::from_millis(100) };
            }
            w if w.starts_with('-') => return Err(USAGE.into()),
            w => positional.push(w.to_string()),
        }
    }
    if positional.len() < 3 || positional[2..].iter().any(|h| !["login", "logout", "account"].contains(&h.as_str())) {
        return Err(USAGE.into());
    }
    let hooks = positional.split_off(2);
    Ok(Args { plugin: positional.pop().unwrap_or_default(), folder: positional.pop().unwrap_or_default(), hooks, script, watchdog })
}

/// Run the hooks in order and print one JSON line per hook. Exit 2 on a usage
/// or setup error; otherwise 0, whatever the hooks answered: the caller judges.
pub fn run(args: &[String]) -> i32 {
    let args = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    if let Err(e) = install_logger() {
        eprintln!("moss-contract: {e}");
        return 2;
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
    match rt.block_on(drive(args)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("moss-contract: {e}");
            2
        }
    }
}

/// A folder's plugin, loaded in the engine with `person` as its desktop.
struct Session {
    person: Arc<ScriptedPerson>,
    manager: PluginManager,
    plugin: Plugin,
    context: String,
}

fn session(folder: &str, plugin: &str, person: ScriptedPerson, watchdog: HookWatchdog) -> Result<Session, String> {
    let person = Arc::new(person);
    let mut host = AdapterHost::headless(crate::build::ports::reporter::discard_owned());
    host.app = Some(person.clone());
    let manager = PluginManager::new(folder, host)?.with_watchdog(watchdog);
    let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == plugin).ok_or_else(|| format!("no plugin {plugin} in {folder}"))?;
    let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
    let context = serde_json::to_string(&ProcessContext::new(folder.to_string(), info, TriggerContext::SettingsManual)).map_err(|e| e.to_string())?;
    Ok(Session { person, manager, plugin, context })
}

async fn drive(args: Args) -> Result<(), String> {
    let Session { person, manager, plugin, context } = session(&args.folder, &args.plugin, ScriptedPerson::new(args.script), args.watchdog)?;
    let secrets = crate::identity::secrets::SecretStore::require("moss-contract")?;
    let scope = crate::identity::keystore::Scope::plugin(&args.plugin).map_err(|e| format!("{e:?}"))?;
    for hook in &args.hooks {
        let started = Instant::now();
        let answer = match hook.as_str() {
            "login" => match manager.sign_in(&plugin, &context).await {
                Ok(signed_in) => json!({ "ok": true, "signed_in": signed_in }),
                Err(e) => json!({ "ok": false, "error": e }),
            },
            "logout" => {
                let failed = manager.sign_out(&plugin, &context, &secrets).await;
                json!({ "ok": failed.is_empty(), "failed": failed })
            }
            _ => json!({ "ok": true }),
        };
        let declared = plugin.manifest.user_supplied_secret_keys().into_iter().chain(plugin.manifest.plugin_owned_secret_keys());
        let held: serde_json::Map<String, Value> = declared.map(|k| {
            let present = matches!(secrets.get(&scope, &k), Ok(Some(_)));
            (k, Value::Bool(present))
        }).collect();
        let line = json!({
            "hook": hook,
            "ms": started.elapsed().as_millis() as u64,
            "answer": answer,
            "account": manager.account_state(&args.plugin).await,
            "secrets": held,
            "cookies": person.cookies.lock().unwrap().get(&args.plugin).cloned(),
            "seen": person.take_seen(),
        });
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(manifest_settings: &str, body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_str().unwrap().to_string();
        let plugin = dir.path().join(".moss/plugins/social");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), format!(r#"{{"name":"social","version":"0.1.0","entry":"main.bundle.js","global_name":"Social","contributes":{{"channel":{{"login":true,"setup":{{"settings":[{manifest_settings}]}}}}}}}}"#)).unwrap();
        std::fs::write(plugin.join("main.bundle.js"), format!("var Social = {{ {body} }};")).unwrap();
        (dir, project)
    }

    async fn sign_in_with(project: &str, person: ScriptedPerson, watchdog: HookWatchdog) -> (Result<crate::plugins::manager::accounts::SignedIn, String>, Arc<ScriptedPerson>) {
        let s = session(project, "social", person, watchdog).unwrap();
        (s.manager.sign_in(&s.plugin, &s.context).await, s.person)
    }

    const PICKER: &str = r#"async login(ctx) {
        const picked = new Promise((resolve) => globalThis.__TAURI__.event.listen("picked", (e) => resolve(e.payload ?? e)));
        await globalThis.__TAURI__.core.invoke("set_action_panel_html", { html: "<button>Continue</button>", title: "Pick" });
        const server = await picked;
        return { status: "completed", account: { id: "acct-1", name: "@ada" } };
    }"#;

    fn picker_person(after_ms: u64) -> ScriptedPerson {
        let script = json!({ "steps": [{ "on": "page", "matching": "Continue", "after_ms": after_ms, "do": "emit", "event": "picked", "payload": { "server": "example.social" } }] });
        ScriptedPerson::new(serde_json::from_value(script).unwrap())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_person_who_answers_inside_the_watchdog_completes_the_sign_in() {
        let (_dir, project) = folder("", PICKER);
        let watchdog = HookWatchdog { limit: Duration::from_secs(2), check: Duration::from_millis(100) };
        let (signed_in, person) = sign_in_with(&project, picker_person(200), watchdog).await;
        assert!(signed_in.is_ok(), "{signed_in:?} {:?}", person.take_seen());
        assert_eq!(person.seen("page-shown").len(), 1);
        assert_eq!(person.seen("person-emitted"), vec![json!({ "event": "picked", "listeners": 1 })]);
    }

    /// The page a sign-in shows holds the watchdog off, however slow the person.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_person_slower_than_the_watchdog_completes_a_sign_in_on_its_own_page() {
        let (_dir, project) = folder("", PICKER);
        let watchdog = HookWatchdog { limit: Duration::from_secs(1), check: Duration::from_millis(100) };
        let (signed_in, person) = sign_in_with(&project, picker_person(2_500), watchdog).await;
        assert!(signed_in.is_ok(), "{signed_in:?} {:?}", person.take_seen());
    }

    /// A wait with no page open is not a person's: the watchdog still ends it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sign_in_waiting_with_no_page_open_is_ended_by_the_watchdog() {
        let (_dir, project) = folder("", r#"async login(ctx) {
            const picked = new Promise((resolve) => globalThis.__TAURI__.event.listen("picked", resolve));
            await globalThis.__TAURI__.event.emit("asked");
            await picked;
            return { status: "completed", account: { id: "acct-1", name: "@ada" } };
        }"#);
        let script = json!({ "steps": [{ "on": "event", "matching": "asked", "after_ms": 2_500, "do": "emit", "event": "picked" }] });
        let watchdog = HookWatchdog { limit: Duration::from_secs(1), check: Duration::from_millis(100) };
        let (signed_in, person) = sign_in_with(&project, ScriptedPerson::new(serde_json::from_value(script).unwrap()), watchdog).await;
        assert_eq!(person.seen("plugin-emitted"), vec![json!({ "event": "asked" })]);
        assert_eq!(signed_in.unwrap_err(), "Hook 'social::login' timed out - no progress for 1 seconds");
    }

    /// A plugin that swallows a refused secret write still shows it in the report.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_refused_secret_write_the_plugin_swallows_is_reported() {
        let session = r#"{"key":"session","type":"secret","hidden":true,"label":"Session"}"#;
        let (_dir, project) = folder(session, r#"async login(ctx) {
            await globalThis.__TAURI__.core.invoke("set_plugin_secret", { key: "app-social-probe", value: "x" }).catch(() => {});
            return { status: "completed", account: { id: "acct-1", name: "@ada" } };
        }"#);
        let (signed_in, person) = sign_in_with(&project, ScriptedPerson::default(), HookWatchdog::default()).await;
        assert!(signed_in.is_ok(), "{signed_in:?}");
        let logged = person.seen("log");
        assert!(logged.iter().any(|l| l["target"] == "plugin::custody" && l["message"].as_str().unwrap().contains("'app-social-probe'")), "{logged:?}");
    }

    #[test]
    fn a_script_names_only_what_a_person_can_do() {
        let parse = |s: &str| serde_json::from_str::<Script>(s);
        assert!(parse(r#"{"steps":[{"on":"browser","matching":"","do":"follow","via":"http://127.0.0.1:1/approve?url="}]}"#).is_ok());
        assert!(parse(r#"{"steps":[{"on":"browser","do":"follow"}]}"#).is_err(), "a step names what it matches");
        assert!(parse(r#"{"steps":[{"on":"page","matching":"","matchng":"Continue","do":"follow"}]}"#).is_err(), "a misspelt field is refused");
        assert!(parse(r#"{"answers":[{"matching":"token","value":"x","after":1}]}"#).is_err());
        assert!(parse(r#"{"steps":[{"on":"page","matching":"","do":"close"}]}"#).is_err());
        assert!(parse(r#"{"steps":[{"on":"pgae","matching":"","do":"follow"}]}"#).is_err());
        assert!(parse(r#"{"step":[]}"#).is_err());
    }

    #[test]
    fn the_arguments_name_a_folder_a_plugin_and_known_hooks() {
        let words = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        let a = parse(&words("/f mastodon login account logout --watchdog-secs 3")).unwrap();
        assert_eq!((a.folder.as_str(), a.plugin.as_str(), a.hooks.len(), a.watchdog.limit), ("/f", "mastodon", 3, Duration::from_secs(3)));
        assert!(parse(&words("/f mastodon publish")).is_err());
        assert!(parse(&words("/f mastodon")).is_err());
    }
}
