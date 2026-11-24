//! A released contract name never leaves silently.
//!
//! `src/contract/released-contract.txt` lists every class, `data-*` value and
//! `--moss-*` property a release has shipped. Each must still be in the
//! contract or have a row in `contract::retired`, which is what the build
//! tells a theme's author. The file is read here only, never compiled into
//! the library. At a release cut, `CONTRACT_LOCK=update` adds the live names
//! to it; it never drops one.

use moss_core::contract::components::COMPONENTS;
use moss_core::contract::custom_props::CUSTOM_PROPS;
use moss_core::contract::retired::{find, RetiredKind, ALIAS_FLOOR, RETIRED};
use moss_core::contract::shortcodes::catalog;
use moss_core::schema_fields::BUILTIN_FIELDS;
use moss_core::contract::tokens::load_tokens;
use std::collections::BTreeSet;

const LOCK: &str = include_str!("../../src/contract/released-contract.txt");

fn live() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for c in COMPONENTS {
        names.insert(format!("class {}", c.class));
        for a in c.data_attrs {
            for v in a.values {
                names.insert(format!("data {}[{}={}]", c.class, a.name, v));
            }
        }
    }
    for group in load_tokens().expect("tokens").groups {
        for t in group.entries {
            names.insert(format!("property --{}", t.name));
        }
    }
    for p in CUSTOM_PROPS {
        names.insert(format!("property {}", p.name));
    }
    // What an author writes in a folder, read from the parser's and the schema's own tables.
    for e in catalog().into_iter().filter(|e| e.authorable) {
        names.insert(format!("shortcode {}", e.name));
        for a in e.attrs {
            names.insert(format!("shortcode-attr {}", a.name));
        }
    }
    for f in BUILTIN_FIELDS {
        names.insert(format!("front-matter {}", f.name));
    }
    // Config keys are assembled in moss-build, out of this crate's reach, so a
    // locked `config-key` line is never reported as gone here.
    names
}

fn locked() -> BTreeSet<&'static str> {
    LOCK.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect()
}

fn kind_of(line: &str) -> Option<(RetiredKind, &str)> {
    let (kind, name) = line.split_once(' ')?;
    let kind = match kind {
        "class" => RetiredKind::Class,
        "data" => RetiredKind::Data,
        "property" => RetiredKind::Property,
        "shortcode" => RetiredKind::Shortcode,
        "shortcode-attr" => RetiredKind::ShortcodeAttr,
        "front-matter" => RetiredKind::FrontMatter,
        "config-key" => RetiredKind::ConfigKey,
        _ => return None,
    };
    Some((kind, name))
}

fn version(v: &str) -> Vec<u32> {
    v.split('.').map(|n| n.parse().expect("numeric version")).collect()
}

#[test]
fn contract_lock_every_released_name_is_live_or_retired() {
    if std::env::var("CONTRACT_LOCK").as_deref() == Ok("update") {
        let header: String = LOCK.lines().take_while(|l| l.starts_with('#')).map(|l| format!("{l}\n")).collect();
        let mut all: BTreeSet<String> = locked().into_iter().map(String::from).collect();
        all.extend(live());
        let body: String = all.iter().map(|l| format!("{l}\n")).collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/contract/released-contract.txt");
        std::fs::write(path, header + &body).expect("write released-contract.txt");
        return;
    }
    let live = live();
    let mut gone = Vec::new();
    for line in locked() {
        let (kind, name) = kind_of(line).unwrap_or_else(|| panic!("unreadable line in released-contract.txt: {line}"));
        if !live.contains(line) && find(kind, name).is_none() {
            gone.push(line);
        }
    }
    assert!(
        gone.is_empty(),
        "released names left the contract with no row in contract::retired:\n  {}\n\n\
         A theme may select these. Add a row saying what replaces each, or that nothing does; \
         the build shows it to the theme's author.",
        gone.join("\n  ")
    );
}

#[test]
fn contract_lock_retired_names_are_not_live() {
    let live = live();
    let reused: Vec<String> = RETIRED
        .iter()
        .filter(|r| !r.aliased)
        .map(|r| format!("{} {}", r.kind.slug(), r.name))
        .filter(|l| live.contains(l))
        .collect();
    assert!(
        reused.is_empty(),
        "names both live and retired:\n  {}\n\nA retired name never comes back with a new meaning; \
         if it returns unchanged, delete its retired row.",
        reused.join("\n  ")
    );
}

#[test]
fn contract_lock_a_rename_from_the_alias_floor_on_stays_aliased() {
    let unaliased: Vec<&str> = RETIRED
        .iter()
        .filter(|r| r.replaced_by().is_some() && !r.aliased)
        .filter(|r| r.retired_in.is_some_and(|v| version(v) >= version(ALIAS_FLOOR)))
        .map(|r| r.name)
        .collect();
    assert!(
        unaliased.is_empty(),
        "renamed from {ALIAS_FLOOR} on without an alias: {unaliased:?}. moss keeps writing the old name \
         beside its replacement so a theme written against it keeps working."
    );
}

/// A release's date in the CHANGELOG, or `None` while it is unreleased.
fn released_on(changelog: &str, v: &str) -> Option<(u32, u32, u32)> {
    let head = format!("## [{v}] - ");
    let line = changelog.lines().find_map(|l| l.strip_prefix(head.as_str()))?;
    date(line.trim())
}

fn date(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.splitn(3, '-').map(|n| n.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Removals that skip the year of notice. Each one changes no theme's page,
/// so the waiver is pinned here, where adding another is a reviewed test edit.
const NOTICE_WAIVED: &[&str] = &[
    // The honeypot div went with it, and its one rule only hid that div.
    "class:moss-form__hp",
];

/// A feature leaves only after a year of notes saying it is going: a removal
/// from the alias floor on names the day the notes began, at least 12 months
/// before its release. An unreleased removal counts from today, which is
/// never later than its release.
#[test]
fn contract_lock_a_removal_had_twelve_months_notice() {
    let changelog = include_str!("../../../../CHANGELOG.md");
    let today = {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let (y, m, d) = moss_core::date::civil_from_days(secs.div_euclid(86_400));
        (y as u32, m, d)
    };
    let early: Vec<String> = RETIRED
        .iter()
        .filter(|r| !r.aliased && !NOTICE_WAIVED.contains(&r.id().as_str()))
        .filter(|r| r.retired_in.is_some_and(|v| version(v) >= version(ALIAS_FLOOR)))
        .filter(|r| {
            let gone = released_on(changelog, r.retired_in.unwrap()).unwrap_or(today);
            match r.deprecated_on.and_then(date) {
                Some((y, m, d)) => (y + 1, m, d) > gone,
                None => true,
            }
        })
        .map(|r| r.id())
        .collect();
    assert!(
        early.is_empty(),
        "removed without 12 months of notice: {early:?}. Set the row's deprecated_on to the day the build \
         first said it was going, and remove it no sooner than a year after."
    );
}

/// A replacement names classes and properties that exist, so the warning
/// never sends an author to another dead name.
#[test]
fn contract_lock_replacements_name_live_classes() {
    let live = live();
    let class_ref = regex::Regex::new(r"\.([A-Za-z0-9_-]+)").unwrap();
    let mut dead = Vec::new();
    for r in RETIRED {
        let Some(now) = r.replaced_by() else { continue };
        match r.kind {
            RetiredKind::Property => {
                if !live.contains(&format!("property {now}")) {
                    dead.push(format!("{} -> {now}", r.name));
                }
            }
            RetiredKind::Class | RetiredKind::Data => {
                for c in class_ref.captures_iter(now) {
                    if !live.contains(&format!("class {}", &c[1])) {
                        dead.push(format!("{} -> .{}", r.name, &c[1]));
                    }
                }
            }
            _ => {}
        }
    }
    assert!(dead.is_empty(), "retired rows point at names moss does not write:\n  {}", dead.join("\n  "));
}
