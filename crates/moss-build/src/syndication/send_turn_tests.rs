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
