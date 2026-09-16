//! One string somebody outside moss wrote, made safe to draw and to say.
//!
//! Plugins and the registry supply names, labels, descriptions and reasons
//! that moss renders inside sentences of its own — "Enter your credentials for
//! X", "X was withdrawn: <reason>". Two properties have to hold before any of
//! them reaches a screen or a screen reader, and they are not the renderer's
//! to decide:
//!
//! - **No invisible direction control.** A `U+202E` inside a name does not
//!   reverse the name, it reverses the sentence moss wrote around it, so a
//!   plugin can make moss appear to say something moss did not say.
//! - **A bound.** Length is the author's to choose and moss's to limit; a grid
//!   lays tiles out from the text they carry, and `aria-label` has no layout to
//!   push back.
//!
//! It lives here, in the crate with no I/O, because both sides call it: the
//! registry client in `src-tauri`, and in `moss-build` the settings-field
//! parser, the setup-verdict deserializer and `Verb::normalized`. The
//! placement was a bet when it was written and the second caller has since
//! landed, which is what the `children_per_dir` waiver bought.

/// A name: it names a thing, it does not explain one.
pub const MAX_NAME: usize = 64;

/// A sentence: long enough for a real advisory, short of a paragraph.
pub const MAX_SENTENCE: usize = 280;

/// Drop every character that occupies no space and can move the ones around it.
///
/// Split out of [`bounded`] for the caller that needs the strip WITHOUT the
/// cut: a job verb carries its own 24-character clamp and would otherwise
/// grow a second copy of this set, which is how the two drift.
pub fn stripped(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c,
                    '\u{061c}'
                    | '\u{200e}'..='\u{200f}'
                    | '\u{2028}'..='\u{2029}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

/// Strip what cannot be seen, then cut to `max` characters.
///
/// Truncation is on a CHARACTER boundary: byte slicing panics on the first
/// author whose 64th byte lands mid-codepoint, and plenty of real plugin names
/// are not ASCII. Control and format characters are dropped BEFORE the count,
/// because they are not length.
pub fn bounded(value: &str, max: usize) -> String {
    let visible = stripped(value);
    if visible.chars().count() <= max {
        return visible;
    }
    let mut kept: String = visible.chars().take(max).collect();
    kept.push('…');
    kept
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authored_string_is_bounded_and_carries_no_invisible_control() {
        assert_eq!(bounded("Ordinary Name", MAX_NAME), "Ordinary Name");

        let clamped = bounded(&"x".repeat(MAX_NAME + 10), MAX_NAME);
        assert_eq!(clamped.chars().count(), MAX_NAME + 1, "clamped, plus the ellipsis");
        assert!(clamped.ends_with('…'), "and it says it was cut");

        // Counted in characters, not bytes: this is a panic if the cut lands
        // mid-codepoint, and plenty of real plugin names are not ASCII.
        let cjk = bounded(&"。".repeat(MAX_NAME + 5), MAX_NAME);
        assert_eq!(cjk.chars().count(), MAX_NAME + 1);

        // Dropped before the count, because they are not length.
        assert_eq!(bounded("Safe\u{202e}gpj.exe", MAX_NAME), "Safegpj.exe");
        assert_eq!(bounded("a\u{061c}b\u{2028}c\u{2069}d\ne", MAX_NAME), "abcde");

    }
}
