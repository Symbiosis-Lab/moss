use super::*;

fn rec(t: &str, name: &str, value: &str) -> DnsRecord {
    DnsRecord { record_type: t.into(), name: name.into(), value: value.into(), ttl: None }
}

#[test]
fn normalize_reduces_any_spelling_to_the_apex() {
    for input in ["example.com", "www.example.com", "WWW.Example.com.", "https://www.example.com/"] {
        assert_eq!(normalize_domain(input).unwrap(), "example.com", "{}", input);
    }
}

#[test]
fn only_a_two_label_registered_domain_is_accepted() {
    for bad in ["", "localhost", "exa mple.com", "example.com/path", "blog.example.com", "www.blog.example.com", "example.co.uk", ".com", "a..com"] {
        assert!(normalize_domain(bad).is_err(), "{}", bad);
    }
}

#[test]
fn the_apex_address_is_added_once_and_an_empty_name_means_the_apex() {
    let records = required_records(&[rec("CNAME", "www", "sites.mosspub.com")]);
    assert_eq!((records[0].record_type.as_str(), records[0].name.as_str()), ("A", "@"));
    assert_eq!(records.len(), 2);
    assert_eq!(required_records(&records).len(), 2);

    let from_server = required_records(&[rec("A", "", "203.0.113.7"), rec("CNAME", "www", "sites.mosspub.com")]);
    assert_eq!(from_server.len(), 2, "the server's own apex address replaces ours");
    assert_eq!((from_server[0].name.as_str(), from_server[0].value.as_str()), ("@", "203.0.113.7"));
}

struct Nothing;
impl Lookup for Nothing {
    fn addrs(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

#[test]
fn the_address_and_www_records_are_always_looked_up_even_when_the_lookup_fails() {
    let rows = [rec("A", "@", "203.0.113.7"), rec("CNAME", "www", "sites.mosspub.com"), rec("TXT", "_x.www", "t")];
    let checks = check_records("example.com", &rows, CdnStatus::Active, &Nothing);
    assert_eq!(checks.iter().map(|c| (c.checked_by, c.done)).collect::<Vec<_>>(), [("dns", false), ("dns", false), ("hosting service", true)]);
}
