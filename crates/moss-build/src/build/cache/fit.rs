//! Whether a derived file made with another recipe still serves today's page.
//!
//! A published file is made once per source: moss's own encoder and quality
//! changes reach new and changed sources, and an unchanged source keeps the
//! bytes already made unless its file no longer serves the page (another
//! pixel size, over the host's per-file cap). [`Need::fits`] is that rule,
//! whether the facts come from a cached blob's header or from what a publish
//! record says is live.

use std::path::Path;

use super::TransformEntry;

/// How a cache lookup treats an entry made with another recipe.
#[derive(Clone, Copy)]
pub enum Fit<'a> {
    /// Only today's recipe: a verdict rather than a published file, or a build
    /// asked to make media again.
    Exact,
    /// Kept while it meets today's need. The need is worked out only for an
    /// entry made with another recipe and not already found to fit today's;
    /// a need of `None` keeps only an entry already found to fit.
    Kept(&'a dyn Fn() -> Option<Need>),
}

/// What today's page needs of a derived file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Need {
    /// Exactly this pixel size.
    Dims(u32, u32),
    /// A `source`-sized picture scaled to fit `bound` on its long edge:
    /// unscaled when it fits, else its long edge at the bound and its short
    /// edge within the resizer's rounding.
    Scaled { source: (u32, u32), bound: u32 },
    /// At most this many bytes.
    AtMost(u64),
    /// Any file of its kind: a still whose size the page does not read.
    Any,
}

/// The facts about a derived file that [`Need::fits`] reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Facts {
    pub bytes: u64,
    pub dims: Option<(u32, u32)>,
}

impl Need {
    pub fn fits(self, facts: Facts) -> bool {
        match self {
            Need::Dims(w, h) => facts.dims == Some((w, h)),
            Need::Scaled { source: (w, h), bound } => facts.dims.is_some_and(|got| {
                if bound >= w.max(h) {
                    return got == (w, h);
                }
                let (long, short) = if w >= h { got } else { (got.1, got.0) };
                let exact = f64::from(w.min(h)) * f64::from(bound) / f64::from(w.max(h));
                long == bound && (f64::from(short) - exact).abs() <= 1.0
            }),
            Need::AtMost(cap) => facts.bytes <= cap,
            Need::Any => true,
        }
    }

    fn reads_dims(self) -> bool {
        matches!(self, Need::Dims(..) | Need::Scaled { .. })
    }
}

impl Fit<'_> {
    /// Whether `entry`, resident at `blob`, serves this build. Only an entry
    /// made with another recipe is measured, and its header is read only when
    /// the need is a pixel size.
    pub fn keeps(self, entry: &TransformEntry, blob: &Path, current_params: &serde_json::Value) -> bool {
        if entry.params == *current_params {
            return true;
        }
        let Fit::Kept(today) = self else { return false };
        if entry.kept_for.as_ref() == Some(current_params) {
            return true;
        }
        let Some(need) = today() else { return false };
        let dims = if need.reads_dims() { crate::build::media::decode::sniff_dimensions(blob).ok() } else { None };
        need.fits(Facts { bytes: entry.size, dims })
    }
}
