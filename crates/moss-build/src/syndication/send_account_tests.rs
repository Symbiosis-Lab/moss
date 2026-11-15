//! A copy stays with the account it was sent from.

use super::*;
use crate::syndication::sign_ins::{SignInState, SignIns};

const SOURCE: &str = "a.md";
const UID: &str = "aaaa0001";

/// The `acct` channel, signed in to `account` (none: `copy_account` fails),
/// with `ops` as its copy hooks.
fn channel(root: &Path, account: Option<&str>, ops: &str) {
    let answer = match account {
        Some(a) => format!("{{ success: true, copy: {{ account: '{a}' }} }}"),
        None => "{ success: false, message: 'offline' }".to_string(),
    };
    let js = format!("{{ copy_account: async function () {{ return {answer}; }}, {ops} }}");
    if root.join(".moss/plugins/acct").exists() {
        std::fs::write(root.join(".moss/plugins/acct/main.bundle.js"), format!("window.ACCTPlugin = {js};")).unwrap();
    } else {
        super::super::test_channel(root, "acct", &js);
    }
}

const REFUSE_ALL: &str = "publish_copy: async function () { throw new Error('must not publish'); }, \
    find_copy: async function () { throw new Error('must not look up'); }, \
    update_copy: async function () { throw new Error('must not update'); }";

fn site(root: &Path, state: State, account: Option<&str>) -> (Records, ArticleMap) {
    std::fs::write(root.join(SOURCE), "---\ntitle: A\n---\nBody.\n").unwrap();
    let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
    let mut r = CopyRecord::due(UID, "acct", SOURCE, "a/");
    r.state = state;
    r.account = account.map(str::to_string);
    // A refused copy keeps no attempt, as `discard_attempt` leaves it.
    if !matches!(state, State::Due | State::Refused) {
        r.form = Some(super::super::channel::Form::Short);
    }
    if state == State::Posted {
        r.remote = Some(super::super::record::RemoteRef { id: "1".into(), url: "https://social.example.net/1".into() });
        r.update_due = true;
    }
    records.save(&r).unwrap();
    let post: ArticleInfo = serde_json::from_value(serde_json::json!({
        "source_path": SOURCE, "title": "A", "content": "Body.\n", "url_path": "a/", "date": null, "uid": UID, "syndicate": ["acct"]
    })).unwrap();
    let mut articles = ArticleMap::default();
    articles.articles.insert(post.url_path.clone(), post);
    (records, articles)
}

async fn run(root: &Path, articles: &ArticleMap) -> Vec<Outcome> {
    run_recording(root, articles, None).await
}

/// A run with no `AppHost`, as from a terminal, updating `sign_ins`.
async fn run_recording(root: &Path, articles: &ArticleMap, sign_ins: Option<&SignIns>) -> Vec<Outcome> {
    let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
    let emit = |_: &Sent| {};
    let opts = SendOptions { sign_ins, ..SendOptions::test("https://example.org", &emit) };
    send_due(root, &manager, articles, &opts).await.into_iter().map(|s| s.outcome).filter(|o| *o != Outcome::Sending).collect()
}

fn other_account() -> Outcome {
    Outcome::Held { why: HeldWhy::OtherAccount, reason: OTHER_ACCOUNT.into() }
}

/// A record from before accounts were kept takes the signed-in account with
/// the mark written before the network is called, so a send cut off there
/// is already tied to the account that made it. Signed in to another, it is
/// neither looked up nor sent again, and the run says so once; signed back
/// in, it is looked up and sent.
#[tokio::test]
async fn a_send_cut_off_under_one_account_waits_for_that_account() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, None);
    channel(root, Some("did:a"), "publish_copy: async function () { throw new Error('connection reset'); }");
    assert!(matches!(&run(root, &articles).await[..], [Outcome::Unconfirmed { .. }]));
    let cut_off = records.get(UID, "acct").unwrap();
    assert_eq!((cut_off.state, cut_off.account.as_deref()), (State::Unconfirmed, Some("did:a")));

    let switch_to_b = || async {
        channel(root, Some("did:b"), REFUSE_ALL);
        assert_eq!(run(root, &articles).await, [other_account()]);
        assert_eq!(run(root, &articles).await, [], "said once");
        let held = records.get(UID, "acct").unwrap();
        assert_eq!((held.state, held.reason.as_deref()), (State::Unconfirmed, cut_off.reason.as_deref()), "the copy keeps its own reason");
    };
    switch_to_b().await;
    // Back on the first account, the lookup still cannot tell; the copy is
    // said to be held again the next time the other account is signed in.
    channel(root, Some("did:a"), "find_copy: async function () { return { success: true, copy: null }; }");
    let back = run(root, &articles).await;
    assert!(matches!(&back[..], [Outcome::Unconfirmed { reason }] if !reason.contains(OTHER_ACCOUNT)), "{back:?}");
    switch_to_b().await;

    channel(root, Some("did:a"), "find_copy: async function () { return { success: true, copy: { absent: true } }; }, \
        publish_copy: async function () { return { success: true, copy: { id: '2', url: 'https://social.example.net/2' } }; }");
    assert_eq!(run(root, &articles).await, [Outcome::Posted { url: "https://social.example.net/2".into() }]);
    assert_eq!(records.get(UID, "acct").unwrap().account.as_deref(), Some("did:a"));
}

/// Every piece of pending work on a copy sent from one account waits while
/// the channel is signed in to another: a resend of an interrupted send (the
/// same key would not stop a second copy on another account), a lookup, an
/// update, a handed-off copy. Work with nothing on the network goes ahead
/// under the new account.
#[tokio::test]
async fn pending_work_on_another_accounts_copy_waits() {
    for state in [State::Sending, State::Unconfirmed, State::Posted, State::HandedOff] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = site(root, state, Some("did:a"));
        let before = records.get(UID, "acct").unwrap();
        channel(root, Some("did:b"), REFUSE_ALL);
        assert_eq!(run(root, &articles).await, [other_account()], "{state:?}");
        let after = records.get(UID, "acct").unwrap();
        assert!(after.held_for_account, "{state:?}");
        assert_eq!(CopyRecord { held_for_account: false, ..after }, before, "{state:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Refused, Some("did:a"));
    channel(root, Some("did:b"), "publish_copy: async function () { return { success: true, copy: { id: '3', url: 'https://social.example.net/3' } }; }");
    assert_eq!(run(root, &articles).await, [Outcome::Posted { url: "https://social.example.net/3".into() }]);
    assert_eq!(records.get(UID, "acct").unwrap().account.as_deref(), Some("did:b"));
}

/// A channel that answers with no account, as when the author posts by hand,
/// ties no copy to one.
#[tokio::test]
async fn a_channel_that_names_no_account_sends_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, None);
    super::super::test_channel(root, "acct", "{ copy_account: async function () { return { success: true }; }, \
        publish_copy: async function () { return { success: true, copy: { id: '4', url: 'https://social.example.net/4' } }; } }");
    assert_eq!(run(root, &articles).await, [Outcome::Posted { url: "https://social.example.net/4".into() }]);
    assert_eq!(records.get(UID, "acct").unwrap().account, None);
}

/// A channel that cannot say which account it is signed in to sends nothing.
#[tokio::test]
async fn nothing_is_sent_while_the_signed_in_account_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, None);
    channel(root, None, REFUSE_ALL);
    assert!(matches!(&run(root, &articles).await[..], [Outcome::Held { why: HeldWhy::Failed, .. }]));
    assert_eq!(records.get(UID, "acct").unwrap().state, State::Due);
}

/// A handed-off copy has nothing to send, so an account that cannot be asked
/// holds it without a word.
#[tokio::test]
async fn a_resting_handoff_says_nothing_while_the_account_is_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::HandedOff, Some("did:a"));
    let before = records.get(UID, "acct").unwrap();
    channel(root, None, REFUSE_ALL);
    assert_eq!(run(root, &articles).await, []);
    assert_eq!(records.get(UID, "acct").unwrap(), before);
}

/// Signed out, a channel's copies wait for someone to sign in, and this
/// computer's record says so, keeping the account it was signed in to.
#[tokio::test]
async fn signed_out_holds_the_copies_and_records_the_lapse() {
    let dir = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sign_ins = SignIns::in_app_data(app.path());
    sign_ins.record("acct", &Account::Known { id: "did:a".into(), name: Some("@ada".into()) }).unwrap();
    let (records, articles) = site(root, State::Due, None);
    super::super::test_channel(root, "acct", &format!("{{ copy_account: async function () {{ return {{ success: true, copy: {{ signed_out: true }} }}; }}, {REFUSE_ALL} }}"));
    let held = run_recording(root, &articles, Some(&sign_ins)).await;
    assert!(matches!(&held[..], [Outcome::Held { why: HeldWhy::SignedOut, .. }]), "{held:?}");
    assert_eq!(records.get(UID, "acct").unwrap().state, State::Due);
    let lapsed = sign_ins.get("acct").unwrap();
    assert_eq!((lapsed.state, lapsed.account.as_deref()), (SignInState::NeedsSignIn, Some("did:a")));
}

/// A channel that reads its session from the webview's cookies cannot read
/// them where there is no app (a terminal run): the read fails rather than
/// answering an empty jar, so the channel cannot say it is signed out, and the
/// app's record of a signed-in session stands.
#[tokio::test]
async fn a_run_with_no_app_leaves_the_sign_in_record_alone() {
    let dir = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sign_ins = SignIns::in_app_data(app.path());
    sign_ins.record("acct", &Account::Known { id: "x:1".into(), name: None }).unwrap();
    let before = sign_ins.get("acct").unwrap();
    let (records, articles) = site(root, State::Due, None);
    super::super::test_channel(root, "acct", &format!("{{ copy_account: async function () {{ \
        try {{ const jar = await globalThis.__TAURI__.core.invoke('get_plugin_cookie', {{ pluginName: 'acct', projectPath: '/p' }}); \
               const session = (jar || []).find(c => c.name === 'auth_token'); \
               return session ? {{ success: true, copy: {{ account: 'x:1' }} }} : {{ success: true, copy: {{ signed_out: true }} }}; }} \
        catch (e) {{ return {{ success: false, message: String(e) }}; }} }}, {REFUSE_ALL} }}"));
    let held = run_recording(root, &articles, Some(&sign_ins)).await;
    assert!(matches!(&held[..], [Outcome::Held { why: HeldWhy::Failed, .. }]), "{held:?}");
    assert_eq!(records.get(UID, "acct").unwrap().state, State::Due);
    assert_eq!(sign_ins.get("acct").unwrap(), before);
}
