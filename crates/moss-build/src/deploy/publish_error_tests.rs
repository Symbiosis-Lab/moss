use super::PublishError;
use crate::deploy::freeze::{single_flight_tests::TEST_LOCK, with_publish_guard};
use crate::deploy::progress::{DeploySink, DeployStage, RecordingSink};
use crate::i18n::AppLanguage;
use std::path::Path;
use std::sync::Arc;

const HOST: &str = "her-blog.example.org";

fn refusals() -> [PublishError; 2] {
    [PublishError::NotSignedIn { host: HOST.into() }, PublishError::NameTaken { address: HOST.into() }]
}

/// The app shows these words; a person there has no terminal, and the host is
/// the folder's, whichever service it is.
#[test]
fn the_plain_words_name_the_address_and_no_command_in_every_language() {
    for lang in AppLanguage::ALL {
        for refusal in refusals() {
            let text = refusal.plain_text_in(lang);
            assert!(text.contains(HOST), "{lang:?} {refusal:?}: {text}");
            for plumbing in ["moss deploy", "--", "mosspub.com", "not_signed_in", "name_taken"] {
                assert!(!text.contains(plumbing), "{lang:?} {refusal:?} says {plumbing:?}: {text}");
            }
        }
    }
}

#[test]
fn the_terminal_words_name_the_flag_that_lifts_each_refusal() {
    let folder = Path::new("/sites/her-blog");
    let [signed_out, taken] = refusals();
    assert!(signed_out.cli_text(folder).ends_with("moss deploy /sites/her-blog --email <address>"));
    assert!(taken.cli_text(folder).ends_with("moss deploy /sites/her-blog --site-id=<name>"));
}

#[tokio::test]
async fn a_refused_publish_fails_with_the_plain_words_and_keeps_its_kind() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let recorder = Arc::new(RecordingSink::default());
    let sink: Arc<dyn DeploySink> = recorder.clone();
    let refusal = PublishError::NotSignedIn { host: HOST.into() };

    let err = with_publish_guard(&sink, async { Err::<(), _>(refusal.clone()) }).await.unwrap_err();

    assert_eq!(err, refusal);
    let failed: Vec<_> = recorder.drain().into_iter().filter(|(stage, _)| *stage == DeployStage::Failed).collect();
    assert_eq!(failed, vec![(DeployStage::Failed, refusal.plain_text())]);
}
