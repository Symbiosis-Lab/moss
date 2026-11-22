//! The site scripts that keep their own copy of a few interface strings,
//! held to the string table.

use super::*;

/// The scroll row's ‹ › buttons are built by a site script, which keeps
/// its own copy of "Previous" and "Next"; this holds that copy to the string
/// table.
#[test]
fn scroll_row_labels_match_the_site_strings() {
    let script = include_str!("../js-src/site/scroll-row.ts");
    for lang in Language::ALL {
        let key = lang.as_bcp47_attr().to_lowercase();
        let line = script
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{key}:")) || l.trim_start().starts_with(&format!("\"{key}\":")))
            .unwrap_or_else(|| panic!("scroll-row.ts has no labels for {key}"));
        for word in ["previous", "next"] {
            let expected = format!("{word}: \"{}\"", t(lang, word));
            assert!(line.contains(&expected), "scroll-row.ts {key} labels must contain {expected}: {line}");
        }
    }
}

/// The sliding strips' pause button is built by a site script with its
/// own copy of the two labels; this holds that copy to the string table.
#[test]
fn marquee_labels_match_the_site_strings() {
    let script = include_str!("../js-src/site/marquee.ts");
    for lang in Language::ALL {
        let key = lang.as_bcp47_attr().to_lowercase();
        let line = script
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{key}:")) || l.trim_start().starts_with(&format!("\"{key}\":")))
            .unwrap_or_else(|| panic!("marquee.ts has no labels for {key}"));
        for (field, word) in [("pause", "pause_motion"), ("play", "play_motion")] {
            let expected = format!("{field}: \"{}\"", t(lang, word));
            assert!(line.contains(&expected), "marquee.ts {key} labels must contain {expected}: {line}");
        }
    }
}
