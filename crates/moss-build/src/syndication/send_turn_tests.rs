//! Each channel sends in its own turn.

use super::*;

const UID: &str = "aaaa0001";

/// One post naming `channels` in that order, each with a due copy.
fn site(root: &Path, channels: &[&str]) -> ArticleMap {
    std::fs::write(root.join("a.md"), "---\ntitle: A\n---\nBody.\n").unwrap();
    let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
    for channel in channels {
        records.save(&CopyRecord::due(UID, channel, "a.md", "a/")).unwrap();
    }
    let post: ArticleInfo = serde_json::from_value(serde_json::json!({
        "source_path": "a.md", "title": "A", "content": "Body.\n", "url_path": "a/", "date": null, "uid": UID, "syndicate": channels
    })).unwrap();
    let mut articles = ArticleMap::default();
    articles.articles.insert(post.url_path.clone(), post);
    articles
}

/// A channel whose `publish_copy` answers after `ms`, numbering its posts.
fn posting_after(root: &Path, name: &str, ms: u32) {
    let js = format!("(() => {{ let n = 0; return {{ publish_copy: async function () {{ n += 1; const id = String(n); \
        await new Promise(r => setTimeout(r, {ms})); \
        return {{ success: true, copy: {{ id, url: 'https://{name}.example.net/' + id }} }}; }} }}; }})()");
    super::super::test_channel(root, name, &js);
}

fn manager(root: &Path) -> std::sync::Arc<PluginManager> {
    crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap()
}

fn posted(sent: &[Sent]) -> Vec<(&str, &str)> {
    sent.iter().filter_map(|s| match &s.outcome {
        Outcome::Posted { url } => Some((s.channel.as_str(), url.as_str())),
        _ => None,
    }).collect()
}

/// A copy waiting on a person (here, a slow answer) holds back only its own
/// channel: the post's next channel posts meanwhile, and the result still
/// reads in the post's order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_channel_holds_back_only_itself() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Named so the slow channel also sorts first.
    let articles = site(root, &["early", "late"]);
    posting_after(root, "early", 1500);
    posting_after(root, "late", 0);
    let heard = std::sync::Mutex::new(Vec::new());
    let emit = |s: &Sent| if matches!(s.outcome, Outcome::Posted { .. }) { heard.lock().unwrap().push(s.channel.clone()) };
    let sent = send_due(root, &manager(root), &articles, &SendOptions::test("https://example.org", &emit)).await;
    assert_eq!(*heard.lock().unwrap(), ["late", "early"], "the quick channel posts while the slow one waits");
    assert_eq!(posted(&sent), [("early", "https://early.example.net/1"), ("late", "https://late.example.net/1")]);
}

/// Two sends working on one channel at once post its copy once: the second
/// waits for the channel and finds the copy already posted. Without the
/// turn, the plugin's hooks would also run twice at once, which the engine
/// cannot do for one hook.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_sends_sharing_a_channel_post_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let articles = site(root, &["one"]);
    posting_after(root, "one", 300);
    let manager = manager(root);
    let emit = |_: &Sent| {};
    let opts = SendOptions::test("https://example.org", &emit);
    let (a, b) = tokio::join!(send_in_turn(root, &manager, &articles, &opts), send_in_turn(root, &manager, &articles, &opts));
    let both: Vec<Sent> = a.into_iter().chain(b).collect();
    assert_eq!(posted(&both), [("one", "https://one.example.net/1")], "{:?}", both.iter().map(|s| &s.outcome).collect::<Vec<_>>());
    let record = Records::in_vault(&crate::moss_paths::MossPaths::new(root)).get(UID, "one").unwrap();
    assert_eq!(record.state, State::Posted);
}

/// A desk with the app's panel turn, logging each page shown and each close
/// that reaches it. Its `return_to_editor` waits for a page to show, so a copy
/// can ask for the panel only once another holds it.
#[derive(Default)]
struct PanelDesk {
    turn: crate::engine::app_host::PanelTurn,
    log: std::sync::Mutex<Vec<String>>,
    shown: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::engine::app_host::AppHost for PanelDesk {
    async fn get_plugin_cookies(&self, _: &str, _: &str) -> Result<String, String> { unreachable!() }
    async fn set_plugin_cookies(&self, _: &str, _: &str, _: serde_json::Value) -> Result<(), String> { unreachable!() }
    async fn clear_plugin_cookies(&self, _: &str, _: &str) -> Result<(), String> { unreachable!() }
    async fn open_action_panel(&self, _: Option<String>, _: Option<String>, _: bool, _: Option<String>) -> Result<(), String> { unreachable!() }
    async fn set_action_panel_html(&self, _: String, _: Option<String>, plugin: Option<String>) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("page {}", plugin.unwrap_or_default()));
        self.shown.notify_one();
        Ok(())
    }
    async fn close_action_panel(&self, plugin: Option<String>) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("close {}", plugin.unwrap_or_default()));
        Ok(())
    }
    async fn open_system_browser(&self, _: String) -> Result<(), String> { unreachable!() }
    async fn return_to_editor(&self) -> Result<(), String> {
        self.shown.notified().await;
        Ok(())
    }
    async fn fill_browser_panel(&self, _: &str, _: &str, _: serde_json::Value) -> Result<serde_json::Value, String> { unreachable!() }
    async fn prompt_credential(&self, _: &str, _: &str, _: &str, _: Option<String>) -> Result<Option<String>, String> { unreachable!() }
    fn listen(&self, _: String, _: Box<dyn Fn(&str) + Send + Sync + 'static>) -> crate::engine::app_host::ListenerId { 0 }
    fn unlisten(&self, _: crate::engine::app_host::ListenerId) {}
    fn emit_plugin_event(&self, _: &str, _: Option<String>) {}
    fn panel_turn(&self) -> &crate::engine::app_host::PanelTurn {
        &self.turn
    }
}

fn manager_at(root: &Path, desk: &std::sync::Arc<PanelDesk>) -> std::sync::Arc<PluginManager> {
    let desk = desk.clone();
    crate::plugins::manager::ManagerCache::new(move || {
        let mut host = crate::plugins::adapter_host::AdapterHost::headless(crate::build::ports::reporter::discard_owned());
        host.app = Some(desk.clone());
        host
    }).get_or_create(&root.to_string_lossy()).unwrap()
}

/// A channel whose `publish_copy` runs `first`, shows a page, waits `ms`, then runs `then` and posts.
fn showing_a_page(root: &Path, name: &str, first: &str, ms: u32, then: &str) {
    let js = format!("({{ publish_copy: async function () {{ {first} \
        await globalThis.__TAURI__.core.invoke('set_action_panel_html', {{ html: '<p>{name}</p>' }}); \
        await new Promise(r => setTimeout(r, {ms})); {then} \
        return {{ success: true, copy: {{ id: '1', url: 'https://{name}.example.net/1' }} }}; }} }})");
    super::super::test_channel(root, name, &js);
}

/// `send_due` at `desk`, failing rather than hanging when a copy never gets the panel.
async fn within_ten_seconds(root: &Path, desk: &std::sync::Arc<PanelDesk>, articles: &ArticleMap, emit: &(dyn Fn(&Sent) + Sync)) -> Vec<Sent> {
    let (manager, opts) = (manager_at(root, desk), SendOptions::test("https://example.org", emit));
    tokio::time::timeout(std::time::Duration::from_secs(10), send_due(root, &manager, articles, &opts)).await.expect("each copy's turn passes to the next when its hook ends")
}

const CLOSES: &str = "await globalThis.__TAURI__.core.invoke('close_action_panel', {});";
const AFTER_A_PAGE: &str = "await globalThis.__TAURI__.core.invoke('return_to_editor', {});";
const SLOWLY: &str = "await new Promise(r => setTimeout(r, 300));";

/// Two copies that show pages take the panel one at a time: the second
/// copy's page shows only after the first copy's hook has ended, so it never
/// replaces a page the person is still using. The late copy asks for the
/// panel only once the early one, slow to start, holds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_copies_show_their_pages_one_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let articles = site(root, &["early", "late"]);
    showing_a_page(root, "early", SLOWLY, 800, CLOSES);
    showing_a_page(root, "late", AFTER_A_PAGE, 0, CLOSES);
    let desk = std::sync::Arc::new(PanelDesk::default());
    let emit = |_: &Sent| {};
    let sent = within_ten_seconds(root, &desk, &articles, &emit).await;
    assert_eq!(posted(&sent).len(), 2, "{:?}", sent.iter().map(|s| &s.outcome).collect::<Vec<_>>());
    assert_eq!(*desk.log.lock().unwrap(), ["page early", "close early", "page late", "close late"]);
}

/// A copy whose hook has ended closes nothing: the page showing is the next
/// copy's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_after_the_turn_has_passed_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let articles = site(root, &["early", "late"]);
    showing_a_page(root, "early", "", 0, &format!("setTimeout(async () => {{ {CLOSES} }}, 400);"));
    showing_a_page(root, "late", AFTER_A_PAGE, 1200, "");
    let desk = std::sync::Arc::new(PanelDesk::default());
    let emit = |_: &Sent| {};
    let sent = within_ten_seconds(root, &desk, &articles, &emit).await;
    assert_eq!(posted(&sent).len(), 2, "{:?}", sent.iter().map(|s| &s.outcome).collect::<Vec<_>>());
    assert_eq!(*desk.log.lock().unwrap(), ["page early", "page late"]);
}
