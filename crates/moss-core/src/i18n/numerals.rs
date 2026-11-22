//! Chinese numeral conversion for vertical CJK typesetting.
//!
//! Used for article counts (3→三) and dates (2025→二〇二五) in vertical mode.
//! Only converts UI chrome numerals — never touches user-authored content.

/// Single digit to Chinese character
fn digit_to_chinese(d: u8) -> char {
    match d {
        0 => '〇',
        1 => '一',
        2 => '二',
        3 => '三',
        4 => '四',
        5 => '五',
        6 => '六',
        7 => '七',
        8 => '八',
        9 => '九',
        _ => '?',
    }
}

/// Convert integer to Chinese numeral string.
/// Handles 0-99 with proper tens place: 10→十, 11→十一, 20→二十, 99→九十九
pub fn to_chinese_numeral(n: usize) -> String {
    if n == 0 {
        return "〇".to_string();
    }
    if n < 10 {
        return digit_to_chinese(n as u8).to_string();
    }
    if n < 100 {
        let tens = n / 10;
        let ones = n % 10;
        let mut s = String::new();
        if tens > 1 {
            s.push(digit_to_chinese(tens as u8));
        }
        s.push('十');
        if ones > 0 {
            s.push(digit_to_chinese(ones as u8));
        }
        return s;
    }
    // For 100+, just spell digit-by-digit (rare for article counts)
    n.to_string().chars().map(|c| {
        digit_to_chinese(c.to_digit(10).unwrap_or(0) as u8)
    }).collect()
}

/// Convert year string digit-by-digit: "2025" → "二〇二五"
pub fn to_chinese_year(year: &str) -> String {
    year.chars().map(|c| {
        if let Some(d) = c.to_digit(10) {
            digit_to_chinese(d as u8)
        } else {
            c
        }
    }).collect()
}

/// Convert month number to Chinese: 1→一月, 9→九月, 12→十二月
pub fn to_chinese_month(month: u32) -> String {
    format!("{}月", to_chinese_numeral(month as usize))
}

/// Convert day number to Chinese: 1→一日, 24→二十四日
pub fn to_chinese_day(day: u32) -> String {
    format!("{}日", to_chinese_numeral(day as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_digits() {
        assert_eq!(to_chinese_numeral(0), "〇");
        assert_eq!(to_chinese_numeral(1), "一");
        assert_eq!(to_chinese_numeral(5), "五");
        assert_eq!(to_chinese_numeral(9), "九");
    }

    #[test]
    fn test_teens() {
        assert_eq!(to_chinese_numeral(10), "十");
        assert_eq!(to_chinese_numeral(11), "十一");
        assert_eq!(to_chinese_numeral(12), "十二");
        assert_eq!(to_chinese_numeral(19), "十九");
    }

    #[test]
    fn test_tens() {
        assert_eq!(to_chinese_numeral(20), "二十");
        assert_eq!(to_chinese_numeral(30), "三十");
        assert_eq!(to_chinese_numeral(99), "九十九");
    }

    #[test]
    fn test_hundreds() {
        assert_eq!(to_chinese_numeral(100), "一〇〇");
        assert_eq!(to_chinese_numeral(123), "一二三");
    }

    #[test]
    fn test_year() {
        assert_eq!(to_chinese_year("2025"), "二〇二五");
        assert_eq!(to_chinese_year("1694"), "一六九四");
    }

    #[test]
    fn test_month() {
        assert_eq!(to_chinese_month(1), "一月");
        assert_eq!(to_chinese_month(9), "九月");
        assert_eq!(to_chinese_month(12), "十二月");
    }

    #[test]
    fn test_day() {
        assert_eq!(to_chinese_day(1), "一日");
        assert_eq!(to_chinese_day(24), "二十四日");
    }
}
