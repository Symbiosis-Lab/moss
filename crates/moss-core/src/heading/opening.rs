//! The label of an untitled page: the opening words of its body.

const CJK_CHARS: usize = 20;
const WORDS: usize = 8;

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}')
}

/// About the first 20 characters of CJK text or 8 words of other text, with an
/// ellipsis when the text goes on. Whitespace runs count as one space.
/// `None` when there is no text.
pub fn opening_words(plain: &str) -> Option<String> {
    let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let letters = text.chars().filter(|c| !c.is_whitespace());
    let cjk = letters.clone().filter(|&c| is_cjk(c)).count();
    if cjk * 2 > letters.count() {
        let head: String = text.chars().take(CJK_CHARS).collect();
        let cut = head.len() < text.len();
        return Some(if cut { format!("{}…", head.trim_end()) } else { head }).filter(|s| !s.is_empty());
    }
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let head = words.iter().take(WORDS).copied().collect::<Vec<_>>().join(" ");
    match words.len() {
        0 => None,
        n if n > WORDS => Some(format!("{head}…")),
        _ => Some(head),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_text_is_cut_by_characters() {
        let text = "今天和朋友在院子里吃了火锅，天气很冷，大家都很开心，一直聊到很晚才散。";
        assert_eq!(opening_words(text).as_deref(), Some("今天和朋友在院子里吃了火锅，天气很冷，大…"));
    }

    #[test]
    fn short_text_is_kept_whole() {
        assert_eq!(opening_words("今天吃火锅。").as_deref(), Some("今天吃火锅。"));
        assert_eq!(opening_words("Three herons").as_deref(), Some("Three herons"));
    }

    #[test]
    fn other_text_is_cut_by_words() {
        let text = "Walked along the river this morning and saw three herons";
        assert_eq!(opening_words(text).as_deref(), Some("Walked along the river this morning and saw…"));
    }

    #[test]
    fn no_text_gives_none() {
        assert_eq!(opening_words("  \n "), None);
    }
}
