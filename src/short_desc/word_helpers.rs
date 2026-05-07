use std::sync::OnceLock;

use regex::Regex;

use super::ShortDescription;

/// Gender and context hints used by word-modification and list-joining helpers.
#[derive(Debug, Clone, Default)]
pub struct WordHints {
    pub is_female: bool,
    pub is_male: bool,
    pub occupation: bool,
}

/// Arguments for [`ShortDescription::add2desc`].
pub(super) struct Add2DescArgs<'a> {
    pub(super) props: &'a [u64],
    pub(super) hints: &'a WordHints,
    pub(super) prefix: Option<&'a str>,
    pub(super) txt_key: Option<&'a str>,
}

fn split_link_wiki_pipe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\[\[.+\|)(.+)(\]\])$").expect("regex is valid"))
}

fn split_link_wiki_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\[\[)(.+)(\]\])$").expect("regex is valid"))
}

/// Matches an HTML anchor tag: captures (opening tag, inner text, closing tag).
pub(super) fn html_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(<a.+?>)(.+)(</a>)$").expect("regex is valid"))
}

fn clean_spaces_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r" +").expect("regex is valid"))
}

fn clean_space_comma_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r" ,").expect("regex is valid"))
}

pub(super) fn uc_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.as_str())
        }
    }
}

/// Split a link string into parts: (full_match, before, inner_text, after).
/// Returns `None` for plain (non-link) text.
pub(super) fn split_link(v: &str) -> Option<(String, String, String, String)> {
    if let Some(caps) = split_link_wiki_pipe_re().captures(v) {
        return Some((
            caps.get(0)?.as_str().to_string(),
            caps.get(1)?.as_str().to_string(),
            caps.get(2)?.as_str().to_string(),
            caps.get(3)?.as_str().to_string(),
        ));
    }

    if let Some(caps) = split_link_wiki_re().captures(v) {
        let inner = caps.get(2)?.as_str();
        return Some((
            caps.get(0)?.as_str().to_string(),
            format!("[[{}|", inner),
            inner.to_string(),
            caps.get(3)?.as_str().to_string(),
        ));
    }

    if let Some(caps) = html_link_re().captures(v) {
        return Some((
            caps.get(0)?.as_str().to_string(),
            caps.get(1)?.as_str().to_string(),
            caps.get(2)?.as_str().to_string(),
            caps.get(3)?.as_str().to_string(),
        ));
    }

    None
}

/// Clean up extra spaces and punctuation artifacts.
pub(super) fn clean_spaces(s: &str) -> String {
    let result = clean_spaces_re().replace_all(s, " ");
    let result = clean_space_comma_re().replace_all(&result, ",");
    result.trim().to_string()
}

/// URL-encode a wiki page title.
pub(super) fn wiki_urlencode(s: &str) -> String {
    let s = s.replace(' ', "_");
    urlencoding::encode(&s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::short_desc::ShortDescription;

    #[test]
    fn test_wiki_urlencode_spaces() {
        assert_eq!(wiki_urlencode("New York"), "New_York");
    }

    #[test]
    fn test_wiki_urlencode_special_chars() {
        assert_eq!(wiki_urlencode("Côte d'Ivoire"), "C%C3%B4te_d%27Ivoire");
    }

    #[test]
    fn test_wiki_urlencode_no_change() {
        assert_eq!(wiki_urlencode("London"), "London");
    }

    #[test]
    fn test_uc_first_unicode() {
        assert_eq!(uc_first("über"), "Über");
    }

    #[test]
    fn test_uc_first_empty() {
        assert_eq!(uc_first(""), "");
    }

    #[test]
    fn test_split_link_wiki_no_pipe() {
        let (full, before, inner, after) = split_link("[[London]]").unwrap();
        assert_eq!(full, "[[London]]");
        assert_eq!(before, "[[London|");
        assert_eq!(inner, "London");
        assert_eq!(after, "]]");
    }

    #[test]
    fn test_split_link_wiki_with_pipe() {
        let (_, before, inner, after) = split_link("[[London|capital of England]]").unwrap();
        assert_eq!(before, "[[London|");
        assert_eq!(inner, "capital of England");
        assert_eq!(after, "]]");
    }

    #[test]
    fn test_split_link_html() {
        let (_, before, inner, after) =
            split_link("<a href='https://example.com'>Example</a>").unwrap();
        assert_eq!(before, "<a href='https://example.com'>");
        assert_eq!(inner, "Example");
        assert_eq!(after, "</a>");
    }

    #[test]
    fn test_split_link_plain_text() {
        assert!(split_link("plain text").is_none());
        assert!(split_link("").is_none());
    }

    #[test]
    fn test_clean_spaces_multiple() {
        assert_eq!(clean_spaces("a   b"), "a b");
    }

    #[test]
    fn test_clean_spaces_space_comma() {
        assert_eq!(clean_spaces("a ,b"), "a,b");
    }

    #[test]
    fn test_clean_spaces_trim() {
        assert_eq!(clean_spaces("  hello  "), "hello");
    }

    #[test]
    fn test_modify_word_fr_acteur() {
        let sd = ShortDescription::new();
        let female = WordHints { is_female: true, ..Default::default() };
        assert_eq!(sd.modify_word("acteur", &female, "fr"), "actrice");
        assert_eq!(sd.modify_word("Acteur", &female, "fr"), "actrice");
    }

    #[test]
    fn test_modify_word_fr_etre_humain() {
        let sd = ShortDescription::new();
        let female = WordHints { is_female: true, ..Default::default() };
        assert_eq!(sd.modify_word("être humain", &female, "fr"), "personne");
    }

    #[test]
    fn test_modify_word_fr_male_unchanged() {
        let sd = ShortDescription::new();
        let male = WordHints { is_male: true, ..Default::default() };
        assert_eq!(sd.modify_word("acteur", &male, "fr"), "acteur");
    }

    #[test]
    fn test_modify_word_de_female_occupation() {
        let sd = ShortDescription::new();
        let female_occ = WordHints { is_female: true, occupation: true, ..Default::default() };
        assert_eq!(sd.modify_word("Schauspieler", &female_occ, "de"), "Schauspielerin");
    }

    #[test]
    fn test_modify_word_de_female_no_occupation() {
        let sd = ShortDescription::new();
        let female = WordHints { is_female: true, occupation: false, ..Default::default() };
        // Without occupation flag, German doesn't transform the word
        assert_eq!(sd.modify_word("Schauspieler", &female, "de"), "Schauspieler");
    }

    #[test]
    fn test_modify_word_unknown_lang_unchanged() {
        let sd = ShortDescription::new();
        let female = WordHints { is_female: true, ..Default::default() };
        assert_eq!(sd.modify_word("actor", &female, "zh"), "actor");
    }

    #[test]
    fn test_list_words_empty() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(sd.list_words(&[], &hints, "en"), "");
    }

    #[test]
    fn test_list_words_single() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(sd.list_words(&["one".to_string()], &hints, "en"), "one");
    }

    #[test]
    fn test_list_words_en_two() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string()], &hints, "en"),
            "a and b"
        );
    }

    #[test]
    fn test_list_words_en_three_oxford_comma() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(
                &["a".to_string(), "b".to_string(), "c".to_string()],
                &hints,
                "en"
            ),
            "a, b, and c"
        );
    }

    #[test]
    fn test_list_words_de() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string(), "c".to_string()], &hints, "de"),
            "a, b und c"
        );
    }

    #[test]
    fn test_list_words_fr() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string()], &hints, "fr"),
            "a et b"
        );
    }

    #[test]
    fn test_list_words_nl() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string()], &hints, "nl"),
            "a en b"
        );
    }

    #[test]
    fn test_list_words_pl() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string()], &hints, "pl"),
            "a i b"
        );
    }

    #[test]
    fn test_list_words_vi_oxford_comma() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(
                &["a".to_string(), "b".to_string(), "c".to_string()],
                &hints,
                "vi"
            ),
            "a, b, và c"
        );
    }

    #[test]
    fn test_list_words_es() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string()], &hints, "es"),
            "a y b"
        );
    }

    #[test]
    fn test_list_words_unknown_lang_comma_separated() {
        let sd = ShortDescription::new();
        let hints = WordHints::default();
        assert_eq!(
            sd.list_words(&["a".to_string(), "b".to_string(), "c".to_string()], &hints, "zh"),
            "a, b, c"
        );
    }

    #[test]
    fn test_modify_word_en_actor_actress_female() {
        let sd = ShortDescription::new();
        let female = WordHints { is_female: true, ..Default::default() };
        assert_eq!(sd.modify_word("actor / actress", &female, "en"), "actress");
    }

    #[test]
    fn test_modify_word_en_actor_actress_male() {
        let sd = ShortDescription::new();
        let male = WordHints { is_male: true, ..Default::default() };
        assert_eq!(sd.modify_word("actor / actress", &male, "en"), "actor");
    }
}

impl ShortDescription {
    /// Apply language-specific word modification (e.g. nationality transformation).
    pub fn txt2(&self, text: &str, key: &str, lang: &str) -> String {
        if let Some(lang_spec) = self.language_specific.get(lang)
            && let Some(key_map) = lang_spec.get(key)
        {
            let link_re = html_link_re();
            if let Some(caps) = link_re.captures(text) {
                let inner = caps.get(2).unwrap().as_str();
                if let Some(replacement) = key_map.get(inner) {
                    return format!(
                        "{}{}{}",
                        caps.get(1).unwrap().as_str(),
                        replacement,
                        caps.get(3).unwrap().as_str()
                    );
                }
            } else if let Some(replacement) = key_map.get(text) {
                return replacement.clone();
            }
        }
        text.to_string()
    }

    /// Modify a word based on gender hints and language.
    pub fn modify_word(&self, word: &str, hints: &WordHints, lang: &str) -> String {
        let lower = word.to_lowercase();
        match lang {
            "en" => {
                if hints.is_female {
                    if lower == "actor" {
                        return "actress".to_string();
                    }
                    if lower == "actor / actress" {
                        return "actress".to_string();
                    }
                } else if hints.is_male && lower == "actor / actress" {
                    return "actor".to_string();
                }
            }
            "fr" => {
                if hints.is_female {
                    if lower == "acteur" {
                        return "actrice".to_string();
                    }
                    if lower == "être humain" {
                        return "personne".to_string();
                    }
                }
            }
            "de" => {
                if hints.is_female && hints.occupation {
                    return format!("{}in", word);
                }
            }
            _ => {}
        }
        word.to_string()
    }

    /// Join a list of words with the appropriate conjunction for the given language.
    pub fn list_words(&self, original_list: &[String], hints: &WordHints, lang: &str) -> String {
        let mut list: Vec<String> = original_list
            .iter()
            .map(|w| self.modify_word(w, hints, lang))
            .collect();

        let conjunction = match lang {
            "en" => "and",
            "de" => "und",
            "fr" | "it" => "et",
            "ga" => "agus",
            "nl" => "en",
            "pl" => "i",
            "vi" => "và",
            "es" | "pt" => "y",
            _ => {
                return list.join(", ");
            }
        };

        match list.len() {
            0 => String::new(),
            1 => list.remove(0),
            2 => format!("{} {} {}", list[0], conjunction, list[1]),
            _ => {
                if lang == "en" || lang == "vi" {
                    let last = list.pop().unwrap();
                    format!("{}, {} {}", list.join(", "), conjunction, last)
                } else {
                    let last = list.pop().unwrap();
                    format!("{} {} {}", list.join(", "), conjunction, last)
                }
            }
        }
    }
}
