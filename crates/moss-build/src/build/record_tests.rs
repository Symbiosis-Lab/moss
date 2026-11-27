use super::*;

const CANARIES: &[&str] = &["/Users/Jane Doe/Mobile Documents/x.md", "https://example.org/a", "jane@example.org", "花园笔记"];

fn token_is_typed(kind: &Kind, token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else { return false };
    let Some(key) = kind.key(name) else { return false };
    let number = value.strip_prefix('-').unwrap_or(value);
    let number = number.strip_suffix("ms").unwrap_or(number);
    match key.ty {
        Type::Word(list) => list.contains(value),
        _ => !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
    }
}

#[test]
fn every_kind_writes_only_its_own_keys_with_typed_values() {
    for kind in KINDS {
        let line = Record::every_key(kind).line();
        let (head, rest) = line.split_once(' ').unwrap();
        assert_eq!(head, format!("{}.summary", kind.name));
        assert_eq!(rest.split(' ').count(), kind.keys.len(), "{line}");
        for token in rest.split(' ') {
            assert!(token_is_typed(kind, token), "{token} in {line}");
        }
        assert!(parse(&line).is_some(), "{line}");
    }
}

#[test]
fn a_record_with_one_foreign_token_does_not_parse() {
    let line = Record::every_key(&BUILD).line();
    assert!(parse(&line).is_some());
    for extra in ["site_id=4", "outcome=ok", "trigger=Garden", "scan=12", "total=12", "changed=-1", "reason=setting:Garden", "loose"] {
        assert!(parse(&format!("{line} {extra}")).is_none(), "{extra}");
    }
    assert!(parse("publish.summary total=1ms").is_none(), "a kind the table lacks");
    assert!(parse(&line.replacen("build.summary", "build.summary ", 1)).is_none(), "an empty token");
}

#[test]
fn no_byte_of_a_canary_reaches_the_line() {
    let mut record = Record::new(&BUILD);
    for canary in CANARIES {
        record.word("outcome", canary);
        record.word("trigger", canary);
        record.word("reason", canary);
        record.count(canary, 1);
        record.ms(canary, 1);
    }
    let line = record.line();
    assert_eq!(without_zero_fill(&line), format!("build.summary unlisted={}", CANARIES.len() * 5));
    for canary in CANARIES {
        for word in canary.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
            assert!(!line.contains(word), "{word} of {canary} reached {line}");
        }
    }
}

#[test]
fn a_value_of_the_wrong_type_is_counted_not_written() {
    let mut record = Record::new(&BUILD);
    record.ms("pages", 3);
    record.count("scan", 3);
    record.ms("total", -1);
    assert_eq!(without_zero_fill(&record.line()), "build.summary unlisted=3");
}

#[test]
fn a_kind_names_each_key_once() {
    for kind in KINDS.iter().chain([&&VIDEO_ENCODE]) {
        let mut names: Vec<_> = kind.keys.iter().map(|k| k.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "{} names a key twice", kind.name);
    }
}

#[test]
fn an_always_count_left_unset_is_written_as_zero_in_both_renderings() {
    let mut record = Record::new(&BUILD);
    record.count("pages_rendered", 3);
    let line = record.line();
    assert!(line.contains(" pages_rendered=3 ") && line.contains(" pages_carried=0 ") && !line.contains(" pages=0"), "{line}");
    let counts = &record.json()["counts"];
    assert_eq!((counts["pages_rendered"].as_u64(), counts["pages_carried"].as_u64(), counts.get("pages")), (Some(3), Some(0), None));
}

#[test]
fn each_trigger_writes_its_own_word_from_the_list() {
    use crate::build::BuildTrigger;
    let words: Vec<_> = [
        BuildTrigger::Full,
        BuildTrigger::Unobserved,
        BuildTrigger::Structural(vec![]),
        BuildTrigger::ContentOnly(vec![]),
        BuildTrigger::ImagesSettled { edited: false },
        BuildTrigger::VideosSettled,
    ]
    .iter()
    .map(trigger_word)
    .collect();
    assert_eq!(words, TRIGGERS);
}
