//! The calendar dates a partial date spans. The display forms are
//! `moss_core::i18n::dates`.

use moss_core::date::PartialDate;

/// The calendar dates a partial date can denote, as chrono dates. Kept here
/// rather than in moss-core, which has no chrono dependency.
pub trait PartialDateRange {
    /// The earliest calendar date this partial date could denote: a missing
    /// day defaults to the 1st, a missing month to January.
    fn earliest(&self) -> Option<chrono::NaiveDate>;
    /// The latest calendar date this partial date could denote: a missing
    /// day defaults to the last day of the month, a missing month to
    /// December 31st. Used by a `since:` cutoff — a post dated only to the
    /// month is excluded only once every day that month could name is past
    /// the cutoff, not just the 1st.
    fn latest(&self) -> Option<chrono::NaiveDate>;
}

impl PartialDateRange for PartialDate {
    fn earliest(&self) -> Option<chrono::NaiveDate> {
        chrono::NaiveDate::from_ymd_opt(self.year, self.month.unwrap_or(1), self.day.unwrap_or(1))
    }

    fn latest(&self) -> Option<chrono::NaiveDate> {
        match (self.month, self.day) {
            (Some(month), Some(day)) => chrono::NaiveDate::from_ymd_opt(self.year, month, day),
            (Some(month), None) => {
                let (next_year, next_month) =
                    if month == 12 { (self.year + 1, 1) } else { (self.year, month + 1) };
                chrono::NaiveDate::from_ymd_opt(next_year, next_month, 1)
                    .and_then(|d| d.pred_opt())
            }
            (None, _) => chrono::NaiveDate::from_ymd_opt(self.year, 12, 31),
        }
    }
}

