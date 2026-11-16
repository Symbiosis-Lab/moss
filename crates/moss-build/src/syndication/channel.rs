//! What a channel's network can carry, as its manifest declares it under
//! `contributes.channel.syndication`.

use serde::{Deserialize, Serialize};

/// The shape of one copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// The post's whole text.
    Short,
    /// A post written in parts, each part a reply to the one before.
    Thread,
    /// The whole article, where the network has an article format.
    Long,
    /// The article's description or opening, with a link to the original.
    /// `handoff` reads as a teaser.
    #[serde(alias = "handoff")]
    Teaser,
}

/// How the network measures a post's length.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Counting {
    /// Counted as Unicode scalar values, which is never fewer than the
    /// grapheme clusters a network counts, so a copy that fits here fits there.
    #[default]
    Graphemes,
    /// Unicode scalar values.
    Chars,
    /// UTF-16 code units, as a JavaScript string's length counts them: an
    /// emoji outside the basic plane counts two.
    Utf16,
    /// Latin text counts one, everything else two, and every link
    /// `link_chars` (X's rule).
    Weighted,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImageLimits {
    #[serde(default)]
    pub max: u32,
    /// The network shows alt text, so moss sends it.
    #[serde(default)]
    pub alt: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelSpec {
    #[serde(default)]
    pub forms: Vec<Form>,
    #[serde(default)]
    pub max_chars: Option<u32>,
    #[serde(default)]
    pub counting: Counting,
    /// What every address counts as, where the network counts it so (X and
    /// Mastodon: 23). Weighted counting assumes 23.
    #[serde(default)]
    pub link_chars: Option<u32>,
    #[serde(default)]
    pub images: Option<ImageLimits>,
    /// The plugin passes the job's idempotency key to its network, so a retry
    /// after an interrupted send cannot make a second copy.
    #[serde(default)]
    pub idempotent: bool,
    /// The setting holding the author's own server (a Mastodon instance, a
    /// self-hosted data server), whose host the channel's copies live on.
    #[serde(default)]
    pub host_setting: Option<String>,
    /// The on/off setting without which the channel does not take `long`:
    /// for a network whose article format only some accounts have.
    #[serde(default)]
    pub long_setting: Option<String>,
}

impl ChannelSpec {
    pub fn takes(&self, form: Form) -> bool {
        self.forms.contains(&form)
    }

    pub fn max_images(&self) -> usize {
        self.images.as_ref().map_or(0, |i| i.max as usize)
    }

    /// An article's pictures are part of its HTML: all of them go, unless the
    /// channel states a limit. `images.max` is what a short post may attach,
    /// read here as an article's limit too: a channel that states it for its
    /// posts has its articles cut to the same number, until the two are
    /// separate fields.
    pub fn max_article_images(&self) -> usize {
        self.images.as_ref().map_or(usize::MAX, |i| i.max as usize)
    }

    /// The text's length as this network counts it.
    pub fn length(&self, text: &str) -> usize {
        let weighted = self.counting == Counting::Weighted;
        let link = if weighted { Some(self.link_chars.unwrap_or(23)) } else { self.link_chars };
        text.split_inclusive(char::is_whitespace)
            .map(|word| {
                let bare = word.trim_end();
                match link {
                    Some(n) if bare.starts_with("http://") || bare.starts_with("https://") => n as usize + word[bare.len()..].chars().count(),
                    _ => word.chars().map(|c| self.width(c)).sum(),
                }
            })
            .sum()
    }

    fn width(&self, c: char) -> usize {
        match self.counting {
            Counting::Weighted if !is_light(c) => 2,
            Counting::Utf16 => c.len_utf16(),
            _ => 1,
        }
    }

    pub fn fits(&self, text: &str) -> bool {
        self.max_chars.is_none_or(|max| self.length(text) <= max as usize)
    }
}

/// The ranges X counts as one: Latin and the punctuation around it.
fn is_light(c: char) -> bool {
    matches!(c as u32, 0..=4351 | 8192..=8205 | 8208..=8223 | 8242..=8247)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_counts_what_the_network_counts_it_as() {
        let plain = ChannelSpec { counting: Counting::Graphemes, ..Default::default() };
        let mastodon = ChannelSpec { counting: Counting::Graphemes, link_chars: Some(23), ..Default::default() };
        assert_eq!(plain.length("see https://a.b"), 15);
        assert_eq!(mastodon.length("see https://a.b"), 27);
        assert_eq!(mastodon.length("秋\u{3000}https://a.b"), 25, "a wide space after the address is one, not three bytes");
    }

    fn spec(counting: Counting, max: u32) -> ChannelSpec {
        ChannelSpec { max_chars: Some(max), counting, link_chars: Some(23), ..Default::default() }
    }

    #[test]
    fn weighted_counting_is_xs_rule() {
        let x = spec(Counting::Weighted, 280);
        assert_eq!(x.length("hello"), 5);
        assert_eq!(x.length("秋天"), 4);
        assert_eq!(x.length("see https://example.org/a/very/long/path/indeed"), 4 + 23);
    }

    #[test]
    fn utf16_counting_is_a_javascript_strings_length() {
        let js = spec(Counting::Utf16, 240);
        assert_eq!(js.length("秋天 🍁"), 5);
        assert_eq!(spec(Counting::Chars, 240).length("秋天 🍁"), 4);
    }

    #[test]
    fn a_text_fits_when_it_is_within_the_limit() {
        let bsky = spec(Counting::Graphemes, 5);
        assert!(bsky.fits("hello"));
        assert!(!bsky.fits("hello!"));
        assert!(ChannelSpec::default().fits(&"x".repeat(10_000)), "no limit declared, no limit applied");
    }

    #[test]
    fn a_manifest_block_reads_with_defaults() {
        let spec: ChannelSpec = serde_json::from_value(serde_json::json!({
            "forms": ["short", "thread", "teaser"], "max_chars": 300, "images": {"max": 4, "alt": true}
        }))
        .unwrap();
        assert!(spec.takes(Form::Thread) && !spec.takes(Form::Long));
        assert_eq!(spec.counting, Counting::Graphemes);
        assert_eq!(spec.max_images(), 4);
        assert!(!spec.idempotent);
    }

    #[test]
    fn a_copy_recorded_as_a_handoff_reads_as_a_teaser() {
        assert_eq!(serde_json::from_str::<Form>(r#""handoff""#).unwrap(), Form::Teaser);
        assert_eq!(serde_json::to_string(&Form::Teaser).unwrap(), r#""teaser""#);
    }
}
