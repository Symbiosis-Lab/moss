use super::*;

const OFFER: &str = r#"
endpoint = "https://booking.example/sites/demo/booking"
timezone = "America/Los_Angeles"
currency = "usd"
start_every = 60
notice_hours = 12
horizon_days = 60

[open]
mon = ["09:00-23:00"]
sat = ["14:00-24:00"]

[cancel]
full_before_hours = 24
late_percent = 50

[pages]
book = "/book/"
done = "/book/done/"
manage = "/book/manage/"
desk = "/book/desk/"

[services.tour]
name = "Guided walk"
per_hour = 4000
hours = { min = 2, max = 6 }
fields = { meeting_point = "required", guests = "optional" }
labels = { guests = "How many guests" }
deposit = { percent = 50, hold_hours = 12, balance_days_before = 7 }

[services.intro]
name = "Intro call"
minutes = 30
price = 0
fields = { about = "optional" }
"#;

fn offer(edit: &str) -> BookingService {
    toml::from_str(&format!("{OFFER}\n{edit}")).expect("offer parses")
}

#[test]
fn the_offer_is_published_as_written() {
    let json = serde_json::to_value(offer("")).unwrap();
    assert_eq!(json["services"]["tour"]["per_hour"], 4000);
    assert_eq!(json["services"]["tour"]["hours"]["min"], 2);
    assert_eq!(json["services"]["intro"]["price"], 0);
    assert_eq!(json["open"]["sat"][0], "14:00-24:00");
    assert_eq!(json["cancel"]["late_percent"], 50);
    assert_eq!(json["pages"]["manage"], "/book/manage/");
    assert_eq!(json["pages"]["desk"], "/book/desk/");
    assert_eq!(json["services"]["tour"]["deposit"]["percent"], 50);
    assert_eq!(json["services"]["tour"]["deposit"]["balance_days_before"], 7);
    assert!(json["services"]["intro"].get("deposit").is_none());
    assert_eq!(json["endpoint"], "https://booking.example/sites/demo/booking");
    assert!(json["services"]["intro"].get("per_hour").is_none(), "a fixed service carries no hourly fields");
    assert!(json.get("enabled").is_none());
    assert_eq!(offer("").problem(), None);
}

#[test]
fn a_misspelt_key_is_an_error_not_a_silent_default() {
    let err = toml::from_str::<BookingService>(&OFFER.replace("notice_hours", "notice_hour")).unwrap_err();
    assert!(err.to_string().contains("notice_hour"), "{err}");
}

#[test]
fn each_broken_rule_names_itself() {
    let upper: BookingService = toml::from_str(&OFFER.replace("currency = \"usd\"", "currency = \"USD\"")).unwrap();
    assert!(upper.problem().unwrap().contains("lowercase ISO"));
    let cases = [
        ("[services.Bad]\nname = \"x\"\nminutes = 30\nprice = 0", "service 'Bad'"),
        ("[services.mixed]\nname = \"x\"\nper_hour = 1\nhours = { min = 1, max = 2 }\nminutes = 30", "service 'mixed'"),
        ("[services.short]\nname = \"x\"\nper_hour = 1\nhours = { min = 3, max = 2 }", "hours.min"),
        ("[services.odd]\nname = \"x\"\nminutes = 30\nprice = 0\nfields = { Where = \"required\" }", "field 'Where'"),
        ("[services.odd2]\nname = \"x\"\nminutes = 30\nprice = 0\nfields = { about = \"maybe\" }", "field 'about'"),
        ("[services.odd3]\nname = \"x\"\nminutes = 30\nprice = 0\nlabels = { about = \"About\" }", "labels.about"),
        ("[services.gift]\nname = \"x\"\nminutes = 30\nprice = 0\ndeposit = { percent = 50, hold_hours = 12, balance_days_before = 7 }", "free service takes no deposit"),
        ("[services.whole]\nname = \"x\"\nminutes = 30\nprice = 100\ndeposit = { percent = 100, hold_hours = 12, balance_days_before = 7 }", "deposit.percent"),
    ];
    for (edit, expected) in cases {
        let problem = offer(edit).problem().unwrap_or_default();
        assert!(problem.contains(expected), "{edit:?} gave {problem:?}");
    }
}

#[test]
fn open_windows_pages_and_endpoint_are_checked() {
    let mut o = offer("");
    o.open.insert("mon".into(), vec!["9:00-17:00".into()]);
    assert!(o.problem().unwrap().contains("open.mon"));
    let mut o = offer("");
    o.open.insert("funday".into(), vec!["09:00-17:00".into()]);
    assert!(o.problem().unwrap().contains("open.funday"));
    let mut o = offer("");
    o.open.insert("tue".into(), vec!["24:00-23:00".into()]);
    assert!(o.problem().unwrap().contains("open.tue"));
    let mut o = offer("");
    o.open.insert("wed".into(), vec!["+9:00-17:00".into()]);
    assert!(o.problem().unwrap().contains("open.wed"));
    let mut o = offer("");
    o.pages.done = "book/done/".into();
    assert!(o.problem().unwrap().contains("pages.done"));
    let mut o = offer("");
    o.pages.done = "//elsewhere.example/".into();
    assert!(o.problem().unwrap().contains("pages.done"), "a page on another host would send a paying client away");
    let mut o = offer("");
    o.timezone = "Foo/Bar".into();
    assert!(o.problem().unwrap().contains("timezone"));
    let mut o = offer("");
    o.timezone = "UTC".into();
    assert_eq!(o.problem(), None, "UTC is a time zone name");
    for page in ["/\t/elsewhere.example/", "/\\elsewhere.example/", "/book/\n"] {
        let mut o = offer("");
        o.pages.done = page.into();
        assert!(o.problem().unwrap_or_default().contains("pages.done"), "{page:?} must be refused");
    }
    let mut o = offer("");
    o.pages.desk = None;
    assert!(o.problem().unwrap().contains("pages.desk"), "a deposit needs the desk's page");
    let mut o = offer("");
    o.endpoint = Some("booking.example".into());
    assert!(o.problem().unwrap().contains("endpoint"));
}

#[test]
fn a_written_charge_is_read_and_must_match_the_keys() {
    let o = offer("[services.call]\nname = \"x\"\ncharge = \"fixed\"\nminutes = 30\nprice = 900");
    assert_eq!(o.problem(), None);
    assert_eq!(serde_json::to_value(&o).unwrap()["services"]["call"]["charge"], "fixed");
    let cases = [
        ("[services.call]\nname = \"x\"\ncharge = \"hourly\"\nminutes = 30\nprice = 900", "keys of the other kind"),
        ("[services.walk]\nname = \"x\"\ncharge = \"fixed\"\nper_hour = 100\nhours = { min = 1, max = 2 }", "keys of the other kind"),
        ("[services.call]\nname = \"x\"\ncharge = \"daily\"\nminutes = 30\nprice = 900", "must be \"hourly\" or \"fixed\""),
    ];
    for (edit, expected) in cases {
        let problem = offer(edit).problem().unwrap_or_default();
        assert!(problem.contains(expected), "{edit:?} gave {problem:?}");
    }
}
