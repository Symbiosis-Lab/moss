use super::*;

#[test]
fn an_empty_output_dir_is_absent_whatever_the_fingerprints_say() {
    assert_eq!(classify(false, Some("same"), "same"), PriorOutput::Absent);
    assert_eq!(classify(false, None, "same"), PriorOutput::Absent);
    assert!(!classify(false, Some("same"), "same").may_show_early());
}

#[test]
fn pages_from_this_same_binary_are_shown_immediately() {
    let out = classify(true, Some("4200-1699"), "4200-1699");
    assert_eq!(out, PriorOutput::Showable);
    assert!(out.may_show_early());
}

#[test]
fn pages_from_a_different_moss_are_served_but_not_advertised() {
    // The regression this exists for: the user updated moss, so the frozen
    // generation was rendered by the old binary and no longer matches what
    // this one emits. Serving it keeps the rebuild window from 404ing; showing
    // it would put a broken-looking site in front of the user.
    let out = classify(true, Some("4100-1600"), "4200-1699");
    assert_eq!(out, PriorOutput::StaleBuilder);
    assert!(!out.may_show_early());
}

#[test]
fn output_that_recorded_no_builder_at_all_is_assumed_stale() {
    // hashes.json from a moss too old to write the field. Same "assume stale
    // when unsure" rule the staging comparison uses — the alternative is
    // showing pages of unknown provenance.
    assert_eq!(classify(true, None, "4200-1699"), PriorOutput::StaleBuilder);
}
