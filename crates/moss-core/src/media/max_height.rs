//! A hero's height cap (`:::hero {max-height=…}`): a px, rem or vw length, or
//! `clamp()` of px and vw lengths, and nothing else, so the build can evaluate
//! it at any window width and measure the crop it makes. A window's height is
//! unknown to the build, so `vh` and `svh` are not taken.

use std::fmt;

/// A unit the cap may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Px,
    Rem,
    Vw,
}

impl Unit {
    const ALL: [Unit; 3] = [Unit::Px, Unit::Rem, Unit::Vw];

    fn as_str(self) -> &'static str {
        match self {
            Unit::Px => "px",
            Unit::Rem => "rem",
            Unit::Vw => "vw",
        }
    }
}

/// One length: a positive number and its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Length(pub f32, pub Unit);

impl Length {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (number, unit) = Unit::ALL.iter().find_map(|unit| Some((s.strip_suffix(unit.as_str())?, *unit)))?;
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return None;
        }
        let value: f32 = number.parse().ok()?;
        (value.is_finite() && value > 0.0).then_some(Length(value, unit))
    }

    /// In px, for a window `width` px wide at the default 16px root.
    pub fn px(self, width: f32) -> f32 {
        let Length(v, unit) = self;
        match unit {
            Unit::Px => v,
            Unit::Rem => v * 16.0,
            Unit::Vw => v * width / 100.0,
        }
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.0, self.1.as_str())
    }
}

/// The cap: one length, or `clamp(min, preferred, max)` whose three parts are
/// each a px or a vw length. Carried in the AST as the CSS it writes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum MaxHeight {
    Length(Length),
    Clamp([Length; 3]),
}

impl MaxHeight {
    /// `None` for anything outside the grammar: `%`, `em`, `vh`, `calc()`,
    /// `min()`, a bare number, a rem inside `clamp()`.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let Some(inner) = raw.strip_prefix("clamp(").and_then(|r| r.strip_suffix(')')) else {
            return Length::parse(raw).map(MaxHeight::Length);
        };
        let parts: Vec<Length> = inner.split(',').map(Length::parse).collect::<Option<_>>()?;
        let parts: [Length; 3] = parts.try_into().ok()?;
        parts.iter().all(|l| matches!(l.1, Unit::Px | Unit::Vw)).then_some(MaxHeight::Clamp(parts))
    }

    /// The cap in px, for a window `width` px wide.
    pub fn px(&self, width: f32) -> f32 {
        match self {
            MaxHeight::Length(l) => l.px(width),
            MaxHeight::Clamp([min, preferred, max]) => preferred.px(width).min(max.px(width)).max(min.px(width)),
        }
    }
}

/// Parsing rejects NaN, so equality is total.
impl Eq for MaxHeight {}

impl TryFrom<String> for MaxHeight {
    type Error = String;
    fn try_from(raw: String) -> Result<Self, String> {
        MaxHeight::parse(&raw).ok_or(raw)
    }
}

impl From<MaxHeight> for String {
    fn from(cap: MaxHeight) -> String {
        cap.to_string()
    }
}

/// The CSS value, rebuilt from the parsed parts, so nothing else reaches a style attribute.
impl fmt::Display for MaxHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaxHeight::Length(l) => write!(f, "{l}"),
            MaxHeight::Clamp([a, b, c]) => write!(f, "clamp({a}, {b}, {c})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_accepted_form_parses_and_evaluates() {
        for (raw, at_1280) in [
            ("500px", 500.0),
            ("30rem", 480.0),
            ("50vw", 640.0),
            ("12.5vw", 160.0),
            ("clamp(420px, 50vw, 680px)", 640.0),
            ("clamp(420px,30vw,680px)", 420.0),
            ("clamp(10vw, 900px, 60vw)", 768.0),
        ] {
            let cap = MaxHeight::parse(raw).unwrap_or_else(|| panic!("{raw} is in the grammar"));
            assert!((cap.px(1280.0) - at_1280).abs() < 0.01, "{raw}: {}", cap.px(1280.0));
            assert_eq!(MaxHeight::parse(&cap.to_string()), Some(cap), "{raw} round-trips as {cap}");
            assert_eq!(serde_json::from_str::<MaxHeight>(&serde_json::to_string(&cap).unwrap()).unwrap(), cap);
        }
        assert_eq!(MaxHeight::parse("clamp(420px,50vw,680px)").unwrap().to_string(), "clamp(420px, 50vw, 680px)");
    }

    #[test]
    fn anything_else_is_rejected() {
        for raw in [
            "", "none", "auto", "500", "50%", "3em", "60vh", "75svh", "-10px", "0px", "px", "1e3px", "500PX", "calc(100vh - 2rem)", "min(500px, 60vh)",
            "clamp(420px, 50vh, 680px)", "clamp(20rem, 50vw, 680px)", "clamp(420px, 680px)", "clamp(1px, 2px, 3px, 4px)", "clamp(420px, 50vw, 680px",
            "500px; background: red", "var(--x)",
        ] {
            assert_eq!(MaxHeight::parse(raw), None, "{raw:?} is outside the grammar");
        }
    }
}
