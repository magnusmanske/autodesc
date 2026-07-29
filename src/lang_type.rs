use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

/// A validated language code (ISO 639-1 two-letter, e.g. `"en"`, `"de"`).
///
/// The set of valid codes is loaded once at startup from Wikidata's SPARQL
/// endpoint, with a hardcoded fallback list if the query fails.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Lang(String);

/// Hardcoded fallback list of ~300 language codes served by Wikidata.
const FALLBACK_LANGS: &[&str] = &[
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

static VALID_LANGS: OnceLock<HashSet<String>> = OnceLock::new();

/// The default language used when none is specified.
pub const DEFAULT_LANG: &str = "en";

impl Default for Lang {
    fn default() -> Self {
        Lang(DEFAULT_LANG.to_string())
    }
}

impl Lang {
    /// Parse a language code. Returns `Ok(Self::default())` for `"any"` or
    /// empty input; otherwise checks against the loaded set of valid codes.
    pub fn parse(code: &str) -> Result<Self, LangError> {
        if code == "any" || code.is_empty() {
            return Ok(Self::default());
        }

        let set = VALID_LANGS
            .get()
            .expect("Lang::init() must be called before Lang::parse()");

        if set.contains(code) {
            Ok(Lang(code.to_string()))
        } else {
            Err(LangError(code.to_string()))
        }
    }

    /// Return the language code string (e.g. `"en"`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Lang {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid language code: {0}")]
pub struct LangError(pub String);

// ── Initialization ─────────────────────────────────────────────────────────

/// Initialize the set of valid language codes.  Tries Wikidata SPARQL first;
/// falls back to the hardcoded list on failure.
///
/// Must be called once before any `Lang::parse()` calls.
pub async fn init_langs() {
    let langs = match fetch_langs_from_sparql().await {
        Ok(set) if set.len() >= 200 => {
            tracing::info!(
                count = set.len(),
                "Loaded language codes from Wikidata SPARQL"
            );
            set
        }
        Ok(set) => {
            tracing::warn!(
                count = set.len(),
                fallback_count = FALLBACK_LANGS.len(),
                "SPARQL returned too few language codes; using hardcoded fallback list"
            );
            hardcoded_langs()
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                fallback_count = FALLBACK_LANGS.len(),
                "Failed to fetch languages from Wikidata; using hardcoded fallback list"
            );
            hardcoded_langs()
        }
    };

    let _ = VALID_LANGS.set(langs);
}

fn hardcoded_langs() -> HashSet<String> {
    FALLBACK_LANGS.iter().map(|s| s.to_string()).collect()
}

async fn fetch_langs_from_sparql() -> Result<HashSet<String>, anyhow::Error> {
    let client = reqwest::Client::builder()
        .user_agent("autodesc/0.2.0 (https://github.com/magnusmanske/autodesc)")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let query = "SELECT ?code WHERE { ?lang wdt:P31 wd:Q34770. ?lang wdt:P218 ?code. }";
    let url = format!(
        "https://query.wikidata.org/sparql?format=json&query={}",
        urlencoding::encode(query)
    );

    let resp: serde_json::Value = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut langs = HashSet::new();
    if let Some(bindings) = resp
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
    {
        for binding in bindings {
            if let Some(code) = binding
                .get("code")
                .and_then(|c| c.get("value"))
                .and_then(|v| v.as_str())
            {
                langs.insert(code.to_string());
            }
        }
    }

    if langs.is_empty() {
        anyhow::bail!("SPARQL query returned no language codes");
    }

    Ok(langs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test with the hardcoded list (no network).
    fn init_test_langs() {
        let _ = VALID_LANGS.set(hardcoded_langs());
    }

    #[test]
    fn parse_valid_code() {
        init_test_langs();
        let lang = Lang::parse("en").unwrap();
        assert_eq!(lang.as_str(), "en");
    }

    #[test]
    fn parse_any_returns_default() {
        init_test_langs();
        let lang = Lang::parse("any").unwrap();
        assert_eq!(lang.as_str(), DEFAULT_LANG);
    }

    #[test]
    fn parse_empty_returns_default() {
        init_test_langs();
        let lang = Lang::parse("").unwrap();
        assert_eq!(lang.as_str(), DEFAULT_LANG);
    }

    #[test]
    fn parse_invalid_code() {
        init_test_langs();
        assert!(Lang::parse("__invalid__").is_err());
    }

    #[test]
    fn default_is_en() {
        init_test_langs();
        let lang = Lang::default();
        assert_eq!(lang.as_str(), "en");
    }

    #[test]
    fn display() {
        init_test_langs();
        let lang = Lang::parse("de").unwrap();
        assert_eq!(lang.to_string(), "de");
    }

    #[test]
    fn serde_roundtrip() {
        init_test_langs();
        let lang = Lang::parse("fr").unwrap();
        let json = serde_json::to_string(&lang).unwrap();
        assert_eq!(json, "\"fr\"");
        let lang2: Lang = serde_json::from_str(&json).unwrap();
        assert_eq!(lang, lang2);
    }
}
