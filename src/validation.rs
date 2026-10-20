//! Input validation for all API query parameters.
//!
//! Every parameter that comes from the outside must be validated before use.
//! This module provides validation helpers and collects them into a single place.

use wikimisc::entity_id::ItemId;
use wikimisc::server::jsonp::{self, CallbackRules};

/// Allowed languages (ISO 639-1 two-letter codes served by Wikidata + a few extras).
const ALLOWED_LANGS: &[&str] = &[
    "aa", "ab", "ace", "ady", "af", "ak", "als", "am", "an", "ang", "ar", "arc", "ary", "arz",
    "as", "ast", "atj", "av", "avk", "awa", "ay", "az", "azb", "ba", "ban", "bar", "bcl", "be",
    "bg", "bh", "bho", "bi", "bjn", "bm", "bn", "bo", "bpy", "br", "bs", "bug", "bxr", "ca", "cbk",
    "cdo", "ce", "ceb", "ch", "cho", "chr", "chy", "ckb", "co", "cr", "crh", "cs", "csb", "cu",
    "cv", "cy", "da", "de", "din", "diq", "dsb", "dty", "dv", "dz", "ee", "el", "en", "eo", "es",
    "et", "eu", "ext", "fa", "ff", "fi", "fj", "fo", "fr", "frp", "frr", "fur", "fy", "ga", "gag",
    "gan", "gd", "gl", "glk", "gn", "gom", "gor", "got", "gu", "gv", "ha", "hak", "haw", "he",
    "hi", "hif", "ho", "hr", "hsb", "ht", "hu", "hy", "hyw", "hz", "ia", "id", "ie", "ig", "ii",
    "ik", "ilo", "inh", "io", "is", "it", "iu", "ja", "jam", "jbo", "jv", "ka", "kaa", "kab",
    "kbd", "kbp", "kg", "ki", "kj", "kk", "kl", "km", "kn", "ko", "koi", "kr", "krc", "ks", "ksh",
    "ku", "kv", "kw", "ky", "la", "lad", "lb", "lbe", "lez", "lfn", "lg", "li", "lij", "lld",
    "lmo", "ln", "lo", "lrc", "lt", "ltg", "lv", "mad", "mai", "mdf", "mg", "mh", "mhr", "mi",
    "min", "mk", "ml", "mn", "mni", "mnw", "mr", "mrj", "ms", "mt", "mus", "mwl", "my", "myv",
    "mzn", "na", "nah", "nan", "nap", "nb", "nds", "ne", "new", "ng", "nia", "nl", "nn", "no",
    "nov", "nqo", "nrm", "nso", "nv", "ny", "oc", "olo", "om", "or", "os", "pa", "pag", "pam",
    "pap", "pcd", "pdc", "pfl", "pi", "pih", "pl", "pms", "pnb", "pnt", "ps", "pt", "qu", "rm",
    "rmy", "rn", "ro", "ru", "rue", "rw", "sa", "sah", "sat", "sc", "scn", "sco", "sd", "se", "sg",
    "sh", "shn", "si", "sk", "skr", "sl", "sm", "smn", "sn", "so", "sq", "sr", "srn", "ss", "st",
    "stq", "su", "sv", "sw", "szl", "ta", "tcy", "te", "tet", "tg", "th", "ti", "tk", "tl", "tn",
    "to", "tpi", "tr", "ts", "tt", "tum", "tw", "ty", "tyv", "udm", "ug", "uk", "ur", "uz", "ve",
    "vec", "vep", "vi", "vls", "vo", "vro", "wa", "war", "wo", "wuu", "xal", "xh", "xmf", "yi",
    "yo", "yue", "za", "zea", "zh", "zu",
];

/// Allowed description modes.
const ALLOWED_MODES: &[&str] = &["short", "long"];

/// Allowed link modes.
const ALLOWED_LINKS: &[&str] = &["text", "wikidata", "wiki", "wikipedia", "reasonator"];

/// Allowed output formats.
const ALLOWED_FORMATS: &[&str] = &["json", "jsonfm", "html"];

/// Validates that a language code is in the allowed set.
pub fn validate_lang(lang: &str) -> bool {
    if lang == "any" || lang.is_empty() {
        return true; // "any" and empty are handled by defaulting to "en"
    }
    ALLOWED_LANGS.contains(&lang)
}

/// Validates that a mode parameter is allowed.
pub fn validate_mode(mode: &str) -> bool {
    ALLOWED_MODES.contains(&mode)
}

/// Validates that a links parameter is allowed.
pub fn validate_links(links: &str) -> bool {
    ALLOWED_LINKS.contains(&links)
}

/// Validates that a format parameter is allowed.
pub fn validate_format(format: &str) -> bool {
    ALLOWED_FORMATS.contains(&format)
}

/// The longest `q` parameter accepted.
const MAX_QID_LEN: usize = 20;

/// Parses a Q-id from user input: `Q42`, `q42` or `42`, with surrounding
/// whitespace allowed (see [`ItemId::parse_lenient`]), at most 20 characters.
pub fn parse_qid(q: &str) -> Option<ItemId> {
    let trimmed = q.trim();
    if trimmed.len() > MAX_QID_LEN {
        return None;
    }
    ItemId::parse_lenient(trimmed).ok()
}

/// Which JSONP callback names are accepted: JavaScript identifiers,
/// including dotted paths like `foo.bar.baz` and indexes like `foo[0]`.
const CALLBACK_RULES: CallbackRules = CallbackRules {
    allow_dotted: true,
    max_len: 256,
};

/// Validates that a JSONP callback name is a safe JavaScript identifier.
///
/// Rejects empty strings and strings containing characters that could be used
/// for XSS (e.g. `<`, `>`, `(`, `)`, etc.).
pub fn validate_jsonp_callback(callback: &str) -> bool {
    jsonp::is_valid_callback(callback, CALLBACK_RULES)
}

/// Maximum length for individual query parameter values.
const MAX_PARAM_LEN: usize = 512;

/// Validates that a parameter value doesn't exceed the maximum length.
pub fn validate_param_length(value: &str) -> bool {
    value.len() <= MAX_PARAM_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_qid() {
        assert_eq!(parse_qid("Q42"), Some(ItemId::new(42)));
        assert_eq!(parse_qid("42"), Some(ItemId::new(42)));
        assert_eq!(parse_qid("q1"), Some(ItemId::new(1)));
        assert_eq!(parse_qid("  Q42  "), Some(ItemId::new(42)));
        assert_eq!(parse_qid("Q1234567890"), Some(ItemId::new(1234567890)));
        assert!(parse_qid("").is_none());
        assert!(parse_qid("   ").is_none());
        assert!(parse_qid("Q").is_none());
        assert!(parse_qid("Qabc").is_none());
        assert!(parse_qid("foo").is_none());
        assert!(parse_qid("Q12345678901234567890").is_none()); // too long
    }

    #[test]
    fn test_validate_jsonp_callback() {
        assert!(validate_jsonp_callback("myCallback"));
        assert!(validate_jsonp_callback("$foo"));
        assert!(validate_jsonp_callback("_bar"));
        assert!(validate_jsonp_callback("foo.bar.baz"));
        assert!(validate_jsonp_callback("foo[0]"));
        assert!(!validate_jsonp_callback(""));
        assert!(!validate_jsonp_callback("<script>alert(1)</script>"));
        assert!(!validate_jsonp_callback("foo()"));
        assert!(!validate_jsonp_callback("foo;alert(1)"));
    }

    #[test]
    fn test_validate_lang() {
        assert!(validate_lang("en"));
        assert!(validate_lang("de"));
        assert!(validate_lang("any"));
        assert!(validate_lang(""));
        assert!(!validate_lang("__invalid__"));
    }

    #[test]
    fn test_validate_mode() {
        assert!(validate_mode("short"));
        assert!(validate_mode("long"));
        assert!(!validate_mode("__invalid__"));
    }

    #[test]
    fn test_validate_links() {
        assert!(validate_links("text"));
        assert!(validate_links("wikidata"));
        assert!(!validate_links("__invalid__"));
    }

    #[test]
    fn test_validate_format() {
        assert!(validate_format("json"));
        assert!(validate_format("html"));
        assert!(!validate_format("__invalid__"));
    }
}
