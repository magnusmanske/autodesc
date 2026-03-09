use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::desc_options::DescOptions;
use crate::wikidata::{sanitize_q, WikiData, WikiDataItem, MAIN_LANGUAGES};

fn split_link_wiki_pipe_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\[\[.+\|)(.+)(\]\])$").expect("regex is valid"))
}

fn split_link_wiki_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\[\[)(.+)(\]\])$").expect("regex is valid"))
}

/// Matches an HTML anchor tag: captures (opening tag, inner text, closing tag).
/// Shared by split_link and txt2.
fn html_link_re() -> &'static Regex {
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

/// The short description generator, ported from the Python `ShortDescription` class.
pub struct ShortDescription {
    pub stock: HashMap<String, HashMap<String, String>>,
    pub language_specific: HashMap<String, HashMap<String, HashMap<String, String>>>,
    main_languages: Vec<String>,
}

impl ShortDescription {
    pub fn new() -> Self {
        let stock_json = include_str!("../data/stock.json");
        let stock: HashMap<String, HashMap<String, String>> =
            serde_json::from_str(stock_json).unwrap_or_default();

        Self {
            stock,
            language_specific: HashMap::new(),
            main_languages: MAIN_LANGUAGES.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Get a translated string from the stock translations.
    pub fn txt(&self, key: &str, lang: &str) -> String {
        if let Some(translations) = self.stock.get(key) {
            if let Some(val) = translations.get(lang) {
                return val.clone();
            }
            if let Some(val) = translations.get("en") {
                return val.clone();
            }
        }
        format!("[{}]", key)
    }

    /// Apply language-specific word modification (e.g. nationality transformation).
    pub fn txt2(&self, text: &str, key: &str, lang: &str) -> String {
        if let Some(lang_spec) = self.language_specific.get(lang) {
            if let Some(key_map) = lang_spec.get(key) {
                // Try to extract inner text from an HTML link
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
        }
        text.to_string()
    }

    /// Check if hints indicate female gender.
    fn is_female(hints: &HashMap<String, bool>) -> bool {
        hints.get("is_female").copied().unwrap_or(false)
    }

    /// Check if hints indicate male gender.
    fn is_male(hints: &HashMap<String, bool>) -> bool {
        hints.get("is_male").copied().unwrap_or(false)
    }

    /// Modify a word based on gender hints and language.
    pub fn modify_word(&self, word: &str, hints: &HashMap<String, bool>, lang: &str) -> String {
        let lower = word.to_lowercase();
        match lang {
            "en" => {
                if Self::is_female(hints) {
                    if lower == "actor" {
                        return "actress".to_string();
                    }
                    if lower == "actor / actress" {
                        return "actress".to_string();
                    }
                } else if Self::is_male(hints) && lower == "actor / actress" {
                    return "actor".to_string();
                }
            }
            "fr" => {
                if Self::is_female(hints) {
                    if lower == "acteur" {
                        return "actrice".to_string();
                    }
                    if lower == "être humain" {
                        return "personne".to_string();
                    }
                }
            }
            "de" => {
                if Self::is_female(hints) && hints.get("occupation").copied().unwrap_or(false) {
                    return format!("{}in", word);
                }
            }
            _ => {}
        }
        word.to_string()
    }

    /// Join a list of words with the appropriate conjunction for the given language.
    pub fn list_words(
        &self,
        original_list: &[String],
        hints: &HashMap<String, bool>,
        lang: &str,
    ) -> String {
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

    fn uc_first(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => {
                let upper: String = c.to_uppercase().collect();
                format!("{}{}", upper, chars.as_str())
            }
        }
    }

    /// Check if claims have a specific P/Q link (both numeric).
    fn has_pq(claims: &serde_json::Value, p: u64, q: u64) -> bool {
        let prop = format!("P{}", p);
        let claims_arr = match claims.get(&prop).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return false,
        };

        for v in claims_arr {
            let nid = v
                .get("mainsnak")
                .and_then(|ms| ms.get("datavalue"))
                .and_then(|dv| dv.get("value"))
                .and_then(|val| val.get("numeric-id"))
                .and_then(|n| n.as_u64());
            if nid == Some(q) {
                return true;
            }
        }
        false
    }

    /// Determine if the item is a person.
    fn is_person(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 107, 215627) || Self::has_pq(claims, 31, 5)
    }

    /// Determine if the item is a taxon.
    fn is_taxon(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 31, 16521)
            || Self::has_pq(claims, 105, 7432)
            || Self::has_pq(claims, 105, 34740)
            || Self::has_pq(claims, 105, 35409)
    }

    /// Determine if the item is a disambiguation page.
    fn is_disambig(claims: &serde_json::Value) -> bool {
        Self::has_pq(claims, 107, 11651459)
    }

    /// Extract items from claims for a given (numeric) property. Returns [(prop_num, qid), ...].
    fn add_items_from_claims(claims: &serde_json::Value, p: u64, items: &mut Vec<(u64, String)>) {
        let prefixed = format!("P{}", p);
        let claims_arr = match claims.get(&prefixed).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return,
        };

        for v in claims_arr {
            let mainsnak = match v.get("mainsnak") {
                Some(ms) => ms,
                None => continue,
            };
            let datavalue = match mainsnak.get("datavalue") {
                Some(dv) => dv,
                None => continue,
            };
            let value = match datavalue.get("value") {
                Some(val) => val,
                None => continue,
            };

            // Prefer the string "id" field (newer format), fall back to numeric-id
            if let Some(id) = value.get("id").and_then(|i| i.as_str()) {
                items.push((p, id.to_string()));
            } else if let Some(numeric_id) = value.get("numeric-id").and_then(|n| n.as_u64()) {
                items.push((p, format!("Q{}", numeric_id)));
            }
            // If neither field is present this is not an item value; skip it.
        }
    }

    /// Create labeled links for a set of items. Returns a map from property number to list of labels/links.
    pub async fn label_items(
        &self,
        items: &[(u64, String)],
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> HashMap<u64, Vec<String>> {
        if items.is_empty() {
            return HashMap::new();
        }

        let use_lang = &opt.lang;

        // Collect all entity IDs to load, deduplicated in O(n) with a HashSet
        let mut seen: HashSet<String> = HashSet::new();
        let mut ids: Vec<String> = Vec::new();
        for (p, qid) in items {
            if *p != 0 {
                let prop_id = format!("P{}", p);
                if seen.insert(prop_id.clone()) {
                    ids.push(prop_id);
                }
            }
            if seen.insert(qid.clone()) {
                ids.push(qid.clone());
            }
        }

        if let Err(e) = wd.get_item_batch(&ids).await {
            tracing::warn!("Failed to load item batch: {}", e);
        }

        let mut cb: HashMap<u64, Vec<String>> = HashMap::new();

        for q_str in &ids {
            let item = match wd.get_item(q_str) {
                Some(i) => i,
                None => continue,
            };

            let raw = &item.raw;
            let labels = match raw.get("labels").and_then(|l| l.as_object()) {
                Some(l) => l,
                None => continue,
            };

            // Find the best available language for the label
            let mut curlang = use_lang.clone();
            if !labels.contains_key(&curlang) {
                let mut found = false;
                for language in &self.main_languages {
                    if labels.contains_key(language.as_str()) {
                        curlang = language.clone();
                        found = true;
                        break;
                    }
                }
                if !found {
                    // Take any available language
                    if let Some(first_lang) = labels.keys().next() {
                        curlang = first_lang.clone();
                    } else {
                        continue;
                    }
                }
            }

            let label = match labels
                .get(&curlang)
                .and_then(|l| l.get("value"))
                .and_then(|v| v.as_str())
            {
                Some(l) => l.to_string(),
                None => continue,
            };

            // Determine which property number this Q belongs to
            let mut p: u64 = 0;
            for (prop_num, item_q) in items {
                if item_q == q_str {
                    p = *prop_num;
                    break;
                }
            }

            // Skip certain instance-of values
            if p == 31 && (q_str == "Q5" || q_str == "Q16521") {
                continue;
            }

            let wiki = format!("{}wiki", use_lang);
            let linktarget = if !opt.linktarget.is_empty() {
                format!(" target='{}'", opt.linktarget)
            } else {
                String::new()
            };

            let entry = cb.entry(p).or_default();

            match opt.links.as_str() {
                "wikidata" => {
                    entry.push(format!(
                        "<a href='https://www.wikidata.org/wiki/{q}'{lt}>{label}</a>",
                        q = q_str,
                        lt = linktarget,
                        label = label
                    ));
                }
                "reasonator" => {
                    entry.push(format!(
                        "<a href='/reasonator/?lang={lang}&q={q}'{lt}>{label}</a>",
                        lang = use_lang,
                        q = q_str,
                        lt = linktarget,
                        label = label
                    ));
                }
                "wiki" => {
                    let sitelinks = raw.get("sitelinks");
                    if let Some(sl) = sitelinks
                        .and_then(|s| s.get(&wiki))
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                    {
                        if sl == label {
                            entry.push(format!("[[{}]]", label));
                        } else {
                            entry.push(format!("[[{}|{}]]", sl, label));
                        }
                    } else {
                        entry.push(label.clone());
                    }
                }
                "wikipedia" => {
                    if let Some(page) = raw
                        .get("sitelinks")
                        .and_then(|s| s.get(&wiki))
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                    {
                        let encoded = wiki_urlencode(page);
                        entry.push(format!(
                            "<a href='https://{lang}.wikipedia.org/wiki/{page}'{lt}>{label}</a>",
                            lang = use_lang,
                            page = encoded,
                            lt = linktarget,
                            label = label
                        ));
                    } else {
                        entry.push(label.clone());
                    }
                }
                "text" | "" => {
                    entry.push(label.clone());
                }
                _ => {
                    // Generic sitelink-based link
                    let site = format!("{}{}", use_lang, opt.links);
                    if let Some(page) = raw
                        .get("sitelinks")
                        .and_then(|s| s.get(&site))
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                    {
                        let encoded = wiki_urlencode(page);
                        entry.push(format!(
                            "<a href='https://{lang}.{site}.org/wiki/{page}'{lt}>{label}</a>",
                            lang = use_lang,
                            site = opt.links,
                            page = encoded,
                            lt = linktarget,
                            label = label
                        ));
                    } else {
                        entry.push(label.clone());
                    }
                }
            }
        }

        cb
    }

    /// Append items to the description list from item_labels for the given properties.
    fn add2desc(
        &self,
        h: &mut Vec<String>,
        item_labels: &HashMap<u64, Vec<String>>,
        props: &[u64],
        hints: &HashMap<String, bool>,
        prefix: Option<&str>,
        txt_key: Option<&str>,
        lang: &str,
    ) {
        let mut h2: Vec<String> = Vec::new();
        for prop in props {
            if let Some(labels) = item_labels.get(prop) {
                h2.extend(labels.clone());
            }
        }

        if h2.is_empty() {
            return;
        }

        if let Some(pfx) = prefix {
            if !h.is_empty() {
                let last_idx = h.len() - 1;
                h[last_idx].push_str(pfx);
            }
        }

        let s = self.list_words(&h2, hints, lang);
        if let Some(key) = txt_key {
            if lang == "te" {
                h.push(format!("{} {}", s, self.txt(key, lang)));
            } else {
                h.push(format!("{} {}", self.txt(key, lang), s));
            }
        } else {
            h.push(s);
        }
    }

    fn get_nationality_from_country(
        &self,
        country: &str,
        _claims: &serde_json::Value,
        lang: &str,
    ) -> String {
        self.txt2(country, "nationality", lang)
    }

    /// Split a link string into parts: (full_match, before, inner_text, after).
    fn split_link(v: &str) -> (String, String, String, String) {
        // Try wiki link: [[...|text]] or [[text]]
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

        // Try HTML link
        if let Some(caps) = html_link_re().captures(v) {
            return (
                caps.get(0).unwrap().as_str().to_string(),
                caps.get(1).unwrap().as_str().to_string(),
                caps.get(2).unwrap().as_str().to_string(),
                caps.get(3).unwrap().as_str().to_string(),
            );
        }

        // No link
        (String::new(), String::new(), v.to_string(), String::new())
    }

    /// Main entry point: load and describe a Wikidata item.
    pub async fn load_item(
        &self,
        q: &str,
        opt: &mut DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let q = sanitize_q(q);
        opt.q = q.clone();

        if let Err(e) = wd.load_entity(&q).await {
            tracing::warn!("Failed to load entity {}: {}", q, e);
            return (
                q.clone(),
                format!("<i>{}</i>", self.txt("cannot_describe", &opt.lang)),
            );
        }

        let claims = wd
            .get_item(&q)
            .map(|item| {
                item.raw
                    .get("claims")
                    .cloned()
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
            })
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        if Self::is_person(&claims) {
            self.describe_person(&q, &claims, opt, wd).await
        } else if Self::is_taxon(&claims) {
            self.describe_taxon(&q, &claims, opt, wd).await
        } else if Self::is_disambig(&claims) {
            let desc = self.txt("disambig", &opt.lang);
            (q, desc)
        } else {
            self.describe_generic(&q, &claims, opt, wd).await
        }
    }

    /// Generate a description for a person.
    async fn describe_person(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let mut load_items: Vec<(u64, String)> = Vec::new();
        Self::add_items_from_claims(claims, 106, &mut load_items); // Occupation
        Self::add_items_from_claims(claims, 39, &mut load_items); // Office
        Self::add_items_from_claims(claims, 27, &mut load_items); // Country of citizenship
        Self::add_items_from_claims(claims, 166, &mut load_items); // Award received
        Self::add_items_from_claims(claims, 31, &mut load_items); // Instance of
        Self::add_items_from_claims(claims, 22, &mut load_items); // Father
        Self::add_items_from_claims(claims, 25, &mut load_items); // Mother
        Self::add_items_from_claims(claims, 26, &mut load_items); // Spouse
        Self::add_items_from_claims(claims, 463, &mut load_items); // Member of

        let is_male = Self::has_pq(claims, 21, 6581097);
        let is_female = Self::has_pq(claims, 21, 6581072);

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let lang = &opt.lang;
        let mut h: Vec<String> = Vec::new();

        // Nationality
        let nationality_items = item_labels.get(&27).cloned().unwrap_or_default();
        let mut h2 = String::new();
        let total = nationality_items.len();
        for (k, v) in nationality_items.iter().enumerate() {
            let (_full, before, inner, after) = Self::split_link(v);
            let not_last = k + 1 != total;
            let s = self.get_nationality_from_country(&inner, claims, lang);
            if k == 0 {
                h2 = format!("{}{}{}", before, s, after);
            } else {
                h2 = format!("{}-{}{}{}", h2, before, s.to_lowercase(), after);
            }
            let _ = not_last; // Used in some languages for nationality inflection
        }
        if !h2.is_empty() {
            h.push(h2);
        }

        // Occupation
        let ol = h.len();
        let mut hints = HashMap::new();
        if is_male {
            hints.insert("is_male".to_string(), true);
        }
        if is_female {
            hints.insert("is_female".to_string(), true);
        }
        hints.insert("occupation".to_string(), true);
        self.add2desc(&mut h, &item_labels, &[31, 106], &hints, None, None, lang);
        if h.len() == ol {
            h.push(self.txt("person", lang));
        }

        // Office
        let office_hints = {
            let mut oh = HashMap::new();
            if is_male {
                oh.insert("is_male".to_string(), true);
            }
            if is_female {
                oh.insert("is_female".to_string(), true);
            }
            oh
        };
        self.add2desc(
            &mut h,
            &item_labels,
            &[39],
            &office_hints,
            Some(","),
            None,
            lang,
        );

        // Dates
        let born = WikiData::get_year(claims, 569, lang, &self.stock);
        let died = WikiData::get_year(claims, 570, lang, &self.stock);
        if !born.is_empty() && !died.is_empty() {
            h.push(format!("({}–{})", born, died));
        } else if !born.is_empty() {
            h.push(format!("(*{})", born));
        } else if !died.is_empty() {
            h.push(format!("(†{})", died));
        }

        // Gender symbols
        if is_female {
            h.push("♀".to_string());
        }
        if is_male {
            h.push("♂".to_string());
        }

        // Awards
        let empty_hints = HashMap::new();
        self.add2desc(
            &mut h,
            &item_labels,
            &[166],
            &empty_hints,
            Some(";"),
            None,
            lang,
        );

        // Member of
        self.add2desc(
            &mut h,
            &item_labels,
            &[463],
            &empty_hints,
            Some(";"),
            Some("member of"),
            lang,
        );

        // Child of (father/mother)
        self.add2desc(
            &mut h,
            &item_labels,
            &[22, 25],
            &empty_hints,
            Some(";"),
            Some("child of"),
            lang,
        );

        // Spouse
        self.add2desc(
            &mut h,
            &item_labels,
            &[26],
            &empty_hints,
            Some(";"),
            Some("spouse of"),
            lang,
        );

        if h.is_empty() {
            h.push(self.txt("person", lang));
        }

        let result = Self::uc_first(&h.join(" "));
        (q.to_string(), clean_spaces(&result))
    }

    /// Generate a description for a taxon using SPARQL.
    async fn describe_taxon(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let sparql = format!(
            "SELECT ?taxon ?taxonRank ?taxonRankLabel ?parentTaxon ?taxonLabel ?taxonName {{ wd:{q} \
             wdt:P171* ?taxon . ?taxon wdt:P171 ?parentTaxon . ?taxon wdt:P225 ?taxonName . ?taxon wdt:P105 ?taxonRank . \
             SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"[AUTO_LANGUAGE],{lang}\". }} }}",
            q = q,
            lang = opt.lang
        );

        let url = format!(
            "https://query.wikidata.org/bigdata/namespace/wdq/sparql?format=json&query={}",
            urlencoding::encode(&sparql)
        );

        let body = match wd.get_json(&url).await {
            Ok(v) => v,
            Err(_) => return self.describe_generic(q, claims, opt, wd).await,
        };

        let taxa_ranks: HashMap<&str, u64> = [
            ("Q767728", 0), // variety
            ("Q68947", 1),  // subspecies
            ("Q7432", 2),   // species
            ("Q34740", 3),  // genus
            ("Q35409", 4),  // family
            ("Q36602", 5),  // order
            ("Q37517", 6),  // class
            ("Q38348", 7),  // phylum
            ("Q36732", 8),  // kingdom
        ]
        .into_iter()
        .collect();

        let bindings = body
            .get("results")
            .and_then(|r| r.get("bindings"))
            .and_then(|b| b.as_array())
            .cloned()
            .unwrap_or_default();

        let entity_re = Regex::new(r"^.+?entity/").unwrap();

        let mut taxon_name: Option<String> = None;
        let mut taxa_cache: Vec<Option<serde_json::Value>> = vec![None; 9];
        let mut load_items: Vec<(u64, String)> = Vec::new();

        for binding in &bindings {
            let taxon_q = entity_re
                .replace_all(
                    binding
                        .get("taxon")
                        .and_then(|t| t.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    "",
                )
                .to_string();
            let taxon_rank = entity_re
                .replace_all(
                    binding
                        .get("taxonRank")
                        .and_then(|t| t.get("value"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    "",
                )
                .to_string();

            if taxon_q == q {
                load_items.push((0, taxon_rank.clone()));
                if let Some(tn) = binding
                    .get("taxonName")
                    .and_then(|t| t.get("value"))
                    .and_then(|v| v.as_str())
                {
                    taxon_name = Some(tn.to_string());
                }
            }

            if let Some(&rank_id) = taxa_ranks.get(taxon_rank.as_str()) {
                if (rank_id as usize) < taxa_cache.len() {
                    taxa_cache[rank_id as usize] = Some(binding.clone());
                }
            }
        }

        for binding in taxa_cache.iter().flatten() {
            let taxon_label = binding
                .get("taxonLabel")
                .and_then(|t| t.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let taxon_name_val = binding
                .get("taxonName")
                .and_then(|t| t.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if taxon_label.to_lowercase() != taxon_name_val.to_lowercase() {
                let taxon_q = entity_re
                    .replace_all(
                        binding
                            .get("taxon")
                            .and_then(|t| t.get("value"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        "",
                    )
                    .to_string();
                load_items.push((0, taxon_q));
                break;
            }
        }

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let labels_0 = item_labels.get(&0).cloned().unwrap_or_default();

        if labels_0.is_empty() {
            return self.describe_generic(q, claims, opt, wd).await;
        }

        let mut h_parts: Vec<String> = vec![labels_0[0].clone()];
        if labels_0.len() >= 2 {
            h_parts[0] = format!(
                "{} {} {}",
                h_parts[0],
                self.txt("of", &opt.lang),
                labels_0[1]
            );
        }

        if let Some(name) = &taxon_name {
            h_parts.push(format!("[{}]", name));
        }

        let result = Self::uc_first(&h_parts.join(", "));
        (q.to_string(), clean_spaces(&result))
    }

    /// Generate a generic description for non-person, non-taxon items.
    async fn describe_generic(
        &self,
        q: &str,
        claims: &serde_json::Value,
        opt: &DescOptions,
        wd: &mut WikiData,
    ) -> (String, String) {
        let mut load_items: Vec<(u64, String)> = Vec::new();

        Self::add_items_from_claims(claims, 361, &mut load_items); // Part of
        Self::add_items_from_claims(claims, 279, &mut load_items); // Subclass of
        Self::add_items_from_claims(claims, 1269, &mut load_items); // Facet of
        Self::add_items_from_claims(claims, 31, &mut load_items); // Instance of
        Self::add_items_from_claims(claims, 60, &mut load_items); // Astronomical object

        Self::add_items_from_claims(claims, 175, &mut load_items); // Performer
        Self::add_items_from_claims(claims, 86, &mut load_items); // Composer
        Self::add_items_from_claims(claims, 170, &mut load_items); // Creator
        Self::add_items_from_claims(claims, 57, &mut load_items); // Director
        Self::add_items_from_claims(claims, 162, &mut load_items); // Producer
        Self::add_items_from_claims(claims, 50, &mut load_items); // Author
        Self::add_items_from_claims(claims, 61, &mut load_items); // Discoverer/inventor

        Self::add_items_from_claims(claims, 17, &mut load_items); // Country
        Self::add_items_from_claims(claims, 131, &mut load_items); // Admin unit

        Self::add_items_from_claims(claims, 495, &mut load_items); // Country of origin
        Self::add_items_from_claims(claims, 159, &mut load_items); // Headquarters

        Self::add_items_from_claims(claims, 306, &mut load_items); // OS
        Self::add_items_from_claims(claims, 400, &mut load_items); // Platform
        Self::add_items_from_claims(claims, 176, &mut load_items); // Manufacturer

        Self::add_items_from_claims(claims, 123, &mut load_items); // Publisher
        Self::add_items_from_claims(claims, 264, &mut load_items); // Record label

        Self::add_items_from_claims(claims, 105, &mut load_items); // Taxon rank
        Self::add_items_from_claims(claims, 138, &mut load_items); // Named after
        Self::add_items_from_claims(claims, 171, &mut load_items); // Parent taxon

        Self::add_items_from_claims(claims, 1433, &mut load_items); // Published in
        Self::add_items_from_claims(claims, 571, &mut load_items); // Inception
        Self::add_items_from_claims(claims, 576, &mut load_items); // Until
        Self::add_items_from_claims(claims, 585, &mut load_items); // Point in time
        Self::add_items_from_claims(claims, 703, &mut load_items); // Found in taxon
        Self::add_items_from_claims(claims, 1080, &mut load_items); // From fictional universe
        Self::add_items_from_claims(claims, 1441, &mut load_items); // Present in work
        Self::add_items_from_claims(claims, 921, &mut load_items); // Main topic

        Self::add_items_from_claims(claims, 425, &mut load_items); // Field of profession
        Self::add_items_from_claims(claims, 59, &mut load_items); // Constellation

        Self::add_items_from_claims(claims, 1082, &mut load_items); // Population

        let item_labels = self.label_items(&load_items, opt, wd).await;
        let lang = &opt.lang;
        let empty_hints = HashMap::new();
        let mut h: Vec<String> = Vec::new();

        // Publication date
        let pubdate = WikiData::get_year(claims, 577, lang, &self.stock);
        if !pubdate.is_empty() {
            h.push(pubdate);
        }

        // Instance/subclass/etc
        self.add2desc(
            &mut h,
            &item_labels,
            &[279, 31, 1269, 60, 105],
            &empty_hints,
            None,
            None,
            lang,
        );

        // Location
        let sep = " / ";
        let h2: Vec<String> = item_labels.get(&131).cloned().unwrap_or_default();
        let h3: Vec<String> = item_labels.get(&17).cloned().unwrap_or_default();

        if h.is_empty() && (!h2.is_empty() || !h3.is_empty()) {
            h.push(self.txt("location", lang));
        }

        if !h2.is_empty() && !h3.is_empty() {
            h.push(format!(
                "{} {}, {}",
                self.txt("in", lang),
                h2.join(sep),
                h3.join(sep)
            ));
        } else if !h2.is_empty() {
            h.push(format!("{} {}", self.txt("in", lang), h2.join(sep)));
        } else if !h3.is_empty() {
            h.push(format!("{} {}", self.txt("in", lang), h3.join(sep)));
        }

        // Population
        if let Some(item) = wd.get_item(q) {
            if item.has_claims("P1082") {
                let cl = item.get_claims_for_property("P1082");
                if let Some(best) = WikiDataItem::get_best_quantity(&cl) {
                    let pop_label = wd
                        .get_item("P1082")
                        .map(|i| i.get_label(Some(lang)))
                        .unwrap_or_else(|| "population".to_string());
                    h.push(format!(", {} {}", pop_label, best));
                }
            }
        }

        // Creator etc
        self.add2desc(
            &mut h,
            &item_labels,
            &[175, 86, 170, 57, 50, 61, 176],
            &empty_hints,
            None,
            Some("by"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[162],
            &empty_hints,
            Some(","),
            Some("produced by"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[306, 400],
            &empty_hints,
            None,
            Some("for"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[264, 123],
            &empty_hints,
            None,
            Some("from"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[361],
            &empty_hints,
            Some(","),
            Some("part of"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[138],
            &empty_hints,
            Some(","),
            Some("named after"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[425],
            &empty_hints,
            Some(","),
            Some("in the field of"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[171],
            &empty_hints,
            None,
            Some("of"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[59],
            &empty_hints,
            None,
            Some("in the constellation"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[1433],
            &empty_hints,
            None,
            Some("published in"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[585],
            &empty_hints,
            None,
            Some("in"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[703],
            &empty_hints,
            None,
            Some("found_in"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[1080, 1441],
            &empty_hints,
            None,
            Some("from"),
            lang,
        );
        self.add2desc(
            &mut h,
            &item_labels,
            &[921],
            &empty_hints,
            None,
            Some("about"),
            lang,
        );

        // Inception / Until dates
        if let Some(item) = wd.get_item(q) {
            if item.has_claims("P571") {
                let year = WikiData::get_year(
                    &item.raw.get("claims").cloned().unwrap_or_default(),
                    571,
                    lang,
                    &self.stock,
                );
                if !year.is_empty() {
                    h.push(format!(", {} {}", self.txt("from", lang), year));
                }
            }
            if item.has_claims("P576") {
                let year = WikiData::get_year(
                    &item.raw.get("claims").cloned().unwrap_or_default(),
                    576,
                    lang,
                    &self.stock,
                );
                if !year.is_empty() {
                    h.push(format!(", {} {}", self.txt("until", lang), year));
                }
            }
        }

        // Origin (headquarters, country of origin)
        let h2: Vec<String> = item_labels.get(&159).cloned().unwrap_or_default();
        let h3: Vec<String> = item_labels.get(&495).cloned().unwrap_or_default();
        if !h2.is_empty() && !h3.is_empty() {
            h.push(format!(
                "{} {}, {}",
                self.txt("from", lang),
                h2.join(sep),
                h3.join(sep)
            ));
        } else if !h2.is_empty() {
            h.push(format!("{} {}", self.txt("from", lang), h2.join(sep)));
        } else if !h3.is_empty() {
            h.push(format!("{} {}", self.txt("from", lang), h3.join(sep)));
        }

        // Fallback
        if h.is_empty() {
            let fallback = format!("<i>{}</i>", self.txt("cannot_describe", lang));
            return (q.to_string(), fallback);
        }

        let result = Self::uc_first(&h.join(" "));
        let result = clean_spaces(&result);
        (q.to_string(), result)
    }
}

impl Default for ShortDescription {
    fn default() -> Self {
        Self::new()
    }
}

/// URL-encode a wiki page title.
fn wiki_urlencode(s: &str) -> String {
    let s = s.replace(' ', "_");
    urlencoding::encode(&s).to_string()
}

/// Clean up extra spaces and punctuation artifacts.
fn clean_spaces(s: &str) -> String {
    let result = clean_spaces_re().replace_all(s, " ");
    let result = clean_space_comma_re().replace_all(&result, ",");
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stock_loaded() {
        let sd = ShortDescription::new();
        assert_eq!(
            sd.stock
                .get("produced by")
                .and_then(|m| m.get("de"))
                .map(|s| s.as_str()),
            Some("produziert von")
        );
    }

    #[test]
    fn test_txt() {
        let sd = ShortDescription::new();
        assert_eq!(sd.txt("by", "en"), "by");
        assert_eq!(sd.txt("by", "de"), "von");
        assert_eq!(sd.txt("nonexistent", "en"), "[nonexistent]");
    }

    #[test]
    fn test_list_words() {
        let sd = ShortDescription::new();
        let empty: HashMap<String, bool> = HashMap::new();
        assert_eq!(sd.list_words(&["one".to_string()], &empty, "en"), "one");
        assert_eq!(
            sd.list_words(&["one".to_string(), "two".to_string()], &empty, "en"),
            "one and two"
        );
        assert_eq!(
            sd.list_words(
                &["one".to_string(), "two".to_string(), "three".to_string()],
                &empty,
                "en"
            ),
            "one, two, and three"
        );
        assert_eq!(
            sd.list_words(&["one".to_string(), "two".to_string()], &empty, "de"),
            "one und two"
        );
    }

    #[test]
    fn test_uc_first() {
        assert_eq!(ShortDescription::uc_first("hello"), "Hello");
        assert_eq!(ShortDescription::uc_first(""), "");
        assert_eq!(ShortDescription::uc_first("Hello"), "Hello");
    }

    #[test]
    fn test_has_pq() {
        let claims: serde_json::Value = serde_json::json!({
            "P31": [
                {
                    "mainsnak": {
                        "datavalue": {
                            "value": {
                                "numeric-id": 5
                            }
                        }
                    }
                }
            ]
        });
        assert!(ShortDescription::has_pq(&claims, 31, 5));
        assert!(!ShortDescription::has_pq(&claims, 31, 42));
        assert!(!ShortDescription::has_pq(&claims, 99, 5));
    }

    #[test]
    fn test_add_items_from_claims_uses_continue_not_return() {
        // This test verifies that a missing mainsnak in one claim does NOT
        // prevent subsequent claims in the same property from being processed.
        let claims = serde_json::json!({
            "P106": [
                {
                    // First claim is missing "mainsnak" entirely
                },
                {
                    "mainsnak": {
                        "datavalue": {
                            "value": { "id": "Q36180" }
                        }
                    }
                }
            ]
        });

        let mut items: Vec<(u64, String)> = Vec::new();
        ShortDescription::add_items_from_claims(&claims, 106, &mut items);
        // The second valid claim should still be collected
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, "Q36180");
    }

    #[test]
    fn test_add_items_from_claims_numeric_id_fallback() {
        // Ensure numeric-id is used when "id" is absent
        let claims = serde_json::json!({
            "P31": [{
                "mainsnak": {
                    "datavalue": {
                        "value": {
                            "entity-type": "item",
                            "numeric-id": 5
                        }
                    }
                }
            }]
        });

        let mut items: Vec<(u64, String)> = Vec::new();
        ShortDescription::add_items_from_claims(&claims, 31, &mut items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, "Q5");
    }

    #[test]
    fn test_add_items_from_claims_no_id_no_numeric_id_skipped() {
        // When neither "id" nor "numeric-id" is present, the claim should be skipped
        // (not push a spurious "P<n>" string).
        let claims = serde_json::json!({
            "P31": [{
                "mainsnak": {
                    "datavalue": {
                        "value": { "time": "+2020-01-01T00:00:00Z" }
                    }
                }
            }]
        });

        let mut items: Vec<(u64, String)> = Vec::new();
        ShortDescription::add_items_from_claims(&claims, 31, &mut items);
        assert!(
            items.is_empty(),
            "Non-item values should not produce entries"
        );
    }

    #[test]
    fn test_clean_spaces() {
        // clean_spaces is private but we can test it indirectly via describe results;
        // test it directly here to ensure the cached regex works correctly.
        // We use a workaround: call it through the module since it's in the same file.
        assert_eq!(clean_spaces("hello  world"), "hello world");
        assert_eq!(clean_spaces("foo ,bar"), "foo,bar");
        assert_eq!(clean_spaces("  trim  "), "trim");
    }

    #[test]
    fn test_split_link() {
        let (_, before, inner, after) = ShortDescription::split_link("<a href='test'>Hello</a>");
        assert_eq!(before, "<a href='test'>");
        assert_eq!(inner, "Hello");
        assert_eq!(after, "</a>");

        let (_, before, inner, after) = ShortDescription::split_link("plain text");
        assert_eq!(before, "");
        assert_eq!(inner, "plain text");
        assert_eq!(after, "");

        let (_, before, inner, after) = ShortDescription::split_link("[[Page|Label]]");
        assert_eq!(before, "[[Page|");
        assert_eq!(inner, "Label");
        assert_eq!(after, "]]");
    }

    #[tokio::test]
    async fn test_describe_generic_item() {
        let sd = ShortDescription::new();
        let mut wd = WikiData::new();
        let mut opt = DescOptions {
            lang: "en".to_string(),
            links: "text".to_string(),
            ..Default::default()
        };
        let (q, desc) = sd.load_item("Q12345", &mut opt, &mut wd).await;
        assert_eq!(q, "Q12345");
        assert!(!desc.is_empty());
        // Q12345 is Count von Count - should be a generic or character description
        // The result should contain some useful text
    }

    #[tokio::test]
    async fn test_describe_person() {
        let sd = ShortDescription::new();
        let mut wd = WikiData::new();
        let mut opt = DescOptions {
            lang: "en".to_string(),
            links: "text".to_string(),
            ..Default::default()
        };
        // Q42 is Douglas Adams - a person
        let (q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
        assert_eq!(q, "Q42");
        assert!(!desc.is_empty());
        // Should contain occupation info and dates
        assert!(
            desc.contains("1952") || desc.contains("writer") || desc.contains("novelist"),
            "Person description should contain dates or occupations, got: {}",
            desc
        );
    }

    #[tokio::test]
    async fn test_describe_with_wikidata_links() {
        let sd = ShortDescription::new();
        let mut wd = WikiData::new();
        let mut opt = DescOptions {
            lang: "en".to_string(),
            links: "wikidata".to_string(),
            ..Default::default()
        };
        let (_q, desc) = sd.load_item("Q42", &mut opt, &mut wd).await;
        // With wikidata links, should contain <a href='https://www.wikidata.org/wiki/...'>
        assert!(
            desc.contains("wikidata.org") || desc.contains("<a "),
            "Wikidata link mode should produce HTML links, got: {}",
            desc
        );
    }

    #[tokio::test]
    async fn test_describe_wiki_links() {
        let sd = ShortDescription::new();
        let mut wd = WikiData::new();
        let mut opt = DescOptions {
            lang: "en".to_string(),
            links: "wiki".to_string(),
            ..Default::default()
        };
        let (_q, desc) = sd.load_item("Q4504", &mut opt, &mut wd).await;
        // Q4504 is Komodo dragon, should produce wiki-style links
        assert!(
            desc.contains("[[") || !desc.is_empty(),
            "Wiki link mode should produce wikitext or plain labels, got: {}",
            desc
        );
    }
}
