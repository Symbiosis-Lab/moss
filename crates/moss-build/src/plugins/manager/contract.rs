//! `moss-contract`: run a plugin's own bundle in this engine with a scripted
//! person in place of the desktop, and report what happened as JSON.
//!
//! A plugin's unit tests fake moss, so they prove the plugin agrees with its own
//! fake. This runs the real thing: the manifest's secret custody, the hook
//! watchdog, the host functions, a file secret store under `$HOME`. Only the
//! desktop is replaced, by [`ScriptedPerson`], which answers pages, pop-ups and
//! the system browser the way a script says a person would, and as slowly.
//!
//! Built only with the `contract` feature; no shipped binary carries it.

use crate::plugins::adapter_host::AdapterHost;
use super::{HookWatchdog, PluginManager};
use crate::plugins::types::{ProcessContext, ProjectInfo, TriggerContext};
use crate::engine::app_host::{AppHost, ListenerId};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const USAGE: &str = "Usage: moss-contract <folder> <plugin> <login|logout|account>... [--person <script.json>] [--watchdog-secs N]";

/// One thing the person does when something appears. `on` is what appeared
/// (`page`, `panel`, `browser`, `event`, `credential`), `matching` a substring
/// of its HTML, URL, event name or credential key; each step acts once.
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub on: String,
    #[serde(default)]
    pub matching: String,
    #[serde(default)]
    pub after_ms: u64,
    /// `emit` an event to the plugin, `close` the panel, `follow` a URL with
    /// redirects (a browser reaching a loopback callback), or `type` a credential.
    #[serde(rename = "do")]
    pub action: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Script {
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Cookies already in a plugin's jar, as a web-session channel's sign-in
    /// page would have left them.
    #[serde(default)]
    pub cookies: HashMap<String, Value>,
}

type Handler = Box<dyn Fn(&str) + Send + Sync + 'static>;

/// The desktop, played by a script. Records everything the plugin asked of it.
pub struct ScriptedPerson {
    steps: Mutex<Vec<Option<Step>>>,
    listeners: Arc<Mutex<HashMap<ListenerId, (String, Handler)>>>,
    next_id: AtomicU32,
    cookies: Mutex<HashMap<String, Value>>,
    seen: Arc<Mutex<Vec<Value>>>,
    started: Instant,
}

impl ScriptedPerson {
    pub fn new(script: Script) -> Self {
        Self {
            steps: Mutex::new(script.steps.into_iter().map(Some).collect()),
            listeners: Arc::default(),
            next_id: AtomicU32::new(1),
            cookies: Mutex::new(script.cookies),
            seen: Arc::default(),
            started: Instant::now(),
        }
    }

    /// Everything seen since the last call, oldest first.
    pub fn take_seen(&self) -> Vec<Value> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }

    fn see(&self, what: &str, detail: Value) {
        self.seen.lock().unwrap().push(json!({ "at_ms": self.started.elapsed().as_millis() as u64, "what": what, "detail": detail }));
    }

    fn take_step(&self, on: &str, text: &str) -> Option<Step> {
        let mut steps = self.steps.lock().unwrap();
        let slot = steps.iter_mut().find(|s| s.as_ref().is_some_and(|s| s.on == on && text.contains(&s.matching)))?;
        slot.take()
    }

    /// Act on the first unused step for `on`, after its delay, off the caller's task.
    fn react(&self, on: &str, text: &str) {
        let Some(step) = self.take_step(on, text) else { return };
        let (listeners, seen, started, url) = (self.listeners.clone(), self.seen.clone(), self.started, text.to_string());
        let run = async move {
            tokio::time::sleep(Duration::from_millis(step.after_ms)).await;
            let note = |what: &str, detail: Value| seen.lock().unwrap().push(json!({ "at_ms": started.elapsed().as_millis() as u64, "what": what, "detail": detail }));
            match step.action.as_str() {
                "emit" => {
                    let payload = step.payload.to_string();
                    let mut heard = 0;
                    for (name, handler) in listeners.lock().unwrap().values() {
                        if *name == step.event {
                            handler(&payload);
                            heard += 1;
                        }
                    }
                    note("person-emitted", json!({ "event": step.event, "listeners": heard }));
                }
                "follow" => {
                    let got = tokio::task::spawn_blocking(move || ureq::get(&url).call().map(|r| r.status()).map_err(|e| e.to_string())).await;
                    note("person-followed", json!({ "result": format!("{got:?}") }));
                }
                other => note("person-cannot", json!({ "do": other })),
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn(run)),
            Err(_) => self.see("person-cannot", json!({ "do": "act outside a runtime" })),
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
        self.see("cookies-cleared", json!({ "plugin": plugin }));
        self.cookies.lock().unwrap().remove(plugin);
        Ok(())
    }
    async fn open_action_panel(&self, url: Option<String>, title: Option<String>, _: bool, _: Option<String>) -> Result<(), String> {
        self.see("panel-opened", json!({ "url": url, "title": title }));
        self.react("panel", url.as_deref().unwrap_or_default());
        Ok(())
    }
    async fn set_action_panel_html(&self, html: String, title: Option<String>, _: Option<String>) -> Result<(), String> {
        self.see("page-shown", json!({ "title": title, "bytes": html.len() }));
        self.react("page", &html);
        Ok(())
    }
    async fn close_action_panel(&self) -> Result<(), String> {
        self.see("panel-closed", Value::Null);
        Ok(())
    }
    async fn open_system_browser(&self, url: String) -> Result<(), String> {
        self.see("browser-opened", json!({ "url": url }));
        self.react("browser", &url);
        Ok(())
    }
    async fn return_to_editor(&self) -> Result<(), String> {
        self.see("returned-to-editor", Value::Null);
        Ok(())
    }
    async fn fill_browser_panel(&self, _: &str, _: &str, fill: Value) -> Result<Value, String> {
        self.see("panel-filled", json!({ "keys": fill.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()) }));
        Ok(json!({ "filled": true }))
    }
    async fn prompt_credential(&self, _: &str, _: &str, key: &str, _: Option<String>) -> Result<Option<String>, String> {
        self.see("credential-asked", json!({ "key": key }));
        let step = self.take_step("credential", key);
        if let Some(s) = &step {
            tokio::time::sleep(Duration::from_millis(s.after_ms)).await;
        }
        Ok(step.map(|s| s.value))
    }
    fn listen(&self, name: String, handler: Handler) -> ListenerId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.listeners.lock().unwrap().insert(id, (name, handler));
        id
    }
    fn unlisten(&self, id: ListenerId) {
        self.listeners.lock().unwrap().remove(&id);
    }
    fn emit_plugin_event(&self, name: &str, _: Option<String>) {
        self.see("plugin-emitted", json!({ "event": name }));
        self.react("event", name);
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
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("a runtime");
    match rt.block_on(drive(args)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("moss-contract: {e}");
            2
        }
    }
}

async fn drive(args: Args) -> Result<(), String> {
    let person = Arc::new(ScriptedPerson::new(args.script));
    let mut host = AdapterHost::headless(crate::build::ports::reporter::discard_owned());
    host.app = Some(person.clone());
    let manager = PluginManager::new(&args.folder, host)?.with_watchdog(args.watchdog);
    let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == args.plugin).ok_or_else(|| format!("no plugin {} in {}", args.plugin, args.folder))?;
    let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
    let context = serde_json::to_string(&ProcessContext::new(args.folder.clone(), info, TriggerContext::SettingsManual)).map_err(|e| e.to_string())?;
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
        let person = Arc::new(person);
        let mut host = AdapterHost::headless(crate::build::ports::reporter::discard_owned());
        host.app = Some(person.clone());
        let manager = PluginManager::new(project, host).unwrap().with_watchdog(watchdog);
        let plugin = manager.plugins().into_iter().find(|p| p.manifest.name == "social").unwrap();
        let info = ProjectInfo { total_files: 0, homepage_file: None, folder_name: None, site_name: None, lang: "en".into() };
        let context = serde_json::to_string(&ProcessContext::new(project.into(), info, TriggerContext::SettingsManual)).unwrap();
        (manager.sign_in(&plugin, &context).await, person)
    }

    const PICKER: &str = r#"async login(ctx) {
        const picked = new Promise((resolve) => globalThis.__TAURI__.event.listen("picked", (e) => resolve(e.payload ?? e)));
        await globalThis.__TAURI__.core.invoke("set_action_panel_html", { html: "<button>Continue</button>", title: "Pick" });
        const server = await picked;
        return { status: "completed", account: { id: "acct-1", name: "@ada" } };
    }"#;

    fn picker_person(after_ms: u64) -> ScriptedPerson {
        ScriptedPerson::new(Script {
            steps: vec![Step { on: "page".into(), matching: "Continue".into(), after_ms, action: "emit".into(), event: "picked".into(), payload: json!({"server": "example.social"}), value: String::new() }],
            cookies: HashMap::new(),
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_person_who_answers_inside_the_watchdog_completes_the_sign_in() {
        let (_dir, project) = folder("", PICKER);
        let watchdog = HookWatchdog { limit: Duration::from_secs(2), check: Duration::from_millis(100) };
        let (signed_in, person) = sign_in_with(&project, picker_person(200), watchdog).await;
        assert!(signed_in.is_ok(), "{signed_in:?} {:?}", person.take_seen());
        let seen: Vec<String> = person.take_seen().iter().map(|s| s["what"].as_str().unwrap().to_string()).collect();
        assert!(seen.contains(&"page-shown".to_string()) && seen.contains(&"person-emitted".to_string()), "{seen:?}");
    }

    /// The shape of a sign-in page that waits on a person with nothing holding
    /// the watchdog off: a person slower than the limit finds the sign-in gone.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_person_slower_than_the_watchdog_finds_an_unheld_wait_cancelled() {
        let (_dir, project) = folder("", PICKER);
        let watchdog = HookWatchdog { limit: Duration::from_secs(1), check: Duration::from_millis(100) };
        let (signed_in, _) = sign_in_with(&project, picker_person(2_500), watchdog).await;
        assert_eq!(signed_in.unwrap_err(), "Hook 'social::login' timed out - no progress for 1 seconds");
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
