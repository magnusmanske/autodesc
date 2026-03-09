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
pub(super) fn split_link(v: &str) -> (String, String, String, String) {
    if let Some(caps) = split_link_wiki_pipe_re().captures(v) {
        return (
            caps.get(0).unwrap().as_str().to_string(),
            caps.get(1).unwrap().as_str().to_string(),
            caps.get(2).unwrap().as_str().to_string(),
            caps.get(3).unwrap().as_str().to_string(),
        );
    }

    if let Some(caps) = split_link_wiki_re().captures(v) {
        let inner = caps.get(2).unwrap().as_str();
        return (
            caps.get(0).unwrap().as_str().to_string(),
            format!("[[{}|", inner),
            inner.to_string(),
            caps.get(3).unwrap().as_str().to_string(),
        );
    }

    if let Some(caps) = html_link_re().captures(v) {
        return (
            caps.get(0).unwrap().as_str().to_string(),
            caps.get(1).unwrap().as_str().to_string(),
            caps.get(2).unwrap().as_str().to_string(),
            caps.get(3).unwrap().as_str().to_string(),
        );
    }

    (String::new(), String::new(), v.to_string(), String::new())
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

impl ShortDescription {
    /// Apply language-specific word modification (e.g. nationality transformation).
    pub fn txt2(&self, text: &str, key: &str, lang: &str) -> String {
        if let Some(lang_spec) = self.language_specific.get(lang)
            && let Some(key_map) = lang_spec.get(key) {
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
