use super::media_settled;
use crate::types::content::SiteHashes;

fn with_image(mut hashes: SiteHashes, path: &str, hash: &str) -> SiteHashes {
    hashes.image_outputs.insert(path.to_string());
    hashes.files.insert(path.to_string(), hash.to_string());
    hashes
}

fn with_video(mut hashes: SiteHashes, path: &str, hash: &str) -> SiteHashes {
    hashes.video_outputs.insert(path.to_string());
    hashes.files.insert(path.to_string(), hash.to_string());
    hashes
}

fn card(n: u8) -> String {
    format!("{}{n:064x}.png", crate::build::served_path::OG_CARD_PREFIX)
}

#[test]
fn a_seal_with_no_last_seal_is_not_settled() {
    assert!(!media_settled(None, &SiteHashes::default()));
}

#[test]
fn sharing_cards_landing_beside_an_empty_last_seal_leave_it_settled() {
    let sealed = with_image(with_image(SiteHashes::default(), &card(1), "a"), &card(2), "b");
    assert!(media_settled(Some(&SiteHashes::default()), &sealed));
}

#[test]
fn a_video_ladder_landing_at_the_seal_leaves_it_unsettled() {
    let previous = with_image(SiteHashes::default(), &card(1), "a");
    let sealed = with_video(previous.clone(), "assets/Clip-One.hls/master.m3u8", "m");
    assert!(!media_settled(Some(&previous), &sealed));
}

#[test]
fn a_picture_variant_landing_or_changing_at_the_seal_leaves_it_unsettled() {
    let previous = with_image(SiteHashes::default(), "media/talk.webp", "old");
    assert!(!media_settled(Some(&SiteHashes::default()), &previous));
    assert!(!media_settled(Some(&previous), &with_image(SiteHashes::default(), "media/talk.webp", "new")));
    assert!(media_settled(Some(&previous), &previous.clone()));
}
